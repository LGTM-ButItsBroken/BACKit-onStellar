//! Yield aggregator (SC-010): idle prediction-market collateral is deployed into
//! allowlisted lending vaults. Principal and yield are tracked separately;
//! harvested yield is split 80% to winning-market payouts and 20% to the treasury.
#![no_std]

mod errors;
mod events;
mod storage;
mod types;
mod vault;

#[cfg(test)]
mod test;

pub use types::{AggregatorConfig, VaultState, DEFAULT_TREASURY_BPS};
pub use vault::{VaultClient, VaultInterface};

use errors::AggregatorError;
use events::{emit_deposited, emit_harvested, emit_unwound, emit_withdrawn};
use soroban_sdk::auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation};
use soroban_sdk::{contract, contractimpl, token, Address, Env, IntoVal, Symbol, Vec};

const BPS: i128 = 10_000;

/// Pre-authorise the nested `transfer` the vault performs when it pulls our funds.
fn authorize_vault_pull(env: &Env, asset: &Address, vault: &Address, amount: i128) {
    let args: Vec<soroban_sdk::Val> =
        (env.current_contract_address(), vault.clone(), amount).into_val(env);
    env.authorize_as_current_contract(Vec::from_array(
        env,
        [InvokerContractAuthEntry::Contract(SubContractInvocation {
            context: ContractContext {
                contract: asset.clone(),
                fn_name: Symbol::new(env, "transfer"),
                args,
            },
            sub_invocations: Vec::new(env),
        })],
    ));
}

fn config(env: &Env) -> Result<AggregatorConfig, AggregatorError> {
    storage::get_config(env).ok_or(AggregatorError::NotInitialized)
}

#[contract]
pub struct YieldAggregator;

#[contractimpl]
impl YieldAggregator {
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        payout_pool: Address,
        token: Address,
        treasury_bps: u32,
    ) -> Result<(), AggregatorError> {
        if storage::get_config(&env).is_some() {
            return Err(AggregatorError::AlreadyInitialized);
        }
        admin.require_auth();
        if treasury_bps as i128 > BPS {
            return Err(AggregatorError::InvalidConfig);
        }
        storage::set_config(
            &env,
            &AggregatorConfig {
                admin,
                treasury,
                payout_pool,
                token,
                treasury_bps,
            },
        );
        Ok(())
    }

    /// Allow (or disallow) deposits into `vault` (admin only).
    pub fn set_vault_allowed(env: Env, vault: Address, allowed: bool) -> Result<(), AggregatorError> {
        config(&env)?.admin.require_auth();
        storage::set_allowed(&env, &vault, allowed);
        Ok(())
    }

    /// Deposit `amount` from `depositor` into `vault`. The depositor's principal
    /// is recorded separately from any yield the vault later earns.
    pub fn deposit(env: Env, depositor: Address, vault: Address, amount: i128) -> Result<(), AggregatorError> {
        depositor.require_auth();
        let cfg = config(&env)?;
        if amount <= 0 {
            return Err(AggregatorError::InvalidAmount);
        }
        if !storage::is_allowed(&env, &vault) {
            return Err(AggregatorError::VaultNotAllowed);
        }
        let mut state = storage::get_vault(&env, &vault);
        if state.frozen {
            return Err(AggregatorError::VaultFrozen);
        }

        let this = env.current_contract_address();
        token::Client::new(&env, &cfg.token).transfer(&depositor, &this, &amount);
        authorize_vault_pull(&env, &cfg.token, &vault, amount);
        VaultClient::new(&env, &vault).deposit(&this, &amount);

        state.principal = state.principal.checked_add(amount).ok_or(AggregatorError::Overflow)?;
        storage::set_vault(&env, &vault, &state);
        let mine = storage::get_deposit(&env, &vault, &depositor);
        storage::set_deposit(
            &env,
            &vault,
            &depositor,
            mine.checked_add(amount).ok_or(AggregatorError::Overflow)?,
        );
        emit_deposited(&env, &vault, &depositor, amount);
        Ok(())
    }

    /// Withdraw up to the depositor's own principal. Yield is not withdrawable
    /// here; it goes out through [`Self::harvest`].
    pub fn withdraw(env: Env, depositor: Address, vault: Address, amount: i128) -> Result<(), AggregatorError> {
        depositor.require_auth();
        let cfg = config(&env)?;
        if amount <= 0 {
            return Err(AggregatorError::InvalidAmount);
        }
        let mine = storage::get_deposit(&env, &vault, &depositor);
        if mine < amount {
            return Err(AggregatorError::InsufficientPrincipal);
        }
        let mut state = storage::get_vault(&env, &vault);

        let this = env.current_contract_address();
        let asset = token::Client::new(&env, &cfg.token);
        if state.frozen {
            // Funds were already pulled back by `emergency_unwind`.
            state.held = state.held.checked_sub(amount).ok_or(AggregatorError::VaultShortfall)?;
        } else {
            let before = asset.balance(&this);
            VaultClient::new(&env, &vault).withdraw(&this, &amount);
            if asset.balance(&this) - before < amount {
                return Err(AggregatorError::VaultShortfall);
            }
        }
        asset.transfer(&this, &depositor, &amount);

        state.principal -= amount;
        storage::set_vault(&env, &vault, &state);
        storage::set_deposit(&env, &vault, &depositor, mine - amount);
        emit_withdrawn(&env, &vault, &depositor, amount);
        Ok(())
    }

    /// Realise the yield above principal and split it: `treasury_bps` (20% by
    /// default) to the treasury, the rest to the payout pool. Anyone may call it.
    /// Returns the yield harvested (0 if there is none).
    pub fn harvest(env: Env, vault: Address) -> Result<i128, AggregatorError> {
        let cfg = config(&env)?;
        let mut state = storage::get_vault(&env, &vault);
        let this = env.current_contract_address();
        let asset = token::Client::new(&env, &cfg.token);

        let yield_amount = if state.frozen {
            state.held.checked_sub(state.principal).unwrap_or(0)
        } else {
            VaultClient::new(&env, &vault)
                .balance(&this)
                .checked_sub(state.principal)
                .unwrap_or(0)
        };
        if yield_amount <= 0 {
            return Ok(0);
        }

        if state.frozen {
            state.held -= yield_amount;
            storage::set_vault(&env, &vault, &state);
        } else {
            let before = asset.balance(&this);
            VaultClient::new(&env, &vault).withdraw(&this, &yield_amount);
            if asset.balance(&this) - before < yield_amount {
                return Err(AggregatorError::VaultShortfall);
            }
        }

        let to_treasury = yield_amount
            .checked_mul(cfg.treasury_bps as i128)
            .ok_or(AggregatorError::Overflow)?
            / BPS;
        let to_payouts = yield_amount - to_treasury;
        if to_treasury > 0 {
            asset.transfer(&this, &cfg.treasury, &to_treasury);
        }
        if to_payouts > 0 {
            asset.transfer(&this, &cfg.payout_pool, &to_payouts);
        }
        emit_harvested(&env, &vault, to_payouts, to_treasury);
        Ok(yield_amount)
    }

    /// Pull everything out of `vault` and stop using it (admin only). Principal
    /// stays withdrawable by depositors; the yield above it can still be harvested.
    pub fn emergency_unwind(env: Env, vault: Address) -> Result<i128, AggregatorError> {
        let cfg = config(&env)?;
        cfg.admin.require_auth();
        let mut state = storage::get_vault(&env, &vault);
        if state.frozen {
            return Ok(state.held);
        }
        let this = env.current_contract_address();
        let asset = token::Client::new(&env, &cfg.token);
        let client = VaultClient::new(&env, &vault);

        let available = client.balance(&this);
        let before = asset.balance(&this);
        if available > 0 {
            client.withdraw(&this, &available);
        }
        let recovered = asset.balance(&this) - before;

        state.frozen = true;
        state.held = recovered;
        storage::set_vault(&env, &vault, &state);
        emit_unwound(&env, &vault, recovered);
        Ok(recovered)
    }

    pub fn get_vault_state(env: Env, vault: Address) -> VaultState {
        storage::get_vault(&env, &vault)
    }

    pub fn get_deposit(env: Env, vault: Address, depositor: Address) -> i128 {
        storage::get_deposit(&env, &vault, &depositor)
    }

    /// Yield currently sitting above principal in `vault`.
    pub fn pending_yield(env: Env, vault: Address) -> Result<i128, AggregatorError> {
        let state = storage::get_vault(&env, &vault);
        let value = if state.frozen {
            state.held
        } else {
            VaultClient::new(&env, &vault).balance(&env.current_contract_address())
        };
        Ok(value.checked_sub(state.principal).unwrap_or(0).max(0))
    }

    pub fn get_config_view(env: Env) -> Result<AggregatorConfig, AggregatorError> {
        config(&env)
    }
}
