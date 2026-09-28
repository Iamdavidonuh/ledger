use crate::currency::Currency;
use crate::entry::BankState;
use crate::id::{AccountId, EntryId, ImportId, MatchId, QueueRowId};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use rust_decimal::Decimal;

/// One uploaded bank statement that passed its balance check. There is no
/// balance-check-passed field: a record only exists once the check passed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Import {
    pub id: ImportId,
    pub account_id: AccountId,
    pub currency: Currency,
    pub file_name: String,
    pub uploaded_at: DateTime<Utc>,
    pub rows_read: i64,
    pub opening_balance: Decimal,
    pub closing_balance: Decimal,
    pub completed: bool,
}

/// What only a Normal row has: the statement line's own text and state, and
/// the category it will be accepted with.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NormalDetail {
    pub description: String,
    pub bank_state: BankState,
    pub category: Option<String>,
}

/// The two kinds of queue row. A RevertedCandidate carries nothing beyond
/// the (date, time, amount) triple it was matched on, so it has no
/// description, bank state or category to leave empty.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum RowDetail {
    Normal(NormalDetail),
    RevertedCandidate,
}

/// A statement line waiting in the review queue. Whether a Normal row is
/// suspicious is not stored: it is whether any ImportQueueRowMatch has this
/// row as its queue_row_id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportQueueRow {
    pub id: QueueRowId,
    pub import_id: ImportId,
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    pub amount: Decimal,
    pub currency: Currency,
    #[serde(flatten)]
    pub detail: RowDetail,
}

impl ImportQueueRow {
    /// The Normal-only part of this row, if it is a Normal row.
    pub fn normal(&self) -> Option<&NormalDetail> {
        match &self.detail {
            RowDetail::Normal(detail) => Some(detail),
            RowDetail::RevertedCandidate => None,
        }
    }

    pub fn is_reverted_candidate(&self) -> bool {
        matches!(self.detail, RowDetail::RevertedCandidate)
    }
}

/// What a queue row match points at: either an existing ledger entry (a
/// suspicious match or a revert suggestion) or another queue row (a
/// suspicious pending-sibling pair). Exactly one is always set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MatchTarget {
    Entry { entry_id: EntryId },
    QueueRow { queue_row_id: QueueRowId },
}

/// For a Normal row: "suspicious, resembles this". For a RevertedCandidate
/// row: "suggested entry to revert".
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportQueueRowMatch {
    pub id: MatchId,
    pub queue_row_id: QueueRowId,
    pub target: MatchTarget,
}

impl ImportQueueRowMatch {
    /// `queue_row_id` resembles (or, for a reverted candidate, would revert)
    /// the existing ledger entry `entry_id`.
    pub fn to_entry(queue_row_id: QueueRowId, entry_id: EntryId) -> Self {
        ImportQueueRowMatch {
            id: MatchId::generate(),
            queue_row_id,
            target: MatchTarget::Entry { entry_id },
        }
    }

    /// `queue_row_id` resembles `other_row_id`, another row in its own
    /// import's queue. The reverse direction is a separate match row.
    pub fn to_queue_row(queue_row_id: QueueRowId, other_row_id: QueueRowId) -> Self {
        ImportQueueRowMatch {
            id: MatchId::generate(),
            queue_row_id,
            target: MatchTarget::QueueRow {
                queue_row_id: other_row_id,
            },
        }
    }
}

/// What the review screen shows beside a row, per kind: a Normal row is
/// suspicious when it resembles something and may carry a category
/// suggestion; a RevertedCandidate row can only carry the entry it would
/// revert.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum RowReview {
    Normal {
        suspicious: bool,
        matched_entry_ids: Vec<EntryId>,
        matched_queue_row_ids: Vec<QueueRowId>,
        suggested_category: Option<String>,
    },
    RevertedCandidate {
        suggested_entry_id: Option<EntryId>,
    },
}

/// A queue row as the review screen reads it: the row itself and its
/// review data, computed fresh on every read.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QueueRowView {
    #[serde(flatten)]
    pub row: ImportQueueRow,
    #[serde(flatten)]
    pub review: RowReview,
}
