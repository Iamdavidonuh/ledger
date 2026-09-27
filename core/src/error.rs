use crate::currency::Currency;
use uuid::Uuid;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LedgerError {
    #[error("account {0} not found")]
    AccountNotFound(Uuid),
    #[error("entry {0} not found")]
    EntryNotFound(Uuid),
    #[error("pot {0} not found")]
    PotNotFound(Uuid),
    #[error("cannot edit a locked field on this entry")]
    EntryLocked,
    #[error("entry is already voided")]
    AlreadyVoided,
    #[error("a void reason is required")]
    VoidReasonRequired,
    #[error("entry parts must sum to the entry amount")]
    PartsDoNotSumToAmount,
    #[error("allocation would leave general savings below zero in {0}")]
    GeneralSavingsWouldGoNegative(Currency),
    #[error("allocation would leave the pot below zero")]
    PotWouldGoNegative,
    #[error("a cross-currency transfer needs an explicit amount received")]
    CrossCurrencyAmountRequired,
    #[error("transfer amounts must be greater than zero")]
    TransferAmountMustBePositive,
    #[error("an entry can only be tagged to a pot in the same currency")]
    PotCurrencyMismatch,
    #[error("storage error: {0}")]
    Storage(String),
}
