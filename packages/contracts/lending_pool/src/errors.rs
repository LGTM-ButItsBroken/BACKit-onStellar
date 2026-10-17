use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum LendingPoolError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    InvalidAmount = 3,
    BelowMinDeposit = 4,
    PoolFull = 5,
    InsufficientShares = 6,
    ZeroSupply = 7,
    InsufficientLiquidity = 8,
    Overflow = 9,
    PoolTooSmall = 10,
    EmptyInput = 11,
    AllocationNotFound = 12,
    AlreadyHarvested = 13,
    MarketNotResolved = 14,
    MarketCallFailed = 15,
    Unauthorized = 16,
    InvalidBps = 17,
    InvalidConfig = 18,
    LoanNotFound = 19,
    /// The borrower already has an active loan against this position.
    LoanExists = 20,
    /// The requested amount exceeds 50% of the collateral mark value.
    ExceedsLtv = 21,
    /// The position is not marked above the minimum win probability, or its market has settled.
    CollateralNotEligible = 22,
    /// The loan is healthy and its market has not settled.
    NotLiquidatable = 23,
    NoCollateralMark = 24,
    /// The loan was already repaid or liquidated.
    LoanClosed = 25,
}
