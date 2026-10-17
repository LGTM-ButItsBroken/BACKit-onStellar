#![allow(deprecated)]

use soroban_sdk::{Address, Env};

pub fn emit_deposited(env: &Env, vault: &Address, depositor: &Address, amount: i128) {
    env.events().publish(
        ("yield_aggregator", "Deposited"),
        (vault.clone(), depositor.clone(), amount),
    );
}

pub fn emit_withdrawn(env: &Env, vault: &Address, depositor: &Address, amount: i128) {
    env.events().publish(
        ("yield_aggregator", "Withdrawn"),
        (vault.clone(), depositor.clone(), amount),
    );
}

pub fn emit_harvested(env: &Env, vault: &Address, to_payouts: i128, to_treasury: i128) {
    env.events().publish(
        ("yield_aggregator", "Harvested"),
        (vault.clone(), to_payouts, to_treasury),
    );
}

pub fn emit_unwound(env: &Env, vault: &Address, recovered: i128) {
    env.events()
        .publish(("yield_aggregator", "Unwound"), (vault.clone(), recovered));
}
