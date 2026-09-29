use crate::account::Account;
use crate::entry::{Entry, EntryPart};
use crate::error::LedgerError;
use crate::id::{AccountId, AllocationId, EntryId, ImportId, MatchId, PotId, QueueRowId};
use crate::import::{Import, ImportQueueRow, ImportQueueRowMatch, MatchTarget};
use crate::pot::{Allocation, Pot};
use crate::valuation::Valuation;
use std::collections::HashMap;

pub trait LedgerStore {
    fn save_account(&mut self, account: Account) -> Result<(), LedgerError>;
    fn get_account(&self, id: AccountId) -> Result<Option<Account>, LedgerError>;
    fn all_accounts(&self) -> Result<Vec<Account>, LedgerError>;

    fn save_entry(&mut self, entry: Entry) -> Result<(), LedgerError>;
    fn get_entry(&self, id: EntryId) -> Result<Option<Entry>, LedgerError>;
    fn entries_for_account(&self, account_id: AccountId) -> Result<Vec<Entry>, LedgerError>;
    fn entries_for_pot(&self, pot_id: PotId) -> Result<Vec<Entry>, LedgerError>;

    fn save_entry_parts(
        &mut self,
        entry_id: EntryId,
        parts: Vec<EntryPart>,
    ) -> Result<(), LedgerError>;
    fn parts_for_entry(&self, entry_id: EntryId) -> Result<Vec<EntryPart>, LedgerError>;

    fn save_pot(&mut self, pot: Pot) -> Result<(), LedgerError>;
    fn get_pot(&self, id: PotId) -> Result<Option<Pot>, LedgerError>;
    fn all_pots(&self) -> Result<Vec<Pot>, LedgerError>;
    fn delete_pot(&mut self, id: PotId) -> Result<(), LedgerError>;
    fn save_allocation(&mut self, allocation: Allocation) -> Result<(), LedgerError>;
    fn allocations_for_pot(&self, pot_id: PotId) -> Result<Vec<Allocation>, LedgerError>;
    fn delete_allocations_for_pot(&mut self, pot_id: PotId) -> Result<(), LedgerError>;

    fn save_valuation(&mut self, valuation: Valuation) -> Result<(), LedgerError>;
    fn valuations_for_account(&self, account_id: AccountId) -> Result<Vec<Valuation>, LedgerError>;

    fn save_import(&mut self, import: Import) -> Result<(), LedgerError>;
    fn get_import(&self, id: ImportId) -> Result<Option<Import>, LedgerError>;
    /// Every import, oldest upload first.
    fn all_imports(&self) -> Result<Vec<Import>, LedgerError>;
    fn delete_import(&mut self, id: ImportId) -> Result<(), LedgerError>;
    fn incomplete_import_for_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<Import>, LedgerError>;

    fn save_queue_row(&mut self, row: ImportQueueRow) -> Result<(), LedgerError>;
    fn get_queue_row(&self, id: QueueRowId) -> Result<Option<ImportQueueRow>, LedgerError>;
    /// One import's queue rows, ordered by date then time ascending (a row
    /// with no time sorts before timed rows on the same date).
    fn queue_rows_for_import(
        &self,
        import_id: ImportId,
    ) -> Result<Vec<ImportQueueRow>, LedgerError>;
    fn delete_queue_row(&mut self, id: QueueRowId) -> Result<(), LedgerError>;

    fn save_queue_row_match(&mut self, m: ImportQueueRowMatch) -> Result<(), LedgerError>;
    fn get_queue_row_match(&self, id: MatchId) -> Result<Option<ImportQueueRowMatch>, LedgerError>;
    /// Every match row with `queue_row_id` or `matched_queue_row_id` equal
    /// to this queue row id.
    fn queue_row_matches_referencing(
        &self,
        queue_row_id: QueueRowId,
    ) -> Result<Vec<ImportQueueRowMatch>, LedgerError>;
    /// Every match row whose `queue_row_id` is one of this import's queue
    /// rows, fetched in one query so a whole queue can be read at once.
    fn queue_row_matches_for_import(
        &self,
        import_id: ImportId,
    ) -> Result<Vec<ImportQueueRowMatch>, LedgerError>;
    fn delete_queue_row_match(&mut self, id: MatchId) -> Result<(), LedgerError>;
    /// On every match row whose `matched_queue_row_id` is `from_queue_row_id`,
    /// clears it and sets `matched_entry_id` to `to_entry_id` instead.
    fn repoint_queue_row_matches(
        &mut self,
        from_queue_row_id: QueueRowId,
        to_entry_id: EntryId,
    ) -> Result<(), LedgerError>;

    /// Runs `f` as one atomic unit: SqliteStore commits only if `f` returns
    /// Ok, and rolls back everything `f` did otherwise, so a mutation that
    /// needs more than one save (a transfer's two entries, a valuation plus
    /// the account it updates) can't leave the store half-written.
    fn transaction<F, T>(&mut self, f: F) -> Result<T, LedgerError>
    where
        F: FnOnce(&mut Self) -> Result<T, LedgerError>,
        Self: Sized;
}

#[derive(Default)]
pub struct InMemoryStore {
    accounts: HashMap<AccountId, Account>,
    entries: HashMap<EntryId, Entry>,
    entry_parts: HashMap<EntryId, Vec<EntryPart>>,
    pots: HashMap<PotId, Pot>,
    allocations: HashMap<AllocationId, Allocation>,
    valuations: HashMap<AccountId, Vec<Valuation>>,
    imports: HashMap<ImportId, Import>,
    queue_rows: HashMap<QueueRowId, ImportQueueRow>,
    queue_row_matches: HashMap<MatchId, ImportQueueRowMatch>,
}

// A HashMap operation cannot fail, so every method here returns Ok. The
// Result in the trait exists for SqliteStore, which does real I/O; this
// implementation just satisfies the same interface.
impl LedgerStore for InMemoryStore {
    fn save_account(&mut self, account: Account) -> Result<(), LedgerError> {
        self.accounts.insert(account.id, account);
        Ok(())
    }

    fn get_account(&self, id: AccountId) -> Result<Option<Account>, LedgerError> {
        Ok(self.accounts.get(&id).cloned())
    }

    fn all_accounts(&self) -> Result<Vec<Account>, LedgerError> {
        Ok(self.accounts.values().cloned().collect())
    }

    fn save_entry(&mut self, entry: Entry) -> Result<(), LedgerError> {
        self.entries.insert(entry.id, entry);
        Ok(())
    }

    fn get_entry(&self, id: EntryId) -> Result<Option<Entry>, LedgerError> {
        Ok(self.entries.get(&id).cloned())
    }

    fn entries_for_account(&self, account_id: AccountId) -> Result<Vec<Entry>, LedgerError> {
        Ok(self
            .entries
            .values()
            .filter(|e| e.account_id == account_id)
            .cloned()
            .collect())
    }

    fn entries_for_pot(&self, pot_id: PotId) -> Result<Vec<Entry>, LedgerError> {
        Ok(self
            .entries
            .values()
            .filter(|e| e.pot_id == Some(pot_id))
            .cloned()
            .collect())
    }

    fn save_entry_parts(
        &mut self,
        entry_id: EntryId,
        parts: Vec<EntryPart>,
    ) -> Result<(), LedgerError> {
        self.entry_parts.insert(entry_id, parts);
        Ok(())
    }

    fn parts_for_entry(&self, entry_id: EntryId) -> Result<Vec<EntryPart>, LedgerError> {
        Ok(self.entry_parts.get(&entry_id).cloned().unwrap_or_default())
    }

    fn save_pot(&mut self, pot: Pot) -> Result<(), LedgerError> {
        self.pots.insert(pot.id, pot);
        Ok(())
    }

    fn get_pot(&self, id: PotId) -> Result<Option<Pot>, LedgerError> {
        Ok(self.pots.get(&id).cloned())
    }

    fn all_pots(&self) -> Result<Vec<Pot>, LedgerError> {
        Ok(self.pots.values().cloned().collect())
    }

    fn delete_pot(&mut self, id: PotId) -> Result<(), LedgerError> {
        self.pots.remove(&id);
        Ok(())
    }

    fn save_allocation(&mut self, allocation: Allocation) -> Result<(), LedgerError> {
        self.allocations.insert(allocation.id, allocation);
        Ok(())
    }

    fn allocations_for_pot(&self, pot_id: PotId) -> Result<Vec<Allocation>, LedgerError> {
        Ok(self
            .allocations
            .values()
            .filter(|a| a.pot_id == pot_id)
            .cloned()
            .collect())
    }

    fn delete_allocations_for_pot(&mut self, pot_id: PotId) -> Result<(), LedgerError> {
        self.allocations.retain(|_, a| a.pot_id != pot_id);
        Ok(())
    }

    fn save_valuation(&mut self, valuation: Valuation) -> Result<(), LedgerError> {
        self.valuations
            .entry(valuation.account_id)
            .or_default()
            .push(valuation);
        Ok(())
    }

    fn valuations_for_account(&self, account_id: AccountId) -> Result<Vec<Valuation>, LedgerError> {
        Ok(self
            .valuations
            .get(&account_id)
            .cloned()
            .unwrap_or_default())
    }

    fn save_import(&mut self, import: Import) -> Result<(), LedgerError> {
        self.imports.insert(import.id, import);
        Ok(())
    }

    fn get_import(&self, id: ImportId) -> Result<Option<Import>, LedgerError> {
        Ok(self.imports.get(&id).cloned())
    }

    fn all_imports(&self) -> Result<Vec<Import>, LedgerError> {
        let mut imports: Vec<Import> = self.imports.values().cloned().collect();
        imports.sort_by_key(|i| i.uploaded_at);
        Ok(imports)
    }

    fn delete_import(&mut self, id: ImportId) -> Result<(), LedgerError> {
        self.imports.remove(&id);
        Ok(())
    }

    fn incomplete_import_for_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<Import>, LedgerError> {
        Ok(self
            .imports
            .values()
            .find(|i| i.account_id == account_id && !i.completed)
            .cloned())
    }

    fn save_queue_row(&mut self, row: ImportQueueRow) -> Result<(), LedgerError> {
        self.queue_rows.insert(row.id, row);
        Ok(())
    }

    fn get_queue_row(&self, id: QueueRowId) -> Result<Option<ImportQueueRow>, LedgerError> {
        Ok(self.queue_rows.get(&id).cloned())
    }

    fn queue_rows_for_import(
        &self,
        import_id: ImportId,
    ) -> Result<Vec<ImportQueueRow>, LedgerError> {
        let mut rows: Vec<ImportQueueRow> = self
            .queue_rows
            .values()
            .filter(|r| r.import_id == import_id)
            .cloned()
            .collect();
        rows.sort_by_key(|r| (r.date, r.time));
        Ok(rows)
    }

    fn delete_queue_row(&mut self, id: QueueRowId) -> Result<(), LedgerError> {
        self.queue_rows.remove(&id);
        Ok(())
    }

    fn save_queue_row_match(&mut self, m: ImportQueueRowMatch) -> Result<(), LedgerError> {
        self.queue_row_matches.insert(m.id, m);
        Ok(())
    }

    fn get_queue_row_match(&self, id: MatchId) -> Result<Option<ImportQueueRowMatch>, LedgerError> {
        Ok(self.queue_row_matches.get(&id).cloned())
    }

    fn queue_row_matches_referencing(
        &self,
        queue_row_id: QueueRowId,
    ) -> Result<Vec<ImportQueueRowMatch>, LedgerError> {
        Ok(self
            .queue_row_matches
            .values()
            .filter(|m| {
                m.queue_row_id == queue_row_id
                    || matches!(m.target, MatchTarget::QueueRow { queue_row_id: id } if id == queue_row_id)
            })
            .cloned()
            .collect())
    }

    fn queue_row_matches_for_import(
        &self,
        import_id: ImportId,
    ) -> Result<Vec<ImportQueueRowMatch>, LedgerError> {
        Ok(self
            .queue_row_matches
            .values()
            .filter(|m| {
                self.queue_rows
                    .get(&m.queue_row_id)
                    .is_some_and(|row| row.import_id == import_id)
            })
            .cloned()
            .collect())
    }

    fn delete_queue_row_match(&mut self, id: MatchId) -> Result<(), LedgerError> {
        self.queue_row_matches.remove(&id);
        Ok(())
    }

    fn repoint_queue_row_matches(
        &mut self,
        from_queue_row_id: QueueRowId,
        to_entry_id: EntryId,
    ) -> Result<(), LedgerError> {
        for m in self.queue_row_matches.values_mut() {
            if matches!(m.target, MatchTarget::QueueRow { queue_row_id: id } if id == from_queue_row_id)
            {
                m.target = MatchTarget::Entry {
                    entry_id: to_entry_id,
                };
            }
        }
        Ok(())
    }

    fn transaction<F, T>(&mut self, f: F) -> Result<T, LedgerError>
    where
        F: FnOnce(&mut Self) -> Result<T, LedgerError>,
    {
        f(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountKind;
    use crate::currency::Currency;
    use rust_decimal_macros::dec;

    use crate::store::import_contract as contract;

    #[test]
    fn an_import_round_trips() {
        contract::an_import_round_trips(InMemoryStore::default());
    }

    #[test]
    fn resaving_an_import_updates_it() {
        contract::resaving_an_import_updates_it(InMemoryStore::default());
    }

    #[test]
    fn incomplete_import_for_account_ignores_completed_and_other_accounts() {
        contract::incomplete_import_for_account_ignores_completed_and_other_accounts(
            InMemoryStore::default(),
        );
    }

    #[test]
    fn a_queue_row_round_trips() {
        contract::a_queue_row_round_trips(InMemoryStore::default());
    }

    #[test]
    fn queue_rows_come_back_for_their_import_by_date_then_time() {
        contract::queue_rows_come_back_for_their_import_by_date_then_time(InMemoryStore::default());
    }

    #[test]
    fn matches_referencing_a_row_come_from_either_side() {
        contract::matches_referencing_a_row_come_from_either_side(InMemoryStore::default());
    }

    #[test]
    fn matches_for_an_import_are_those_of_its_own_rows() {
        contract::matches_for_an_import_are_those_of_its_own_rows(InMemoryStore::default());
    }

    #[test]
    fn repointing_rewrites_only_rows_that_pointed_at_the_queue_row() {
        contract::repointing_rewrites_only_rows_that_pointed_at_the_queue_row(
            InMemoryStore::default(),
        );
    }

    #[test]
    fn saved_account_can_be_read_back_by_id() {
        let mut store = InMemoryStore::default();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        let id = account.id;
        store.save_account(account.clone()).unwrap();
        assert_eq!(store.get_account(id), Ok(Some(account)));
    }

    #[test]
    fn unknown_id_returns_none() {
        let store = InMemoryStore::default();
        assert_eq!(store.get_account(AccountId::generate()), Ok(None));
    }

    #[test]
    fn all_accounts_lists_every_saved_account() {
        let mut store = InMemoryStore::default();
        let eur = Currency::new("EUR").unwrap();
        store
            .save_account(Account::new("A", eur.clone(), AccountKind::Own, dec!(0)))
            .unwrap();
        store
            .save_account(Account::new("B", eur, AccountKind::Own, dec!(0)))
            .unwrap();
        assert_eq!(store.all_accounts().unwrap().len(), 2);
    }
}

/// The import tables' store contract, written once and run against both
/// InMemoryStore (below) and SqliteStore (in sqlite_store.rs), so the two
/// implementations can't drift apart on ordering or match-row semantics.
#[cfg(test)]
pub(crate) mod import_contract {
    use super::LedgerStore;
    use crate::account::{Account, AccountKind};
    use crate::currency::Currency;
    use crate::entry::{BankState, Entry, EntrySource};
    use crate::id::{AccountId, EntryId, ImportId, QueueRowId};
    use crate::import::{Import, ImportQueueRow, ImportQueueRowMatch, NormalDetail, RowDetail};
    use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
    use rust_decimal_macros::dec;

    fn an_account<S: LedgerStore>(store: &mut S) -> AccountId {
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).unwrap();
        account.id
    }

    fn a_saved_import<S: LedgerStore>(store: &mut S) -> ImportId {
        let account_id = an_account(store);
        let import = an_import(account_id);
        store.save_import(import.clone()).unwrap();
        import.id
    }

    fn saved_rows<S: LedgerStore>(store: &mut S, n: usize) -> Vec<QueueRowId> {
        let import_id = a_saved_import(store);
        (0..n)
            .map(|_| {
                let row = a_row(import_id, 1, None);
                store.save_queue_row(row.clone()).unwrap();
                row.id
            })
            .collect()
    }

    fn a_saved_entry<S: LedgerStore>(store: &mut S) -> EntryId {
        let account_id = an_account(store);
        let entry = Entry {
            id: EntryId::generate(),
            account_id,
            date: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            time: None,
            amount: dec!(-1),
            currency: Currency::new("EUR").unwrap(),
            description: "x".to_string(),
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
        store.save_entry(entry.clone()).unwrap();
        entry.id
    }

    pub fn an_import(account_id: AccountId) -> Import {
        Import {
            id: ImportId::generate(),
            account_id,
            currency: Currency::new("EUR").unwrap(),
            file_name: "statement.csv".to_string(),
            uploaded_at: Utc.with_ymd_and_hms(2026, 9, 27, 10, 30, 0).unwrap(),
            rows_read: 3,
            opening_balance: dec!(100.50),
            closing_balance: dec!(-20.25),
            completed: false,
        }
    }

    pub fn a_row(import_id: ImportId, day: u32, time: Option<(u32, u32, u32)>) -> ImportQueueRow {
        ImportQueueRow {
            id: QueueRowId::generate(),
            import_id,
            date: NaiveDate::from_ymd_opt(2026, 3, day).unwrap(),
            time: time.map(|(h, m, s)| NaiveTime::from_hms_opt(h, m, s).unwrap()),
            amount: dec!(-12.34),
            currency: Currency::new("EUR").unwrap(),
            detail: RowDetail::Normal(NormalDetail {
                description: "Coffee, cake".to_string(),
                bank_state: BankState::Pending,
                category: None,
            }),
        }
    }

    pub fn an_import_round_trips<S: LedgerStore>(mut store: S) {
        let import = an_import(an_account(&mut store));
        store.save_import(import.clone()).unwrap();
        assert_eq!(store.get_import(import.id), Ok(Some(import.clone())));
        assert_eq!(store.get_import(ImportId::generate()), Ok(None));
        assert_eq!(store.all_imports(), Ok(vec![import.clone()]));
        store.delete_import(import.id).unwrap();
        assert_eq!(store.get_import(import.id), Ok(None));
    }

    pub fn resaving_an_import_updates_it<S: LedgerStore>(mut store: S) {
        let mut import = an_import(an_account(&mut store));
        store.save_import(import.clone()).unwrap();
        import.completed = true;
        store.save_import(import.clone()).unwrap();
        assert_eq!(store.all_imports(), Ok(vec![import]));
    }

    pub fn incomplete_import_for_account_ignores_completed_and_other_accounts<S: LedgerStore>(
        mut store: S,
    ) {
        let account = an_account(&mut store);
        let mut done = an_import(account);
        done.completed = true;
        store.save_import(done).unwrap();
        let other_account = an_account(&mut store);
        store.save_import(an_import(other_account)).unwrap();
        assert_eq!(store.incomplete_import_for_account(account), Ok(None));
        let open = an_import(account);
        store.save_import(open.clone()).unwrap();
        assert_eq!(store.incomplete_import_for_account(account), Ok(Some(open)));
    }

    pub fn a_queue_row_round_trips<S: LedgerStore>(mut store: S) {
        let import_id = a_saved_import(&mut store);
        let mut row = a_row(import_id, 5, Some((23, 59, 1)));
        row.detail = RowDetail::Normal(NormalDetail {
            description: "Coffee, cake".to_string(),
            bank_state: BankState::Pending,
            category: Some("Food".to_string()),
        });
        store.save_queue_row(row.clone()).unwrap();
        assert_eq!(store.get_queue_row(row.id), Ok(Some(row.clone())));
        row.detail = RowDetail::RevertedCandidate;
        store.save_queue_row(row.clone()).unwrap();
        assert_eq!(store.get_queue_row(row.id), Ok(Some(row.clone())));
        store.delete_queue_row(row.id).unwrap();
        assert_eq!(store.get_queue_row(row.id), Ok(None));
    }

    pub fn queue_rows_come_back_for_their_import_by_date_then_time<S: LedgerStore>(mut store: S) {
        let import_id = a_saved_import(&mut store);
        let late = a_row(import_id, 9, Some((8, 0, 0)));
        let early_later_time = a_row(import_id, 2, Some((18, 0, 0)));
        let early_earlier_time = a_row(import_id, 2, Some((9, 30, 0)));
        let early_no_time = a_row(import_id, 2, None);
        let other_import = a_row(a_saved_import(&mut store), 1, None);
        for r in [
            &late,
            &early_later_time,
            &other_import,
            &early_earlier_time,
            &early_no_time,
        ] {
            store.save_queue_row(r.clone()).unwrap();
        }
        assert_eq!(
            store.queue_rows_for_import(import_id),
            Ok(vec![
                early_no_time,
                early_earlier_time,
                early_later_time,
                late
            ])
        );
    }

    pub fn matches_referencing_a_row_come_from_either_side<S: LedgerStore>(mut store: S) {
        let rows = saved_rows(&mut store, 3);
        let (a, b, c) = (rows[0], rows[1], rows[2]);
        let a_to_b = ImportQueueRowMatch::to_queue_row(a, b);
        let b_to_a = ImportQueueRowMatch::to_queue_row(b, a);
        let a_to_entry = ImportQueueRowMatch::to_entry(a, a_saved_entry(&mut store));
        let c_to_entry = ImportQueueRowMatch::to_entry(c, a_saved_entry(&mut store));
        for m in [&a_to_b, &b_to_a, &a_to_entry, &c_to_entry] {
            store.save_queue_row_match(m.clone()).unwrap();
        }
        let mut found = store.queue_row_matches_referencing(a).unwrap();
        found.sort_by_key(|m| m.id);
        let mut expected = vec![a_to_b, b_to_a.clone(), a_to_entry];
        expected.sort_by_key(|m| m.id);
        assert_eq!(found, expected);
        assert_eq!(
            store.get_queue_row_match(b_to_a.id),
            Ok(Some(b_to_a.clone()))
        );
        store.delete_queue_row_match(b_to_a.id).unwrap();
        assert_eq!(store.get_queue_row_match(b_to_a.id), Ok(None));
    }

    pub fn matches_for_an_import_are_those_of_its_own_rows<S: LedgerStore>(mut store: S) {
        let ours = saved_rows(&mut store, 2);
        let theirs = saved_rows(&mut store, 1);
        let entry = a_saved_entry(&mut store);
        let ours_to_row = ImportQueueRowMatch::to_queue_row(ours[0], ours[1]);
        let ours_to_entry = ImportQueueRowMatch::to_entry(ours[1], entry);
        let theirs_to_entry = ImportQueueRowMatch::to_entry(theirs[0], entry);
        for m in [&ours_to_row, &ours_to_entry, &theirs_to_entry] {
            store.save_queue_row_match(m.clone()).unwrap();
        }
        let import_id = store.get_queue_row(ours[0]).unwrap().unwrap().import_id;
        let mut found = store.queue_row_matches_for_import(import_id).unwrap();
        found.sort_by_key(|m| m.id);
        let mut expected = vec![ours_to_row, ours_to_entry];
        expected.sort_by_key(|m| m.id);
        assert_eq!(found, expected);
    }

    pub fn repointing_rewrites_only_rows_that_pointed_at_the_queue_row<S: LedgerStore>(
        mut store: S,
    ) {
        use crate::import::MatchTarget;
        let rows = saved_rows(&mut store, 2);
        let (a, b) = (rows[0], rows[1]);
        let new_entry = a_saved_entry(&mut store);
        let a_to_b = ImportQueueRowMatch::to_queue_row(a, b);
        let b_to_a = ImportQueueRowMatch::to_queue_row(b, a);
        let b_to_entry = ImportQueueRowMatch::to_entry(b, a_saved_entry(&mut store));
        for m in [&a_to_b, &b_to_a, &b_to_entry] {
            store.save_queue_row_match(m.clone()).unwrap();
        }
        store.repoint_queue_row_matches(a, new_entry).unwrap();
        assert_eq!(
            store.get_queue_row_match(b_to_a.id),
            Ok(Some(ImportQueueRowMatch {
                id: b_to_a.id,
                queue_row_id: b,
                target: MatchTarget::Entry {
                    entry_id: new_entry
                },
            }))
        );
        assert_eq!(store.get_queue_row_match(a_to_b.id), Ok(Some(a_to_b)));
        assert_eq!(
            store.get_queue_row_match(b_to_entry.id),
            Ok(Some(b_to_entry))
        );
    }
}
