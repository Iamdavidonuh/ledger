use crate::id::AccountId;
use chrono::NaiveDate;
use rust_decimal::Decimal;

/// One side of a transfer: an account and the positive amount that leaves
/// or arrives there, in that account's own currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferLeg {
    pub account_id: AccountId,
    pub amount: Decimal,
}

impl TransferLeg {
    pub fn new(account_id: AccountId, amount: Decimal) -> Self {
        TransferLeg { account_id, amount }
    }
}

/// Money moving from one tracked account to another. The two legs carry
/// separate amounts so a cross-currency transfer can state both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    pub from: TransferLeg,
    pub to: TransferLeg,
    pub date: NaiveDate,
    pub description: String,
}

impl Transfer {
    pub fn new(
        from: TransferLeg,
        to: TransferLeg,
        date: NaiveDate,
        description: impl Into<String>,
    ) -> Self {
        Transfer {
            from,
            to,
            date,
            description: description.into(),
        }
    }
}
