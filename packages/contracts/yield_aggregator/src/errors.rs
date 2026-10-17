use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum AggregatorError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidAmount = 4,
    /// The vault is not on the admin's allowlist.
    VaultNotAllowed = 5,
    /// The vault was emergency-unwound and accepts no new deposits.
    VaultFrozen = 6,
    /// The depositor's principal in the vault is smaller than the request.
    InsufficientPrincipal = 7,
    InvalidConfig = 8,
    /// `fee_bps` must be at most 10 000.
    Overflow = 9,
    /// The vault returned less than was asked of it.
    VaultShortfall = 10,
}
