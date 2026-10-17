//! The interface the aggregator expects from a lending vault (Blend, Phoenix, or
//! an adapter in front of them). `balance` includes accrued interest.
use soroban_sdk::{contractclient, Address, Env};

#[contractclient(name = "VaultClient")]
pub trait VaultInterface {
    /// Pull `amount` of the asset from `from` (who authorises the transfer).
    fn deposit(env: Env, from: Address, amount: i128);
    /// Send `amount` of the asset to `to`.
    fn withdraw(env: Env, to: Address, amount: i128);
    /// What `who` could withdraw right now, principal plus accrued yield.
    fn balance(env: Env, who: Address) -> i128;
}
