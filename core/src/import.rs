use crate::currency::Currency;
use crate::entry::BankState;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

/// One uploaded bank statement that passed its balance check. There is no
/// balance-check-passed field: a record only exists once the check passed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Import {
    pub id: Uuid,
    pub account_id: Uuid,
    pub currency: Currency,
    pub file_name: String,
    pub uploaded_at: DateTime<Utc>,
    pub rows_read: i64,
    pub opening_balance: Decimal,
    pub closing_balance: Decimal,
    pub completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum QueueRowKind {
    Normal,
    RevertedCandidate,
}

/// A statement line waiting in the review queue. Whether a Normal row is
/// suspicious is not stored: it is whether any ImportQueueRowMatch has this
/// row as its queue_row_id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportQueueRow {
    pub id: Uuid,
    pub import_id: Uuid,
    pub kind: QueueRowKind,
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    pub amount: Decimal,
    pub currency: Currency,
    pub description: String,
    pub bank_state: BankState,
    pub category: Option<String>,
}

/// For a Normal row: "suspicious, resembles this". For a RevertedCandidate
/// row: "suggested entry to revert". Exactly one of the two targets is set.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportQueueRowMatch {
    pub id: Uuid,
    pub queue_row_id: Uuid,
    pub matched_entry_id: Option<Uuid>,
    pub matched_queue_row_id: Option<Uuid>,
}

/// A queue row as the review screen reads it: the row itself, what it
/// matched (for a Normal row these make it suspicious; for a
/// RevertedCandidate row the one matched entry is the suggestion to
/// revert), and for a Normal row a category suggestion computed fresh.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QueueRowView {
    #[serde(flatten)]
    pub row: ImportQueueRow,
    pub suspicious: bool,
    pub matched_entry_ids: Vec<Uuid>,
    pub matched_queue_row_ids: Vec<Uuid>,
    pub suggested_category: Option<String>,
    pub suggested_entry_id: Option<Uuid>,
}
