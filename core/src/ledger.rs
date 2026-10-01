use crate::account::{Account, AccountKind};
use crate::currency::Currency;
use crate::entry::{BankState, Entry, EntryMetadata, EntryPart, EntrySource};
use crate::error::LedgerError;
use crate::id::{AccountId, AllocationId, EntryId, EntryPartId, PotId, ValuationId};
use crate::pot::{Allocation, Pot};
use crate::store::LedgerStore;
use crate::transfer::Transfer;
use crate::valuation::Valuation;
use chrono::NaiveDate;
use rust_decimal::Decimal;

mod imports;

pub struct Ledger<S: LedgerStore> {
    store: S,
}

impl<S: LedgerStore> Ledger<S> {
    pub fn new(store: S) -> Self {
        Ledger { store }
    }

    pub async fn open_account(
        &mut self,
        name: &str,
        currency: Currency,
        kind: AccountKind,
        opening_balance: Decimal,
    ) -> Result<Account, LedgerError> {
        let account = Account::new(name, currency, kind, opening_balance);
        self.store.save_account(account.clone()).await?;
        Ok(account)
    }

    pub async fn record_manual_entry(
        &mut self,
        account_id: AccountId,
        date: NaiveDate,
        amount: Decimal,
        description: &str,
    ) -> Result<Entry, LedgerError> {
        let account = self
            .store
            .get_account(account_id)
            .await?
            .ok_or(LedgerError::AccountNotFound(account_id))?;
        let entry = Entry::manual(&account, date, amount, description);
        self.store.save_entry(entry.clone()).await?;
        Ok(entry)
    }

    pub async fn account_balance(&mut self, account_id: AccountId) -> Result<Decimal, LedgerError> {
        let account = self
            .store
            .get_account(account_id)
            .await?
            .ok_or(LedgerError::AccountNotFound(account_id))?;
        let posted: Decimal = self
            .store
            .entries_for_account(account_id)
            .await?
            .into_iter()
            .filter(|e| !e.is_voided() && e.bank_state != BankState::Reverted)
            .map(|e| e.amount)
            .sum();
        Ok(account.opening_balance + posted)
    }

    pub async fn accounts(&mut self) -> Result<Vec<Account>, LedgerError> {
        self.store.all_accounts().await
    }

    pub async fn account(&mut self, id: AccountId) -> Result<Option<Account>, LedgerError> {
        self.store.get_account(id).await
    }

    pub async fn pots(&mut self) -> Result<Vec<Pot>, LedgerError> {
        self.store.all_pots().await
    }

    pub async fn pot(&mut self, id: PotId) -> Result<Option<Pot>, LedgerError> {
        self.store.get_pot(id).await
    }

    pub async fn entries(&mut self, account_id: AccountId) -> Result<Vec<Entry>, LedgerError> {
        self.store.entries_for_account(account_id).await
    }

    pub async fn edit_manual_entry_amount(
        &mut self,
        entry_id: EntryId,
        new_amount: Decimal,
    ) -> Result<Entry, LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)
            .await?
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
            let would_be = self.pot_balance(pot_id).await? - entry.amount + new_amount;
            if would_be < Decimal::ZERO {
                return Err(LedgerError::PotWouldGoNegative);
            }
        }
        entry.amount = new_amount;
        self.store.save_entry(entry.clone()).await?;
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
    pub async fn update_entry_metadata(
        &mut self,
        entry_id: EntryId,
        metadata: EntryMetadata,
    ) -> Result<Entry, LedgerError> {
        let EntryMetadata {
            category,
            tags,
            note,
            pot_id,
        } = metadata;
        let mut entry = self
            .store
            .get_entry(entry_id)
            .await?
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
                let would_be = self.pot_balance(old_pot).await? - entry.amount;
                if would_be < Decimal::ZERO {
                    return Err(LedgerError::PotWouldGoNegative);
                }
            }
            if let Some(new_pot) = pot_id {
                let pot = self
                    .store
                    .get_pot(new_pot)
                    .await?
                    .ok_or(LedgerError::PotNotFound(new_pot))?;
                if entry.currency != pot.currency {
                    return Err(LedgerError::PotCurrencyMismatch);
                }
                if counts_toward_pots {
                    let would_be = self.pot_balance(new_pot).await? + entry.amount;
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
        self.store.save_entry(entry.clone()).await?;
        Ok(entry)
    }

    pub async fn confirm_entry(&mut self, entry_id: EntryId) -> Result<Entry, LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)
            .await?
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        if entry.is_voided() {
            return Err(LedgerError::AlreadyVoided);
        }
        entry.confirmed = true;
        self.store.save_entry(entry.clone()).await?;
        Ok(entry)
    }

    pub async fn void_entry(
        &mut self,
        entry_id: EntryId,
        reason: &str,
    ) -> Result<Entry, LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)
            .await?
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
            let would_be = self.pot_balance(pot_id).await? - entry.amount;
            if would_be < Decimal::ZERO {
                return Err(LedgerError::PotWouldGoNegative);
            }
        }
        entry.voided_reason = Some(reason.to_string());
        self.store.save_entry(entry.clone()).await?;
        Ok(entry)
    }

    pub async fn split_entry(
        &mut self,
        entry_id: EntryId,
        parts: Vec<(Decimal, Option<String>)>,
    ) -> Result<Vec<EntryPart>, LedgerError> {
        let entry = self
            .store
            .get_entry(entry_id)
            .await?
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
                id: EntryPartId::generate(),
                entry_id,
                amount,
                category,
                transfer_account_id: None,
            })
            .collect();
        self.store
            .save_entry_parts(entry_id, built.clone())
            .await?;
        Ok(built)
    }

    pub async fn parts_for_entry(
        &mut self,
        entry_id: EntryId,
    ) -> Result<Vec<EntryPart>, LedgerError> {
        self.store.parts_for_entry(entry_id).await
    }

    pub async fn open_pot(
        &mut self,
        name: &str,
        currency: Currency,
        target: Option<Decimal>,
        priority: Option<i32>,
    ) -> Result<Pot, LedgerError> {
        let pot = Pot {
            id: PotId::generate(),
            name: name.to_string(),
            currency,
            target,
            priority,
        };
        self.store.save_pot(pot.clone()).await?;
        Ok(pot)
    }

    /// A pot's balance was never money "in" it, only money already in your
    /// own accounts that this pot laid a claim on -- general_savings is
    /// defined as own_accounts_total minus pots_total, so once the pot
    /// itself is gone that claim is gone too and the money is back in
    /// general savings with no transfer of any kind. What does need doing:
    /// its allocation rows (which only make sense pointing at a pot) are
    /// deleted, and any entry tagged to it, voided or reverted included, is
    /// untagged rather than left pointing at a pot that no longer exists.
    /// The entries themselves, and the real transactions they represent,
    /// are untouched.
    pub async fn delete_pot(&mut self, pot_id: PotId) -> Result<(), LedgerError> {
        self.store
            .get_pot(pot_id)
            .await?
            .ok_or(LedgerError::PotNotFound(pot_id))?;
        for mut entry in self.store.entries_for_pot(pot_id).await? {
            entry.pot_id = None;
            self.store.save_entry(entry).await?;
        }
        self.store.delete_allocations_for_pot(pot_id).await?;
        self.store.delete_pot(pot_id).await?;
        Ok(())
    }

    pub async fn pot_balance(&mut self, pot_id: PotId) -> Result<Decimal, LedgerError> {
        self.store
            .get_pot(pot_id)
            .await?
            .ok_or(LedgerError::PotNotFound(pot_id))?;
        let from_allocations: Decimal = self
            .store
            .allocations_for_pot(pot_id)
            .await?
            .into_iter()
            .map(|a| a.amount)
            .sum();
        let from_tagged_entries: Decimal = self
            .entries_tagged_to_pot(pot_id)
            .await?
            .into_iter()
            .map(|e| e.amount)
            .sum();
        Ok(from_allocations + from_tagged_entries)
    }

    /// Every entry, across every account, tagged to this pot. A positive
    /// (money-in) entry adds to the pot, a negative (expense) entry draws
    /// it down, matching the spec's pot balance rule directly since amounts
    /// are already signed.
    async fn entries_tagged_to_pot(&mut self, pot_id: PotId) -> Result<Vec<Entry>, LedgerError> {
        Ok(self
            .store
            .entries_for_pot(pot_id)
            .await?
            .into_iter()
            .filter(|entry| !entry.is_voided() && entry.bank_state != BankState::Reverted)
            .collect())
    }

    async fn own_accounts_total(&mut self, currency: &Currency) -> Result<Decimal, LedgerError> {
        let accounts = self
            .store
            .all_accounts()
            .await?
            .into_iter()
            .filter(|a| a.kind == AccountKind::Own && &a.currency == currency)
            .collect::<Vec<_>>();
        let mut total = Decimal::ZERO;
        for a in accounts {
            total += self.account_balance(a.id).await?;
        }
        Ok(total)
    }

    async fn pots_total(&mut self, currency: &Currency) -> Result<Decimal, LedgerError> {
        let pots = self
            .store
            .all_pots()
            .await?
            .into_iter()
            .filter(|p| &p.currency == currency)
            .collect::<Vec<_>>();
        let mut total = Decimal::ZERO;
        for p in pots {
            total += self.pot_balance(p.id).await?;
        }
        Ok(total)
    }

    pub async fn general_savings(&mut self, currency: &Currency) -> Result<Decimal, LedgerError> {
        Ok(self.own_accounts_total(currency).await?
            - self.pots_total(currency).await?)
    }

    pub async fn allocate_to_pot(
        &mut self,
        pot_id: PotId,
        amount: Decimal,
        date: NaiveDate,
    ) -> Result<Allocation, LedgerError> {
        let pot = self
            .store
            .get_pot(pot_id)
            .await?
            .ok_or(LedgerError::PotNotFound(pot_id))?;
        if amount > Decimal::ZERO {
            let savings_after = self.general_savings(&pot.currency).await? - amount;
            if savings_after < Decimal::ZERO {
                return Err(LedgerError::GeneralSavingsWouldGoNegative(pot.currency));
            }
        } else {
            let pot_balance_after = self.pot_balance(pot_id).await? + amount;
            if pot_balance_after < Decimal::ZERO {
                return Err(LedgerError::PotWouldGoNegative);
            }
        }
        let allocation = Allocation {
            id: AllocationId::generate(),
            pot_id,
            amount,
            date,
            note: None,
        };
        self.store.save_allocation(allocation.clone()).await?;
        Ok(allocation)
    }

    pub async fn transfer(
        &mut self,
        transfer: Transfer,
    ) -> Result<(Entry, Entry), LedgerError> {
        let (out_entry, in_entry) = self.build_transfer(&transfer).await?;
        self.store.begin().await?;
        let result = async {
            self.store.save_entry(out_entry.clone()).await?;
            self.store.save_entry(in_entry.clone()).await
        }
        .await;
        match result {
            Ok(()) => self.store.commit().await?,
            Err(e) => { self.store.rollback().await?; return Err(e); }
        }
        Ok((out_entry, in_entry))
    }

    /// Both amounts must be positive, and they differ exactly when the two
    /// accounts' currencies do.
    fn validate_transfer_amounts(
        transfer: &Transfer,
        from_currency: &Currency,
        to_currency: &Currency,
    ) -> Result<(), LedgerError> {
        let (sent, received) = (transfer.from.amount, transfer.to.amount);
        if sent <= Decimal::ZERO || received <= Decimal::ZERO {
            return Err(LedgerError::TransferAmountMustBePositive);
        }
        let cross_currency = from_currency != to_currency;
        if cross_currency == (received == sent) {
            return Err(LedgerError::CrossCurrencyAmountRequired);
        }
        Ok(())
    }

    /// Validates a transfer and builds its two entries without saving
    /// either, so a caller can save them inside its own transaction
    /// (`transfer` itself, or accepting an import queue row as a transfer).
    async fn build_transfer(
        &mut self,
        transfer: &Transfer,
    ) -> Result<(Entry, Entry), LedgerError> {
        let (from_id, to_id) = (transfer.from.account_id, transfer.to.account_id);
        if from_id == to_id {
            return Err(LedgerError::TransferToSelfNotAllowed);
        }
        let from = self
            .store
            .get_account(from_id)
            .await?
            .ok_or(LedgerError::AccountNotFound(from_id))?;
        let to = self
            .store
            .get_account(to_id)
            .await?
            .ok_or(LedgerError::AccountNotFound(to_id))?;
        Self::validate_transfer_amounts(transfer, &from.currency, &to.currency)?;
        let out_entry = Entry {
            transfer_account_id: Some(to_id),
            ..Entry::manual(
                &from,
                transfer.date,
                -transfer.from.amount,
                transfer.description.as_str(),
            )
        };
        let in_entry = Entry {
            transfer_account_id: Some(from_id),
            ..Entry::manual(
                &to,
                transfer.date,
                transfer.to.amount,
                transfer.description.as_str(),
            )
        };
        Ok((out_entry, in_entry))
    }

    pub async fn current_value(
        &mut self,
        account_id: AccountId,
    ) -> Result<Decimal, LedgerError> {
        let account = self
            .store
            .get_account(account_id)
            .await?
            .ok_or(LedgerError::AccountNotFound(account_id))?;
        match account.current_value {
            Some(v) => Ok(v),
            None => self.account_balance(account_id).await,
        }
    }

    pub async fn update_current_value(
        &mut self,
        account_id: AccountId,
        new_value: Decimal,
        category: &str,
        date: NaiveDate,
    ) -> Result<Valuation, LedgerError> {
        let mut account = self
            .store
            .get_account(account_id)
            .await?
            .ok_or(LedgerError::AccountNotFound(account_id))?;
        let old_value = self.current_value(account_id).await?;
        let valuation = Valuation {
            id: ValuationId::generate(),
            account_id,
            date,
            old_value,
            new_value,
            category: category.to_string(),
        };
        account.current_value = Some(new_value);
        self.store.begin().await?;
        let result = async {
            self.store.save_account(account).await?;
            self.store.save_valuation(valuation.clone()).await
        }
        .await;
        match result {
            Ok(()) => self.store.commit().await?,
            Err(e) => { self.store.rollback().await?; return Err(e); }
        }
        Ok(valuation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InMemoryStore;
    use crate::transfer::TransferLeg;
    use rust_decimal_macros::dec;

    fn a_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
    }

    fn tagged_to(pot: &Pot) -> EntryMetadata {
        EntryMetadata {
            pot_id: Some(pot.id),
            ..EntryMetadata::default()
        }
    }

    /// A fresh ledger with two Own EUR accounts: "A" holding 100, "B" empty.
    async fn two_euro_accounts() -> (Ledger<InMemoryStore>, Account, Account) {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger
            .open_account("A", eur.clone(), AccountKind::Own, dec!(100))
            .await
            .unwrap();
        let b = ledger
            .open_account("B", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        (ledger, a, b)
    }

    fn leg(account: &Account, amount: Decimal) -> TransferLeg {
        TransferLeg::new(account.id, amount)
    }

    fn a_transfer(from: TransferLeg, to: TransferLeg, description: &str) -> Transfer {
        Transfer::new(from, to, a_date(), description)
    }

    #[tokio::test]
    async fn a_new_account_with_an_opening_balance_and_no_entries_has_that_balance() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(100))
            .await
            .unwrap();
        assert_eq!(ledger.account_balance(account.id).await, Ok(dec!(100)));
    }

    #[tokio::test]
    async fn a_manual_entry_changes_the_balance() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(100))
            .await
            .unwrap();
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "Groceries")
            .await
            .unwrap();
        assert_eq!(ledger.account_balance(account.id).await, Ok(dec!(80)));
    }

    #[tokio::test]
    async fn several_entries_all_count_toward_the_balance() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        ledger
            .record_manual_entry(account.id, a_date(), dec!(500), "Pay")
            .await
            .unwrap();
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-30), "Shopping")
            .await
            .unwrap();
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-10), "Coffee")
            .await
            .unwrap();
        assert_eq!(ledger.account_balance(account.id).await, Ok(dec!(460)));
    }

    #[tokio::test]
    async fn the_balance_of_an_unknown_account_is_an_error() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        assert!(matches!(
            ledger.account_balance(AccountId::generate()).await,
            Err(LedgerError::AccountNotFound(_))
        ));
    }

    #[tokio::test]
    async fn recording_against_an_unknown_account_is_an_error() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let result = ledger
            .record_manual_entry(AccountId::generate(), a_date(), dec!(10), "x")
            .await;
        assert!(matches!(result, Err(LedgerError::AccountNotFound(_))));
    }

    #[tokio::test]
    async fn a_manual_unconfirmed_entry_amount_can_be_edited() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-4.5), "Bread")
            .await
            .unwrap();
        let edited = ledger
            .edit_manual_entry_amount(entry.id, dec!(-45.0))
            .await
            .unwrap();
        assert_eq!(edited.amount, dec!(-45.0));
        assert_eq!(ledger.account_balance(account.id).await, Ok(dec!(-45.0)));
    }

    #[tokio::test]
    async fn a_confirmed_entry_cannot_be_edited() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-4.5), "Bread")
            .await
            .unwrap();
        ledger.confirm_entry(entry.id).await.unwrap();
        let result = ledger
            .edit_manual_entry_amount(entry.id, dec!(-45.0))
            .await;
        assert_eq!(result, Err(LedgerError::EntryLocked));
    }

    #[tokio::test]
    async fn voiding_an_entry_excludes_it_from_the_balance() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(100))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "Mistake")
            .await
            .unwrap();
        ledger
            .void_entry(entry.id, "typed the wrong amount")
            .await
            .unwrap();
        assert_eq!(ledger.account_balance(account.id).await, Ok(dec!(100)));
    }

    #[tokio::test]
    async fn voiding_needs_a_reason() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "x")
            .await
            .unwrap();
        assert_eq!(
            ledger.void_entry(entry.id, "").await,
            Err(LedgerError::VoidReasonRequired)
        );
    }

    #[tokio::test]
    async fn a_voided_entry_cannot_be_edited() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "x")
            .await
            .unwrap();
        ledger
            .void_entry(entry.id, "typed the wrong amount")
            .await
            .unwrap();
        assert_eq!(
            ledger.edit_manual_entry_amount(entry.id, dec!(-5)).await,
            Err(LedgerError::AlreadyVoided)
        );
    }

    #[tokio::test]
    async fn a_voided_entry_cannot_be_confirmed() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "x")
            .await
            .unwrap();
        ledger
            .void_entry(entry.id, "typed the wrong amount")
            .await
            .unwrap();
        assert_eq!(
            ledger.confirm_entry(entry.id).await,
            Err(LedgerError::AlreadyVoided)
        );
    }

    #[tokio::test]
    async fn voiding_twice_is_an_error() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "x")
            .await
            .unwrap();
        ledger
            .void_entry(entry.id, "first reason")
            .await
            .unwrap();
        assert_eq!(
            ledger.void_entry(entry.id, "second reason").await,
            Err(LedgerError::AlreadyVoided)
        );
    }

    #[tokio::test]
    async fn voiding_a_pot_tagged_income_entry_that_would_push_the_pot_negative_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000))
            .await
            .unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).await.unwrap();
        let income = ledger
            .record_manual_entry(checking.id, a_date(), dec!(50), "Sold something")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(income.id, tagged_to(&pot))
            .await
            .unwrap();
        let expense = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-30), "Camera strap")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(expense.id, tagged_to(&pot))
            .await
            .unwrap();
        assert_eq!(
            ledger.void_entry(income.id, "wrong account").await,
            Err(LedgerError::PotWouldGoNegative)
        );
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(20)));
    }

    /// Marks an entry Reverted directly in the store, the state
    /// resolve_reverted_candidate would leave it in.
    async fn mark_reverted<S: LedgerStore>(ledger: &mut Ledger<S>, entry_id: EntryId) {
        let mut entry = ledger.store.get_entry(entry_id).await.unwrap().unwrap();
        entry.bank_state = BankState::Reverted;
        ledger.store.save_entry(entry).await.unwrap();
    }

    /// A pot holding 20: +50 income and -30 expense, both tagged to it.
    async fn a_pot_holding_twenty<S: LedgerStore>(
        ledger: &mut Ledger<S>,
    ) -> (Account, Pot, Entry) {
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000))
            .await
            .unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).await.unwrap();
        let income = ledger
            .record_manual_entry(checking.id, a_date(), dec!(50), "Sold something")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(income.id, tagged_to(&pot))
            .await
            .unwrap();
        let expense = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-30), "Camera strap")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(expense.id, tagged_to(&pot))
            .await
            .unwrap();
        (checking, pot, income)
    }

    #[tokio::test]
    async fn voiding_a_reverted_pot_tagged_entry_does_not_count_it_against_the_pot_again() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let (checking, pot, _) = a_pot_holding_twenty(&mut ledger).await;
        let big_income = ledger
            .record_manual_entry(checking.id, a_date(), dec!(100), "Refund")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(big_income.id, tagged_to(&pot))
            .await
            .unwrap();
        mark_reverted(&mut ledger, big_income.id).await;
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(20)));
        // 20 - 100 would be negative, but the reverted +100 is already out of the pot.
        ledger
            .void_entry(big_income.id, "bank reversed it")
            .await
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(20)));
    }

    #[tokio::test]
    async fn editing_the_amount_of_a_reverted_pot_tagged_entry_does_not_count_it_against_the_pot()
    {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let (checking, pot, _) = a_pot_holding_twenty(&mut ledger).await;
        let expense = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-10), "Lens cap")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(expense.id, tagged_to(&pot))
            .await
            .unwrap();
        mark_reverted(&mut ledger, expense.id).await;
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(20)));
        // 20 - (-10) + (-500) would be negative, but a reverted entry never counts.
        ledger
            .edit_manual_entry_amount(expense.id, dec!(-500))
            .await
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(20)));
    }

    #[tokio::test]
    async fn moving_a_reverted_entry_onto_or_off_a_pot_skips_the_floor_check() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let (checking, pot, _) = a_pot_holding_twenty(&mut ledger).await;
        let big_expense = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-900), "Camera shop")
            .await
            .unwrap();
        mark_reverted(&mut ledger, big_expense.id).await;
        ledger
            .update_entry_metadata(big_expense.id, tagged_to(&pot))
            .await
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(20)));

        let big_income = ledger
            .record_manual_entry(checking.id, a_date(), dec!(100), "Refund")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(big_income.id, tagged_to(&pot))
            .await
            .unwrap();
        mark_reverted(&mut ledger, big_income.id).await;
        ledger
            .update_entry_metadata(big_income.id, EntryMetadata::default())
            .await
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(20)));
    }

    #[tokio::test]
    async fn a_reverted_entry_still_cannot_be_tagged_to_a_missing_or_other_currency_pot() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let usd = Currency::new("USD").unwrap();
        let checking = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let usd_pot = ledger.open_pot("Trip", usd, None, None).await.unwrap();
        let entry = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-10), "x")
            .await
            .unwrap();
        mark_reverted(&mut ledger, entry.id).await;
        assert_eq!(
            ledger
                .update_entry_metadata(entry.id, tagged_to(&usd_pot))
                .await,
            Err(LedgerError::PotCurrencyMismatch)
        );
        let missing = PotId::generate();
        assert_eq!(
            ledger
                .update_entry_metadata(
                    entry.id,
                    EntryMetadata {
                        pot_id: Some(missing),
                        ..EntryMetadata::default()
                    },
                )
                .await,
            Err(LedgerError::PotNotFound(missing))
        );
    }

    #[tokio::test]
    async fn a_completed_entry_is_still_floor_checked_on_void() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let (_, pot, income) = a_pot_holding_twenty(&mut ledger).await;
        assert_eq!(
            ledger.void_entry(income.id, "wrong account").await,
            Err(LedgerError::PotWouldGoNegative)
        );
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(20)));
    }

    #[tokio::test]
    async fn splitting_an_entry_into_matching_parts_succeeds() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-425), "Mixed payment")
            .await
            .unwrap();
        let parts = ledger
            .split_entry(
                entry.id,
                vec![
                    (dec!(-300), Some("Loan".to_string())),
                    (dec!(-125), Some("Gifts".to_string())),
                ],
            )
            .await
            .unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(
            ledger.parts_for_entry(entry.id).await.unwrap().len(),
            2
        );
    }

    #[tokio::test]
    async fn splitting_an_entry_into_parts_that_do_not_sum_correctly_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-425), "Mixed payment")
            .await
            .unwrap();
        let result = ledger
            .split_entry(entry.id, vec![(dec!(-300), None), (dec!(-100), None)])
            .await;
        assert_eq!(result, Err(LedgerError::PartsDoNotSumToAmount));
        assert!(ledger
            .parts_for_entry(entry.id)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn a_voided_entry_cannot_be_split() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-425), "Mixed payment")
            .await
            .unwrap();
        ledger
            .void_entry(entry.id, "wrong account")
            .await
            .unwrap();
        let result = ledger
            .split_entry(entry.id, vec![(dec!(-300), None), (dec!(-125), None)])
            .await;
        assert_eq!(result, Err(LedgerError::AlreadyVoided));
    }

    #[tokio::test]
    async fn a_new_pot_starts_at_zero() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let pot = ledger
            .open_pot("Emergency fund", eur, Some(dec!(2000)), Some(1))
            .await
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(0)));
    }

    #[tokio::test]
    async fn allocating_up_to_exactly_general_savings_succeeds() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(100))
            .await
            .unwrap();
        let pot = ledger
            .open_pot("Emergency fund", eur.clone(), None, None)
            .await
            .unwrap();
        ledger
            .allocate_to_pot(pot.id, dec!(100), a_date())
            .await
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(100)));
        assert_eq!(ledger.general_savings(&eur).await, Ok(dec!(0)));
    }

    #[tokio::test]
    async fn allocating_one_cent_more_than_general_savings_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(100))
            .await
            .unwrap();
        let pot = ledger
            .open_pot("Emergency fund", eur.clone(), None, None)
            .await
            .unwrap();
        let result = ledger
            .allocate_to_pot(pot.id, dec!(100.01), a_date())
            .await;
        assert_eq!(
            result,
            Err(LedgerError::GeneralSavingsWouldGoNegative(eur))
        );
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(0)));
    }

    #[tokio::test]
    async fn unallocating_more_than_a_pot_holds_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(100))
            .await
            .unwrap();
        let pot = ledger
            .open_pot("Emergency fund", eur, None, None)
            .await
            .unwrap();
        ledger
            .allocate_to_pot(pot.id, dec!(50), a_date())
            .await
            .unwrap();
        let result = ledger.allocate_to_pot(pot.id, dec!(-60), a_date()).await;
        assert_eq!(result, Err(LedgerError::PotWouldGoNegative));
    }

    #[tokio::test]
    async fn unallocating_moves_money_back_to_general_savings() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(100))
            .await
            .unwrap();
        let pot = ledger
            .open_pot("Emergency fund", eur.clone(), None, None)
            .await
            .unwrap();
        ledger
            .allocate_to_pot(pot.id, dec!(100), a_date())
            .await
            .unwrap();
        ledger
            .allocate_to_pot(pot.id, dec!(-40), a_date())
            .await
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(60)));
        assert_eq!(ledger.general_savings(&eur).await, Ok(dec!(40)));
    }

    #[tokio::test]
    async fn deleting_an_allocated_pot_returns_its_balance_to_general_savings() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(100))
            .await
            .unwrap();
        let pot = ledger
            .open_pot("Emergency fund", eur.clone(), None, None)
            .await
            .unwrap();
        ledger
            .allocate_to_pot(pot.id, dec!(60), a_date())
            .await
            .unwrap();
        assert_eq!(ledger.general_savings(&eur).await, Ok(dec!(40)));

        ledger.delete_pot(pot.id).await.unwrap();

        assert_eq!(ledger.general_savings(&eur).await, Ok(dec!(100)));
        assert_eq!(ledger.pot(pot.id).await, Ok(None));
    }

    #[tokio::test]
    async fn deleting_a_pot_untags_its_entries_without_touching_them_otherwise() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let (checking, pot, income) = a_pot_holding_twenty(&mut ledger).await;

        ledger.delete_pot(pot.id).await.unwrap();

        let reloaded = ledger
            .entries(checking.id)
            .await
            .unwrap()
            .into_iter()
            .find(|e| e.id == income.id)
            .unwrap();
        assert_eq!(reloaded.pot_id, None);
        assert_eq!(reloaded.amount, dec!(50));
        assert_eq!(reloaded.description, "Sold something");
    }

    #[tokio::test]
    async fn deleting_an_unknown_pot_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let unknown = PotId::generate();
        assert_eq!(
            ledger.delete_pot(unknown).await,
            Err(LedgerError::PotNotFound(unknown))
        );
    }

    #[tokio::test]
    async fn general_savings_only_counts_own_accounts_not_outside_or_person_accounts() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(100))
            .await
            .unwrap();
        ledger
            .open_account("A friend", eur.clone(), AccountKind::Person, dec!(500))
            .await
            .unwrap();
        assert_eq!(ledger.general_savings(&eur).await, Ok(dec!(100)));
    }

    #[tokio::test]
    async fn tagging_an_expense_to_a_pot_draws_the_pot_down_and_leaves_general_savings_alone() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000))
            .await
            .unwrap();
        let pot = ledger
            .open_pot("Camera", eur.clone(), None, None)
            .await
            .unwrap();
        ledger
            .allocate_to_pot(pot.id, dec!(900), a_date())
            .await
            .unwrap();
        assert_eq!(ledger.general_savings(&eur).await, Ok(dec!(100)));

        let purchase = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-900), "Camera shop")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(
                purchase.id,
                EntryMetadata {
                    category: Some("Electronics".to_string()),
                    ..tagged_to(&pot)
                },
            )
            .await
            .unwrap();

        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(0)));
        assert_eq!(ledger.general_savings(&eur).await, Ok(dec!(100)));
    }

    #[tokio::test]
    async fn tagging_an_expense_that_would_push_the_pot_negative_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000))
            .await
            .unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).await.unwrap();
        ledger
            .allocate_to_pot(pot.id, dec!(500), a_date())
            .await
            .unwrap();
        let purchase = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-900), "Camera shop")
            .await
            .unwrap();
        let result = ledger
            .update_entry_metadata(purchase.id, tagged_to(&pot))
            .await;
        assert_eq!(result, Err(LedgerError::PotWouldGoNegative));
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(500)));
    }

    #[tokio::test]
    async fn moving_a_tag_off_a_pot_that_would_go_negative_without_it_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000))
            .await
            .unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).await.unwrap();
        let income = ledger
            .record_manual_entry(checking.id, a_date(), dec!(50), "Sold something")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(income.id, tagged_to(&pot))
            .await
            .unwrap();
        let expense = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-30), "Camera strap")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(expense.id, tagged_to(&pot))
            .await
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(20)));

        // Untagging the +50 income would leave the pot at -30.
        let result = ledger
            .update_entry_metadata(income.id, EntryMetadata::default())
            .await;
        assert_eq!(result, Err(LedgerError::PotWouldGoNegative));
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(20)));
    }

    #[tokio::test]
    async fn category_tags_and_note_can_be_changed_on_a_locked_imported_entry() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-20), "Wolt")
            .await
            .unwrap();
        ledger.confirm_entry(entry.id).await.unwrap();
        // The amount is locked now, but its metadata is not.
        assert_eq!(
            ledger.edit_manual_entry_amount(entry.id, dec!(-1)).await,
            Err(LedgerError::EntryLocked)
        );
        let updated = ledger
            .update_entry_metadata(
                entry.id,
                EntryMetadata {
                    category: Some("Food delivery".to_string()),
                    tags: vec!["late night".to_string()],
                    note: Some("forgot to cook".to_string()),
                    pot_id: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.category, Some("Food delivery".to_string()));
        assert_eq!(updated.tags, vec!["late night".to_string()]);
        assert_eq!(updated.note, Some("forgot to cook".to_string()));
    }

    #[tokio::test]
    async fn a_voided_entry_cannot_have_its_metadata_changed() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-20), "x")
            .await
            .unwrap();
        ledger
            .void_entry(entry.id, "wrong amount")
            .await
            .unwrap();
        let result = ledger
            .update_entry_metadata(
                entry.id,
                EntryMetadata {
                    category: Some("Groceries".to_string()),
                    ..EntryMetadata::default()
                },
            )
            .await;
        assert_eq!(result, Err(LedgerError::AlreadyVoided));
    }

    #[tokio::test]
    async fn tagging_an_entry_to_a_pot_in_a_different_currency_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let usd = Currency::new("USD").unwrap();
        let card = ledger
            .open_account("Card", usd, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).await.unwrap();
        let entry = ledger
            .record_manual_entry(card.id, a_date(), dec!(-50), "x")
            .await
            .unwrap();
        let result = ledger
            .update_entry_metadata(entry.id, tagged_to(&pot))
            .await;
        assert_eq!(result, Err(LedgerError::PotCurrencyMismatch));
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(0)));
    }

    #[tokio::test]
    async fn editing_the_amount_of_a_pot_tagged_entry_that_would_push_the_pot_negative_is_refused()
    {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000))
            .await
            .unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).await.unwrap();
        ledger
            .allocate_to_pot(pot.id, dec!(500), a_date())
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-100), "Lens")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(entry.id, tagged_to(&pot))
            .await
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(400)));

        let result = ledger
            .edit_manual_entry_amount(entry.id, dec!(-600))
            .await;
        assert_eq!(result, Err(LedgerError::PotWouldGoNegative));
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(400)));
    }

    #[tokio::test]
    async fn editing_the_amount_of_a_pot_tagged_entry_within_the_pots_means_succeeds() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(1000))
            .await
            .unwrap();
        let pot = ledger.open_pot("Camera", eur, None, None).await.unwrap();
        ledger
            .allocate_to_pot(pot.id, dec!(500), a_date())
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(checking.id, a_date(), dec!(-100), "Lens")
            .await
            .unwrap();
        ledger
            .update_entry_metadata(entry.id, tagged_to(&pot))
            .await
            .unwrap();
        ledger
            .edit_manual_entry_amount(entry.id, dec!(-150))
            .await
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(350)));
    }

    #[tokio::test]
    async fn accounts_lists_every_open_account() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger
            .open_account("A", eur.clone(), AccountKind::Own, dec!(0))
            .await
            .unwrap();
        ledger
            .open_account("B", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        assert_eq!(ledger.accounts().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn pots_lists_every_open_pot() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_pot("A", eur.clone(), None, None).await.unwrap();
        ledger.open_pot("B", eur, None, None).await.unwrap();
        assert_eq!(ledger.pots().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn account_finds_a_saved_account_by_id_and_none_for_an_unknown_one() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        assert_eq!(ledger.account(a.id).await, Ok(Some(a)));
        assert_eq!(ledger.account(AccountId::generate()).await, Ok(None));
    }

    #[tokio::test]
    async fn pot_finds_a_saved_pot_by_id_and_none_for_an_unknown_one() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let p = ledger.open_pot("Camera", eur, None, None).await.unwrap();
        assert_eq!(ledger.pot(p.id).await, Ok(Some(p)));
        assert_eq!(ledger.pot(PotId::generate()).await, Ok(None));
    }

    #[tokio::test]
    async fn entries_lists_every_entry_for_an_account() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-10), "x")
            .await
            .unwrap();
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-5), "y")
            .await
            .unwrap();
        assert_eq!(ledger.entries(account.id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_same_currency_transfer_moves_the_same_amount_both_ways() {
        let (mut ledger, a, b) = two_euro_accounts().await;
        ledger
            .transfer(a_transfer(leg(&a, dec!(40)), leg(&b, dec!(40)), "move"))
            .await
            .unwrap();
        assert_eq!(ledger.account_balance(a.id).await, Ok(dec!(60)));
        assert_eq!(ledger.account_balance(b.id).await, Ok(dec!(40)));
    }

    #[tokio::test]
    async fn a_same_currency_transfer_with_mismatched_amounts_is_refused() {
        let (mut ledger, a, b) = two_euro_accounts().await;
        let result = ledger
            .transfer(a_transfer(leg(&a, dec!(40)), leg(&b, dec!(35)), "move"))
            .await;
        assert_eq!(result, Err(LedgerError::CrossCurrencyAmountRequired));
    }

    #[tokio::test]
    async fn a_transfer_with_a_negative_amount_is_refused() {
        let (mut ledger, a, b) = two_euro_accounts().await;
        let result = ledger
            .transfer(a_transfer(
                leg(&a, dec!(-40)),
                leg(&b, dec!(-40)),
                "move",
            ))
            .await;
        assert_eq!(result, Err(LedgerError::TransferAmountMustBePositive));
        assert_eq!(ledger.account_balance(a.id).await, Ok(dec!(100)));
        assert_eq!(ledger.account_balance(b.id).await, Ok(dec!(0)));
    }

    #[tokio::test]
    async fn a_transfer_of_zero_is_refused() {
        let (mut ledger, a, b) = two_euro_accounts().await;
        let result = ledger
            .transfer(a_transfer(leg(&a, dec!(0)), leg(&b, dec!(0)), "move"))
            .await;
        assert_eq!(result, Err(LedgerError::TransferAmountMustBePositive));
    }

    #[tokio::test]
    async fn a_transfer_from_an_account_to_itself_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger
            .open_account("A", eur, AccountKind::Own, dec!(100))
            .await
            .unwrap();
        let result = ledger
            .transfer(a_transfer(leg(&a, dec!(40)), leg(&a, dec!(40)), "move"))
            .await;
        assert_eq!(result, Err(LedgerError::TransferToSelfNotAllowed));
        assert!(ledger.entries(a.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_cross_currency_transfer_needs_an_explicit_amount_received() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let ngn = Currency::new("NGN").unwrap();
        let a = ledger
            .open_account("A", eur, AccountKind::Own, dec!(200))
            .await
            .unwrap();
        let b = ledger
            .open_account("B", ngn, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        ledger
            .transfer(a_transfer(
                leg(&a, dec!(200)),
                leg(&b, dec!(370000)),
                "move",
            ))
            .await
            .unwrap();
        assert_eq!(ledger.account_balance(a.id).await, Ok(dec!(0)));
        assert_eq!(ledger.account_balance(b.id).await, Ok(dec!(370000)));
    }

    #[tokio::test]
    async fn a_cross_currency_transfer_given_the_same_amount_on_both_sides_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let ngn = Currency::new("NGN").unwrap();
        let a = ledger
            .open_account("A", eur, AccountKind::Own, dec!(200))
            .await
            .unwrap();
        let b = ledger
            .open_account("B", ngn, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let result = ledger
            .transfer(a_transfer(leg(&a, dec!(200)), leg(&b, dec!(200)), "move"))
            .await;
        assert_eq!(result, Err(LedgerError::CrossCurrencyAmountRequired));
        assert_eq!(ledger.account_balance(a.id).await, Ok(dec!(200)));
        assert_eq!(ledger.account_balance(b.id).await, Ok(dec!(0)));
    }

    #[tokio::test]
    async fn voiding_one_side_of_a_transfer_leaves_the_other_side_untouched() {
        let (mut ledger, a, b) = two_euro_accounts().await;
        let (out_entry, in_entry) = ledger
            .transfer(a_transfer(leg(&a, dec!(40)), leg(&b, dec!(40)), "move"))
            .await
            .unwrap();
        ledger
            .void_entry(out_entry.id, "recorded against the wrong account")
            .await
            .unwrap();
        assert_eq!(ledger.account_balance(a.id).await, Ok(dec!(100)));
        assert_eq!(ledger.account_balance(b.id).await, Ok(dec!(40)));
        assert!(
            !ledger
                .store
                .get_entry(in_entry.id)
                .await
                .unwrap()
                .unwrap()
                .is_voided()
        );
    }

    #[tokio::test]
    async fn lending_money_is_a_transfer_to_a_person_account_and_can_make_it_negative() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(500))
            .await
            .unwrap();
        let friend = ledger
            .open_account("A friend", eur, AccountKind::Person, dec!(0))
            .await
            .unwrap();
        ledger
            .transfer(a_transfer(
                leg(&checking, dec!(100)),
                leg(&friend, dec!(100)),
                "loan",
            ))
            .await
            .unwrap();
        assert_eq!(ledger.account_balance(friend.id).await, Ok(dec!(100)));
        ledger
            .transfer(a_transfer(
                leg(&friend, dec!(30)),
                leg(&checking, dec!(30)),
                "repaid",
            ))
            .await
            .unwrap();
        assert_eq!(ledger.account_balance(friend.id).await, Ok(dec!(70)));
    }

    #[tokio::test]
    async fn an_own_account_can_go_negative() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(50))
            .await
            .unwrap();
        let other = ledger
            .open_account("Other", eur, AccountKind::Own, dec!(0))
            .await
            .unwrap();
        ledger
            .transfer(a_transfer(
                leg(&checking, dec!(80)),
                leg(&other, dec!(80)),
                "overdraw",
            ))
            .await
            .unwrap();
        assert_eq!(
            ledger.account_balance(checking.id).await,
            Ok(dec!(-30))
        );
    }

    #[tokio::test]
    async fn an_investment_created_with_an_opening_value_and_no_transaction_has_that_value() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let etf = ledger
            .open_account("An ETF position", eur, AccountKind::Investment, dec!(1000))
            .await
            .unwrap();
        assert_eq!(ledger.current_value(etf.id).await, Ok(dec!(1000)));
    }

    #[tokio::test]
    async fn updating_the_current_value_records_a_valuation_and_changes_current_value() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let etf = ledger
            .open_account("An ETF position", eur, AccountKind::Investment, dec!(1000))
            .await
            .unwrap();
        let valuation = ledger
            .update_current_value(etf.id, dec!(1042), "Investment gain", a_date())
            .await
            .unwrap();
        assert_eq!(valuation.old_value, dec!(1000));
        assert_eq!(valuation.new_value, dec!(1042));
        assert_eq!(valuation.gain(), dec!(42));
        assert_eq!(ledger.current_value(etf.id).await, Ok(dec!(1042)));
    }

    #[tokio::test]
    async fn a_person_account_current_value_above_cash_lent_is_recorded_as_interest() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let checking = ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(500))
            .await
            .unwrap();
        let friend = ledger
            .open_account("A friend", eur, AccountKind::Person, dec!(0))
            .await
            .unwrap();
        ledger
            .transfer(a_transfer(
                leg(&checking, dec!(100)),
                leg(&friend, dec!(100)),
                "loan",
            ))
            .await
            .unwrap();
        let valuation = ledger
            .update_current_value(friend.id, dec!(110), "Loan interest", a_date())
            .await
            .unwrap();
        assert_eq!(valuation.gain(), dec!(10));
        assert_eq!(ledger.current_value(friend.id).await, Ok(dec!(110)));
    }

    #[tokio::test]
    async fn updating_the_current_value_twice_measures_the_gain_from_the_last_update() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let etf = ledger
            .open_account("An ETF position", eur, AccountKind::Investment, dec!(1000))
            .await
            .unwrap();
        ledger
            .update_current_value(etf.id, dec!(1100), "Investment gain", a_date())
            .await
            .unwrap();
        let second = ledger
            .update_current_value(etf.id, dec!(1080), "Investment gain", a_date())
            .await
            .unwrap();
        assert_eq!(second.old_value, dec!(1100));
        assert_eq!(second.gain(), dec!(-20));
    }
}
