use crate::account::Account;
use crate::currency::Currency;
use crate::id::{AccountId, EntryId, EntryPartId, PotId};
use chrono::{NaiveDate, NaiveTime};
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntrySource {
    Manual,
    Imported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BankState {
    Completed,
    Pending,
    Reverted,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub id: EntryId,
    pub account_id: AccountId,
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    pub amount: Decimal,
    pub currency: Currency,
    pub description: String,
    pub note: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub pot_id: Option<PotId>,
    pub transfer_account_id: Option<AccountId>,
    pub source: EntrySource,
    pub bank_state: BankState,
    pub confirmed: bool,
    pub voided_reason: Option<String>,
}

/// The parts of an entry that stay editable once it is saved, even on a
/// locked imported entry. Updating replaces all four together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryMetadata {
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub pot_id: Option<PotId>,
}

impl Entry {
    pub fn is_voided(&self) -> bool {
        self.voided_reason.is_some()
    }

    /// A manually recorded entry on `account`, in the account's own
    /// currency: Manual source, Completed bank state, no time, no category,
    /// no tags, no pot, no transfer link, unconfirmed, not voided.
    pub fn manual(
        account: &Account,
        date: NaiveDate,
        amount: Decimal,
        description: impl Into<String>,
    ) -> Self {
        Entry {
            id: EntryId::generate(),
            account_id: account.id,
            date,
            time: None,
            amount,
            currency: account.currency.clone(),
            description: description.into(),
            note: None,
            category: None,
            tags: Vec::new(),
            pot_id: None,
            transfer_account_id: None,
            source: EntrySource::Manual,
            bank_state: BankState::Completed,
            confirmed: false,
            voided_reason: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EntryPart {
    pub id: EntryPartId,
    pub entry_id: EntryId,
    pub amount: Decimal,
    pub category: Option<String>,
    pub transfer_account_id: Option<AccountId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> Entry {
        Entry {
            id: EntryId::generate(),
            account_id: AccountId::generate(),
            date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            time: None,
            amount: Decimal::ZERO,
            currency: Currency::new("EUR").unwrap(),
            description: "test".to_string(),
            note: None,
            category: None,
            tags: Vec::new(),
            pot_id: None,
            transfer_account_id: None,
            source: EntrySource::Manual,
            bank_state: BankState::Completed,
            confirmed: false,
            voided_reason: None,
        }
    }

    #[test]
    fn a_fresh_entry_is_not_voided() {
        assert!(!sample_entry().is_voided());
    }

    #[test]
    fn an_entry_with_a_reason_is_voided() {
        let mut e = sample_entry();
        e.voided_reason = Some("typo".to_string());
        assert!(e.is_voided());
    }
}
