use crate::account::Account;
use crate::entry::{Entry, EntryPart};
use crate::error::LedgerError;
use crate::id::{AccountId, AllocationId, EntryId, ImportId, MatchId, PotId, QueueRowId};
use crate::import::{Import, ImportQueueRow, ImportQueueRowMatch, MatchTarget};
use crate::pot::{Allocation, Pot};
use crate::valuation::Valuation;
use std::collections::HashMap;
use std::future::Future;

/// All implementations of `LedgerStore` must be `Send + Sync + 'static`
/// so they can be safely shared across async tasks in a multi-threaded
/// Tokio runtime. The explicit `impl Future + Send` return types ensure
/// every method call produces a `Send` future, satisfying Axum's handler
/// requirements.
pub trait LedgerStore: Send + Sync + 'static {
    fn save_account(
        &mut self,
        account: Account,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn get_account(
        &mut self,
        id: AccountId,
    ) -> impl Future<Output = Result<Option<Account>, LedgerError>> + Send + '_;

    fn all_accounts(&mut self) -> impl Future<Output = Result<Vec<Account>, LedgerError>> + Send + '_;

    fn save_entry(
        &mut self,
        entry: Entry,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn get_entry(
        &mut self,
        id: EntryId,
    ) -> impl Future<Output = Result<Option<Entry>, LedgerError>> + Send + '_;

    fn entries_for_account(
        &mut self,
        account_id: AccountId,
    ) -> impl Future<Output = Result<Vec<Entry>, LedgerError>> + Send + '_;

    fn entries_for_pot(
        &mut self,
        pot_id: PotId,
    ) -> impl Future<Output = Result<Vec<Entry>, LedgerError>> + Send + '_;

    fn save_entry_parts(
        &mut self,
        entry_id: EntryId,
        parts: Vec<EntryPart>,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn parts_for_entry(
        &mut self,
        entry_id: EntryId,
    ) -> impl Future<Output = Result<Vec<EntryPart>, LedgerError>> + Send + '_;

    fn save_pot(
        &mut self,
        pot: Pot,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn get_pot(
        &mut self,
        id: PotId,
    ) -> impl Future<Output = Result<Option<Pot>, LedgerError>> + Send + '_;

    fn all_pots(&mut self) -> impl Future<Output = Result<Vec<Pot>, LedgerError>> + Send + '_;

    fn delete_pot(
        &mut self,
        id: PotId,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn save_allocation(
        &mut self,
        allocation: Allocation,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn allocations_for_pot(
        &mut self,
        pot_id: PotId,
    ) -> impl Future<Output = Result<Vec<Allocation>, LedgerError>> + Send + '_;

    fn delete_allocations_for_pot(
        &mut self,
        pot_id: PotId,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn save_valuation(
        &mut self,
        valuation: Valuation,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn valuations_for_account(
        &mut self,
        account_id: AccountId,
    ) -> impl Future<Output = Result<Vec<Valuation>, LedgerError>> + Send + '_;

    fn save_import(
        &mut self,
        import: Import,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn get_import(
        &mut self,
        id: ImportId,
    ) -> impl Future<Output = Result<Option<Import>, LedgerError>> + Send + '_;

    /// Every import, oldest upload first.
    fn all_imports(&mut self) -> impl Future<Output = Result<Vec<Import>, LedgerError>> + Send + '_;

    fn delete_import(
        &mut self,
        id: ImportId,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn incomplete_import_for_account(
        &mut self,
        account_id: AccountId,
    ) -> impl Future<Output = Result<Option<Import>, LedgerError>> + Send + '_;

    fn save_queue_row(
        &mut self,
        row: ImportQueueRow,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn get_queue_row(
        &mut self,
        id: QueueRowId,
    ) -> impl Future<Output = Result<Option<ImportQueueRow>, LedgerError>> + Send + '_;

    /// One import's queue rows, ordered by date then time ascending (a row
    /// with no time sorts before timed rows on the same date).
    fn queue_rows_for_import(
        &mut self,
        import_id: ImportId,
    ) -> impl Future<Output = Result<Vec<ImportQueueRow>, LedgerError>> + Send + '_;

    fn delete_queue_row(
        &mut self,
        id: QueueRowId,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn save_queue_row_match(
        &mut self,
        m: ImportQueueRowMatch,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    fn get_queue_row_match(
        &mut self,
        id: MatchId,
    ) -> impl Future<Output = Result<Option<ImportQueueRowMatch>, LedgerError>> + Send + '_;

    /// Every match row with `queue_row_id` or `matched_queue_row_id` equal
    /// to this queue row id.
    fn queue_row_matches_referencing(
        &mut self,
        queue_row_id: QueueRowId,
    ) -> impl Future<Output = Result<Vec<ImportQueueRowMatch>, LedgerError>> + Send + '_;

    /// Every match row whose `queue_row_id` is one of this import's queue
    /// rows, fetched in one query so a whole queue can be read at once.
    fn queue_row_matches_for_import(
        &mut self,
        import_id: ImportId,
    ) -> impl Future<Output = Result<Vec<ImportQueueRowMatch>, LedgerError>> + Send + '_;

    fn delete_queue_row_match(
        &mut self,
        id: MatchId,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    /// On every match row whose `matched_queue_row_id` is `from_queue_row_id`,
    /// clears it and sets `matched_entry_id` to `to_entry_id` instead.
    fn repoint_queue_row_matches(
        &mut self,
        from_queue_row_id: QueueRowId,
        to_entry_id: EntryId,
    ) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    /// Begins an atomic unit of work. Every write made after this call and
    /// before `commit` or `rollback` is part of the same transaction.
    /// For `InMemoryStore` this is a no-op since in-memory state is always
    /// consistent and there is nothing to roll back.
    fn begin(&mut self) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    /// Commits the current transaction. Panics if called without a prior `begin`.
    fn commit(&mut self) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;

    /// Rolls back the current transaction. Silently ignored if no transaction
    /// is open (e.g. `begin` was never called or a prior `rollback` already ran).
    fn rollback(&mut self) -> impl Future<Output = Result<(), LedgerError>> + Send + '_;
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
// Result in the trait exists for PgStore, which does real I/O; this
// implementation just satisfies the same interface.
impl LedgerStore for InMemoryStore {
    async fn save_account(&mut self, account: Account) -> Result<(), LedgerError> {
        self.accounts.insert(account.id, account);
        Ok(())
    }

    async fn get_account(&mut self, id: AccountId) -> Result<Option<Account>, LedgerError> {
        Ok(self.accounts.get(&id).cloned())
    }

    async fn all_accounts(&mut self) -> Result<Vec<Account>, LedgerError> {
        Ok(self.accounts.values().cloned().collect())
    }

    async fn save_entry(&mut self, entry: Entry) -> Result<(), LedgerError> {
        self.entries.insert(entry.id, entry);
        Ok(())
    }

    async fn get_entry(&mut self, id: EntryId) -> Result<Option<Entry>, LedgerError> {
        Ok(self.entries.get(&id).cloned())
    }

    async fn entries_for_account(&mut self, account_id: AccountId) -> Result<Vec<Entry>, LedgerError> {
        Ok(self
            .entries
            .values()
            .filter(|e| e.account_id == account_id)
            .cloned()
            .collect())
    }

    async fn entries_for_pot(&mut self, pot_id: PotId) -> Result<Vec<Entry>, LedgerError> {
        Ok(self
            .entries
            .values()
            .filter(|e| e.pot_id == Some(pot_id))
            .cloned()
            .collect())
    }

    async fn save_entry_parts(
        &mut self,
        entry_id: EntryId,
        parts: Vec<EntryPart>,
    ) -> Result<(), LedgerError> {
        self.entry_parts.insert(entry_id, parts);
        Ok(())
    }

    async fn parts_for_entry(&mut self, entry_id: EntryId) -> Result<Vec<EntryPart>, LedgerError> {
        Ok(self.entry_parts.get(&entry_id).cloned().unwrap_or_default())
    }

    async fn save_pot(&mut self, pot: Pot) -> Result<(), LedgerError> {
        self.pots.insert(pot.id, pot);
        Ok(())
    }

    async fn get_pot(&mut self, id: PotId) -> Result<Option<Pot>, LedgerError> {
        Ok(self.pots.get(&id).cloned())
    }

    async fn all_pots(&mut self) -> Result<Vec<Pot>, LedgerError> {
        Ok(self.pots.values().cloned().collect())
    }

    async fn delete_pot(&mut self, id: PotId) -> Result<(), LedgerError> {
        self.pots.remove(&id);
        Ok(())
    }

    async fn save_allocation(&mut self, allocation: Allocation) -> Result<(), LedgerError> {
        self.allocations.insert(allocation.id, allocation);
        Ok(())
    }

    async fn allocations_for_pot(&mut self, pot_id: PotId) -> Result<Vec<Allocation>, LedgerError> {
        Ok(self
            .allocations
            .values()
            .filter(|a| a.pot_id == pot_id)
            .cloned()
            .collect())
    }

    async fn delete_allocations_for_pot(&mut self, pot_id: PotId) -> Result<(), LedgerError> {
        self.allocations.retain(|_, a| a.pot_id != pot_id);
        Ok(())
    }

    async fn save_valuation(&mut self, valuation: Valuation) -> Result<(), LedgerError> {
        self.valuations
            .entry(valuation.account_id)
            .or_default()
            .push(valuation);
        Ok(())
    }

    async fn valuations_for_account(
        &mut self,
        account_id: AccountId,
    ) -> Result<Vec<Valuation>, LedgerError> {
        Ok(self
            .valuations
            .get(&account_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn save_import(&mut self, import: Import) -> Result<(), LedgerError> {
        self.imports.insert(import.id, import);
        Ok(())
    }

    async fn get_import(&mut self, id: ImportId) -> Result<Option<Import>, LedgerError> {
        Ok(self.imports.get(&id).cloned())
    }

    async fn all_imports(&mut self) -> Result<Vec<Import>, LedgerError> {
        let mut imports: Vec<Import> = self.imports.values().cloned().collect();
        imports.sort_by_key(|i| i.uploaded_at);
        Ok(imports)
    }

    async fn delete_import(&mut self, id: ImportId) -> Result<(), LedgerError> {
        self.imports.remove(&id);
        Ok(())
    }

    async fn incomplete_import_for_account(
        &mut self,
        account_id: AccountId,
    ) -> Result<Option<Import>, LedgerError> {
        Ok(self
            .imports
            .values()
            .find(|i| i.account_id == account_id && !i.completed)
            .cloned())
    }

    async fn save_queue_row(&mut self, row: ImportQueueRow) -> Result<(), LedgerError> {
        self.queue_rows.insert(row.id, row);
        Ok(())
    }

    async fn get_queue_row(&mut self, id: QueueRowId) -> Result<Option<ImportQueueRow>, LedgerError> {
        Ok(self.queue_rows.get(&id).cloned())
    }

    async fn queue_rows_for_import(
        &mut self,
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

    async fn delete_queue_row(&mut self, id: QueueRowId) -> Result<(), LedgerError> {
        self.queue_rows.remove(&id);
        Ok(())
    }

    async fn save_queue_row_match(&mut self, m: ImportQueueRowMatch) -> Result<(), LedgerError> {
        self.queue_row_matches.insert(m.id, m);
        Ok(())
    }

    async fn get_queue_row_match(
        &mut self,
        id: MatchId,
    ) -> Result<Option<ImportQueueRowMatch>, LedgerError> {
        Ok(self.queue_row_matches.get(&id).cloned())
    }

    async fn queue_row_matches_referencing(
        &mut self,
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

    async fn queue_row_matches_for_import(
        &mut self,
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

    async fn delete_queue_row_match(&mut self, id: MatchId) -> Result<(), LedgerError> {
        self.queue_row_matches.remove(&id);
        Ok(())
    }

    async fn repoint_queue_row_matches(
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

    async fn begin(&mut self) -> Result<(), LedgerError> {
        Ok(())
    }

    async fn commit(&mut self) -> Result<(), LedgerError> {
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), LedgerError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountKind;
    use crate::currency::Currency;
    use rust_decimal_macros::dec;

    use crate::store::import_contract as contract;

    #[tokio::test]
    async fn an_import_round_trips() {
        contract::an_import_round_trips(InMemoryStore::default()).await;
    }

    #[tokio::test]
    async fn resaving_an_import_updates_it() {
        contract::resaving_an_import_updates_it(InMemoryStore::default()).await;
    }

    #[tokio::test]
    async fn incomplete_import_for_account_ignores_completed_and_other_accounts() {
        contract::incomplete_import_for_account_ignores_completed_and_other_accounts(
            InMemoryStore::default(),
        )
        .await;
    }

    #[tokio::test]
    async fn a_queue_row_round_trips() {
        contract::a_queue_row_round_trips(InMemoryStore::default()).await;
    }

    #[tokio::test]
    async fn queue_rows_come_back_for_their_import_by_date_then_time() {
        contract::queue_rows_come_back_for_their_import_by_date_then_time(
            InMemoryStore::default(),
        )
        .await;
    }

    #[tokio::test]
    async fn matches_referencing_a_row_come_from_either_side() {
        contract::matches_referencing_a_row_come_from_either_side(InMemoryStore::default()).await;
    }

    #[tokio::test]
    async fn matches_for_an_import_are_those_of_its_own_rows() {
        contract::matches_for_an_import_are_those_of_its_own_rows(InMemoryStore::default()).await;
    }

    #[tokio::test]
    async fn repointing_rewrites_only_rows_that_pointed_at_the_queue_row() {
        contract::repointing_rewrites_only_rows_that_pointed_at_the_queue_row(
            InMemoryStore::default(),
        )
        .await;
    }

    #[tokio::test]
    async fn saved_account_can_be_read_back_by_id() {
        let mut store = InMemoryStore::default();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        let id = account.id;
        store.save_account(account.clone()).await.unwrap();
        assert_eq!(store.get_account(id).await, Ok(Some(account)));
    }

    #[tokio::test]
    async fn unknown_id_returns_none() {
        let mut store = InMemoryStore::default();
        assert_eq!(store.get_account(AccountId::generate()).await, Ok(None));
    }

    #[tokio::test]
    async fn all_accounts_lists_every_saved_account() {
        let mut store = InMemoryStore::default();
        let eur = Currency::new("EUR").unwrap();
        store
            .save_account(Account::new("A", eur.clone(), AccountKind::Own, dec!(0)))
            .await
            .unwrap();
        store
            .save_account(Account::new("B", eur, AccountKind::Own, dec!(0)))
            .await
            .unwrap();
        assert_eq!(store.all_accounts().await.unwrap().len(), 2);
    }
}

/// The import tables' store contract, written once and run against both
/// InMemoryStore (below) and PgStore (requires a live database), so the two
/// implementations cannot drift apart on ordering or match-row semantics.
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

    async fn an_account<S: LedgerStore>(store: &mut S) -> AccountId {
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).await.unwrap();
        account.id
    }

    async fn a_saved_import<S: LedgerStore>(store: &mut S) -> ImportId {
        let account_id = an_account(store).await;
        let import = an_import(account_id);
        store.save_import(import.clone()).await.unwrap();
        import.id
    }

    async fn saved_rows<S: LedgerStore>(store: &mut S, n: usize) -> Vec<QueueRowId> {
        let import_id = a_saved_import(store).await;
        let mut ids = Vec::new();
        for _ in 0..n {
            let row = a_row(import_id, 1, None);
            store.save_queue_row(row.clone()).await.unwrap();
            ids.push(row.id);
        }
        ids
    }

    async fn a_saved_entry<S: LedgerStore>(store: &mut S) -> EntryId {
        let account_id = an_account(store).await;
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
        store.save_entry(entry.clone()).await.unwrap();
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

    pub async fn an_import_round_trips<S: LedgerStore>(mut store: S) {
        let import = an_import(an_account(&mut store).await);
        store.save_import(import.clone()).await.unwrap();
        assert_eq!(
            store.get_import(import.id).await,
            Ok(Some(import.clone()))
        );
        assert_eq!(store.get_import(ImportId::generate()).await, Ok(None));
        assert_eq!(store.all_imports().await, Ok(vec![import.clone()]));
        store.delete_import(import.id).await.unwrap();
        assert_eq!(store.get_import(import.id).await, Ok(None));
    }

    pub async fn resaving_an_import_updates_it<S: LedgerStore>(mut store: S) {
        let mut import = an_import(an_account(&mut store).await);
        store.save_import(import.clone()).await.unwrap();
        import.completed = true;
        store.save_import(import.clone()).await.unwrap();
        assert_eq!(store.all_imports().await, Ok(vec![import]));
    }

    pub async fn incomplete_import_for_account_ignores_completed_and_other_accounts<
        S: LedgerStore,
    >(
        mut store: S,
    ) {
        let account = an_account(&mut store).await;
        let mut done = an_import(account);
        done.completed = true;
        store.save_import(done).await.unwrap();
        let other_account = an_account(&mut store).await;
        store.save_import(an_import(other_account)).await.unwrap();
        assert_eq!(
            store.incomplete_import_for_account(account).await,
            Ok(None)
        );
        let open = an_import(account);
        store.save_import(open.clone()).await.unwrap();
        assert_eq!(
            store.incomplete_import_for_account(account).await,
            Ok(Some(open))
        );
    }

    pub async fn a_queue_row_round_trips<S: LedgerStore>(mut store: S) {
        let import_id = a_saved_import(&mut store).await;
        let mut row = a_row(import_id, 5, Some((23, 59, 1)));
        row.detail = RowDetail::Normal(NormalDetail {
            description: "Coffee, cake".to_string(),
            bank_state: BankState::Pending,
            category: Some("Food".to_string()),
        });
        store.save_queue_row(row.clone()).await.unwrap();
        assert_eq!(
            store.get_queue_row(row.id).await,
            Ok(Some(row.clone()))
        );
        row.detail = RowDetail::RevertedCandidate;
        store.save_queue_row(row.clone()).await.unwrap();
        assert_eq!(
            store.get_queue_row(row.id).await,
            Ok(Some(row.clone()))
        );
        store.delete_queue_row(row.id).await.unwrap();
        assert_eq!(store.get_queue_row(row.id).await, Ok(None));
    }

    pub async fn queue_rows_come_back_for_their_import_by_date_then_time<S: LedgerStore>(
        mut store: S,
    ) {
        let import_id = a_saved_import(&mut store).await;
        let late = a_row(import_id, 9, Some((8, 0, 0)));
        let early_later_time = a_row(import_id, 2, Some((18, 0, 0)));
        let early_earlier_time = a_row(import_id, 2, Some((9, 30, 0)));
        let early_no_time = a_row(import_id, 2, None);
        let other_import = a_row(a_saved_import(&mut store).await, 1, None);
        for r in [
            &late,
            &early_later_time,
            &other_import,
            &early_earlier_time,
            &early_no_time,
        ] {
            store.save_queue_row(r.clone()).await.unwrap();
        }
        assert_eq!(
            store.queue_rows_for_import(import_id).await,
            Ok(vec![
                early_no_time,
                early_earlier_time,
                early_later_time,
                late
            ])
        );
    }

    pub async fn matches_referencing_a_row_come_from_either_side<S: LedgerStore>(mut store: S) {
        let rows = saved_rows(&mut store, 3).await;
        let (a, b, c) = (rows[0], rows[1], rows[2]);
        let a_to_b = ImportQueueRowMatch::to_queue_row(a, b);
        let b_to_a = ImportQueueRowMatch::to_queue_row(b, a);
        let a_to_entry = ImportQueueRowMatch::to_entry(a, a_saved_entry(&mut store).await);
        let c_to_entry = ImportQueueRowMatch::to_entry(c, a_saved_entry(&mut store).await);
        for m in [&a_to_b, &b_to_a, &a_to_entry, &c_to_entry] {
            store.save_queue_row_match(m.clone()).await.unwrap();
        }
        let mut found = store.queue_row_matches_referencing(a).await.unwrap();
        found.sort_by_key(|m| m.id);
        let mut expected = vec![a_to_b, b_to_a.clone(), a_to_entry];
        expected.sort_by_key(|m| m.id);
        assert_eq!(found, expected);
        assert_eq!(
            store.get_queue_row_match(b_to_a.id).await,
            Ok(Some(b_to_a.clone()))
        );
        store.delete_queue_row_match(b_to_a.id).await.unwrap();
        assert_eq!(store.get_queue_row_match(b_to_a.id).await, Ok(None));
    }

    pub async fn matches_for_an_import_are_those_of_its_own_rows<S: LedgerStore>(mut store: S) {
        let ours = saved_rows(&mut store, 2).await;
        let theirs = saved_rows(&mut store, 1).await;
        let entry = a_saved_entry(&mut store).await;
        let ours_to_row = ImportQueueRowMatch::to_queue_row(ours[0], ours[1]);
        let ours_to_entry = ImportQueueRowMatch::to_entry(ours[1], entry);
        let theirs_to_entry = ImportQueueRowMatch::to_entry(theirs[0], entry);
        for m in [&ours_to_row, &ours_to_entry, &theirs_to_entry] {
            store.save_queue_row_match(m.clone()).await.unwrap();
        }
        let import_id = store.get_queue_row(ours[0]).await.unwrap().unwrap().import_id;
        let mut found = store
            .queue_row_matches_for_import(import_id)
            .await
            .unwrap();
        found.sort_by_key(|m| m.id);
        let mut expected = vec![ours_to_row, ours_to_entry];
        expected.sort_by_key(|m| m.id);
        assert_eq!(found, expected);
    }

    pub async fn repointing_rewrites_only_rows_that_pointed_at_the_queue_row<S: LedgerStore>(
        mut store: S,
    ) {
        use crate::import::MatchTarget;
        let rows = saved_rows(&mut store, 2).await;
        let (a, b) = (rows[0], rows[1]);
        let new_entry = a_saved_entry(&mut store).await;
        let a_to_b = ImportQueueRowMatch::to_queue_row(a, b);
        let b_to_a = ImportQueueRowMatch::to_queue_row(b, a);
        let b_to_entry = ImportQueueRowMatch::to_entry(b, a_saved_entry(&mut store).await);
        for m in [&a_to_b, &b_to_a, &b_to_entry] {
            store.save_queue_row_match(m.clone()).await.unwrap();
        }
        store
            .repoint_queue_row_matches(a, new_entry)
            .await
            .unwrap();
        assert_eq!(
            store.get_queue_row_match(b_to_a.id).await,
            Ok(Some(ImportQueueRowMatch {
                id: b_to_a.id,
                queue_row_id: b,
                target: MatchTarget::Entry {
                    entry_id: new_entry
                },
            }))
        );
        assert_eq!(
            store.get_queue_row_match(a_to_b.id).await,
            Ok(Some(a_to_b))
        );
        assert_eq!(
            store.get_queue_row_match(b_to_entry.id).await,
            Ok(Some(b_to_entry))
        );
    }
}
