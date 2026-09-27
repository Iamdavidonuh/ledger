use crate::account::{Account, AccountKind};
use crate::currency::Currency;
use crate::entry::{BankState, Entry, EntryPart, EntrySource};
use crate::error::LedgerError;
use crate::pot::{Allocation, Pot};
use crate::store::LedgerStore;
use crate::valuation::Valuation;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

pub struct Ledger<S: LedgerStore> {
    store: S,
}

impl<S: LedgerStore> Ledger<S> {
    pub fn new(store: S) -> Self {
        Ledger { store }
    }

    pub fn open_account(
        &mut self,
        name: &str,
        currency: Currency,
        kind: AccountKind,
        opening_balance: Decimal,
    ) -> Account {
        let account = Account::new(name, currency, kind, opening_balance);
        self.store.save_account(account.clone());
        account
    }

    pub fn record_manual_entry(
        &mut self,
        account_id: Uuid,
        date: NaiveDate,
        amount: Decimal,
        description: &str,
    ) -> Result<Entry, LedgerError> {
        let account = self
            .store
            .get_account(account_id)
            .ok_or(LedgerError::AccountNotFound(account_id))?;
        let entry = Entry {
            id: Uuid::new_v4(),
            account_id,
            date,
            time: None,
            amount,
            currency: account.currency,
            description: description.to_string(),
            note: None,
            category: None,
            tags: Vec::new(),
            pot_id: None,
            transfer_account_id: None,
            source: EntrySource::Manual,
            bank_state: BankState::Completed,
            confirmed: false,
            voided_reason: None,
        };
        self.store.save_entry(entry.clone());
        Ok(entry)
    }

    pub fn account_balance(&self, account_id: Uuid) -> Result<Decimal, LedgerError> {
        let account = self
            .store
            .get_account(account_id)
            .ok_or(LedgerError::AccountNotFound(account_id))?;
        let posted: Decimal = self
            .store
            .entries_for_account(account_id)
            .into_iter()
            .filter(|e| !e.is_voided() && e.bank_state != BankState::Reverted)
            .map(|e| e.amount)
            .sum();
        Ok(account.opening_balance + posted)
    }

    pub fn edit_manual_entry_amount(
        &mut self,
        entry_id: Uuid,
        new_amount: Decimal,
    ) -> Result<Entry, LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        if entry.is_voided() {
            return Err(LedgerError::AlreadyVoided);
        }
        if entry.source != EntrySource::Manual || entry.confirmed {
            return Err(LedgerError::EntryLocked);
        }
        entry.amount = new_amount;
        self.store.save_entry(entry.clone());
        Ok(entry)
    }

    pub fn confirm_entry(&mut self, entry_id: Uuid) -> Result<Entry, LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        if entry.is_voided() {
            return Err(LedgerError::AlreadyVoided);
        }
        entry.confirmed = true;
        self.store.save_entry(entry.clone());
        Ok(entry)
    }

    pub fn void_entry(&mut self, entry_id: Uuid, reason: &str) -> Result<Entry, LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        if entry.is_voided() {
            return Err(LedgerError::AlreadyVoided);
        }
        if reason.trim().is_empty() {
            return Err(LedgerError::VoidReasonRequired);
        }
        entry.voided_reason = Some(reason.to_string());
        self.store.save_entry(entry.clone());
        Ok(entry)
    }

    pub fn split_entry(
        &mut self,
        entry_id: Uuid,
        parts: Vec<(Decimal, Option<String>)>,
    ) -> Result<Vec<EntryPart>, LedgerError> {
        let entry = self
            .store
            .get_entry(entry_id)
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        let sum: Decimal = parts.iter().map(|(amount, _)| *amount).sum();
        if sum != entry.amount {
            return Err(LedgerError::PartsDoNotSumToAmount);
        }
        let built: Vec<EntryPart> = parts
            .into_iter()
            .map(|(amount, category)| EntryPart {
                id: Uuid::new_v4(),
                entry_id,
                amount,
                category,
                transfer_account_id: None,
            })
            .collect();
        self.store.save_entry_parts(entry_id, built.clone());
        Ok(built)
    }

    pub fn parts_for_entry(&self, entry_id: Uuid) -> Vec<EntryPart> {
        self.store.parts_for_entry(entry_id)
    }

    pub fn open_pot(
        &mut self,
        name: &str,
        currency: Currency,
        target: Option<Decimal>,
        priority: Option<i32>,
    ) -> Pot {
        let pot = Pot {
            id: Uuid::new_v4(),
            name: name.to_string(),
            currency,
            target,
            priority,
        };
        self.store.save_pot(pot.clone());
        pot
    }

    pub fn pot_balance(&self, pot_id: Uuid) -> Result<Decimal, LedgerError> {
        self.store
            .get_pot(pot_id)
            .ok_or(LedgerError::PotNotFound(pot_id))?;
        Ok(self
            .store
            .allocations_for_pot(pot_id)
            .into_iter()
            .map(|a| a.amount)
            .sum())
    }

    fn own_accounts_total(&self, currency: &Currency) -> Decimal {
        self.store
            .all_accounts()
            .into_iter()
            .filter(|a| a.kind == AccountKind::Own && &a.currency == currency)
            // account_balance only fails on an unknown account id, and every
            // id here came from all_accounts() on this same store, so it
            // cannot fail. Falling back to zero rather than propagating a
            // Result keeps this and pots_total simple call sites for
            // general_savings, which the rest of the crate treats as
            // infallible.
            .map(|a| self.account_balance(a.id).unwrap_or(Decimal::ZERO))
            .sum()
    }

    fn pots_total(&self, currency: &Currency) -> Decimal {
        self.store
            .all_pots()
            .into_iter()
            .filter(|p| &p.currency == currency)
            // Same reasoning as own_accounts_total: pot_balance only fails
            // on an unknown pot id, and every id here came from all_pots()
            // on this same store.
            .map(|p| self.pot_balance(p.id).unwrap_or(Decimal::ZERO))
            .sum()
    }

    pub fn general_savings(&self, currency: &Currency) -> Decimal {
        self.own_accounts_total(currency) - self.pots_total(currency)
    }

    pub fn allocate_to_pot(
        &mut self,
        pot_id: Uuid,
        amount: Decimal,
        date: NaiveDate,
    ) -> Result<Allocation, LedgerError> {
        let pot = self
            .store
            .get_pot(pot_id)
            .ok_or(LedgerError::PotNotFound(pot_id))?;
        if amount > Decimal::ZERO {
            let savings_after = self.general_savings(&pot.currency) - amount;
            if savings_after < Decimal::ZERO {
                return Err(LedgerError::GeneralSavingsWouldGoNegative(pot.currency));
            }
        } else {
            let pot_balance_after = self.pot_balance(pot_id)? + amount;
            if pot_balance_after < Decimal::ZERO {
                return Err(LedgerError::PotWouldGoNegative);
            }
        }
        let allocation = Allocation {
            id: Uuid::new_v4(),
            pot_id,
            amount,
            date,
            note: None,
        };
        self.store.save_allocation(allocation.clone());
        Ok(allocation)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn transfer(
        &mut self,
        from_account: Uuid,
        to_account: Uuid,
        date: NaiveDate,
        amount_sent: Decimal,
        amount_received: Decimal,
        description: &str,
    ) -> Result<(Entry, Entry), LedgerError> {
        let from = self
            .store
            .get_account(from_account)
            .ok_or(LedgerError::AccountNotFound(from_account))?;
        let to = self
            .store
            .get_account(to_account)
            .ok_or(LedgerError::AccountNotFound(to_account))?;
        if from.currency != to.currency && amount_received == amount_sent {
            return Err(LedgerError::CrossCurrencyAmountRequired);
        }
        if from.currency == to.currency && amount_received != amount_sent {
            return Err(LedgerError::CrossCurrencyAmountRequired);
        }
        let out_entry = Entry {
            id: Uuid::new_v4(),
            account_id: from_account,
            date,
            time: None,
            amount: -amount_sent,
            currency: from.currency,
            description: description.to_string(),
            note: None,
            category: None,
            tags: Vec::new(),
            pot_id: None,
            transfer_account_id: Some(to_account),
            source: EntrySource::Manual,
            bank_state: BankState::Completed,
            confirmed: false,
            voided_reason: None,
        };
        let in_entry = Entry {
            id: Uuid::new_v4(),
            account_id: to_account,
            date,
            time: None,
            amount: amount_received,
            currency: to.currency,
            description: description.to_string(),
            note: None,
            category: None,
            tags: Vec::new(),
            pot_id: None,
            transfer_account_id: Some(from_account),
            source: EntrySource::Manual,
            bank_state: BankState::Completed,
            confirmed: false,
            voided_reason: None,
        };
        self.store.save_entry(out_entry.clone());
        self.store.save_entry(in_entry.clone());
        Ok((out_entry, in_entry))
    }

    pub fn current_value(&self, account_id: Uuid) -> Result<Decimal, LedgerError> {
        let account = self
            .store
            .get_account(account_id)
            .ok_or(LedgerError::AccountNotFound(account_id))?;
        match account.current_value {
            Some(v) => Ok(v),
            None => self.account_balance(account_id),
        }
    }

    pub fn update_current_value(
        &mut self,
        account_id: Uuid,
        new_value: Decimal,
        category: &str,
        date: NaiveDate,
    ) -> Result<Valuation, LedgerError> {
        let mut account = self
            .store
            .get_account(account_id)
            .ok_or(LedgerError::AccountNotFound(account_id))?;
        let old_value = self.current_value(account_id)?;
        let valuation = Valuation {
            id: Uuid::new_v4(),
            account_id,
            date,
            old_value,
            new_value,
            category: category.to_string(),
        };
        account.current_value = Some(new_value);
        self.store.save_account(account);
        self.store.save_valuation(valuation.clone());
        Ok(valuation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InMemoryStore;
    use rust_decimal_macros::dec;

    fn a_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
    }

    #[test]
    fn a_new_account_with_an_opening_balance_and_no_entries_has_that_balance() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(100));
        assert_eq!(ledger.account_balance(account.id), Ok(dec!(100)));
    }

    #[test]
    fn a_manual_entry_changes_the_balance() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(100));
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "Groceries")
            .unwrap();
        assert_eq!(ledger.account_balance(account.id), Ok(dec!(80)));
    }

    #[test]
    fn several_entries_all_count_toward_the_balance() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0));
        ledger
            .record_manual_entry(account.id, a_date(), dec!(500), "Pay")
            .unwrap();
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-30), "Shopping")
            .unwrap();
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-10), "Coffee")
            .unwrap();
        assert_eq!(ledger.account_balance(account.id), Ok(dec!(460)));
    }

    #[test]
    fn the_balance_of_an_unknown_account_is_an_error() {
        let ledger = Ledger::new(InMemoryStore::default());
        assert!(matches!(
            ledger.account_balance(Uuid::new_v4()),
            Err(LedgerError::AccountNotFound(_))
        ));
    }

    #[test]
    fn recording_against_an_unknown_account_is_an_error() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let result = ledger.record_manual_entry(Uuid::new_v4(), a_date(), dec!(10), "x");
        assert!(matches!(result, Err(LedgerError::AccountNotFound(_))));
    }

    #[test]
    fn a_manual_unconfirmed_entry_amount_can_be_edited() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0));
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-4.5), "Bread")
            .unwrap();
        let edited = ledger
            .edit_manual_entry_amount(entry.id, dec!(-45.0))
            .unwrap();
        assert_eq!(edited.amount, dec!(-45.0));
        assert_eq!(ledger.account_balance(account.id), Ok(dec!(-45.0)));
    }

    #[test]
    fn a_confirmed_entry_cannot_be_edited() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0));
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-4.5), "Bread")
            .unwrap();
        ledger.confirm_entry(entry.id).unwrap();
        let result = ledger.edit_manual_entry_amount(entry.id, dec!(-45.0));
        assert_eq!(result, Err(LedgerError::EntryLocked));
    }

    #[test]
    fn voiding_an_entry_excludes_it_from_the_balance() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(100));
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "Mistake")
            .unwrap();
        ledger.void_entry(entry.id, "typed the wrong amount").unwrap();
        assert_eq!(ledger.account_balance(account.id), Ok(dec!(100)));
    }

    #[test]
    fn voiding_needs_a_reason() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0));
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "x")
            .unwrap();
        assert_eq!(
            ledger.void_entry(entry.id, ""),
            Err(LedgerError::VoidReasonRequired)
        );
    }

    #[test]
    fn a_voided_entry_cannot_be_edited() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0));
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "x")
            .unwrap();
        ledger.void_entry(entry.id, "typed the wrong amount").unwrap();
        assert_eq!(
            ledger.edit_manual_entry_amount(entry.id, dec!(-5)),
            Err(LedgerError::AlreadyVoided)
        );
    }

    #[test]
    fn a_voided_entry_cannot_be_confirmed() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0));
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "x")
            .unwrap();
        ledger.void_entry(entry.id, "typed the wrong amount").unwrap();
        assert_eq!(
            ledger.confirm_entry(entry.id),
            Err(LedgerError::AlreadyVoided)
        );
    }

    #[test]
    fn voiding_twice_is_an_error() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0));
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "x")
            .unwrap();
        ledger.void_entry(entry.id, "first reason").unwrap();
        assert_eq!(
            ledger.void_entry(entry.id, "second reason"),
            Err(LedgerError::AlreadyVoided)
        );
    }

    #[test]
    fn splitting_an_entry_into_matching_parts_succeeds() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0));
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-425), "Mixed payment")
            .unwrap();
        let parts = ledger
            .split_entry(
                entry.id,
                vec![
                    (dec!(-300), Some("Loan".to_string())),
                    (dec!(-125), Some("Gifts".to_string())),
                ],
            )
            .unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(ledger.parts_for_entry(entry.id).len(), 2);
    }

    #[test]
    fn splitting_an_entry_into_parts_that_do_not_sum_correctly_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0));
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-425), "Mixed payment")
            .unwrap();
        let result = ledger.split_entry(entry.id, vec![(dec!(-300), None), (dec!(-100), None)]);
        assert_eq!(result, Err(LedgerError::PartsDoNotSumToAmount));
        assert!(ledger.parts_for_entry(entry.id).is_empty());
    }

    #[test]
    fn a_new_pot_starts_at_zero() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let pot = ledger.open_pot("Emergency fund", eur, Some(dec!(2000)), Some(1));
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(0)));
    }

    #[test]
    fn allocating_up_to_exactly_general_savings_succeeds() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100));
        let pot = ledger.open_pot("Emergency fund", eur.clone(), None, None);
        ledger.allocate_to_pot(pot.id, dec!(100), a_date()).unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(100)));
        assert_eq!(ledger.general_savings(&eur), dec!(0));
    }

    #[test]
    fn allocating_one_cent_more_than_general_savings_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100));
        let pot = ledger.open_pot("Emergency fund", eur.clone(), None, None);
        let result = ledger.allocate_to_pot(pot.id, dec!(100.01), a_date());
        assert_eq!(result, Err(LedgerError::GeneralSavingsWouldGoNegative(eur)));
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(0)));
    }

    #[test]
    fn unallocating_more_than_a_pot_holds_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100));
        let pot = ledger.open_pot("Emergency fund", eur, None, None);
        ledger.allocate_to_pot(pot.id, dec!(50), a_date()).unwrap();
        let result = ledger.allocate_to_pot(pot.id, dec!(-60), a_date());
        assert_eq!(result, Err(LedgerError::PotWouldGoNegative));
    }

    #[test]
    fn unallocating_moves_money_back_to_general_savings() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100));
        let pot = ledger.open_pot("Emergency fund", eur.clone(), None, None);
        ledger.allocate_to_pot(pot.id, dec!(100), a_date()).unwrap();
        ledger.allocate_to_pot(pot.id, dec!(-40), a_date()).unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(60)));
        assert_eq!(ledger.general_savings(&eur), dec!(40));
    }

    #[test]
    fn general_savings_only_counts_own_accounts_not_outside_or_person_accounts() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100));
        ledger.open_account("A friend", eur.clone(), AccountKind::Person, dec!(500));
        assert_eq!(ledger.general_savings(&eur), dec!(100));
    }

    #[test]
    fn a_same_currency_transfer_moves_the_same_amount_both_ways() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger.open_account("A", eur.clone(), AccountKind::Own, dec!(100));
        let b = ledger.open_account("B", eur, AccountKind::Own, dec!(0));
        ledger
            .transfer(a.id, b.id, a_date(), dec!(40), dec!(40), "move")
            .unwrap();
        assert_eq!(ledger.account_balance(a.id), Ok(dec!(60)));
        assert_eq!(ledger.account_balance(b.id), Ok(dec!(40)));
    }

    #[test]
    fn a_same_currency_transfer_with_mismatched_amounts_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger.open_account("A", eur.clone(), AccountKind::Own, dec!(100));
        let b = ledger.open_account("B", eur, AccountKind::Own, dec!(0));
        let result = ledger.transfer(a.id, b.id, a_date(), dec!(40), dec!(35), "move");
        assert_eq!(result, Err(LedgerError::CrossCurrencyAmountRequired));
    }

    #[test]
    fn a_cross_currency_transfer_needs_an_explicit_amount_received() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let ngn = Currency::new("NGN").unwrap();
        let a = ledger.open_account("A", eur, AccountKind::Own, dec!(200));
        let b = ledger.open_account("B", ngn, AccountKind::Own, dec!(0));
        ledger
            .transfer(a.id, b.id, a_date(), dec!(200), dec!(370000), "move")
            .unwrap();
        assert_eq!(ledger.account_balance(a.id), Ok(dec!(0)));
        assert_eq!(ledger.account_balance(b.id), Ok(dec!(370000)));
    }

    #[test]
    fn a_cross_currency_transfer_given_the_same_amount_on_both_sides_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let ngn = Currency::new("NGN").unwrap();
        let a = ledger.open_account("A", eur, AccountKind::Own, dec!(200));
        let b = ledger.open_account("B", ngn, AccountKind::Own, dec!(0));
        let result = ledger.transfer(a.id, b.id, a_date(), dec!(200), dec!(200), "move");
        assert_eq!(result, Err(LedgerError::CrossCurrencyAmountRequired));
        assert_eq!(ledger.account_balance(a.id), Ok(dec!(200)));
        assert_eq!(ledger.account_balance(b.id), Ok(dec!(0)));
    }

    #[test]
    fn voiding_one_side_of_a_transfer_leaves_the_other_side_untouched() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger.open_account("A", eur.clone(), AccountKind::Own, dec!(100));
        let b = ledger.open_account("B", eur, AccountKind::Own, dec!(0));
        let (out_entry, in_entry) = ledger
            .transfer(a.id, b.id, a_date(), dec!(40), dec!(40), "move")
            .unwrap();
        ledger
            .void_entry(out_entry.id, "recorded against the wrong account")
            .unwrap();
        assert_eq!(ledger.account_balance(a.id), Ok(dec!(100)));
        assert_eq!(ledger.account_balance(b.id), Ok(dec!(40)));
        assert!(!ledger.store.get_entry(in_entry.id).unwrap().is_voided());
    }

    #[test]
    fn lending_money_is_a_transfer_to_a_person_account_and_can_make_it_negative() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(500));
        let friend = ledger.open_account("A friend", eur, AccountKind::Person, dec!(0));
        ledger
            .transfer(checking.id, friend.id, a_date(), dec!(100), dec!(100), "loan")
            .unwrap();
        assert_eq!(ledger.account_balance(friend.id), Ok(dec!(100)));
        ledger
            .transfer(friend.id, checking.id, a_date(), dec!(30), dec!(30), "repaid")
            .unwrap();
        assert_eq!(ledger.account_balance(friend.id), Ok(dec!(70)));
    }

    #[test]
    fn an_own_account_can_go_negative() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(50));
        let other = ledger.open_account("Other", eur, AccountKind::Own, dec!(0));
        ledger
            .transfer(checking.id, other.id, a_date(), dec!(80), dec!(80), "overdraw")
            .unwrap();
        assert_eq!(ledger.account_balance(checking.id), Ok(dec!(-30)));
    }

    #[test]
    fn an_investment_created_with_an_opening_value_and_no_transaction_has_that_value() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let etf = ledger.open_account("An ETF position", eur, AccountKind::Investment, dec!(1000));
        assert_eq!(ledger.current_value(etf.id), Ok(dec!(1000)));
    }

    #[test]
    fn updating_the_current_value_records_a_valuation_and_changes_current_value() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let etf = ledger.open_account("An ETF position", eur, AccountKind::Investment, dec!(1000));
        let valuation = ledger
            .update_current_value(etf.id, dec!(1042), "Investment gain", a_date())
            .unwrap();
        assert_eq!(valuation.old_value, dec!(1000));
        assert_eq!(valuation.new_value, dec!(1042));
        assert_eq!(valuation.gain(), dec!(42));
        assert_eq!(ledger.current_value(etf.id), Ok(dec!(1042)));
    }

    #[test]
    fn a_person_account_current_value_above_cash_lent_is_recorded_as_interest() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(500));
        let friend = ledger.open_account("A friend", eur, AccountKind::Person, dec!(0));
        ledger
            .transfer(checking.id, friend.id, a_date(), dec!(100), dec!(100), "loan")
            .unwrap();
        let valuation = ledger
            .update_current_value(friend.id, dec!(110), "Loan interest", a_date())
            .unwrap();
        assert_eq!(valuation.gain(), dec!(10));
        assert_eq!(ledger.current_value(friend.id), Ok(dec!(110)));
    }

    #[test]
    fn updating_the_current_value_twice_measures_the_gain_from_the_last_update() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let etf = ledger.open_account("An ETF position", eur, AccountKind::Investment, dec!(1000));
        ledger
            .update_current_value(etf.id, dec!(1100), "Investment gain", a_date())
            .unwrap();
        let second = ledger
            .update_current_value(etf.id, dec!(1080), "Investment gain", a_date())
            .unwrap();
        assert_eq!(second.old_value, dec!(1100));
        assert_eq!(second.gain(), dec!(-20));
    }
}
