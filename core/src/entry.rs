use crate::currency::Currency;
use chrono::{NaiveDate, NaiveTime};
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EntrySource {
    Manual,
    Imported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BankState {
    Completed,
    Pending,
    Reverted,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub id: Uuid,
    pub account_id: Uuid,
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    pub amount: Decimal,
    pub currency: Currency,
    pub description: String,
    pub note: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub pot_id: Option<Uuid>,
    pub transfer_account_id: Option<Uuid>,
    pub source: EntrySource,
    pub bank_state: BankState,
    pub confirmed: bool,
    pub voided_reason: Option<String>,
}

impl Entry {
    pub fn is_voided(&self) -> bool {
        self.voided_reason.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EntryPart {
    pub id: Uuid,
    pub entry_id: Uuid,
    pub amount: Decimal,
    pub category: Option<String>,
    pub transfer_account_id: Option<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> Entry {
        Entry {
            id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
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
