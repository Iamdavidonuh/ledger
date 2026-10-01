//! The import review queue's side of `Ledger`. Every method here does its
//! lookups, validation and pot checks, and builds any `Entry` it needs,
//! before opening its one `self.store.transaction(...)`; the closure only
//! saves or deletes rows that are already fully decided, since it gets the
//! store, not the `Ledger`.

use super::Ledger;
use crate::account::Account;
use crate::entry::{BankState, Entry, EntrySource};
use crate::error::LedgerError;
use crate::id::{AccountId, EntryId, ImportId, MatchId, QueueRowId};
use crate::import::{
    Import, ImportQueueRow, ImportQueueRowMatch, MatchTarget, NormalDetail, QueueRowView,
    RowDetail, RowReview,
};
use crate::store::LedgerStore;
use crate::transfer::{Transfer, TransferLeg};
use rust_decimal::Decimal;
use std::collections::{BTreeSet, HashMap};

#[cfg(test)]
mod tests;

/// How a row leaves its queue, which decides what happens to the match rows
/// around it.
enum Removal {
    /// The row became this entry. Its own match rows go, and any sibling
    /// that pointed at it is repointed at the entry instead, so the sibling
    /// stays suspicious rather than silently turning clean.
    Accepted { entry_id: EntryId },
    /// The row was discarded or resolved without becoming an entry, so
    /// every match row that references it, on either side, goes.
    Dropped,
}

/// Everything that taking one row out of its queue writes, worked out
/// before the transaction opens so the apply step is a pure sequence of writes.
struct RowRemoval {
    row_id: QueueRowId,
    match_ids: Vec<MatchId>,
    repoint_to: Option<EntryId>,
    /// The import marked completed, if this is the last row in its queue.
    completed: Option<Import>,
}

impl RowRemoval {
    async fn apply<S: LedgerStore>(self, store: &mut S) -> Result<(), LedgerError> {
        for id in self.match_ids {
            store.delete_queue_row_match(id).await?;
        }
        if let Some(entry_id) = self.repoint_to {
            store
                .repoint_queue_row_matches(self.row_id, entry_id)
                .await?;
        }
        store.delete_queue_row(self.row_id).await?;
        if let Some(import) = self.completed {
            store.save_import(import).await?;
        }
        Ok(())
    }
}

/// One import's match rows, grouped by the row each one belongs to.
struct MatchIndex(HashMap<QueueRowId, Vec<MatchTarget>>);

impl MatchIndex {
    fn new(matches: Vec<ImportQueueRowMatch>) -> Self {
        let mut by_row: HashMap<QueueRowId, Vec<MatchTarget>> = HashMap::new();
        for m in matches {
            by_row.entry(m.queue_row_id).or_default().push(m.target);
        }
        MatchIndex(by_row)
    }

    fn targets(&self, row_id: QueueRowId) -> &[MatchTarget] {
        self.0.get(&row_id).map_or(&[], Vec::as_slice)
    }

    fn entry_ids(&self, row_id: QueueRowId) -> Vec<EntryId> {
        self.targets(row_id)
            .iter()
            .filter_map(|target| match target {
                MatchTarget::Entry { entry_id } => Some(*entry_id),
                MatchTarget::QueueRow { .. } => None,
            })
            .collect()
    }

    fn queue_row_ids(&self, row_id: QueueRowId) -> Vec<QueueRowId> {
        self.targets(row_id)
            .iter()
            .filter_map(|target| match target {
                MatchTarget::QueueRow { queue_row_id } => Some(*queue_row_id),
                MatchTarget::Entry { .. } => None,
            })
            .collect()
    }

    fn is_suspicious(&self, row_id: QueueRowId) -> bool {
        !self.targets(row_id).is_empty()
    }
}

/// The categories each description has been given in one account, read once
/// so a whole queue can be suggested for without re-reading the entries.
/// Voided and Reverted entries are left out, since they are no longer real
/// facts about the account, and so are entries with no category, rather than
/// counting as disagreeing.
struct CategoryHistory(HashMap<String, BTreeSet<String>>);

impl CategoryHistory {
    fn new(entries: Vec<Entry>) -> Self {
        let mut by_description: HashMap<String, BTreeSet<String>> = HashMap::new();
        for entry in entries {
            if entry.is_voided() || entry.bank_state == BankState::Reverted {
                continue;
            }
            if let Some(category) = entry.category {
                by_description
                    .entry(entry.description)
                    .or_default()
                    .insert(category);
            }
        }
        CategoryHistory(by_description)
    }

    /// The one category every entry with this description agrees on, if
    /// there is exactly one.
    fn suggestion(&self, description: &str) -> Option<String> {
        let categories = self.0.get(description)?;
        match (categories.len(), categories.first()) {
            (1, Some(category)) => Some(category.clone()),
            _ => None,
        }
    }
}

impl<S: LedgerStore> Ledger<S> {
    pub async fn imports(&mut self) -> Result<Vec<Import>, LedgerError> {
        self.store.all_imports().await
    }

    pub async fn import(&mut self, id: ImportId) -> Result<Option<Import>, LedgerError> {
        self.store.get_import(id).await
    }

    pub async fn incomplete_import_for_account(
        &mut self,
        account_id: AccountId,
    ) -> Result<Option<Import>, LedgerError> {
        self.store.incomplete_import_for_account(account_id).await
    }

    /// The import's queue in date-then-time order. A Normal row gets a
    /// fresh category suggestion; a RevertedCandidate row gets its
    /// suggested entry, if it has one.
    pub async fn import_queue(
        &mut self,
        import_id: ImportId,
    ) -> Result<Vec<QueueRowView>, LedgerError> {
        let import = self.find_import(import_id).await?;
        let matches = MatchIndex::new(self.store.queue_row_matches_for_import(import_id).await?);
        let history = self.category_history(import.account_id).await?;
        self.store
            .queue_rows_for_import(import_id)
            .await?
            .into_iter()
            .map(|row| {
                let review = match &row.detail {
                    RowDetail::Normal(detail) => RowReview::Normal {
                        suspicious: matches.is_suspicious(row.id),
                        matched_entry_ids: matches.entry_ids(row.id),
                        matched_queue_row_ids: matches.queue_row_ids(row.id),
                        suggested_category: history.suggestion(&detail.description),
                    },
                    RowDetail::RevertedCandidate => RowReview::RevertedCandidate {
                        suggested_entry_id: matches.entry_ids(row.id).first().copied(),
                    },
                };
                Ok(QueueRowView { row, review })
            })
            .collect()
    }

    /// Saves an already-built import, its queue rows and their match rows
    /// in one transaction. Re-checks for an incomplete import first, since
    /// parsing happens between the upload handler's own check and this call.
    pub async fn stage_import(
        &mut self,
        import: Import,
        queue_rows: Vec<ImportQueueRow>,
        matches: Vec<ImportQueueRowMatch>,
    ) -> Result<(), LedgerError> {
        if self
            .store
            .incomplete_import_for_account(import.account_id)
            .await?
            .is_some()
        {
            return Err(LedgerError::IncompleteImportExists);
        }
        self.store.begin().await?;
        let result = async {
            self.store.save_import(import).await?;
            for row in queue_rows {
                self.store.save_queue_row(row).await?;
            }
            for m in matches {
                self.store.save_queue_row_match(m).await?;
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => self.store.commit().await,
            Err(e) => {
                self.store.rollback().await?;
                Err(e)
            }
        }
    }

    /// Accepts a Normal row into the ledger as an imported entry. Category:
    /// the one passed here, else the one saved on the row, else a fresh
    /// suggestion, else none.
    pub async fn accept_queue_row(
        &mut self,
        import_id: ImportId,
        row_id: QueueRowId,
        category: Option<String>,
    ) -> Result<Entry, LedgerError> {
        let (import, row, detail) = self.find_normal_row(import_id, row_id).await?;
        let category = match category.or_else(|| detail.category.clone()) {
            Some(category) => Some(category),
            None => {
                self.suggest_category(import.account_id, &detail.description)
                    .await?
            }
        };
        let account = self.account_of(&import).await?;
        let entry = Self::imported_entry(&account, &row, &NormalDetail { category, ..detail });
        let removal = self
            .plan_removal(&import, row_id, Removal::Accepted { entry_id: entry.id })
            .await?;
        self.store.begin().await?;
        let result = async {
            self.store.save_entry(entry.clone()).await?;
            removal.apply(&mut self.store).await
        }
        .await;
        match result {
            Ok(()) => self.store.commit().await?,
            Err(e) => {
                self.store.rollback().await?;
                return Err(e);
            }
        }
        Ok(entry)
    }

    /// Accepts a Normal row as one side of a transfer with another tracked
    /// account. The row's sign picks the direction: money leaving this
    /// account makes it the sender, money arriving makes it the receiver.
    /// Returns (sent, received) entries, as `transfer` does.
    pub async fn accept_queue_row_as_transfer(
        &mut self,
        import_id: ImportId,
        row_id: QueueRowId,
        other_account_id: AccountId,
        other_amount: Option<Decimal>,
    ) -> Result<(Entry, Entry), LedgerError> {
        let (import, row, detail) = self.find_normal_row(import_id, row_id).await?;
        let this_side = TransferLeg::new(import.account_id, row.amount.abs());
        let other_side =
            TransferLeg::new(other_account_id, other_amount.unwrap_or(this_side.amount));
        let incoming = row.amount > Decimal::ZERO;
        let (from, to) = if incoming {
            (other_side, this_side)
        } else {
            (this_side, other_side)
        };
        let transfer = Transfer::new(from, to, row.date, detail.description);
        let (out_entry, in_entry) = self.build_transfer(&transfer).await?;
        let this_account_entry_id = if incoming { in_entry.id } else { out_entry.id };
        let removal = self
            .plan_removal(
                &import,
                row_id,
                Removal::Accepted {
                    entry_id: this_account_entry_id,
                },
            )
            .await?;
        self.store.begin().await?;
        let result = async {
            self.store.save_entry(out_entry.clone()).await?;
            self.store.save_entry(in_entry.clone()).await?;
            removal.apply(&mut self.store).await
        }
        .await;
        match result {
            Ok(()) => self.store.commit().await?,
            Err(e) => {
                self.store.rollback().await?;
                return Err(e);
            }
        }
        Ok((out_entry, in_entry))
    }

    /// Confirms a RevertedCandidate row's suggestion: marks the suggested
    /// entry Reverted (or leaves it as is, if it already is) and removes
    /// the row either way.
    pub async fn resolve_reverted_candidate(
        &mut self,
        import_id: ImportId,
        row_id: QueueRowId,
    ) -> Result<Entry, LedgerError> {
        let (import, _) = self.find_reverted_candidate(import_id, row_id).await?;
        let entry_id = self
            .own_matches(row_id)
            .await?
            .into_iter()
            .find_map(|m| match m.target {
                MatchTarget::Entry { entry_id } => Some(entry_id),
                MatchTarget::QueueRow { .. } => None,
            })
            .ok_or(LedgerError::NoSuggestedMatch)?;
        let (entry, needs_save) = self.prepare_revert(entry_id).await?;
        let removal = self.plan_removal(&import, row_id, Removal::Dropped).await?;
        self.store.begin().await?;
        let result = async {
            if needs_save {
                self.store.save_entry(entry.clone()).await?;
            }
            removal.apply(&mut self.store).await
        }
        .await;
        match result {
            Ok(()) => self.store.commit().await?,
            Err(e) => {
                self.store.rollback().await?;
                return Err(e);
            }
        }
        Ok(entry)
    }

    /// Removes a row of either kind without applying it, along with every
    /// match row that references it on either side.
    pub async fn discard_queue_row(
        &mut self,
        import_id: ImportId,
        row_id: QueueRowId,
    ) -> Result<(), LedgerError> {
        let (import, _) = self.find_queue_row(import_id, row_id).await?;
        let removal = self.plan_removal(&import, row_id, Removal::Dropped).await?;
        self.store.begin().await?;
        let result = removal.apply(&mut self.store).await;
        match result {
            Ok(()) => self.store.commit().await,
            Err(e) => {
                self.store.rollback().await?;
                Err(e)
            }
        }
    }

    /// Removes every row of the import, of either kind, without applying
    /// any of them. Returns how many rows were discarded.
    pub async fn discard_import(&mut self, import_id: ImportId) -> Result<usize, LedgerError> {
        let import = self.find_import(import_id).await?;
        let rows = self.store.queue_rows_for_import(import_id).await?;
        let match_ids: Vec<MatchId> = self
            .store
            .queue_row_matches_for_import(import_id)
            .await?
            .into_iter()
            .map(|m| m.id)
            .collect();
        let discarded = rows.len();
        self.store.begin().await?;
        let result = async {
            for id in match_ids {
                self.store.delete_queue_row_match(id).await?;
            }
            for row in &rows {
                self.store.delete_queue_row(row.id).await?;
            }
            self.store
                .save_import(Import {
                    completed: true,
                    ..import
                })
                .await
        }
        .await;
        match result {
            Ok(()) => self.store.commit().await?,
            Err(e) => {
                self.store.rollback().await?;
                return Err(e);
            }
        }
        Ok(discarded)
    }

    /// Accepts every remaining row at once, refused while any suspicious
    /// Normal row or any RevertedCandidate row is still in the queue. Each
    /// row's category is its own saved one, else a suggestion from entries
    /// already in the ledger (not from rows accepted earlier in this call).
    pub async fn bulk_accept_import(&mut self, import_id: ImportId) -> Result<usize, LedgerError> {
        let import = self.find_import(import_id).await?;
        let matches = MatchIndex::new(self.store.queue_row_matches_for_import(import_id).await?);

        let mut clean = Vec::new();
        let (mut suspicious_count, mut reverted_candidate_count) = (0, 0);
        for row in self.store.queue_rows_for_import(import_id).await? {
            match &row.detail {
                RowDetail::RevertedCandidate => reverted_candidate_count += 1,
                RowDetail::Normal(_) if matches.is_suspicious(row.id) => suspicious_count += 1,
                RowDetail::Normal(detail) => clean.push((row.clone(), detail.clone())),
            }
        }
        if suspicious_count > 0 || reverted_candidate_count > 0 {
            return Err(LedgerError::BulkAcceptBlocked {
                suspicious_count,
                reverted_candidate_count,
            });
        }

        let account = self.account_of(&import).await?;
        let history = self.category_history(import.account_id).await?;
        let (row_ids, entries): (Vec<QueueRowId>, Vec<Entry>) = clean
            .into_iter()
            .map(|(row, detail)| {
                let category = detail
                    .category
                    .clone()
                    .or_else(|| history.suggestion(&detail.description));
                let entry =
                    Self::imported_entry(&account, &row, &NormalDetail { category, ..detail });
                (row.id, entry)
            })
            .unzip();

        let accepted = entries.len();
        self.store.begin().await?;
        let result = async {
            for entry in entries {
                self.store.save_entry(entry).await?;
            }
            for id in row_ids {
                self.store.delete_queue_row(id).await?;
            }
            self.store
                .save_import(Import {
                    completed: true,
                    ..import
                })
                .await
        }
        .await;
        match result {
            Ok(()) => self.store.commit().await?,
            Err(e) => {
                self.store.rollback().await?;
                return Err(e);
            }
        }
        Ok(accepted)
    }

    /// Sets (or clears) the category a Normal row will be accepted with,
    /// without accepting it.
    pub async fn set_queue_row_category(
        &mut self,
        import_id: ImportId,
        row_id: QueueRowId,
        category: Option<String>,
    ) -> Result<ImportQueueRow, LedgerError> {
        let (_, row, detail) = self.find_normal_row(import_id, row_id).await?;
        let updated = ImportQueueRow {
            detail: RowDetail::Normal(NormalDetail { category, ..detail }),
            ..row
        };
        self.store.save_queue_row(updated.clone()).await?;
        Ok(updated)
    }

    /// The entry a Normal row becomes when accepted: Imported source, so its
    /// facts are locked, with the row's own time and bank state and the
    /// category already decided in `detail`.
    fn imported_entry(account: &Account, row: &ImportQueueRow, detail: &NormalDetail) -> Entry {
        Entry {
            source: EntrySource::Imported,
            time: row.time,
            bank_state: detail.bank_state,
            category: detail.category.clone(),
            ..Entry::manual(account, row.date, row.amount, detail.description.as_str())
        }
    }

    /// Looks up and validates reverting an entry without saving anything.
    /// Returns the entry to save and whether it needs saving: an entry
    /// that is already Reverted comes back unchanged with `false`, and has
    /// already left any pot's balance, so it skips the pot check.
    async fn prepare_revert(&mut self, entry_id: EntryId) -> Result<(Entry, bool), LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)
            .await?
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        if entry.is_voided() {
            return Err(LedgerError::AlreadyVoided);
        }
        if entry.bank_state == BankState::Reverted {
            return Ok((entry, false));
        }
        // Reverting drops the entry out of entries_tagged_to_pot exactly
        // like voiding does, so it gets the same never-below-zero check.
        if let Some(pot_id) = entry.pot_id {
            if self.pot_balance(pot_id).await? - entry.amount < Decimal::ZERO {
                return Err(LedgerError::PotWouldGoNegative);
            }
        }
        entry.bank_state = BankState::Reverted;
        Ok((entry, true))
    }

    async fn category_history(
        &mut self,
        account_id: AccountId,
    ) -> Result<CategoryHistory, LedgerError> {
        Ok(CategoryHistory::new(
            self.store.entries_for_account(account_id).await?,
        ))
    }

    /// A category every categorised entry in this account with exactly this
    /// description agrees on.
    async fn suggest_category(
        &mut self,
        account_id: AccountId,
        description: &str,
    ) -> Result<Option<String>, LedgerError> {
        Ok(self
            .category_history(account_id)
            .await?
            .suggestion(description))
    }

    async fn account_of(&mut self, import: &Import) -> Result<Account, LedgerError> {
        self.store
            .get_account(import.account_id)
            .await?
            .ok_or(LedgerError::AccountNotFound(import.account_id))
    }

    async fn find_import(&mut self, import_id: ImportId) -> Result<Import, LedgerError> {
        self.store
            .get_import(import_id)
            .await?
            .ok_or(LedgerError::ImportNotFound(import_id))
    }

    /// The import and one of its rows, of either kind. A row that exists but
    /// belongs to a different import is reported exactly like a missing one.
    async fn find_queue_row(
        &mut self,
        import_id: ImportId,
        row_id: QueueRowId,
    ) -> Result<(Import, ImportQueueRow), LedgerError> {
        let import = self.find_import(import_id).await?;
        let row = self
            .store
            .get_queue_row(row_id)
            .await?
            .filter(|r| r.import_id == import_id)
            .ok_or(LedgerError::QueueRowNotFound(row_id))?;
        Ok((import, row))
    }

    /// Like `find_queue_row`, but only a Normal row will do, and its
    /// Normal-only details come back with it.
    async fn find_normal_row(
        &mut self,
        import_id: ImportId,
        row_id: QueueRowId,
    ) -> Result<(Import, ImportQueueRow, NormalDetail), LedgerError> {
        let (import, row) = self.find_queue_row(import_id, row_id).await?;
        let detail = row
            .normal()
            .cloned()
            .ok_or(LedgerError::WrongQueueRowKind)?;
        Ok((import, row, detail))
    }

    /// Like `find_queue_row`, but only a RevertedCandidate row will do.
    async fn find_reverted_candidate(
        &mut self,
        import_id: ImportId,
        row_id: QueueRowId,
    ) -> Result<(Import, ImportQueueRow), LedgerError> {
        let (import, row) = self.find_queue_row(import_id, row_id).await?;
        if !row.is_reverted_candidate() {
            return Err(LedgerError::WrongQueueRowKind);
        }
        Ok((import, row))
    }

    /// Match rows where this row is the origin (it is the suspicious one or
    /// the candidate), not where a sibling points at it.
    async fn own_matches(
        &mut self,
        row_id: QueueRowId,
    ) -> Result<Vec<ImportQueueRowMatch>, LedgerError> {
        Ok(self
            .store
            .queue_row_matches_referencing(row_id)
            .await?
            .into_iter()
            .filter(|m| m.queue_row_id == row_id)
            .collect())
    }

    /// Works out what taking `row_id` out of `import`'s queue writes.
    async fn plan_removal(
        &mut self,
        import: &Import,
        row_id: QueueRowId,
        how: Removal,
    ) -> Result<RowRemoval, LedgerError> {
        let (matches, repoint_to) = match how {
            Removal::Accepted { entry_id } => (self.own_matches(row_id).await?, Some(entry_id)),
            Removal::Dropped => (
                self.store.queue_row_matches_referencing(row_id).await?,
                None,
            ),
        };
        let is_last_row = self.store.queue_rows_for_import(import.id).await?.len() <= 1;
        Ok(RowRemoval {
            row_id,
            match_ids: matches.into_iter().map(|m| m.id).collect(),
            repoint_to,
            completed: is_last_row.then(|| Import {
                completed: true,
                ..import.clone()
            }),
        })
    }
}
