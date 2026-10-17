use soroban_sdk::{contracttype, Address};

/// Share of harvested yield that goes to the protocol treasury (20%).
pub const DEFAULT_TREASURY_BPS: u32 = 2_000;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AggregatorConfig {
    pub admin: Address,
    /// Receives the treasury share of harvested yield.
    pub treasury: Address,
    /// Receives the remainder, to be credited to winning market payouts.
    pub payout_pool: Address,
    /// The asset deposited into vaults (USDC).
    pub token: Address,
    pub treasury_bps: u32,
}

/// Accounting for one vault. Principal is what depositors put in; everything the
/// vault holds above it is yield and is only ever paid out by `harvest`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct VaultState {
    pub principal: i128,
    /// `true` after `emergency_unwind`: funds are back in this contract.
    pub frozen: bool,
    /// Tokens recovered by `emergency_unwind` that still sit in this contract.
    pub held: i128,
}
