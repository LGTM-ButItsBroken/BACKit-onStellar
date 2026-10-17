#![cfg(test)]

extern crate std;

use crate::errors::AggregatorError;
use crate::{YieldAggregator, YieldAggregatorClient};
use soroban_sdk::{
    contract, contractimpl,
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Symbol,
};

/// A lending vault whose `balance` can be made to grow, standing in for Blend / Phoenix.
#[contract]
pub struct MockVault;

#[contractimpl]
impl MockVault {
    pub fn init(env: Env, asset: Address) {
        env.storage().instance().set(&Symbol::new(&env, "asset"), &asset);
    }

    pub fn deposit(env: Env, from: Address, amount: i128) {
        let asset: Address = env.storage().instance().get(&Symbol::new(&env, "asset")).unwrap();
        TokenClient::new(&env, &asset).transfer(&from, &env.current_contract_address(), &amount);
        let bal: i128 = env.storage().instance().get(&from).unwrap_or(0);
        env.storage().instance().set(&from, &(bal + amount));
    }

    pub fn withdraw(env: Env, to: Address, amount: i128) {
        let asset: Address = env.storage().instance().get(&Symbol::new(&env, "asset")).unwrap();
        TokenClient::new(&env, &asset).transfer(&env.current_contract_address(), &to, &amount);
        let bal: i128 = env.storage().instance().get(&to).unwrap_or(0);
        env.storage().instance().set(&to, &(bal - amount));
    }

    pub fn balance(env: Env, who: Address) -> i128 {
        env.storage().instance().get(&who).unwrap_or(0)
    }

    /// Credit interest to `who` (the tokens are minted to the vault separately).
    pub fn accrue(env: Env, who: Address, amount: i128) {
        let bal: i128 = env.storage().instance().get(&who).unwrap_or(0);
        env.storage().instance().set(&who, &(bal + amount));
    }
}

struct Fixture<'a> {
    env: Env,
    agg: YieldAggregatorClient<'a>,
    vault: Address,
    vault_client: MockVaultClient<'a>,
    token: Address,
    depositor: Address,
    treasury: Address,
    payouts: Address,
}

fn setup<'a>() -> Fixture<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let payouts = Address::generate(&env);
    let depositor = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(admin.clone()).address();
    StellarAssetClient::new(&env, &token).mint(&depositor, &10_000);

    let vault = env.register(MockVault, ());
    let vault_client = MockVaultClient::new(&env, &vault);
    vault_client.init(&token);

    let agg_id = env.register(YieldAggregator, ());
    let agg = YieldAggregatorClient::new(&env, &agg_id);
    agg.initialize(&admin, &treasury, &payouts, &token, &2_000u32);
    agg.set_vault_allowed(&vault, &true);

    Fixture {
        env,
        agg,
        vault,
        vault_client,
        token,
        depositor,
        treasury,
        payouts,
    }
}

fn bal(f: &Fixture, who: &Address) -> i128 {
    TokenClient::new(&f.env, &f.token).balance(who)
}

/// Add `amount` of interest to the vault's holding for the aggregator.
fn accrue(f: &Fixture, amount: i128) {
    StellarAssetClient::new(&f.env, &f.token).mint(&f.vault, &amount);
    f.vault_client.accrue(&f.agg.address, &amount);
}

#[test]
fn initialize_twice_is_rejected_and_bps_are_bounded() {
    let f = setup();
    let a = Address::generate(&f.env);
    assert_eq!(
        f.agg.try_initialize(&a, &a, &a, &f.token, &2_000u32),
        Err(Ok(AggregatorError::AlreadyInitialized))
    );
    let fresh = YieldAggregatorClient::new(&f.env, &f.env.register(YieldAggregator, ()));
    assert_eq!(
        fresh.try_initialize(&a, &a, &a, &f.token, &10_001u32),
        Err(Ok(AggregatorError::InvalidConfig))
    );
}

#[test]
fn deposit_and_withdraw_move_collateral_through_the_vault() {
    let f = setup();

    f.agg.deposit(&f.depositor, &f.vault, &1_000);
    assert_eq!(bal(&f, &f.depositor), 9_000);
    assert_eq!(f.vault_client.balance(&f.agg.address), 1_000);
    assert_eq!(f.agg.get_vault_state(&f.vault).principal, 1_000);
    assert_eq!(f.agg.get_deposit(&f.vault, &f.depositor), 1_000);

    f.agg.withdraw(&f.depositor, &f.vault, &400);
    assert_eq!(bal(&f, &f.depositor), 9_400);
    assert_eq!(f.vault_client.balance(&f.agg.address), 600);
    assert_eq!(f.agg.get_vault_state(&f.vault).principal, 600);
}

#[test]
fn deposits_are_limited_to_allowlisted_vaults_and_positive_amounts() {
    let f = setup();
    let other_vault = f.env.register(MockVault, ());
    assert_eq!(
        f.agg.try_deposit(&f.depositor, &other_vault, &100),
        Err(Ok(AggregatorError::VaultNotAllowed))
    );
    assert_eq!(
        f.agg.try_deposit(&f.depositor, &f.vault, &0),
        Err(Ok(AggregatorError::InvalidAmount))
    );
}

#[test]
fn cannot_withdraw_more_than_own_principal() {
    let f = setup();
    f.agg.deposit(&f.depositor, &f.vault, &1_000);
    let stranger = Address::generate(&f.env);
    assert_eq!(
        f.agg.try_withdraw(&f.depositor, &f.vault, &1_001),
        Err(Ok(AggregatorError::InsufficientPrincipal))
    );
    assert_eq!(
        f.agg.try_withdraw(&stranger, &f.vault, &1),
        Err(Ok(AggregatorError::InsufficientPrincipal))
    );
}

#[test]
fn yield_is_tracked_apart_from_principal() {
    let f = setup();
    f.agg.deposit(&f.depositor, &f.vault, &1_000);
    accrue(&f, 100);

    assert_eq!(f.agg.pending_yield(&f.vault), 100);
    assert_eq!(f.agg.get_vault_state(&f.vault).principal, 1_000);
    // Yield does not raise what a depositor may withdraw as principal.
    assert_eq!(
        f.agg.try_withdraw(&f.depositor, &f.vault, &1_100),
        Err(Ok(AggregatorError::InsufficientPrincipal))
    );
}

#[test]
fn harvest_splits_yield_80_20_between_payouts_and_treasury() {
    let f = setup();
    f.agg.deposit(&f.depositor, &f.vault, &1_000);
    accrue(&f, 100);

    assert_eq!(f.agg.harvest(&f.vault), 100);

    assert_eq!(bal(&f, &f.payouts), 80);
    assert_eq!(bal(&f, &f.treasury), 20);
    // Principal is untouched and nothing is left to harvest.
    assert_eq!(f.vault_client.balance(&f.agg.address), 1_000);
    assert_eq!(f.agg.pending_yield(&f.vault), 0);
    assert_eq!(f.agg.harvest(&f.vault), 0);
}

#[test]
fn harvest_with_no_yield_is_a_no_op() {
    let f = setup();
    f.agg.deposit(&f.depositor, &f.vault, &1_000);
    assert_eq!(f.agg.harvest(&f.vault), 0);
    assert_eq!(bal(&f, &f.payouts), 0);
    assert_eq!(bal(&f, &f.treasury), 0);
}

#[test]
fn emergency_unwind_recovers_everything_and_blocks_new_deposits() {
    let f = setup();
    f.agg.deposit(&f.depositor, &f.vault, &1_000);
    accrue(&f, 100);

    let recovered = f.agg.emergency_unwind(&f.vault);

    assert_eq!(recovered, 1_100);
    assert_eq!(f.vault_client.balance(&f.agg.address), 0);
    assert_eq!(bal(&f, &f.agg.address), 1_100);
    let state = f.agg.get_vault_state(&f.vault);
    assert!(state.frozen);
    assert_eq!(state.held, 1_100);
    assert_eq!(
        f.agg.try_deposit(&f.depositor, &f.vault, &10),
        Err(Ok(AggregatorError::VaultFrozen))
    );
}

#[test]
fn after_unwind_depositors_withdraw_principal_and_yield_is_still_harvestable() {
    let f = setup();
    f.agg.deposit(&f.depositor, &f.vault, &1_000);
    accrue(&f, 100);
    f.agg.emergency_unwind(&f.vault);

    f.agg.withdraw(&f.depositor, &f.vault, &1_000);
    assert_eq!(bal(&f, &f.depositor), 10_000);

    assert_eq!(f.agg.harvest(&f.vault), 100);
    assert_eq!(bal(&f, &f.payouts), 80);
    assert_eq!(bal(&f, &f.treasury), 20);
    assert_eq!(bal(&f, &f.agg.address), 0);
}
