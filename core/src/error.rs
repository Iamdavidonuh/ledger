use crate::currency::Currency;
use crate::id::{AccountId, EntryId, ImportId, PotId, QueueRowId};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LedgerError {
    #[error("account {0} not found")]
    AccountNotFound(AccountId),
    #[error("entry {0} not found")]
    EntryNotFound(EntryId),
    #[error("pot {0} not found")]
    PotNotFound(PotId),
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
    #[error("a transfer needs two different accounts")]
    TransferToSelfNotAllowed,
    #[error("this account already has an import that is not fully resolved")]
    IncompleteImportExists,
    #[error("bulk accept is blocked: {suspicious_count} suspicious and {reverted_candidate_count} reverted-candidate rows need a decision first")]
    BulkAcceptBlocked {
        suspicious_count: usize,
        reverted_candidate_count: usize,
    },
    #[error("this queue row has no suggested entry to revert; discard it instead")]
    NoSuggestedMatch,
    #[error("that action does not apply to this kind of queue row")]
    WrongQueueRowKind,
    #[error("import {0} not found")]
    ImportNotFound(ImportId),
    #[error("queue row {0} not found")]
    QueueRowNotFound(QueueRowId),
    #[error("storage error: {0}")]
    Storage(String),
}
