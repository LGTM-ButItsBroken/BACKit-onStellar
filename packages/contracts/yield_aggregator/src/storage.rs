use crate::types::{AggregatorConfig, VaultState};
use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
pub enum DataKey {
    Config,
    Allowed(Address),
    Vault(Address),
    Deposit(Address, Address),
}

pub fn set_config(env: &Env, config: &AggregatorConfig) {
    env.storage().instance().set(&DataKey::Config, config);
}

pub fn get_config(env: &Env) -> Option<AggregatorConfig> {
    env.storage().instance().get(&DataKey::Config)
}

pub fn is_allowed(env: &Env, vault: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Allowed(vault.clone()))
        .unwrap_or(false)
}

pub fn set_allowed(env: &Env, vault: &Address, allowed: bool) {
    env.storage()
        .persistent()
        .set(&DataKey::Allowed(vault.clone()), &allowed);
}

pub fn get_vault(env: &Env, vault: &Address) -> VaultState {
    env.storage()
        .persistent()
        .get(&DataKey::Vault(vault.clone()))
        .unwrap_or(VaultState {
            principal: 0,
            frozen: false,
            held: 0,
        })
}

pub fn set_vault(env: &Env, vault: &Address, state: &VaultState) {
    env.storage()
        .persistent()
        .set(&DataKey::Vault(vault.clone()), state);
}

pub fn get_deposit(env: &Env, vault: &Address, depositor: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Deposit(vault.clone(), depositor.clone()))
        .unwrap_or(0)
}

pub fn set_deposit(env: &Env, vault: &Address, depositor: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::Deposit(vault.clone(), depositor.clone()), &amount);
}
