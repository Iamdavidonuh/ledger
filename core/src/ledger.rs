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
    ) -> Result<Account, LedgerError> {
        let account = Account::new(name, currency, kind, opening_balance);
        self.store.save_account(account.clone())?;
        Ok(account)
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
            .get_account(account_id)?
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
        self.store.save_entry(entry.clone())?;
        Ok(entry)
    }

    pub fn account_balance(&self, account_id: Uuid) -> Result<Decimal, LedgerError> {
        let account = self
            .store
            .get_account(account_id)?
            .ok_or(LedgerError::AccountNotFound(account_id))?;
        let posted: Decimal = self
            .store
            .entries_for_account(account_id)?
            .into_iter()
            .filter(|e| !e.is_voided() && e.bank_state != BankState::Reverted)
            .map(|e| e.amount)
            .sum();
        Ok(account.opening_balance + posted)
    }

    pub fn accounts(&self) -> Result<Vec<Account>, LedgerError> {
        self.store.all_accounts()
    }

    pub fn account(&self, id: Uuid) -> Result<Option<Account>, LedgerError> {
        self.store.get_account(id)
    }

    pub fn pots(&self) -> Result<Vec<Pot>, LedgerError> {
        self.store.all_pots()
    }

    pub fn pot(&self, id: Uuid) -> Result<Option<Pot>, LedgerError> {
        self.store.get_pot(id)
    }

    pub fn entries(&self, account_id: Uuid) -> Result<Vec<Entry>, LedgerError> {
        self.store.entries_for_account(account_id)
    }

    pub fn edit_manual_entry_amount(
        &mut self,
        entry_id: Uuid,
        new_amount: Decimal,
    ) -> Result<Entry, LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)?
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        if entry.is_voided() {
            return Err(LedgerError::AlreadyVoided);
        }
        if entry.source != EntrySource::Manual || entry.confirmed {
            return Err(LedgerError::EntryLocked);
        }
        // If this entry is tagged to a pot, changing its amount changes
        // that pot's balance too (pot_balance sums tagged entries directly),
        // so it needs the same never-below-zero check allocate_to_pot and
        // update_entry_metadata already apply. A Reverted entry is not
        // counted in any pot's balance before or after the edit, so there
        // is nothing to check.
        let counts_toward_pots = entry.bank_state != BankState::Reverted;
        if let (Some(pot_id), true) = (entry.pot_id, counts_toward_pots) {
            let would_be = self.pot_balance(pot_id)? - entry.amount + new_amount;
            if would_be < Decimal::ZERO {
                return Err(LedgerError::PotWouldGoNegative);
            }
        }
        entry.amount = new_amount;
        self.store.save_entry(entry.clone())?;
        Ok(entry)
    }

    /// Category, tags, note and pot are always editable, even on an
    /// imported or confirmed entry: only amount, date, time and account are
    /// locked. Voided entries stay untouched entirely, the same as the
    /// other entry-changing methods. Moving a tag onto or off a pot is
    /// checked against that pot's never-below-zero floor the same way an
    /// allocation is, since a tagged expense draws a pot down exactly like
    /// an allocation would. A new pot must be in the entry's own currency;
    /// pot_balance sums tagged entries' raw amounts, so a mismatched
    /// currency would otherwise mix units in that pot's total.
    pub fn update_entry_metadata(
        &mut self,
        entry_id: Uuid,
        category: Option<String>,
        tags: Vec<String>,
        note: Option<String>,
        pot_id: Option<Uuid>,
    ) -> Result<Entry, LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)?
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        if entry.is_voided() {
            return Err(LedgerError::AlreadyVoided);
        }
        // A Reverted entry is excluded from pot_balance whatever its pot_id
        // says, so moving it between pots changes no pot's balance: only the
        // floor comparisons are skipped, the pot and currency checks still run.
        let counts_toward_pots = entry.bank_state != BankState::Reverted;
        if pot_id != entry.pot_id {
            if let (Some(old_pot), true) = (entry.pot_id, counts_toward_pots) {
                let would_be = self.pot_balance(old_pot)? - entry.amount;
                if would_be < Decimal::ZERO {
                    return Err(LedgerError::PotWouldGoNegative);
                }
            }
            if let Some(new_pot) = pot_id {
                let pot = self
                    .store
                    .get_pot(new_pot)?
                    .ok_or(LedgerError::PotNotFound(new_pot))?;
                if entry.currency != pot.currency {
                    return Err(LedgerError::PotCurrencyMismatch);
                }
                if counts_toward_pots {
                    let would_be = self.pot_balance(new_pot)? + entry.amount;
                    if would_be < Decimal::ZERO {
                        return Err(LedgerError::PotWouldGoNegative);
                    }
                }
            }
        }
        entry.category = category;
        entry.tags = tags;
        entry.note = note;
        entry.pot_id = pot_id;
        self.store.save_entry(entry.clone())?;
        Ok(entry)
    }

    pub fn confirm_entry(&mut self, entry_id: Uuid) -> Result<Entry, LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)?
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        if entry.is_voided() {
            return Err(LedgerError::AlreadyVoided);
        }
        entry.confirmed = true;
        self.store.save_entry(entry.clone())?;
        Ok(entry)
    }

    pub fn void_entry(&mut self, entry_id: Uuid, reason: &str) -> Result<Entry, LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)?
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        if entry.is_voided() {
            return Err(LedgerError::AlreadyVoided);
        }
        if reason.trim().is_empty() {
            return Err(LedgerError::VoidReasonRequired);
        }
        // Voiding drops this entry out of entries_tagged_to_pot, so it needs
        // the same never-below-zero check every other pot-affecting write
        // applies, unless it is Reverted and so already out of the pot.
        let counts_toward_pots = entry.bank_state != BankState::Reverted;
        if let (Some(pot_id), true) = (entry.pot_id, counts_toward_pots) {
            let would_be = self.pot_balance(pot_id)? - entry.amount;
            if would_be < Decimal::ZERO {
                return Err(LedgerError::PotWouldGoNegative);
            }
        }
        entry.voided_reason = Some(reason.to_string());
        self.store.save_entry(entry.clone())?;
        Ok(entry)
    }

    pub fn split_entry(
        &mut self,
        entry_id: Uuid,
        parts: Vec<(Decimal, Option<String>)>,
    ) -> Result<Vec<EntryPart>, LedgerError> {
        let entry = self
            .store
            .get_entry(entry_id)?
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        if entry.is_voided() {
            return Err(LedgerError::AlreadyVoided);
        }
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
        self.store.save_entry_parts(entry_id, built.clone())?;
        Ok(built)
    }

    pub fn parts_for_entry(&self, entry_id: Uuid) -> Result<Vec<EntryPart>, LedgerError> {
        self.store.parts_for_entry(entry_id)
    }

    pub fn open_pot(
        &mut self,
        name: &str,
        currency: Currency,
        target: Option<Decimal>,
        priority: Option<i32>,
    ) -> Result<Pot, LedgerError> {
        let pot = Pot {
            id: Uuid::new_v4(),
            name: name.to_string(),
            currency,
            target,
            priority,
        };
        self.store.save_pot(pot.clone())?;
        Ok(pot)
    }

    pub fn pot_balance(&self, pot_id: Uuid) -> Result<Decimal, LedgerError> {
        self.store
            .get_pot(pot_id)?
            .ok_or(LedgerError::PotNotFound(pot_id))?;
        let from_allocations: Decimal = self
            .store
            .allocations_for_pot(pot_id)?
            .into_iter()
            .map(|a| a.amount)
            .sum();
        let from_tagged_entries: Decimal = self
            .entries_tagged_to_pot(pot_id)?
            .into_iter()
            .map(|e| e.amount)
            .sum();
        Ok(from_allocations + from_tagged_entries)
    }

    /// Every entry, across every account, tagged to this pot. A positive
    /// (money-in) entry adds to the pot, a negative (expense) entry draws
    /// it down, matching the spec's pot balance rule directly since amounts
    /// are already signed.
    fn entries_tagged_to_pot(&self, pot_id: Uuid) -> Result<Vec<Entry>, LedgerError> {
        Ok(self
            .store
            .entries_for_pot(pot_id)?
            .into_iter()
            .filter(|entry| !entry.is_voided() && entry.bank_state != BankState::Reverted)
            .collect())
    }

    fn own_accounts_total(&self, currency: &Currency) -> Result<Decimal, LedgerError> {
        let mut total = Decimal::ZERO;
        for account in self.store.all_accounts()? {
            if account.kind == AccountKind::Own && &account.currency == currency {
                total += self.account_balance(account.id)?;
            }
        }
        Ok(total)
    }

    fn pots_total(&self, currency: &Currency) -> Result<Decimal, LedgerError> {
        let mut total = Decimal::ZERO;
        for pot in self.store.all_pots()? {
            if &pot.currency == currency {
                total += self.pot_balance(pot.id)?;
            }
        }
        Ok(total)
    }

    pub fn general_savings(&self, currency: &Currency) -> Result<Decimal, LedgerError> {
        Ok(self.own_accounts_total(currency)? - self.pots_total(currency)?)
    }

    pub fn allocate_to_pot(
        &mut self,
        pot_id: Uuid,
        amount: Decimal,
        date: NaiveDate,
    ) -> Result<Allocation, LedgerError> {
        let pot = self
            .store
            .get_pot(pot_id)?
            .ok_or(LedgerError::PotNotFound(pot_id))?;
        if amount > Decimal::ZERO {
            let savings_after = self.general_savings(&pot.currency)? - amount;
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
        self.store.save_allocation(allocation.clone())?;
        Ok(allocation)
    }

    pub fn transfer(
        &mut self,
        from_account: Uuid,
        to_account: Uuid,
        date: NaiveDate,
        amount_sent: Decimal,
        amount_received: Decimal,
        description: &str,
    ) -> Result<(Entry, Entry), LedgerError> {
        let (out_entry, in_entry) =
            self.build_transfer(from_account, to_account, date, amount_sent, amount_received, description)?;
        self.store.transaction(|store| {
            store.save_entry(out_entry.clone())?;
            store.save_entry(in_entry.clone())
        })?;
        Ok((out_entry, in_entry))
    }

    /// Validates a transfer and builds its two entries without saving
    /// either, so a caller can save them inside its own transaction
    /// (`transfer` itself, or accepting an import queue row as a transfer).
    fn build_transfer(
        &self,
        from_account: Uuid,
        to_account: Uuid,
        date: NaiveDate,
        amount_sent: Decimal,
        amount_received: Decimal,
        description: &str,
    ) -> Result<(Entry, Entry), LedgerError> {
        if from_account == to_account {
            return Err(LedgerError::TransferToSelfNotAllowed);
        }
        if amount_sent <= Decimal::ZERO || amount_received <= Decimal::ZERO {
            return Err(LedgerError::TransferAmountMustBePositive);
        }
        let from = self
            .store
            .get_account(from_account)?
            .ok_or(LedgerError::AccountNotFound(from_account))?;
        let to = self
            .store
            .get_account(to_account)?
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
        Ok((out_entry, in_entry))
    }

    pub fn current_value(&self, account_id: Uuid) -> Result<Decimal, LedgerError> {
        let account = self
            .store
            .get_account(account_id)?
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
            .get_account(account_id)?
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
        self.store.transaction(|store| {
            store.save_account(account)?;
            store.save_valuation(valuation.clone())
        })?;
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
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(100)).unwrap();
        assert_eq!(ledger.account_balance(account.id), Ok(dec!(100)));
    }

    #[test]
    fn a_manual_entry_changes_the_balance() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(100)).unwrap();
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "Groceries")
            .unwrap();
        assert_eq!(ledger.account_balance(account.id), Ok(dec!(80)));
    }

    #[test]
    fn several_entries_all_count_toward_the_balance() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
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
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
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
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
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
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(100)).unwrap();
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
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
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
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
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
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
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
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
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
    fn voiding_a_pot_tagged_income_entry_that_would_push_the_pot_negative_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000)).unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).unwrap();
        let income = ledger
            .record_manual_entry(checking.id, a_date(), dec!(50), "Sold something")
            .unwrap();
        ledger
            .update_entry_metadata(income.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        let expense = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-30), "Camera strap")
            .unwrap();
        ledger
            .update_entry_metadata(expense.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        assert_eq!(
            ledger.void_entry(income.id, "wrong account"),
            Err(LedgerError::PotWouldGoNegative)
        );
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(20)));
    }

    /// Marks an entry Reverted directly in the store, the state
    /// resolve_reverted_candidate would leave it in.
    fn mark_reverted<S: LedgerStore>(ledger: &mut Ledger<S>, entry_id: Uuid) {
        let mut entry = ledger.store.get_entry(entry_id).unwrap().unwrap();
        entry.bank_state = BankState::Reverted;
        ledger.store.save_entry(entry).unwrap();
    }

    /// A pot holding 20: +50 income and -30 expense, both tagged to it.
    fn a_pot_holding_twenty<S: LedgerStore>(ledger: &mut Ledger<S>) -> (Account, Pot, Entry) {
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000)).unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).unwrap();
        let income = ledger
            .record_manual_entry(checking.id, a_date(), dec!(50), "Sold something")
            .unwrap();
        ledger
            .update_entry_metadata(income.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        let expense = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-30), "Camera strap")
            .unwrap();
        ledger
            .update_entry_metadata(expense.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        (checking, pot, income)
    }

    #[test]
    fn voiding_a_reverted_pot_tagged_entry_does_not_count_it_against_the_pot_again() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let (checking, pot, _) = a_pot_holding_twenty(&mut ledger);
        let big_income = ledger
            .record_manual_entry(checking.id, a_date(), dec!(100), "Refund")
            .unwrap();
        ledger
            .update_entry_metadata(big_income.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        mark_reverted(&mut ledger, big_income.id);
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(20)));
        // 20 - 100 would be negative, but the reverted +100 is already out of the pot.
        ledger.void_entry(big_income.id, "bank reversed it").unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(20)));
    }

    #[test]
    fn editing_the_amount_of_a_reverted_pot_tagged_entry_does_not_count_it_against_the_pot() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let (checking, pot, _) = a_pot_holding_twenty(&mut ledger);
        let expense = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-10), "Lens cap")
            .unwrap();
        ledger
            .update_entry_metadata(expense.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        mark_reverted(&mut ledger, expense.id);
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(20)));
        // 20 - (-10) + (-500) would be negative, but a reverted entry never counts.
        ledger.edit_manual_entry_amount(expense.id, dec!(-500)).unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(20)));
    }

    #[test]
    fn moving_a_reverted_entry_onto_or_off_a_pot_skips_the_floor_check() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let (checking, pot, _) = a_pot_holding_twenty(&mut ledger);
        let big_expense = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-900), "Camera shop")
            .unwrap();
        mark_reverted(&mut ledger, big_expense.id);
        ledger
            .update_entry_metadata(big_expense.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(20)));

        let big_income = ledger
            .record_manual_entry(checking.id, a_date(), dec!(100), "Refund")
            .unwrap();
        ledger
            .update_entry_metadata(big_income.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        mark_reverted(&mut ledger, big_income.id);
        ledger
            .update_entry_metadata(big_income.id, None, Vec::new(), None, None)
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(20)));
    }

    #[test]
    fn a_reverted_entry_still_cannot_be_tagged_to_a_missing_or_other_currency_pot() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let usd = Currency::new("USD").unwrap();
        let checking = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
        let usd_pot = ledger.open_pot("Trip", usd, None, None).unwrap();
        let entry = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-10), "x")
            .unwrap();
        mark_reverted(&mut ledger, entry.id);
        assert_eq!(
            ledger.update_entry_metadata(entry.id, None, Vec::new(), None, Some(usd_pot.id)),
            Err(LedgerError::PotCurrencyMismatch)
        );
        let missing = Uuid::new_v4();
        assert_eq!(
            ledger.update_entry_metadata(entry.id, None, Vec::new(), None, Some(missing)),
            Err(LedgerError::PotNotFound(missing))
        );
    }

    #[test]
    fn a_completed_entry_is_still_floor_checked_on_void() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let (_, pot, income) = a_pot_holding_twenty(&mut ledger);
        assert_eq!(
            ledger.void_entry(income.id, "wrong account"),
            Err(LedgerError::PotWouldGoNegative)
        );
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(20)));
    }

    #[test]
    fn splitting_an_entry_into_matching_parts_succeeds() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
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
        assert_eq!(ledger.parts_for_entry(entry.id).unwrap().len(), 2);
    }

    #[test]
    fn splitting_an_entry_into_parts_that_do_not_sum_correctly_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-425), "Mixed payment")
            .unwrap();
        let result = ledger.split_entry(entry.id, vec![(dec!(-300), None), (dec!(-100), None)]);
        assert_eq!(result, Err(LedgerError::PartsDoNotSumToAmount));
        assert!(ledger.parts_for_entry(entry.id).unwrap().is_empty());
    }

    #[test]
    fn a_voided_entry_cannot_be_split() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-425), "Mixed payment")
            .unwrap();
        ledger.void_entry(entry.id, "wrong account").unwrap();
        let result = ledger.split_entry(entry.id, vec![(dec!(-300), None), (dec!(-125), None)]);
        assert_eq!(result, Err(LedgerError::AlreadyVoided));
    }

    #[test]
    fn a_new_pot_starts_at_zero() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let pot = ledger.open_pot("Emergency fund", eur, Some(dec!(2000)), Some(1)).unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(0)));
    }

    #[test]
    fn allocating_up_to_exactly_general_savings_succeeds() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100)).unwrap();
        let pot = ledger.open_pot("Emergency fund", eur.clone(), None, None).unwrap();
        ledger.allocate_to_pot(pot.id, dec!(100), a_date()).unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(100)));
        assert_eq!(ledger.general_savings(&eur), Ok(dec!(0)));
    }

    #[test]
    fn allocating_one_cent_more_than_general_savings_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100)).unwrap();
        let pot = ledger.open_pot("Emergency fund", eur.clone(), None, None).unwrap();
        let result = ledger.allocate_to_pot(pot.id, dec!(100.01), a_date());
        assert_eq!(result, Err(LedgerError::GeneralSavingsWouldGoNegative(eur)));
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(0)));
    }

    #[test]
    fn unallocating_more_than_a_pot_holds_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100)).unwrap();
        let pot = ledger.open_pot("Emergency fund", eur, None, None).unwrap();
        ledger.allocate_to_pot(pot.id, dec!(50), a_date()).unwrap();
        let result = ledger.allocate_to_pot(pot.id, dec!(-60), a_date());
        assert_eq!(result, Err(LedgerError::PotWouldGoNegative));
    }

    #[test]
    fn unallocating_moves_money_back_to_general_savings() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100)).unwrap();
        let pot = ledger.open_pot("Emergency fund", eur.clone(), None, None).unwrap();
        ledger.allocate_to_pot(pot.id, dec!(100), a_date()).unwrap();
        ledger.allocate_to_pot(pot.id, dec!(-40), a_date()).unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(60)));
        assert_eq!(ledger.general_savings(&eur), Ok(dec!(40)));
    }

    #[test]
    fn general_savings_only_counts_own_accounts_not_outside_or_person_accounts() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100)).unwrap();
        ledger.open_account("A friend", eur.clone(), AccountKind::Person, dec!(500)).unwrap();
        assert_eq!(ledger.general_savings(&eur), Ok(dec!(100)));
    }

    #[test]
    fn tagging_an_expense_to_a_pot_draws_the_pot_down_and_leaves_general_savings_alone() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000)).unwrap();
        let pot = ledger.open_pot("Camera", eur.clone(), None, None).unwrap();
        ledger.allocate_to_pot(pot.id, dec!(900), a_date()).unwrap();
        assert_eq!(ledger.general_savings(&eur), Ok(dec!(100)));

        let purchase = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-900), "Camera shop")
            .unwrap();
        ledger
            .update_entry_metadata(purchase.id, Some("Electronics".to_string()), Vec::new(), None, Some(pot.id))
            .unwrap();

        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(0)));
        assert_eq!(ledger.general_savings(&eur), Ok(dec!(100)));
    }

    #[test]
    fn tagging_an_expense_that_would_push_the_pot_negative_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000)).unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).unwrap();
        ledger.allocate_to_pot(pot.id, dec!(500), a_date()).unwrap();
        let purchase = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-900), "Camera shop")
            .unwrap();
        let result = ledger.update_entry_metadata(purchase.id, None, Vec::new(), None, Some(pot.id));
        assert_eq!(result, Err(LedgerError::PotWouldGoNegative));
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(500)));
    }

    #[test]
    fn moving_a_tag_off_a_pot_that_would_go_negative_without_it_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000)).unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).unwrap();
        let income = ledger
            .record_manual_entry(checking.id, a_date(), dec!(50), "Sold something")
            .unwrap();
        ledger
            .update_entry_metadata(income.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        let expense = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-30), "Camera strap")
            .unwrap();
        ledger
            .update_entry_metadata(expense.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(20)));

        // Untagging the +50 income would leave the pot at -30.
        let result = ledger.update_entry_metadata(income.id, None, Vec::new(), None, None);
        assert_eq!(result, Err(LedgerError::PotWouldGoNegative));
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(20)));
    }

    #[test]
    fn category_tags_and_note_can_be_changed_on_a_locked_imported_entry() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
        let entry = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-20), "Wolt")
            .unwrap();
        ledger.confirm_entry(entry.id).unwrap();
        // The amount is locked now, but its metadata is not.
        assert_eq!(
            ledger.edit_manual_entry_amount(entry.id, dec!(-1)),
            Err(LedgerError::EntryLocked)
        );
        let updated = ledger
            .update_entry_metadata(
                entry.id,
                Some("Food delivery".to_string()),
                vec!["late night".to_string()],
                Some("forgot to cook".to_string()),
                None,
            )
            .unwrap();
        assert_eq!(updated.category, Some("Food delivery".to_string()));
        assert_eq!(updated.tags, vec!["late night".to_string()]);
        assert_eq!(updated.note, Some("forgot to cook".to_string()));
    }

    #[test]
    fn a_voided_entry_cannot_have_its_metadata_changed() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
        let entry = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-20), "x")
            .unwrap();
        ledger.void_entry(entry.id, "wrong amount").unwrap();
        let result = ledger.update_entry_metadata(entry.id, Some("Groceries".to_string()), Vec::new(), None, None);
        assert_eq!(result, Err(LedgerError::AlreadyVoided));
    }

    #[test]
    fn tagging_an_entry_to_a_pot_in_a_different_currency_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let usd = Currency::new("USD").unwrap();
        let card = ledger.open_account("Card", usd, AccountKind::Own, dec!(0)).unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).unwrap();
        let entry = ledger
            .record_manual_entry(card.id, a_date(), dec!(-50), "x")
            .unwrap();
        let result = ledger.update_entry_metadata(entry.id, None, Vec::new(), None, Some(pot.id));
        assert_eq!(result, Err(LedgerError::PotCurrencyMismatch));
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(0)));
    }

    #[test]
    fn editing_the_amount_of_a_pot_tagged_entry_that_would_push_the_pot_negative_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000)).unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).unwrap();
        ledger.allocate_to_pot(pot.id, dec!(500), a_date()).unwrap();
        let entry = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-100), "Lens")
            .unwrap();
        ledger
            .update_entry_metadata(entry.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(400)));

        let result = ledger.edit_manual_entry_amount(entry.id, dec!(-600));
        assert_eq!(result, Err(LedgerError::PotWouldGoNegative));
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(400)));
    }

    #[test]
    fn editing_the_amount_of_a_pot_tagged_entry_within_the_pots_means_succeeds() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000)).unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).unwrap();
        ledger.allocate_to_pot(pot.id, dec!(500), a_date()).unwrap();
        let entry = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-100), "Lens")
            .unwrap();
        ledger
            .update_entry_metadata(entry.id, None, Vec::new(), None, Some(pot.id))
            .unwrap();
        ledger.edit_manual_entry_amount(entry.id, dec!(-150)).unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(350)));
    }

    #[test]
    fn accounts_lists_every_open_account() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("A", eur.clone(), AccountKind::Own, dec!(0)).unwrap();
        ledger.open_account("B", eur, AccountKind::Own, dec!(0)).unwrap();
        assert_eq!(ledger.accounts().unwrap().len(), 2);
    }

    #[test]
    fn pots_lists_every_open_pot() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_pot("A", eur.clone(), None, None).unwrap();
        ledger.open_pot("B", eur, None, None).unwrap();
        assert_eq!(ledger.pots().unwrap().len(), 2);
    }

    #[test]
    fn account_finds_a_saved_account_by_id_and_none_for_an_unknown_one() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
        assert_eq!(ledger.account(a.id), Ok(Some(a)));
        assert_eq!(ledger.account(Uuid::new_v4()), Ok(None));
    }

    #[test]
    fn pot_finds_a_saved_pot_by_id_and_none_for_an_unknown_one() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let p = ledger.open_pot("Camera", eur, None, None).unwrap();
        assert_eq!(ledger.pot(p.id), Ok(Some(p)));
        assert_eq!(ledger.pot(Uuid::new_v4()), Ok(None));
    }

    #[test]
    fn entries_lists_every_entry_for_an_account() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
        ledger.record_manual_entry(account.id, a_date(), dec!(-10), "x").unwrap();
        ledger.record_manual_entry(account.id, a_date(), dec!(-5), "y").unwrap();
        assert_eq!(ledger.entries(account.id).unwrap().len(), 2);
    }

    #[test]
    fn a_same_currency_transfer_moves_the_same_amount_both_ways() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger.open_account("A", eur.clone(), AccountKind::Own, dec!(100)).unwrap();
        let b = ledger.open_account("B", eur, AccountKind::Own, dec!(0)).unwrap();
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
        let a = ledger.open_account("A", eur.clone(), AccountKind::Own, dec!(100)).unwrap();
        let b = ledger.open_account("B", eur, AccountKind::Own, dec!(0)).unwrap();
        let result = ledger.transfer(a.id, b.id, a_date(), dec!(40), dec!(35), "move");
        assert_eq!(result, Err(LedgerError::CrossCurrencyAmountRequired));
    }

    #[test]
    fn a_transfer_with_a_negative_amount_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger.open_account("A", eur.clone(), AccountKind::Own, dec!(100)).unwrap();
        let b = ledger.open_account("B", eur, AccountKind::Own, dec!(0)).unwrap();
        let result = ledger.transfer(a.id, b.id, a_date(), dec!(-40), dec!(-40), "move");
        assert_eq!(result, Err(LedgerError::TransferAmountMustBePositive));
        assert_eq!(ledger.account_balance(a.id), Ok(dec!(100)));
        assert_eq!(ledger.account_balance(b.id), Ok(dec!(0)));
    }

    #[test]
    fn a_transfer_of_zero_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger.open_account("A", eur.clone(), AccountKind::Own, dec!(100)).unwrap();
        let b = ledger.open_account("B", eur, AccountKind::Own, dec!(0)).unwrap();
        let result = ledger.transfer(a.id, b.id, a_date(), dec!(0), dec!(0), "move");
        assert_eq!(result, Err(LedgerError::TransferAmountMustBePositive));
    }

    #[test]
    fn a_transfer_from_an_account_to_itself_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger.open_account("A", eur, AccountKind::Own, dec!(100)).unwrap();
        let result = ledger.transfer(a.id, a.id, a_date(), dec!(40), dec!(40), "move");
        assert_eq!(result, Err(LedgerError::TransferToSelfNotAllowed));
        assert!(ledger.entries(a.id).unwrap().is_empty());
    }

    #[test]
    fn a_cross_currency_transfer_needs_an_explicit_amount_received() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let ngn = Currency::new("NGN").unwrap();
        let a = ledger.open_account("A", eur, AccountKind::Own, dec!(200)).unwrap();
        let b = ledger.open_account("B", ngn, AccountKind::Own, dec!(0)).unwrap();
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
        let a = ledger.open_account("A", eur, AccountKind::Own, dec!(200)).unwrap();
        let b = ledger.open_account("B", ngn, AccountKind::Own, dec!(0)).unwrap();
        let result = ledger.transfer(a.id, b.id, a_date(), dec!(200), dec!(200), "move");
        assert_eq!(result, Err(LedgerError::CrossCurrencyAmountRequired));
        assert_eq!(ledger.account_balance(a.id), Ok(dec!(200)));
        assert_eq!(ledger.account_balance(b.id), Ok(dec!(0)));
    }

    #[test]
    fn voiding_one_side_of_a_transfer_leaves_the_other_side_untouched() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger.open_account("A", eur.clone(), AccountKind::Own, dec!(100)).unwrap();
        let b = ledger.open_account("B", eur, AccountKind::Own, dec!(0)).unwrap();
        let (out_entry, in_entry) = ledger
            .transfer(a.id, b.id, a_date(), dec!(40), dec!(40), "move")
            .unwrap();
        ledger
            .void_entry(out_entry.id, "recorded against the wrong account")
            .unwrap();
        assert_eq!(ledger.account_balance(a.id), Ok(dec!(100)));
        assert_eq!(ledger.account_balance(b.id), Ok(dec!(40)));
        assert!(!ledger.store.get_entry(in_entry.id).unwrap().unwrap().is_voided());
    }

    #[test]
    fn lending_money_is_a_transfer_to_a_person_account_and_can_make_it_negative() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(500)).unwrap();
        let friend = ledger.open_account("A friend", eur, AccountKind::Person, dec!(0)).unwrap();
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
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(50)).unwrap();
        let other = ledger.open_account("Other", eur, AccountKind::Own, dec!(0)).unwrap();
        ledger
            .transfer(checking.id, other.id, a_date(), dec!(80), dec!(80), "overdraw")
            .unwrap();
        assert_eq!(ledger.account_balance(checking.id), Ok(dec!(-30)));
    }

    #[test]
    fn an_investment_created_with_an_opening_value_and_no_transaction_has_that_value() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let etf = ledger.open_account("An ETF position", eur, AccountKind::Investment, dec!(1000)).unwrap();
        assert_eq!(ledger.current_value(etf.id), Ok(dec!(1000)));
    }

    #[test]
    fn updating_the_current_value_records_a_valuation_and_changes_current_value() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let etf = ledger.open_account("An ETF position", eur, AccountKind::Investment, dec!(1000)).unwrap();
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
        let checking = ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(500)).unwrap();
        let friend = ledger.open_account("A friend", eur, AccountKind::Person, dec!(0)).unwrap();
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
        let etf = ledger.open_account("An ETF position", eur, AccountKind::Investment, dec!(1000)).unwrap();
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
