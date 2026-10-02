use crate::account::{Account, AccountKind};
use crate::currency::Currency;
use crate::entry::{BankState, Entry, EntryPart, EntrySource};
use crate::error::LedgerError;
use crate::id::{
    AccountId, AllocationId, EntryId, EntryPartId, ImportId, MatchId, PotId, QueueRowId,
    ValuationId,
};
use crate::import::{
    Import, ImportQueueRow, ImportQueueRowMatch, MatchTarget, NormalDetail, RowDetail,
};
use crate::pot::{Allocation, Pot};
use crate::store::LedgerStore;
use crate::valuation::Valuation;
use rust_decimal::Decimal;
use sqlx::{
    pool::PoolConnection, postgres::PgTransactionManager, PgPool, Postgres, TransactionManager,
};
use uuid::Uuid;

impl From<sqlx::Error> for LedgerError {
    fn from(e: sqlx::Error) -> Self {
        LedgerError::Storage(e.to_string())
    }
}

fn account_kind_to_str(kind: AccountKind) -> &'static str {
    match kind {
        AccountKind::Own => "own",
        AccountKind::Outside => "outside",
        AccountKind::Person => "person",
        AccountKind::Investment => "investment",
    }
}

fn account_kind_from_str(s: &str) -> Result<AccountKind, LedgerError> {
    match s {
        "own" => Ok(AccountKind::Own),
        "outside" => Ok(AccountKind::Outside),
        "person" => Ok(AccountKind::Person),
        "investment" => Ok(AccountKind::Investment),
        other => Err(LedgerError::Storage(format!(
            "unknown account kind {other:?} stored in the database"
        ))),
    }
}

fn entry_source_to_str(source: EntrySource) -> &'static str {
    match source {
        EntrySource::Manual => "manual",
        EntrySource::Imported => "imported",
    }
}

fn entry_source_from_str(s: &str) -> Result<EntrySource, LedgerError> {
    match s {
        "manual" => Ok(EntrySource::Manual),
        "imported" => Ok(EntrySource::Imported),
        other => Err(LedgerError::Storage(format!(
            "unknown entry source {other:?} stored in the database"
        ))),
    }
}

fn bank_state_to_str(state: BankState) -> &'static str {
    match state {
        BankState::Completed => "completed",
        BankState::Pending => "pending",
        BankState::Reverted => "reverted",
    }
}

fn bank_state_from_str(s: &str) -> Result<BankState, LedgerError> {
    match s {
        "completed" => Ok(BankState::Completed),
        "pending" => Ok(BankState::Pending),
        "reverted" => Ok(BankState::Reverted),
        other => Err(LedgerError::Storage(format!(
            "unknown bank state {other:?} stored in the database"
        ))),
    }
}

const NORMAL_KIND: &str = "normal";
const REVERTED_CANDIDATE_KIND: &str = "reverted_candidate";

struct QueueRowColumns {
    kind: &'static str,
    description: String,
    bank_state: &'static str,
    category: Option<String>,
}

impl From<RowDetail> for QueueRowColumns {
    fn from(detail: RowDetail) -> Self {
        match detail {
            RowDetail::Normal(normal) => QueueRowColumns {
                kind: NORMAL_KIND,
                description: normal.description,
                bank_state: bank_state_to_str(normal.bank_state),
                category: normal.category,
            },
            RowDetail::RevertedCandidate => QueueRowColumns {
                kind: REVERTED_CANDIDATE_KIND,
                description: String::new(),
                bank_state: bank_state_to_str(BankState::Reverted),
                category: None,
            },
        }
    }
}

fn row_detail_from_columns(
    kind: &str,
    description: String,
    bank_state: &str,
    category: Option<String>,
) -> Result<RowDetail, LedgerError> {
    match kind {
        NORMAL_KIND => Ok(RowDetail::Normal(NormalDetail {
            description,
            bank_state: bank_state_from_str(bank_state)?,
            category,
        })),
        REVERTED_CANDIDATE_KIND => Ok(RowDetail::RevertedCandidate),
        other => Err(LedgerError::Storage(format!(
            "unknown queue row kind {other:?} stored in the database"
        ))),
    }
}

fn currency_from_str(s: &str) -> Result<Currency, LedgerError> {
    Currency::new(s).map_err(|e| LedgerError::Storage(e.to_string()))
}

/// Postgres stores UUIDs natively; these helpers convert the newtype wrappers.
fn uuid_to_pg<T: Into<Uuid>>(id: T) -> sqlx::types::Uuid {
    let u: Uuid = id.into();
    sqlx::types::Uuid::from_bytes(*u.as_bytes())
}

fn pg_to_uuid(u: sqlx::types::Uuid) -> Uuid {
    Uuid::from_bytes(*u.as_bytes())
}

fn pg_to_id<T: From<Uuid>>(u: sqlx::types::Uuid) -> T {
    T::from(pg_to_uuid(u))
}

/// The store type for production use. Each instance wraps one
/// `PoolConnection<Postgres>`. The connection is returned to the pool
/// when the `PgStore` is dropped, so there is no leak.
///
/// All queries go through the same connection, which means a
/// `transaction` closure sees all prior writes on that connection and
/// everything inside it is genuinely atomic.
///
/// Use `PgStore::acquire` to get a store from a pool. `AppState::with_ledger`
/// does this once per request.
pub struct PgStore {
    conn: PoolConnection<Postgres>,
}

impl PgStore {
    /// Acquires one connection from the pool. The connection is returned
    /// to the pool when this `PgStore` is dropped.
    pub async fn acquire(pool: &PgPool) -> Result<Self, LedgerError> {
        let conn = pool.acquire().await?;
        Ok(PgStore { conn })
    }
}

impl LedgerStore for PgStore {
    async fn save_account(&mut self, account: Account) -> Result<(), LedgerError> {
        sqlx::query(
            "INSERT INTO accounts (id, name, currency, kind, opening_balance, archived, current_value)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                currency = EXCLUDED.currency,
                kind = EXCLUDED.kind,
                opening_balance = EXCLUDED.opening_balance,
                archived = EXCLUDED.archived,
                current_value = EXCLUDED.current_value",
        )
        .bind(uuid_to_pg(account.id))
        .bind(&account.name)
        .bind(account.currency.code())
        .bind(account_kind_to_str(account.kind))
        .bind(account.opening_balance)
        .bind(account.archived)
        .bind(account.current_value)
        .execute(&mut *self.conn)
        .await?;
        Ok(())
    }

    async fn get_account(&mut self, id: AccountId) -> Result<Option<Account>, LedgerError> {
        let row = sqlx::query_as::<_, AccountRow>(
            "SELECT id, name, currency, kind, opening_balance, archived, current_value
             FROM accounts WHERE id = $1",
        )
        .bind(uuid_to_pg(id))
        .fetch_optional(&mut *self.conn)
        .await?;
        row.map(account_from_row).transpose()
    }

    async fn all_accounts(&mut self) -> Result<Vec<Account>, LedgerError> {
        let rows = sqlx::query_as::<_, AccountRow>(
            "SELECT id, name, currency, kind, opening_balance, archived, current_value FROM accounts",
        )
        .fetch_all(&mut *self.conn)
        .await?;
        rows.into_iter().map(account_from_row).collect()
    }

    async fn save_entry(&mut self, entry: Entry) -> Result<(), LedgerError> {
        // All three operations (upsert row, delete old tags, insert new tags)
        // must be atomic. Use a SAVEPOINT when already inside a transaction
        // (depth > 0), otherwise begin an explicit transaction. SAVEPOINT
        // outside a transaction block is a Postgres error.
        let in_txn = PgTransactionManager::get_transaction_depth(&*self.conn) > 0;
        if in_txn {
            sqlx::query("SAVEPOINT save_entry")
                .execute(&mut *self.conn)
                .await?;
        } else {
            self.begin().await?;
        }

        let result: Result<(), LedgerError> = async {
            sqlx::query(
                "INSERT INTO entries
                    (id, account_id, date, time, amount, currency, description, note, category,
                     pot_id, transfer_account_id, source, bank_state, confirmed, voided_reason)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
                 ON CONFLICT (id) DO UPDATE SET
                    account_id = EXCLUDED.account_id, date = EXCLUDED.date, time = EXCLUDED.time,
                    amount = EXCLUDED.amount, currency = EXCLUDED.currency,
                    description = EXCLUDED.description, note = EXCLUDED.note,
                    category = EXCLUDED.category, pot_id = EXCLUDED.pot_id,
                    transfer_account_id = EXCLUDED.transfer_account_id,
                    source = EXCLUDED.source, bank_state = EXCLUDED.bank_state,
                    confirmed = EXCLUDED.confirmed, voided_reason = EXCLUDED.voided_reason",
            )
            .bind(uuid_to_pg(entry.id))
            .bind(uuid_to_pg(entry.account_id))
            .bind(entry.date)
            .bind(entry.time)
            .bind(entry.amount)
            .bind(entry.currency.code())
            .bind(&entry.description)
            .bind(&entry.note)
            .bind(&entry.category)
            .bind(entry.pot_id.map(uuid_to_pg))
            .bind(entry.transfer_account_id.map(uuid_to_pg))
            .bind(entry_source_to_str(entry.source))
            .bind(bank_state_to_str(entry.bank_state))
            .bind(entry.confirmed)
            .bind(&entry.voided_reason)
            .execute(&mut *self.conn)
            .await?;

            sqlx::query("DELETE FROM entry_tags WHERE entry_id = $1")
                .bind(uuid_to_pg(entry.id))
                .execute(&mut *self.conn)
                .await?;

            let mut seen = std::collections::HashSet::new();
            for tag in &entry.tags {
                if seen.insert(tag.clone()) {
                    sqlx::query(
                        "INSERT INTO entry_tags (entry_id, tag) VALUES ($1, $2)
                         ON CONFLICT DO NOTHING",
                    )
                    .bind(uuid_to_pg(entry.id))
                    .bind(tag)
                    .execute(&mut *self.conn)
                    .await?;
                }
            }
            Ok(())
        }
        .await;

        match result {
            Ok(()) => {
                if in_txn {
                    sqlx::query("RELEASE SAVEPOINT save_entry")
                        .execute(&mut *self.conn)
                        .await?;
                } else {
                    self.commit().await?;
                }
                Ok(())
            }
            Err(e) => {
                if in_txn {
                    let _ = sqlx::query("ROLLBACK TO SAVEPOINT save_entry")
                        .execute(&mut *self.conn)
                        .await;
                    let _ = sqlx::query("RELEASE SAVEPOINT save_entry")
                        .execute(&mut *self.conn)
                        .await;
                } else {
                    self.rollback().await?;
                }
                Err(e)
            }
        }
    }

    async fn get_entry(&mut self, id: EntryId) -> Result<Option<Entry>, LedgerError> {
        let row = sqlx::query_as::<_, EntryRow>(
            "SELECT id, account_id, date, time, amount, currency, description, note, category,
                    pot_id, transfer_account_id, source, bank_state, confirmed, voided_reason
             FROM entries WHERE id = $1",
        )
        .bind(uuid_to_pg(id))
        .fetch_optional(&mut *self.conn)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let tags = self.tags_for_entry(id).await?;
        entry_from_row(row, tags).map(Some)
    }

    async fn entries_for_account(
        &mut self,
        account_id: AccountId,
    ) -> Result<Vec<Entry>, LedgerError> {
        self.fetch_entries_where(
            "SELECT e.id, e.account_id, e.date, e.time, e.amount, e.currency,
                    e.description, e.note, e.category, e.pot_id, e.transfer_account_id,
                    e.source, e.bank_state, e.confirmed, e.voided_reason,
                    COALESCE(array_agg(et.tag ORDER BY et.tag) FILTER (WHERE et.tag IS NOT NULL), '{}') AS tags
             FROM entries e
             LEFT JOIN entry_tags et ON et.entry_id = e.id
             WHERE e.account_id = $1
             GROUP BY e.id",
            uuid_to_pg(account_id),
        )
        .await
    }

    async fn entries_for_pot(&mut self, pot_id: PotId) -> Result<Vec<Entry>, LedgerError> {
        self.fetch_entries_where(
            "SELECT e.id, e.account_id, e.date, e.time, e.amount, e.currency,
                    e.description, e.note, e.category, e.pot_id, e.transfer_account_id,
                    e.source, e.bank_state, e.confirmed, e.voided_reason,
                    COALESCE(array_agg(et.tag ORDER BY et.tag) FILTER (WHERE et.tag IS NOT NULL), '{}') AS tags
             FROM entries e
             LEFT JOIN entry_tags et ON et.entry_id = e.id
             WHERE e.pot_id = $1
             GROUP BY e.id",
            uuid_to_pg(pot_id),
        )
        .await
    }

    async fn save_entry_parts(
        &mut self,
        entry_id: EntryId,
        parts: Vec<EntryPart>,
    ) -> Result<(), LedgerError> {
        let in_txn = PgTransactionManager::get_transaction_depth(&*self.conn) > 0;
        if in_txn {
            sqlx::query("SAVEPOINT save_entry_parts")
                .execute(&mut *self.conn)
                .await?;
        } else {
            self.begin().await?;
        }

        let result: Result<(), LedgerError> = async {
            sqlx::query("DELETE FROM entry_parts WHERE entry_id = $1")
                .bind(uuid_to_pg(entry_id))
                .execute(&mut *self.conn)
                .await?;
            for part in parts {
                sqlx::query(
                    "INSERT INTO entry_parts (id, entry_id, amount, category, transfer_account_id)
                     VALUES ($1, $2, $3, $4, $5)",
                )
                .bind(uuid_to_pg(part.id))
                .bind(uuid_to_pg(entry_id))
                .bind(part.amount)
                .bind(&part.category)
                .bind(part.transfer_account_id.map(uuid_to_pg))
                .execute(&mut *self.conn)
                .await?;
            }
            Ok(())
        }
        .await;

        match result {
            Ok(()) => {
                if in_txn {
                    sqlx::query("RELEASE SAVEPOINT save_entry_parts")
                        .execute(&mut *self.conn)
                        .await?;
                } else {
                    self.commit().await?;
                }
                Ok(())
            }
            Err(e) => {
                if in_txn {
                    let _ = sqlx::query("ROLLBACK TO SAVEPOINT save_entry_parts")
                        .execute(&mut *self.conn)
                        .await;
                    let _ = sqlx::query("RELEASE SAVEPOINT save_entry_parts")
                        .execute(&mut *self.conn)
                        .await;
                } else {
                    self.rollback().await?;
                }
                Err(e)
            }
        }
    }

    async fn parts_for_entry(&mut self, entry_id: EntryId) -> Result<Vec<EntryPart>, LedgerError> {
        let rows = sqlx::query_as::<_, EntryPartRow>(
            "SELECT id, amount, category, transfer_account_id
             FROM entry_parts WHERE entry_id = $1",
        )
        .bind(uuid_to_pg(entry_id))
        .fetch_all(&mut *self.conn)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(EntryPart {
                    id: pg_to_id::<EntryPartId>(r.id),
                    entry_id,
                    amount: r.amount,
                    category: r.category,
                    transfer_account_id: r.transfer_account_id.map(pg_to_id::<AccountId>),
                })
            })
            .collect()
    }

    async fn save_pot(&mut self, pot: Pot) -> Result<(), LedgerError> {
        sqlx::query(
            "INSERT INTO pots (id, name, currency, target, priority)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name, currency = EXCLUDED.currency,
                target = EXCLUDED.target, priority = EXCLUDED.priority",
        )
        .bind(uuid_to_pg(pot.id))
        .bind(&pot.name)
        .bind(pot.currency.code())
        .bind(pot.target)
        .bind(pot.priority)
        .execute(&mut *self.conn)
        .await?;
        Ok(())
    }

    async fn get_pot(&mut self, id: PotId) -> Result<Option<Pot>, LedgerError> {
        let row = sqlx::query_as::<_, PotRow>(
            "SELECT id, name, currency, target, priority FROM pots WHERE id = $1",
        )
        .bind(uuid_to_pg(id))
        .fetch_optional(&mut *self.conn)
        .await?;
        row.map(pot_from_row).transpose()
    }

    async fn all_pots(&mut self) -> Result<Vec<Pot>, LedgerError> {
        let rows =
            sqlx::query_as::<_, PotRow>("SELECT id, name, currency, target, priority FROM pots")
                .fetch_all(&mut *self.conn)
                .await?;
        rows.into_iter().map(pot_from_row).collect()
    }

    async fn delete_pot(&mut self, id: PotId) -> Result<(), LedgerError> {
        sqlx::query("DELETE FROM pots WHERE id = $1")
            .bind(uuid_to_pg(id))
            .execute(&mut *self.conn)
            .await?;
        Ok(())
    }

    async fn save_allocation(&mut self, allocation: Allocation) -> Result<(), LedgerError> {
        sqlx::query(
            "INSERT INTO allocations (id, pot_id, amount, date, note)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (id) DO UPDATE SET
                pot_id = EXCLUDED.pot_id, amount = EXCLUDED.amount,
                date = EXCLUDED.date, note = EXCLUDED.note",
        )
        .bind(uuid_to_pg(allocation.id))
        .bind(uuid_to_pg(allocation.pot_id))
        .bind(allocation.amount)
        .bind(allocation.date)
        .bind(&allocation.note)
        .execute(&mut *self.conn)
        .await?;
        Ok(())
    }

    async fn allocations_for_pot(&mut self, pot_id: PotId) -> Result<Vec<Allocation>, LedgerError> {
        let rows = sqlx::query_as::<_, AllocationRow>(
            "SELECT id, amount, date, note FROM allocations WHERE pot_id = $1",
        )
        .bind(uuid_to_pg(pot_id))
        .fetch_all(&mut *self.conn)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(Allocation {
                    id: pg_to_id::<AllocationId>(r.id),
                    pot_id,
                    amount: r.amount,
                    date: r.date,
                    note: r.note,
                })
            })
            .collect()
    }

    async fn delete_allocations_for_pot(&mut self, pot_id: PotId) -> Result<(), LedgerError> {
        sqlx::query("DELETE FROM allocations WHERE pot_id = $1")
            .bind(uuid_to_pg(pot_id))
            .execute(&mut *self.conn)
            .await?;
        Ok(())
    }

    async fn save_valuation(&mut self, valuation: Valuation) -> Result<(), LedgerError> {
        sqlx::query(
            "INSERT INTO valuations (id, account_id, date, old_value, new_value, category)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(uuid_to_pg(valuation.id))
        .bind(uuid_to_pg(valuation.account_id))
        .bind(valuation.date)
        .bind(valuation.old_value)
        .bind(valuation.new_value)
        .bind(&valuation.category)
        .execute(&mut *self.conn)
        .await?;
        Ok(())
    }

    async fn valuations_for_account(
        &mut self,
        account_id: AccountId,
    ) -> Result<Vec<Valuation>, LedgerError> {
        let rows = sqlx::query_as::<_, ValuationRow>(
            "SELECT id, date, old_value, new_value, category
             FROM valuations WHERE account_id = $1",
        )
        .bind(uuid_to_pg(account_id))
        .fetch_all(&mut *self.conn)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(Valuation {
                    id: pg_to_id::<ValuationId>(r.id),
                    account_id,
                    date: r.date,
                    old_value: r.old_value,
                    new_value: r.new_value,
                    category: r.category,
                })
            })
            .collect()
    }

    async fn save_import(&mut self, import: Import) -> Result<(), LedgerError> {
        sqlx::query(
            "INSERT INTO imports
                (id, account_id, currency, file_name, uploaded_at, rows_read,
                 opening_balance, closing_balance, completed)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (id) DO UPDATE SET
                account_id = EXCLUDED.account_id, currency = EXCLUDED.currency,
                file_name = EXCLUDED.file_name, uploaded_at = EXCLUDED.uploaded_at,
                rows_read = EXCLUDED.rows_read, opening_balance = EXCLUDED.opening_balance,
                closing_balance = EXCLUDED.closing_balance, completed = EXCLUDED.completed",
        )
        .bind(uuid_to_pg(import.id))
        .bind(uuid_to_pg(import.account_id))
        .bind(import.currency.code())
        .bind(&import.file_name)
        .bind(import.uploaded_at)
        .bind(import.rows_read)
        .bind(import.opening_balance)
        .bind(import.closing_balance)
        .bind(import.completed)
        .execute(&mut *self.conn)
        .await?;
        Ok(())
    }

    async fn get_import(&mut self, id: ImportId) -> Result<Option<Import>, LedgerError> {
        let row = sqlx::query_as::<_, ImportRow>(
            "SELECT id, account_id, currency, file_name, uploaded_at, rows_read,
                    opening_balance, closing_balance, completed
             FROM imports WHERE id = $1",
        )
        .bind(uuid_to_pg(id))
        .fetch_optional(&mut *self.conn)
        .await?;
        row.map(import_from_row).transpose()
    }

    async fn all_imports(&mut self) -> Result<Vec<Import>, LedgerError> {
        let rows = sqlx::query_as::<_, ImportRow>(
            "SELECT id, account_id, currency, file_name, uploaded_at, rows_read,
                    opening_balance, closing_balance, completed
             FROM imports ORDER BY uploaded_at",
        )
        .fetch_all(&mut *self.conn)
        .await?;
        rows.into_iter().map(import_from_row).collect()
    }

    async fn delete_import(&mut self, id: ImportId) -> Result<(), LedgerError> {
        sqlx::query("DELETE FROM imports WHERE id = $1")
            .bind(uuid_to_pg(id))
            .execute(&mut *self.conn)
            .await?;
        Ok(())
    }

    async fn incomplete_import_for_account(
        &mut self,
        account_id: AccountId,
    ) -> Result<Option<Import>, LedgerError> {
        let row = sqlx::query_as::<_, ImportRow>(
            "SELECT id, account_id, currency, file_name, uploaded_at, rows_read,
                    opening_balance, closing_balance, completed
             FROM imports WHERE account_id = $1 AND completed = false LIMIT 1",
        )
        .bind(uuid_to_pg(account_id))
        .fetch_optional(&mut *self.conn)
        .await?;
        row.map(import_from_row).transpose()
    }

    async fn save_queue_row(&mut self, row: ImportQueueRow) -> Result<(), LedgerError> {
        let cols = QueueRowColumns::from(row.detail);
        sqlx::query(
            "INSERT INTO import_queue_rows
                (id, import_id, kind, date, time, amount, currency, description, bank_state, category)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET
                import_id = EXCLUDED.import_id, kind = EXCLUDED.kind,
                date = EXCLUDED.date, time = EXCLUDED.time,
                amount = EXCLUDED.amount, currency = EXCLUDED.currency,
                description = EXCLUDED.description, bank_state = EXCLUDED.bank_state,
                category = EXCLUDED.category",
        )
        .bind(uuid_to_pg(row.id))
        .bind(uuid_to_pg(row.import_id))
        .bind(cols.kind)
        .bind(row.date)
        .bind(row.time)
        .bind(row.amount)
        .bind(row.currency.code())
        .bind(&cols.description)
        .bind(cols.bank_state)
        .bind(&cols.category)
        .execute(&mut *self.conn)
        .await?;
        Ok(())
    }

    async fn get_queue_row(
        &mut self,
        id: QueueRowId,
    ) -> Result<Option<ImportQueueRow>, LedgerError> {
        let row = sqlx::query_as::<_, QueueRowRow>(
            "SELECT id, import_id, kind, date, time, amount, currency, description, bank_state, category
             FROM import_queue_rows WHERE id = $1",
        )
        .bind(uuid_to_pg(id))
        .fetch_optional(&mut *self.conn)
        .await?;
        row.map(queue_row_from_row).transpose()
    }

    async fn queue_rows_for_import(
        &mut self,
        import_id: ImportId,
    ) -> Result<Vec<ImportQueueRow>, LedgerError> {
        let rows = sqlx::query_as::<_, QueueRowRow>(
            "SELECT id, import_id, kind, date, time, amount, currency, description, bank_state, category
             FROM import_queue_rows WHERE import_id = $1
             ORDER BY date, time NULLS FIRST",
        )
        .bind(uuid_to_pg(import_id))
        .fetch_all(&mut *self.conn)
        .await?;
        rows.into_iter().map(queue_row_from_row).collect()
    }

    async fn delete_queue_row(&mut self, id: QueueRowId) -> Result<(), LedgerError> {
        sqlx::query("DELETE FROM import_queue_rows WHERE id = $1")
            .bind(uuid_to_pg(id))
            .execute(&mut *self.conn)
            .await?;
        Ok(())
    }

    async fn save_queue_row_match(&mut self, m: ImportQueueRowMatch) -> Result<(), LedgerError> {
        let (entry_id, row_id) = match m.target {
            MatchTarget::Entry { entry_id } => (Some(uuid_to_pg(entry_id)), None),
            MatchTarget::QueueRow { queue_row_id } => (None, Some(uuid_to_pg(queue_row_id))),
        };
        sqlx::query(
            "INSERT INTO import_queue_row_matches
                (id, queue_row_id, matched_entry_id, matched_queue_row_id)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO UPDATE SET
                queue_row_id = EXCLUDED.queue_row_id,
                matched_entry_id = EXCLUDED.matched_entry_id,
                matched_queue_row_id = EXCLUDED.matched_queue_row_id",
        )
        .bind(uuid_to_pg(m.id))
        .bind(uuid_to_pg(m.queue_row_id))
        .bind(entry_id)
        .bind(row_id)
        .execute(&mut *self.conn)
        .await?;
        Ok(())
    }

    async fn get_queue_row_match(
        &mut self,
        id: MatchId,
    ) -> Result<Option<ImportQueueRowMatch>, LedgerError> {
        let row = sqlx::query_as::<_, MatchRow>(
            "SELECT id, queue_row_id, matched_entry_id, matched_queue_row_id
             FROM import_queue_row_matches WHERE id = $1",
        )
        .bind(uuid_to_pg(id))
        .fetch_optional(&mut *self.conn)
        .await?;
        row.map(match_from_row).transpose()
    }

    async fn queue_row_matches_referencing(
        &mut self,
        queue_row_id: QueueRowId,
    ) -> Result<Vec<ImportQueueRowMatch>, LedgerError> {
        let rows = sqlx::query_as::<_, MatchRow>(
            "SELECT id, queue_row_id, matched_entry_id, matched_queue_row_id
             FROM import_queue_row_matches
             WHERE queue_row_id = $1 OR matched_queue_row_id = $1",
        )
        .bind(uuid_to_pg(queue_row_id))
        .fetch_all(&mut *self.conn)
        .await?;
        rows.into_iter().map(match_from_row).collect()
    }

    async fn queue_row_matches_for_import(
        &mut self,
        import_id: ImportId,
    ) -> Result<Vec<ImportQueueRowMatch>, LedgerError> {
        let rows = sqlx::query_as::<_, MatchRow>(
            "SELECT m.id, m.queue_row_id, m.matched_entry_id, m.matched_queue_row_id
             FROM import_queue_row_matches m
             JOIN import_queue_rows r ON r.id = m.queue_row_id
             WHERE r.import_id = $1",
        )
        .bind(uuid_to_pg(import_id))
        .fetch_all(&mut *self.conn)
        .await?;
        rows.into_iter().map(match_from_row).collect()
    }

    async fn delete_queue_row_match(&mut self, id: MatchId) -> Result<(), LedgerError> {
        sqlx::query("DELETE FROM import_queue_row_matches WHERE id = $1")
            .bind(uuid_to_pg(id))
            .execute(&mut *self.conn)
            .await?;
        Ok(())
    }

    async fn repoint_queue_row_matches(
        &mut self,
        from_queue_row_id: QueueRowId,
        to_entry_id: EntryId,
    ) -> Result<(), LedgerError> {
        sqlx::query(
            "UPDATE import_queue_row_matches
             SET matched_queue_row_id = NULL, matched_entry_id = $2
             WHERE matched_queue_row_id = $1",
        )
        .bind(uuid_to_pg(from_queue_row_id))
        .bind(uuid_to_pg(to_entry_id))
        .execute(&mut *self.conn)
        .await?;
        Ok(())
    }

    async fn begin(&mut self) -> Result<(), LedgerError> {
        PgTransactionManager::begin(&mut *self.conn, None).await?;
        Ok(())
    }

    async fn commit(&mut self) -> Result<(), LedgerError> {
        if let Err(e) = PgTransactionManager::commit(&mut *self.conn).await {
            // Commit failed: roll back to reset transaction_depth so the
            // connection is clean when it returns to the pool.
            let _ = PgTransactionManager::rollback(&mut *self.conn).await;
            return Err(LedgerError::from(e));
        }
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), LedgerError> {
        let _ = PgTransactionManager::rollback(&mut *self.conn).await;
        Ok(())
    }
}

impl PgStore {
    async fn tags_for_entry(&mut self, entry_id: EntryId) -> Result<Vec<String>, LedgerError> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT tag FROM entry_tags WHERE entry_id = $1 ORDER BY tag")
                .bind(uuid_to_pg(entry_id))
                .fetch_all(&mut *self.conn)
                .await?;
        Ok(rows.into_iter().map(|(t,)| t).collect())
    }

    /// Fetches entries with their tags in a single query using a LEFT JOIN
    /// and array aggregation. The `$1` bind position is for the WHERE clause
    /// value (account_id or pot_id).
    async fn fetch_entries_where(
        &mut self,
        sql: &str,
        id: sqlx::types::Uuid,
    ) -> Result<Vec<Entry>, LedgerError> {
        let rows = sqlx::query_as::<_, EntryWithTagsRow>(sql)
            .bind(id)
            .fetch_all(&mut *self.conn)
            .await?;
        rows.into_iter().map(entry_with_tags_from_row).collect()
    }
}

// ---------------------------------------------------------------------------
// Row structs for sqlx::query_as
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct AccountRow {
    id: sqlx::types::Uuid,
    name: String,
    currency: String,
    kind: String,
    opening_balance: Decimal,
    archived: bool,
    current_value: Option<Decimal>,
}

fn account_from_row(r: AccountRow) -> Result<Account, LedgerError> {
    Ok(Account {
        id: pg_to_id::<AccountId>(r.id),
        name: r.name,
        currency: currency_from_str(&r.currency)?,
        kind: account_kind_from_str(&r.kind)?,
        opening_balance: r.opening_balance,
        archived: r.archived,
        current_value: r.current_value,
    })
}

#[derive(sqlx::FromRow)]
struct EntryRow {
    id: sqlx::types::Uuid,
    account_id: sqlx::types::Uuid,
    date: chrono::NaiveDate,
    time: Option<chrono::NaiveTime>,
    amount: Decimal,
    currency: String,
    description: String,
    note: Option<String>,
    category: Option<String>,
    pot_id: Option<sqlx::types::Uuid>,
    transfer_account_id: Option<sqlx::types::Uuid>,
    source: String,
    bank_state: String,
    confirmed: bool,
    voided_reason: Option<String>,
}

fn entry_from_row(r: EntryRow, tags: Vec<String>) -> Result<Entry, LedgerError> {
    Ok(Entry {
        id: pg_to_id::<EntryId>(r.id),
        account_id: pg_to_id::<AccountId>(r.account_id),
        date: r.date,
        time: r.time,
        amount: r.amount,
        currency: currency_from_str(&r.currency)?,
        description: r.description,
        note: r.note,
        category: r.category,
        tags,
        pot_id: r.pot_id.map(pg_to_id::<PotId>),
        transfer_account_id: r.transfer_account_id.map(pg_to_id::<AccountId>),
        source: entry_source_from_str(&r.source)?,
        bank_state: bank_state_from_str(&r.bank_state)?,
        confirmed: r.confirmed,
        voided_reason: r.voided_reason,
    })
}

/// Row type for the bulk entry-with-tags join query.
#[derive(sqlx::FromRow)]
struct EntryWithTagsRow {
    id: sqlx::types::Uuid,
    account_id: sqlx::types::Uuid,
    date: chrono::NaiveDate,
    time: Option<chrono::NaiveTime>,
    amount: Decimal,
    currency: String,
    description: String,
    note: Option<String>,
    category: Option<String>,
    pot_id: Option<sqlx::types::Uuid>,
    transfer_account_id: Option<sqlx::types::Uuid>,
    source: String,
    bank_state: String,
    confirmed: bool,
    voided_reason: Option<String>,
    tags: Vec<String>,
}

fn entry_with_tags_from_row(r: EntryWithTagsRow) -> Result<Entry, LedgerError> {
    Ok(Entry {
        id: pg_to_id::<EntryId>(r.id),
        account_id: pg_to_id::<AccountId>(r.account_id),
        date: r.date,
        time: r.time,
        amount: r.amount,
        currency: currency_from_str(&r.currency)?,
        description: r.description,
        note: r.note,
        category: r.category,
        tags: r.tags,
        pot_id: r.pot_id.map(pg_to_id::<PotId>),
        transfer_account_id: r.transfer_account_id.map(pg_to_id::<AccountId>),
        source: entry_source_from_str(&r.source)?,
        bank_state: bank_state_from_str(&r.bank_state)?,
        confirmed: r.confirmed,
        voided_reason: r.voided_reason,
    })
}

#[derive(sqlx::FromRow)]
struct EntryPartRow {
    id: sqlx::types::Uuid,
    amount: Decimal,
    category: Option<String>,
    transfer_account_id: Option<sqlx::types::Uuid>,
}

#[derive(sqlx::FromRow)]
struct PotRow {
    id: sqlx::types::Uuid,
    name: String,
    currency: String,
    target: Option<Decimal>,
    priority: Option<i32>,
}

fn pot_from_row(r: PotRow) -> Result<Pot, LedgerError> {
    Ok(Pot {
        id: pg_to_id::<PotId>(r.id),
        name: r.name,
        currency: currency_from_str(&r.currency)?,
        target: r.target,
        priority: r.priority,
    })
}

#[derive(sqlx::FromRow)]
struct AllocationRow {
    id: sqlx::types::Uuid,
    amount: Decimal,
    date: chrono::NaiveDate,
    note: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ValuationRow {
    id: sqlx::types::Uuid,
    date: chrono::NaiveDate,
    old_value: Decimal,
    new_value: Decimal,
    category: String,
}

#[derive(sqlx::FromRow)]
struct ImportRow {
    id: sqlx::types::Uuid,
    account_id: sqlx::types::Uuid,
    currency: String,
    file_name: String,
    uploaded_at: chrono::DateTime<chrono::Utc>,
    rows_read: i64,
    opening_balance: Decimal,
    closing_balance: Decimal,
    completed: bool,
}

fn import_from_row(r: ImportRow) -> Result<Import, LedgerError> {
    Ok(Import {
        id: pg_to_id::<ImportId>(r.id),
        account_id: pg_to_id::<AccountId>(r.account_id),
        currency: currency_from_str(&r.currency)?,
        file_name: r.file_name,
        uploaded_at: r.uploaded_at,
        rows_read: r.rows_read,
        opening_balance: r.opening_balance,
        closing_balance: r.closing_balance,
        completed: r.completed,
    })
}

#[derive(sqlx::FromRow)]
struct QueueRowRow {
    id: sqlx::types::Uuid,
    import_id: sqlx::types::Uuid,
    kind: String,
    date: chrono::NaiveDate,
    time: Option<chrono::NaiveTime>,
    amount: Decimal,
    currency: String,
    description: String,
    bank_state: String,
    category: Option<String>,
}

fn queue_row_from_row(r: QueueRowRow) -> Result<ImportQueueRow, LedgerError> {
    Ok(ImportQueueRow {
        id: pg_to_id::<QueueRowId>(r.id),
        import_id: pg_to_id::<ImportId>(r.import_id),
        date: r.date,
        time: r.time,
        amount: r.amount,
        currency: currency_from_str(&r.currency)?,
        detail: row_detail_from_columns(&r.kind, r.description, &r.bank_state, r.category)?,
    })
}

#[derive(sqlx::FromRow)]
struct MatchRow {
    id: sqlx::types::Uuid,
    queue_row_id: sqlx::types::Uuid,
    matched_entry_id: Option<sqlx::types::Uuid>,
    matched_queue_row_id: Option<sqlx::types::Uuid>,
}

fn match_from_row(r: MatchRow) -> Result<ImportQueueRowMatch, LedgerError> {
    let target = match (r.matched_entry_id, r.matched_queue_row_id) {
        (Some(entry_id), None) => MatchTarget::Entry {
            entry_id: pg_to_id::<EntryId>(entry_id),
        },
        (None, Some(queue_row_id)) => MatchTarget::QueueRow {
            queue_row_id: pg_to_id::<QueueRowId>(queue_row_id),
        },
        _ => {
            return Err(LedgerError::Storage(
                "import_queue_row_matches row must have exactly one of matched_entry_id or matched_queue_row_id".to_string(),
            ))
        }
    };
    Ok(ImportQueueRowMatch {
        id: pg_to_id::<MatchId>(r.id),
        queue_row_id: pg_to_id::<QueueRowId>(r.queue_row_id),
        target,
    })
}

/// Integration tests against a real Postgres. Skipped automatically when
/// DATABASE_URL is not set, so `cargo test` in a plain dev environment
/// still passes cleanly.
///
/// Run all integration tests with:
///   docker compose -f docker-compose.test.yml run --rm test
#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountKind;
    use crate::currency::Currency;
    use crate::ledger::Ledger;
    use crate::store::import_contract as contract;
    use crate::transfer::{Transfer, TransferLeg};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use sqlx::PgPool;

    const TRUNCATE: &str = "TRUNCATE accounts, entries, entry_tags, entry_parts, \
        pots, allocations, valuations, imports, import_queue_rows, \
        import_queue_row_matches CASCADE";

    /// A live pool connected to DATABASE_URL, with migrations applied and all
    /// tables wiped. Returns `None` when DATABASE_URL is not set so each test
    /// can skip gracefully without failing.
    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let pool = PgPool::connect(&url).await.ok()?;
        sqlx::migrate!("../core/migrations").run(&pool).await.ok()?;
        sqlx::query(TRUNCATE).execute(&pool).await.ok()?;
        Some(pool)
    }

    /// A fresh `PgStore` backed by a clean database. Each test that calls
    /// this gets an independent store with no prior data.
    async fn test_store() -> Option<PgStore> {
        let pool = test_pool().await?;
        PgStore::acquire(&pool).await.ok()
    }

    /// A `Ledger<PgStore>` backed by a clean database.
    async fn test_ledger() -> Option<Ledger<PgStore>> {
        test_store().await.map(Ledger::new)
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn eur() -> Currency {
        Currency::new("EUR").unwrap()
    }

    // ---- store contract tests ------------------------------------------
    // Each delegates to the shared pub async fn in core::store::import_contract
    // so the same assertions run against both InMemoryStore and PgStore.

    #[tokio::test]
    async fn an_import_round_trips() {
        let Some(s) = test_store().await else { return };
        contract::an_import_round_trips(s).await;
    }

    #[tokio::test]
    async fn resaving_an_import_updates_it() {
        let Some(s) = test_store().await else { return };
        contract::resaving_an_import_updates_it(s).await;
    }

    #[tokio::test]
    async fn incomplete_import_for_account_ignores_completed_and_other_accounts() {
        let Some(s) = test_store().await else { return };
        contract::incomplete_import_for_account_ignores_completed_and_other_accounts(s).await;
    }

    #[tokio::test]
    async fn a_queue_row_round_trips() {
        let Some(s) = test_store().await else { return };
        contract::a_queue_row_round_trips(s).await;
    }

    #[tokio::test]
    async fn queue_rows_come_back_for_their_import_by_date_then_time() {
        let Some(s) = test_store().await else { return };
        contract::queue_rows_come_back_for_their_import_by_date_then_time(s).await;
    }

    #[tokio::test]
    async fn matches_referencing_a_row_come_from_either_side() {
        let Some(s) = test_store().await else { return };
        contract::matches_referencing_a_row_come_from_either_side(s).await;
    }

    #[tokio::test]
    async fn matches_for_an_import_are_those_of_its_own_rows() {
        let Some(s) = test_store().await else { return };
        contract::matches_for_an_import_are_those_of_its_own_rows(s).await;
    }

    #[tokio::test]
    async fn repointing_rewrites_only_rows_that_pointed_at_the_queue_row() {
        let Some(s) = test_store().await else { return };
        contract::repointing_rewrites_only_rows_that_pointed_at_the_queue_row(s).await;
    }

    // ---- Ledger-level integration tests -----------------------------------
    // These exercise domain logic paths that combine multiple store writes
    // in a single transaction and verify the results round-trip through
    // Postgres correctly.

    #[tokio::test]
    async fn account_balance_and_manual_entries_persist() {
        let Some(mut ledger) = test_ledger().await else {
            return;
        };
        let account = ledger
            .open_account("Checking", eur(), AccountKind::Own, dec!(100))
            .await
            .unwrap();
        ledger
            .record_manual_entry(account.id, date(2026, 1, 1), dec!(-20), "Groceries")
            .await
            .unwrap();
        assert_eq!(ledger.account_balance(account.id).await, Ok(dec!(80)));
    }

    #[tokio::test]
    async fn voiding_an_entry_removes_it_from_the_balance() {
        let Some(mut ledger) = test_ledger().await else {
            return;
        };
        let account = ledger
            .open_account("Checking", eur(), AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, date(2026, 1, 1), dec!(-50), "Mistake")
            .await
            .unwrap();
        ledger.void_entry(entry.id, "wrong amount").await.unwrap();
        assert_eq!(ledger.account_balance(account.id).await, Ok(dec!(0)));
    }

    #[tokio::test]
    async fn transfer_saves_both_entries_atomically() {
        let Some(mut ledger) = test_ledger().await else {
            return;
        };
        let a = ledger
            .open_account("A", eur(), AccountKind::Own, dec!(200))
            .await
            .unwrap();
        let b = ledger
            .open_account("B", eur(), AccountKind::Own, dec!(0))
            .await
            .unwrap();
        ledger
            .transfer(Transfer::new(
                TransferLeg::new(a.id, dec!(50)),
                TransferLeg::new(b.id, dec!(50)),
                date(2026, 1, 1),
                "move".to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(ledger.account_balance(a.id).await, Ok(dec!(150)));
        assert_eq!(ledger.account_balance(b.id).await, Ok(dec!(50)));
    }

    #[tokio::test]
    async fn pot_allocate_and_delete_return_balance_to_savings() {
        let Some(mut ledger) = test_ledger().await else {
            return;
        };
        ledger
            .open_account("Savings", eur(), AccountKind::Own, dec!(500))
            .await
            .unwrap();
        let pot = ledger
            .open_pot("Trip", eur(), Some(dec!(300)), None)
            .await
            .unwrap();
        ledger
            .allocate_to_pot(pot.id, dec!(200), date(2026, 1, 1))
            .await
            .unwrap();
        assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(200)));
        assert_eq!(ledger.general_savings(&eur()).await, Ok(dec!(300)));
        ledger.delete_pot(pot.id).await.unwrap();
        assert_eq!(ledger.pot(pot.id).await, Ok(None));
        assert_eq!(ledger.general_savings(&eur()).await, Ok(dec!(500)));
    }

    #[tokio::test]
    async fn entry_tags_round_trip() {
        let Some(mut ledger) = test_ledger().await else {
            return;
        };
        let account = ledger
            .open_account("Checking", eur(), AccountKind::Own, dec!(0))
            .await
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, date(2026, 1, 1), dec!(-10), "Coffee")
            .await
            .unwrap();
        use crate::entry::EntryMetadata;
        let updated = ledger
            .update_entry_metadata(
                entry.id,
                EntryMetadata {
                    category: Some("Food".to_string()),
                    tags: vec!["morning".to_string(), "work".to_string()],
                    note: Some("nice place".to_string()),
                    pot_id: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.category, Some("Food".to_string()));
        let mut tags = updated.tags;
        tags.sort();
        assert_eq!(tags, vec!["morning".to_string(), "work".to_string()]);
        assert_eq!(updated.note, Some("nice place".to_string()));
    }
}
