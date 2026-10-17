//! Collateralised borrowing against winning prediction positions (SC-009).
//!
//! A borrower pledges a prediction position that a price oracle has marked as
//! dominant (win probability of at least 80%) and borrows pool tokens against
//! its mark value at up to 50% loan-to-value. Simple interest accrues per ledger.
//! A loan whose health factor (mark value / debt) falls below 1.05, or whose
//! market has settled, can be liquidated by anyone who repays the debt and takes
//! over the position.
#![allow(deprecated)]

use soroban_sdk::{contracttype, Address, Env};

use crate::errors::LendingPoolError;
use crate::storage::get_config;
use crate::{token_balance, transfer_token};

/// Maximum loan-to-value, in basis points of the collateral mark value.
pub const MAX_LTV_BPS: i128 = 5_000;
/// Minimum win probability, in basis points, for a position to be accepted.
pub const MIN_WIN_PROB_BPS: u32 = 8_000;
/// Health factor (basis points, 10_000 = 1.0) below which a loan is liquidatable.
pub const LIQUIDATION_HEALTH_BPS: i128 = 10_500;
/// Interest per ledger in parts per billion (~10% APR at 6.3M ledgers a year).
pub const INTEREST_PPB_PER_LEDGER: i128 = 16;

const BPS: i128 = 10_000;
const PPB: i128 = 1_000_000_000;

/// The oracle's current valuation of a borrower's position in a call.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CollateralMark {
    pub value: i128,
    pub win_prob_bps: u32,
    /// `true` once the underlying market has settled.
    pub settled: bool,
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoanStatus {
    Active,
    Repaid,
    Liquidated,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Loan {
    pub borrower: Address,
    pub call_id: u64,
    pub principal: i128,
    pub opened_ledger: u32,
    pub status: LoanStatus,
}

#[contracttype]
enum LoanKey {
    Oracle,
    Mark(Address, u64),
    Loan(Address, u64),
    TotalBorrowed,
}

pub fn get_oracle(env: &Env) -> Option<Address> {
    env.storage().instance().get(&LoanKey::Oracle)
}

pub fn set_oracle(env: &Env, oracle: &Address) {
    env.storage().instance().set(&LoanKey::Oracle, oracle);
}

pub fn total_borrowed(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&LoanKey::TotalBorrowed)
        .unwrap_or(0)
}

fn set_total_borrowed(env: &Env, amount: i128) {
    env.storage().instance().set(&LoanKey::TotalBorrowed, &amount);
}

pub fn get_mark(env: &Env, borrower: &Address, call_id: u64) -> Option<CollateralMark> {
    env.storage()
        .persistent()
        .get(&LoanKey::Mark(borrower.clone(), call_id))
}

pub fn get_loan(env: &Env, borrower: &Address, call_id: u64) -> Option<Loan> {
    env.storage()
        .persistent()
        .get(&LoanKey::Loan(borrower.clone(), call_id))
}

fn set_loan(env: &Env, loan: &Loan) {
    env.storage()
        .persistent()
        .set(&LoanKey::Loan(loan.borrower.clone(), loan.call_id), loan);
}

/// Record the oracle's valuation. Only the configured price oracle may call this.
pub fn set_mark(
    env: &Env,
    oracle: &Address,
    borrower: &Address,
    call_id: u64,
    mark: &CollateralMark,
) -> Result<(), LendingPoolError> {
    oracle.require_auth();
    if get_oracle(env).as_ref() != Some(oracle) {
        return Err(LendingPoolError::Unauthorized);
    }
    if mark.value < 0 || mark.win_prob_bps > BPS as u32 {
        return Err(LendingPoolError::InvalidAmount);
    }
    env.storage()
        .persistent()
        .set(&LoanKey::Mark(borrower.clone(), call_id), mark);
    Ok(())
}

/// Principal plus simple interest accrued since the loan opened.
pub fn debt_of(env: &Env, loan: &Loan) -> Result<i128, LendingPoolError> {
    let elapsed = env.ledger().sequence().saturating_sub(loan.opened_ledger) as i128;
    let interest = loan
        .principal
        .checked_mul(INTEREST_PPB_PER_LEDGER)
        .and_then(|v| v.checked_mul(elapsed))
        .ok_or(LendingPoolError::Overflow)?
        / PPB;
    loan.principal.checked_add(interest).ok_or(LendingPoolError::Overflow)
}

/// Mark value over debt in basis points (10_000 = 1.0). A settled-and-lost
/// position has value 0 and therefore health 0.
pub fn health_bps(mark: &CollateralMark, debt: i128) -> Result<i128, LendingPoolError> {
    if debt <= 0 {
        return Ok(i128::MAX);
    }
    mark.value
        .checked_mul(BPS)
        .map(|v| v / debt)
        .ok_or(LendingPoolError::Overflow)
}

pub fn borrow(
    env: &Env,
    borrower: &Address,
    call_id: u64,
    amount: i128,
) -> Result<(), LendingPoolError> {
    borrower.require_auth();
    let config = get_config(env).ok_or(LendingPoolError::NotInitialized)?;
    if amount <= 0 {
        return Err(LendingPoolError::InvalidAmount);
    }
    if let Some(existing) = get_loan(env, borrower, call_id) {
        if existing.status == LoanStatus::Active {
            return Err(LendingPoolError::LoanExists);
        }
    }

    let mark = get_mark(env, borrower, call_id).ok_or(LendingPoolError::NoCollateralMark)?;
    if mark.settled || mark.win_prob_bps < MIN_WIN_PROB_BPS {
        return Err(LendingPoolError::CollateralNotEligible);
    }
    let max_borrow = mark
        .value
        .checked_mul(MAX_LTV_BPS)
        .ok_or(LendingPoolError::Overflow)?
        / BPS;
    if amount > max_borrow {
        return Err(LendingPoolError::ExceedsLtv);
    }
    if token_balance(env, &config.stake_token) < amount {
        return Err(LendingPoolError::InsufficientLiquidity);
    }

    set_loan(
        env,
        &Loan {
            borrower: borrower.clone(),
            call_id,
            principal: amount,
            opened_ledger: env.ledger().sequence(),
            status: LoanStatus::Active,
        },
    );
    set_total_borrowed(
        env,
        total_borrowed(env)
            .checked_add(amount)
            .ok_or(LendingPoolError::Overflow)?,
    );
    transfer_token(env, &config.stake_token, &env.current_contract_address(), borrower, amount);

    env.events()
        .publish(("lending_pool", "LoanOpened"), (borrower.clone(), call_id, amount));
    Ok(())
}

fn close(
    env: &Env,
    payer: &Address,
    mut loan: Loan,
    status: LoanStatus,
) -> Result<i128, LendingPoolError> {
    let config = get_config(env).ok_or(LendingPoolError::NotInitialized)?;
    let debt = debt_of(env, &loan)?;
    transfer_token(env, &config.stake_token, payer, &env.current_contract_address(), debt);

    set_total_borrowed(
        env,
        total_borrowed(env)
            .checked_sub(loan.principal)
            .ok_or(LendingPoolError::Overflow)?,
    );
    loan.status = status;
    set_loan(env, &loan);
    Ok(debt)
}

pub fn repay(env: &Env, borrower: &Address, call_id: u64) -> Result<i128, LendingPoolError> {
    borrower.require_auth();
    let loan = get_loan(env, borrower, call_id).ok_or(LendingPoolError::LoanNotFound)?;
    if loan.status != LoanStatus::Active {
        return Err(LendingPoolError::LoanClosed);
    }
    let debt = close(env, borrower, loan, LoanStatus::Repaid)?;
    env.events()
        .publish(("lending_pool", "LoanRepaid"), (borrower.clone(), call_id, debt));
    Ok(debt)
}

/// Liquidate an unhealthy or settled loan: `liquidator` repays the full debt
/// and takes over the position.
pub fn liquidate(
    env: &Env,
    liquidator: &Address,
    borrower: &Address,
    call_id: u64,
) -> Result<i128, LendingPoolError> {
    liquidator.require_auth();
    let loan = get_loan(env, borrower, call_id).ok_or(LendingPoolError::LoanNotFound)?;
    if loan.status != LoanStatus::Active {
        return Err(LendingPoolError::LoanClosed);
    }
    let mark = get_mark(env, borrower, call_id).ok_or(LendingPoolError::NoCollateralMark)?;
    let debt = debt_of(env, &loan)?;
    if !mark.settled && health_bps(&mark, debt)? >= LIQUIDATION_HEALTH_BPS {
        return Err(LendingPoolError::NotLiquidatable);
    }
    let paid = close(env, liquidator, loan, LoanStatus::Liquidated)?;
    env.events().publish(
        ("lending_pool", "LoanLiquidated"),
        (borrower.clone(), call_id, liquidator.clone(), paid),
    );
    Ok(paid)
}
