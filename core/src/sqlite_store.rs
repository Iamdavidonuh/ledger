use crate::account::{Account, AccountKind};
use crate::currency::Currency;
use crate::entry::{BankState, Entry, EntryPart, EntrySource};
use crate::error::LedgerError;
use crate::id::{AccountId, EntryId, ImportId, MatchId, PotId, QueueRowId};
use crate::import::{
    Import, ImportQueueRow, ImportQueueRowMatch, MatchTarget, NormalDetail, RowDetail,
};
use crate::pot::{Allocation, Pot};
use crate::store::LedgerStore;
use crate::valuation::Valuation;
use rusqlite::{Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use rust_decimal::Decimal;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum SqliteStoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
}

// Every LedgerStore method below returns a real Result and propagates real
// failures (a bad write, a row that will not parse) as LedgerError::Storage,
// rather than panicking on them. None of this is truly impossible: a disk
// can fill up, a write can fail, and if a future migration ever renames a
// stored enum string without updating existing rows, an old row will not
// parse. Every one of those is a genuine possible outcome, surfaced to the
// caller like any other LedgerError, not a program bug worth crashing on.
impl From<rusqlite::Error> for LedgerError {
    fn from(e: rusqlite::Error) -> Self {
        LedgerError::Storage(e.to_string())
    }
}

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../migrations/0001_initial.sql")),
        M::up(include_str!("../migrations/0002_entries_pot_id_index.sql")),
        M::up(include_str!("../migrations/0003_import.sql")),
    ])
}

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open_in_memory() -> Result<Self, SqliteStoreError> {
        let mut conn = Connection::open_in_memory()?;
        migrations().to_latest(&mut conn)?;
        Ok(SqliteStore { conn })
    }

    pub fn open(path: &std::path::Path) -> Result<Self, SqliteStoreError> {
        let mut conn = Connection::open(path)?;
        migrations().to_latest(&mut conn)?;
        Ok(SqliteStore { conn })
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

/// The table keeps one column set for both kinds of row; a candidate stores
/// an empty description, the `reverted` bank state and no category.
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

fn datetime_from_text(s: &str) -> Result<chrono::DateTime<chrono::Utc>, LedgerError> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&chrono::Utc))
        .map_err(|_| LedgerError::Storage(format!("stored value {s:?} is not a valid timestamp")))
}

fn decimal_from_text(s: &str) -> Result<Decimal, LedgerError> {
    Decimal::from_str(s).map_err(|_| {
        LedgerError::Storage(format!("stored value {s:?} is not a valid decimal amount"))
    })
}

/// Reads any of the ledger's id types back from its stored UUID text.
fn id_from_text<T: From<Uuid>>(s: &str) -> Result<T, LedgerError> {
    Uuid::parse_str(s)
        .map(T::from)
        .map_err(|_| LedgerError::Storage(format!("stored value {s:?} is not a valid UUID")))
}

fn date_from_text(s: &str) -> Result<chrono::NaiveDate, LedgerError> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| LedgerError::Storage(format!("stored value {s:?} is not a valid date")))
}

fn time_from_text(s: &str) -> Result<chrono::NaiveTime, LedgerError> {
    chrono::NaiveTime::parse_from_str(s, "%H:%M:%S%.f")
        .map_err(|_| LedgerError::Storage(format!("stored value {s:?} is not a valid time")))
}

fn currency_from_text(s: &str) -> Result<Currency, LedgerError> {
    Currency::new(s).map_err(|e| LedgerError::Storage(e.to_string()))
}

const IMPORT_COLUMNS: &str =
    "id, account_id, currency, file_name, uploaded_at, rows_read, opening_balance, closing_balance, completed";
const QUEUE_ROW_COLUMNS: &str =
    "id, import_id, kind, date, time, amount, currency, description, bank_state, category";
const MATCH_COLUMNS: &str = "id, queue_row_id, matched_entry_id, matched_queue_row_id";

type RawImport = (
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    i64,
);

fn raw_import(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawImport> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

fn import_from_raw(raw: RawImport) -> Result<Import, LedgerError> {
    let (
        id,
        account_id,
        currency,
        file_name,
        uploaded_at,
        rows_read,
        opening_balance,
        closing_balance,
        completed,
    ) = raw;
    Ok(Import {
        id: id_from_text(&id)?,
        account_id: id_from_text(&account_id)?,
        currency: currency_from_text(&currency)?,
        file_name,
        uploaded_at: datetime_from_text(&uploaded_at)?,
        rows_read,
        opening_balance: decimal_from_text(&opening_balance)?,
        closing_balance: decimal_from_text(&closing_balance)?,
        completed: completed != 0,
    })
}

type RawQueueRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
    Option<String>,
);

fn raw_queue_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawQueueRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

fn queue_row_from_raw(raw: RawQueueRow) -> Result<ImportQueueRow, LedgerError> {
    let (id, import_id, kind, date, time, amount, currency, description, bank_state, category) =
        raw;
    Ok(ImportQueueRow {
        id: id_from_text(&id)?,
        import_id: id_from_text(&import_id)?,
        date: date_from_text(&date)?,
        time: time.map(|t| time_from_text(&t)).transpose()?,
        amount: decimal_from_text(&amount)?,
        currency: currency_from_text(&currency)?,
        detail: row_detail_from_columns(&kind, description, &bank_state, category)?,
    })
}

type RawMatch = (String, String, Option<String>, Option<String>);

fn raw_match(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawMatch> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn match_from_raw(raw: RawMatch) -> Result<ImportQueueRowMatch, LedgerError> {
    let (id, queue_row_id, matched_entry_id, matched_queue_row_id) = raw;
    let target = match (
        matched_entry_id.map(|s| id_from_text(&s)).transpose()?,
        matched_queue_row_id.map(|s| id_from_text(&s)).transpose()?,
    ) {
        (Some(entry_id), None) => MatchTarget::Entry { entry_id },
        (None, Some(queue_row_id)) => MatchTarget::QueueRow { queue_row_id },
        _ => {
            return Err(LedgerError::Storage(
                "import_queue_row_matches row must have exactly one of matched_entry_id or matched_queue_row_id".to_string(),
            ))
        }
    };
    Ok(ImportQueueRowMatch {
        id: id_from_text(&id)?,
        queue_row_id: id_from_text(&queue_row_id)?,
        target,
    })
}

impl SqliteStore {
    fn query_imports(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<Import>, LedgerError> {
        let mut stmt = self.conn.prepare(sql)?;
        let raws = stmt
            .query_map(params, raw_import)?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        raws.into_iter().map(import_from_raw).collect()
    }

    fn query_queue_rows(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<ImportQueueRow>, LedgerError> {
        let mut stmt = self.conn.prepare(sql)?;
        let raws = stmt
            .query_map(params, raw_queue_row)?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        raws.into_iter().map(queue_row_from_raw).collect()
    }

    fn query_matches(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<ImportQueueRowMatch>, LedgerError> {
        let mut stmt = self.conn.prepare(sql)?;
        let raws = stmt
            .query_map(params, raw_match)?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        raws.into_iter().map(match_from_raw).collect()
    }

    fn tags_for_entry(&self, entry_id: EntryId) -> Result<Vec<String>, LedgerError> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM entry_tags WHERE entry_id = ?1")?;
        let tags = stmt
            .query_map(rusqlite::params![entry_id.to_string()], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<String>, rusqlite::Error>>()?;
        Ok(tags)
    }
}

impl LedgerStore for SqliteStore {
    fn save_account(&mut self, account: Account) -> Result<(), LedgerError> {
        self.conn.execute(
            "INSERT INTO accounts (id, name, currency, kind, opening_balance, archived, current_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                currency = excluded.currency,
                kind = excluded.kind,
                opening_balance = excluded.opening_balance,
                archived = excluded.archived,
                current_value = excluded.current_value",
            rusqlite::params![
                account.id.to_string(),
                account.name,
                account.currency.code(),
                account_kind_to_str(account.kind),
                account.opening_balance.to_string(),
                account.archived as i64,
                account.current_value.map(|v| v.to_string()),
            ],
        )?;
        Ok(())
    }

    fn get_account(&self, id: AccountId) -> Result<Option<Account>, LedgerError> {
        let row = self
            .conn
            .query_row(
                "SELECT name, currency, kind, opening_balance, archived, current_value
                 FROM accounts WHERE id = ?1",
                rusqlite::params![id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((name, currency, kind, opening_balance, archived, current_value)) = row else {
            return Ok(None);
        };
        Ok(Some(Account {
            id,
            name,
            currency: currency_from_text(&currency)?,
            kind: account_kind_from_str(&kind)?,
            opening_balance: decimal_from_text(&opening_balance)?,
            archived: archived != 0,
            current_value: current_value.map(|v| decimal_from_text(&v)).transpose()?,
        }))
    }

    fn all_accounts(&self) -> Result<Vec<Account>, LedgerError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, currency, kind, opening_balance, archived, current_value FROM accounts",
        )?;
        let raws = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        raws.into_iter()
            .map(
                |(id, name, currency, kind, opening_balance, archived, current_value)| {
                    Ok(Account {
                        id: id_from_text(&id)?,
                        name,
                        currency: currency_from_text(&currency)?,
                        kind: account_kind_from_str(&kind)?,
                        opening_balance: decimal_from_text(&opening_balance)?,
                        archived: archived != 0,
                        current_value: current_value.map(|v| decimal_from_text(&v)).transpose()?,
                    })
                },
            )
            .collect()
    }

    fn save_entry(&mut self, entry: Entry) -> Result<(), LedgerError> {
        let tx = self.conn.savepoint()?;
        tx.execute(
            "INSERT INTO entries
                (id, account_id, date, time, amount, currency, description, note, category,
                 pot_id, transfer_account_id, source, bank_state, confirmed, voided_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET
                account_id = excluded.account_id, date = excluded.date, time = excluded.time,
                amount = excluded.amount, currency = excluded.currency,
                description = excluded.description, note = excluded.note, category = excluded.category,
                pot_id = excluded.pot_id, transfer_account_id = excluded.transfer_account_id,
                source = excluded.source, bank_state = excluded.bank_state,
                confirmed = excluded.confirmed, voided_reason = excluded.voided_reason",
            rusqlite::params![
                entry.id.to_string(),
                entry.account_id.to_string(),
                entry.date.to_string(),
                entry.time.map(|t| t.to_string()),
                entry.amount.to_string(),
                entry.currency.code(),
                entry.description,
                entry.note,
                entry.category,
                entry.pot_id.map(|id| id.to_string()),
                entry.transfer_account_id.map(|id| id.to_string()),
                entry_source_to_str(entry.source),
                bank_state_to_str(entry.bank_state),
                entry.confirmed as i64,
                entry.voided_reason,
            ],
        )?;
        tx.execute(
            "DELETE FROM entry_tags WHERE entry_id = ?1",
            rusqlite::params![entry.id.to_string()],
        )?;
        let mut seen = std::collections::HashSet::new();
        for tag in &entry.tags {
            if seen.insert(tag) {
                tx.execute(
                    "INSERT INTO entry_tags (entry_id, tag) VALUES (?1, ?2)",
                    rusqlite::params![entry.id.to_string(), tag],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn get_entry(&self, id: EntryId) -> Result<Option<Entry>, LedgerError> {
        let row = self
            .conn
            .query_row(
                "SELECT account_id, date, time, amount, currency, description, note, category,
                        pot_id, transfer_account_id, source, bank_state, confirmed, voided_reason
                 FROM entries WHERE id = ?1",
                rusqlite::params![id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, String>(11)?,
                        row.get::<_, i64>(12)?,
                        row.get::<_, Option<String>>(13)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            account_id,
            date,
            time,
            amount,
            currency,
            description,
            note,
            category,
            pot_id,
            transfer_account_id,
            source,
            bank_state,
            confirmed,
            voided_reason,
        )) = row
        else {
            return Ok(None);
        };
        let tags = self.tags_for_entry(id)?;
        Ok(Some(Entry {
            id,
            account_id: id_from_text(&account_id)?,
            date: date_from_text(&date)?,
            time: time.map(|t| time_from_text(&t)).transpose()?,
            amount: decimal_from_text(&amount)?,
            currency: currency_from_text(&currency)?,
            description,
            note,
            category,
            tags,
            pot_id: pot_id.map(|s| id_from_text(&s)).transpose()?,
            transfer_account_id: transfer_account_id.map(|s| id_from_text(&s)).transpose()?,
            source: entry_source_from_str(&source)?,
            bank_state: bank_state_from_str(&bank_state)?,
            confirmed: confirmed != 0,
            voided_reason,
        }))
    }

    fn entries_for_account(&self, account_id: AccountId) -> Result<Vec<Entry>, LedgerError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM entries WHERE account_id = ?1")?;
        let ids: Vec<EntryId> = stmt
            .query_map(rusqlite::params![account_id.to_string()], |row| {
                row.get::<_, String>(0)
            })?
            .map(|id_str| -> Result<EntryId, LedgerError> { id_from_text(&id_str?) })
            .collect::<Result<Vec<EntryId>, LedgerError>>()?;
        ids.into_iter()
            .map(|id| {
                self.get_entry(id)?.ok_or_else(|| {
                    LedgerError::Storage(format!(
                        "entry {id} listed by id but could not be read back"
                    ))
                })
            })
            .collect()
    }

    fn entries_for_pot(&self, pot_id: PotId) -> Result<Vec<Entry>, LedgerError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM entries WHERE pot_id = ?1")?;
        let ids: Vec<EntryId> = stmt
            .query_map(rusqlite::params![pot_id.to_string()], |row| {
                row.get::<_, String>(0)
            })?
            .map(|id_str| -> Result<EntryId, LedgerError> { id_from_text(&id_str?) })
            .collect::<Result<Vec<EntryId>, LedgerError>>()?;
        ids.into_iter()
            .map(|id| {
                self.get_entry(id)?.ok_or_else(|| {
                    LedgerError::Storage(format!(
                        "entry {id} listed by id but could not be read back"
                    ))
                })
            })
            .collect()
    }

    fn save_entry_parts(
        &mut self,
        entry_id: EntryId,
        parts: Vec<EntryPart>,
    ) -> Result<(), LedgerError> {
        let tx = self.conn.savepoint()?;
        tx.execute(
            "DELETE FROM entry_parts WHERE entry_id = ?1",
            rusqlite::params![entry_id.to_string()],
        )?;
        parts
            .into_iter()
            .try_for_each(|part| -> Result<(), LedgerError> {
                tx.execute(
                    "INSERT INTO entry_parts (id, entry_id, amount, category, transfer_account_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        part.id.to_string(),
                        entry_id.to_string(),
                        part.amount.to_string(),
                        part.category,
                        part.transfer_account_id.map(|id| id.to_string()),
                    ],
                )?;
                Ok(())
            })?;
        tx.commit()?;
        Ok(())
    }

    fn parts_for_entry(&self, entry_id: EntryId) -> Result<Vec<EntryPart>, LedgerError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, amount, category, transfer_account_id FROM entry_parts WHERE entry_id = ?1",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![entry_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        rows.into_iter()
            .map(|(id, amount, category, transfer_account_id)| {
                Ok(EntryPart {
                    id: id_from_text(&id)?,
                    entry_id,
                    amount: decimal_from_text(&amount)?,
                    category,
                    transfer_account_id: transfer_account_id
                        .map(|s| id_from_text(&s))
                        .transpose()?,
                })
            })
            .collect()
    }

    fn save_pot(&mut self, pot: Pot) -> Result<(), LedgerError> {
        self.conn.execute(
            "INSERT INTO pots (id, name, currency, target, priority)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name, currency = excluded.currency,
                target = excluded.target, priority = excluded.priority",
            rusqlite::params![
                pot.id.to_string(),
                pot.name,
                pot.currency.code(),
                pot.target.map(|v| v.to_string()),
                pot.priority,
            ],
        )?;
        Ok(())
    }

    fn get_pot(&self, id: PotId) -> Result<Option<Pot>, LedgerError> {
        let row = self
            .conn
            .query_row(
                "SELECT name, currency, target, priority FROM pots WHERE id = ?1",
                rusqlite::params![id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<i32>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((name, currency, target, priority)) = row else {
            return Ok(None);
        };
        Ok(Some(Pot {
            id,
            name,
            currency: currency_from_text(&currency)?,
            target: target.map(|t| decimal_from_text(&t)).transpose()?,
            priority,
        }))
    }

    fn all_pots(&self) -> Result<Vec<Pot>, LedgerError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, currency, target, priority FROM pots")?;
        let raws = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i32>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        raws.into_iter()
            .map(|(id, name, currency, target, priority)| {
                Ok(Pot {
                    id: id_from_text(&id)?,
                    name,
                    currency: currency_from_text(&currency)?,
                    target: target.map(|t| decimal_from_text(&t)).transpose()?,
                    priority,
                })
            })
            .collect()
    }

    fn delete_pot(&mut self, id: PotId) -> Result<(), LedgerError> {
        self.conn
            .execute("DELETE FROM pots WHERE id = ?1", [id.to_string()])?;
        Ok(())
    }

    fn save_allocation(&mut self, allocation: Allocation) -> Result<(), LedgerError> {
        self.conn.execute(
            "INSERT INTO allocations (id, pot_id, amount, date, note)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                pot_id = excluded.pot_id, amount = excluded.amount,
                date = excluded.date, note = excluded.note",
            rusqlite::params![
                allocation.id.to_string(),
                allocation.pot_id.to_string(),
                allocation.amount.to_string(),
                allocation.date.to_string(),
                allocation.note,
            ],
        )?;
        Ok(())
    }

    fn allocations_for_pot(&self, pot_id: PotId) -> Result<Vec<Allocation>, LedgerError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, amount, date, note FROM allocations WHERE pot_id = ?1")?;
        let rows = stmt
            .query_map(rusqlite::params![pot_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        rows.into_iter()
            .map(|(id, amount, date, note)| {
                Ok(Allocation {
                    id: id_from_text(&id)?,
                    pot_id,
                    amount: decimal_from_text(&amount)?,
                    date: date_from_text(&date)?,
                    note,
                })
            })
            .collect()
    }

    fn delete_allocations_for_pot(&mut self, pot_id: PotId) -> Result<(), LedgerError> {
        self.conn.execute(
            "DELETE FROM allocations WHERE pot_id = ?1",
            [pot_id.to_string()],
        )?;
        Ok(())
    }

    // Unlike every other save_* method, this is a plain INSERT with no
    // ON CONFLICT DO UPDATE, and that is intentional: a valuation is a
    // historical fact recording one value change, never edited afterwards.
    // The Ledger engine always calls update_current_value with a freshly
    // generated id for each new valuation, so a save with a colliding id
    // should never happen in normal use; if it does, this returns a
    // Storage error like any other write failure, rather than silently
    // overwriting the earlier valuation.
    fn save_valuation(&mut self, valuation: Valuation) -> Result<(), LedgerError> {
        self.conn.execute(
            "INSERT INTO valuations (id, account_id, date, old_value, new_value, category)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                valuation.id.to_string(),
                valuation.account_id.to_string(),
                valuation.date.to_string(),
                valuation.old_value.to_string(),
                valuation.new_value.to_string(),
                valuation.category,
            ],
        )?;
        Ok(())
    }

    fn valuations_for_account(&self, account_id: AccountId) -> Result<Vec<Valuation>, LedgerError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, date, old_value, new_value, category FROM valuations WHERE account_id = ?1",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![account_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        rows.into_iter()
            .map(|(id, date, old_value, new_value, category)| {
                Ok(Valuation {
                    id: id_from_text(&id)?,
                    account_id,
                    date: date_from_text(&date)?,
                    old_value: decimal_from_text(&old_value)?,
                    new_value: decimal_from_text(&new_value)?,
                    category,
                })
            })
            .collect()
    }

    fn save_import(&mut self, import: Import) -> Result<(), LedgerError> {
        self.conn.execute(
            &format!(
                "INSERT INTO imports ({IMPORT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                    account_id = excluded.account_id, currency = excluded.currency,
                    file_name = excluded.file_name, uploaded_at = excluded.uploaded_at,
                    rows_read = excluded.rows_read, opening_balance = excluded.opening_balance,
                    closing_balance = excluded.closing_balance, completed = excluded.completed"
            ),
            rusqlite::params![
                import.id.to_string(),
                import.account_id.to_string(),
                import.currency.code(),
                import.file_name,
                import.uploaded_at.to_rfc3339(),
                import.rows_read,
                import.opening_balance.to_string(),
                import.closing_balance.to_string(),
                import.completed as i64,
            ],
        )?;
        Ok(())
    }

    fn get_import(&self, id: ImportId) -> Result<Option<Import>, LedgerError> {
        Ok(self
            .query_imports(
                &format!("SELECT {IMPORT_COLUMNS} FROM imports WHERE id = ?1"),
                [id.to_string()],
            )?
            .pop())
    }

    fn all_imports(&self) -> Result<Vec<Import>, LedgerError> {
        self.query_imports(
            &format!("SELECT {IMPORT_COLUMNS} FROM imports ORDER BY uploaded_at"),
            [],
        )
    }

    fn delete_import(&mut self, id: ImportId) -> Result<(), LedgerError> {
        self.conn
            .execute("DELETE FROM imports WHERE id = ?1", [id.to_string()])?;
        Ok(())
    }

    fn incomplete_import_for_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<Import>, LedgerError> {
        Ok(self
            .query_imports(
                &format!("SELECT {IMPORT_COLUMNS} FROM imports WHERE account_id = ?1 AND completed = 0 LIMIT 1"),
                [account_id.to_string()],
            )?
            .pop())
    }

    fn save_queue_row(&mut self, row: ImportQueueRow) -> Result<(), LedgerError> {
        let columns = QueueRowColumns::from(row.detail);
        self.conn.execute(
            &format!(
                "INSERT INTO import_queue_rows ({QUEUE_ROW_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                    import_id = excluded.import_id, kind = excluded.kind, date = excluded.date,
                    time = excluded.time, amount = excluded.amount, currency = excluded.currency,
                    description = excluded.description, bank_state = excluded.bank_state,
                    category = excluded.category"
            ),
            rusqlite::params![
                row.id.to_string(),
                row.import_id.to_string(),
                columns.kind,
                row.date.to_string(),
                row.time.map(|t| t.to_string()),
                row.amount.to_string(),
                row.currency.code(),
                columns.description,
                columns.bank_state,
                columns.category,
            ],
        )?;
        Ok(())
    }

    fn get_queue_row(&self, id: QueueRowId) -> Result<Option<ImportQueueRow>, LedgerError> {
        Ok(self
            .query_queue_rows(
                &format!("SELECT {QUEUE_ROW_COLUMNS} FROM import_queue_rows WHERE id = ?1"),
                [id.to_string()],
            )?
            .pop())
    }

    fn queue_rows_for_import(
        &self,
        import_id: ImportId,
    ) -> Result<Vec<ImportQueueRow>, LedgerError> {
        self.query_queue_rows(
            &format!("SELECT {QUEUE_ROW_COLUMNS} FROM import_queue_rows WHERE import_id = ?1 ORDER BY date, time"),
            [import_id.to_string()],
        )
    }

    fn delete_queue_row(&mut self, id: QueueRowId) -> Result<(), LedgerError> {
        self.conn.execute(
            "DELETE FROM import_queue_rows WHERE id = ?1",
            [id.to_string()],
        )?;
        Ok(())
    }

    fn save_queue_row_match(&mut self, m: ImportQueueRowMatch) -> Result<(), LedgerError> {
        let (entry_id, row_id) = match m.target {
            MatchTarget::Entry { entry_id } => (Some(entry_id.to_string()), None),
            MatchTarget::QueueRow { queue_row_id } => (None, Some(queue_row_id.to_string())),
        };
        self.conn.execute(
            &format!(
                "INSERT INTO import_queue_row_matches ({MATCH_COLUMNS}) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET
                    queue_row_id = excluded.queue_row_id, matched_entry_id = excluded.matched_entry_id,
                    matched_queue_row_id = excluded.matched_queue_row_id"
            ),
            rusqlite::params![m.id.to_string(), m.queue_row_id.to_string(), entry_id, row_id],
        )?;
        Ok(())
    }

    fn get_queue_row_match(&self, id: MatchId) -> Result<Option<ImportQueueRowMatch>, LedgerError> {
        Ok(self
            .query_matches(
                &format!("SELECT {MATCH_COLUMNS} FROM import_queue_row_matches WHERE id = ?1"),
                [id.to_string()],
            )?
            .pop())
    }

    fn queue_row_matches_referencing(
        &self,
        queue_row_id: QueueRowId,
    ) -> Result<Vec<ImportQueueRowMatch>, LedgerError> {
        let id = queue_row_id.to_string();
        self.query_matches(
            &format!(
                "SELECT {MATCH_COLUMNS} FROM import_queue_row_matches
                 WHERE queue_row_id = ?1 OR matched_queue_row_id = ?1"
            ),
            rusqlite::params![id],
        )
    }

    fn queue_row_matches_for_import(
        &self,
        import_id: ImportId,
    ) -> Result<Vec<ImportQueueRowMatch>, LedgerError> {
        self.query_matches(
            "SELECT m.id, m.queue_row_id, m.matched_entry_id, m.matched_queue_row_id
             FROM import_queue_row_matches m
             JOIN import_queue_rows r ON r.id = m.queue_row_id
             WHERE r.import_id = ?1",
            rusqlite::params![import_id.to_string()],
        )
    }

    fn delete_queue_row_match(&mut self, id: MatchId) -> Result<(), LedgerError> {
        self.conn.execute(
            "DELETE FROM import_queue_row_matches WHERE id = ?1",
            [id.to_string()],
        )?;
        Ok(())
    }

    fn repoint_queue_row_matches(
        &mut self,
        from_queue_row_id: QueueRowId,
        to_entry_id: EntryId,
    ) -> Result<(), LedgerError> {
        self.conn.execute(
            "UPDATE import_queue_row_matches
             SET matched_queue_row_id = NULL, matched_entry_id = ?2
             WHERE matched_queue_row_id = ?1",
            rusqlite::params![from_queue_row_id.to_string(), to_entry_id.to_string()],
        )?;
        Ok(())
    }

    // A raw SAVEPOINT rather than rusqlite's Transaction wrapper: the
    // wrapper borrows self.conn for its own lifetime, which would leave no
    // way to also lend `self` to `f`. SAVEPOINT also nests, unlike BEGIN, so
    // it stays correct even though save_entry/save_entry_parts open their
    // own savepoint when called from inside `f`.
    fn transaction<F, T>(&mut self, f: F) -> Result<T, LedgerError>
    where
        F: FnOnce(&mut Self) -> Result<T, LedgerError>,
    {
        self.conn.execute_batch("SAVEPOINT ledger_txn")?;
        match f(self) {
            Ok(value) => {
                self.conn.execute_batch("RELEASE SAVEPOINT ledger_txn")?;
                Ok(value)
            }
            Err(err) => {
                let _ = self.conn.execute_batch(
                    "ROLLBACK TO SAVEPOINT ledger_txn; RELEASE SAVEPOINT ledger_txn",
                );
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{BankState, Entry, EntryPart, EntrySource};
    use crate::error::LedgerError;
    use crate::id::{AllocationId, EntryPartId, ValuationId};
    use crate::valuation::Valuation;
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    fn a_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
    }

    fn sample_entry(account_id: AccountId) -> Entry {
        Entry {
            id: EntryId::generate(),
            account_id,
            date: a_date(),
            time: None,
            amount: dec!(-20),
            currency: Currency::new("EUR").unwrap(),
            description: "Groceries".to_string(),
            note: None,
            category: None,
            tags: vec!["one".to_string(), "two".to_string()],
            pot_id: None,
            transfer_account_id: None,
            source: EntrySource::Manual,
            bank_state: BankState::Completed,
            confirmed: false,
            voided_reason: None,
        }
    }

    use crate::store::import_contract as contract;

    #[test]
    fn an_import_round_trips() {
        contract::an_import_round_trips(SqliteStore::open_in_memory().unwrap());
    }

    #[test]
    fn resaving_an_import_updates_it() {
        contract::resaving_an_import_updates_it(SqliteStore::open_in_memory().unwrap());
    }

    #[test]
    fn incomplete_import_for_account_ignores_completed_and_other_accounts() {
        contract::incomplete_import_for_account_ignores_completed_and_other_accounts(
            SqliteStore::open_in_memory().unwrap(),
        );
    }

    #[test]
    fn a_queue_row_round_trips() {
        contract::a_queue_row_round_trips(SqliteStore::open_in_memory().unwrap());
    }

    #[test]
    fn queue_rows_come_back_for_their_import_by_date_then_time() {
        contract::queue_rows_come_back_for_their_import_by_date_then_time(
            SqliteStore::open_in_memory().unwrap(),
        );
    }

    #[test]
    fn matches_referencing_a_row_come_from_either_side() {
        contract::matches_referencing_a_row_come_from_either_side(
            SqliteStore::open_in_memory().unwrap(),
        );
    }

    #[test]
    fn matches_for_an_import_are_those_of_its_own_rows() {
        contract::matches_for_an_import_are_those_of_its_own_rows(
            SqliteStore::open_in_memory().unwrap(),
        );
    }

    #[test]
    fn repointing_rewrites_only_rows_that_pointed_at_the_queue_row() {
        contract::repointing_rewrites_only_rows_that_pointed_at_the_queue_row(
            SqliteStore::open_in_memory().unwrap(),
        );
    }

    #[test]
    fn a_match_pointing_at_a_nonexistent_entry_is_rejected_by_the_store() {
        // The MatchTarget enum enforces exactly-one-target at the Rust level.
        // This test confirms the SQLite FK constraint also rejects a match
        // whose target entry does not exist.
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).unwrap();
        let import = contract::an_import(account.id);
        store.save_import(import.clone()).unwrap();
        let row = contract::a_row(import.id, 1, None);
        store.save_queue_row(row.clone()).unwrap();
        let dangling = ImportQueueRowMatch::to_entry(row.id, EntryId::generate());
        assert!(store.save_queue_row_match(dangling).is_err());
    }

    #[test]
    fn queue_rows_survive_closing_and_reopening_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        let row;
        {
            let mut store = SqliteStore::open(&path).unwrap();
            let account = Account::new(
                "Checking",
                Currency::new("EUR").unwrap(),
                AccountKind::Own,
                dec!(0),
            );
            store.save_account(account.clone()).unwrap();
            let import = contract::an_import(account.id);
            store.save_import(import.clone()).unwrap();
            row = contract::a_row(import.id, 4, Some((1, 2, 3)));
            store.save_queue_row(row.clone()).unwrap();
        }
        let reopened = SqliteStore::open(&path).unwrap();
        assert_eq!(reopened.queue_rows_for_import(row.import_id), Ok(vec![row]));
    }

    #[test]
    fn opening_in_memory_runs_the_migration_without_error() {
        SqliteStore::open_in_memory().unwrap();
    }

    #[test]
    fn saved_account_can_be_read_back_by_id() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(100.50),
        );
        let id = account.id;
        store.save_account(account.clone()).unwrap();
        assert_eq!(store.get_account(id), Ok(Some(account)));
    }

    #[test]
    fn a_negative_decimal_round_trips_exactly() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(-1042.50),
        );
        let id = account.id;
        store.save_account(account).unwrap();
        assert_eq!(
            store.get_account(id).unwrap().unwrap().opening_balance,
            dec!(-1042.50)
        );
    }

    #[test]
    fn a_large_amount_with_many_decimal_places_round_trips_exactly() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(1234567.123456789),
        );
        let id = account.id;
        store.save_account(account).unwrap();
        assert_eq!(
            store.get_account(id).unwrap().unwrap().opening_balance,
            dec!(1234567.123456789)
        );
    }

    #[test]
    fn an_entry_with_a_time_round_trips_exactly() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).unwrap();
        let mut entry = sample_entry(account.id);
        entry.time = Some(chrono::NaiveTime::from_hms_opt(14, 32, 7).unwrap());
        store.save_entry(entry.clone()).unwrap();
        assert_eq!(store.get_entry(entry.id).unwrap().unwrap().time, entry.time);
    }

    #[test]
    fn an_entry_with_a_time_including_fractional_seconds_round_trips_exactly() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).unwrap();
        let mut entry = sample_entry(account.id);
        entry.time = Some(chrono::NaiveTime::from_hms_milli_opt(14, 32, 7, 250).unwrap());
        store.save_entry(entry.clone()).unwrap();
        assert_eq!(store.get_entry(entry.id).unwrap().unwrap().time, entry.time);
    }

    #[test]
    fn saving_a_valuation_with_a_reused_id_returns_an_error_rather_than_silently_overwriting() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "An ETF position",
            Currency::new("EUR").unwrap(),
            AccountKind::Investment,
            dec!(1000),
        );
        store.save_account(account.clone()).unwrap();
        let valuation = Valuation {
            id: ValuationId::generate(),
            account_id: account.id,
            date: a_date(),
            old_value: dec!(1000),
            new_value: dec!(1042),
            category: "Investment gain".to_string(),
        };
        store.save_valuation(valuation.clone()).unwrap();
        assert!(store.save_valuation(valuation).is_err());
    }

    #[test]
    fn unknown_id_returns_none() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.get_account(AccountId::generate()), Ok(None));
    }

    #[test]
    fn all_accounts_lists_every_saved_account() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let eur = Currency::new("EUR").unwrap();
        store
            .save_account(Account::new("A", eur.clone(), AccountKind::Own, dec!(0)))
            .unwrap();
        store
            .save_account(Account::new("B", eur, AccountKind::Investment, dec!(500)))
            .unwrap();
        assert_eq!(store.all_accounts().unwrap().len(), 2);
    }

    #[test]
    fn saving_an_account_with_the_same_id_again_replaces_it() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let mut account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).unwrap();
        account.archived = true;
        store.save_account(account.clone()).unwrap();
        assert_eq!(store.all_accounts().unwrap().len(), 1);
        assert!(store.get_account(account.id).unwrap().unwrap().archived);
    }

    #[test]
    fn a_failed_transaction_rolls_back_everything_it_wrote() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        let account_id = account.id;
        let result = store.transaction(|store| -> Result<(), LedgerError> {
            store.save_account(account.clone())?;
            Err(LedgerError::Storage(
                "simulated mid-transaction failure".to_string(),
            ))
        });
        assert!(result.is_err());
        assert_eq!(store.get_account(account_id).unwrap(), None);
    }

    #[test]
    fn a_successful_transaction_commits_every_write_it_made() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        let account_id = account.id;
        store
            .transaction(|store| store.save_account(account.clone()))
            .unwrap();
        assert!(store.get_account(account_id).unwrap().is_some());
    }

    #[test]
    fn data_survives_closing_and_reopening_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        let account_id;
        {
            let mut store = SqliteStore::open(&path).unwrap();
            let account = Account::new(
                "Checking",
                Currency::new("EUR").unwrap(),
                AccountKind::Own,
                dec!(250),
            );
            account_id = account.id;
            store.save_account(account).unwrap();
        }
        let reopened = SqliteStore::open(&path).unwrap();
        assert_eq!(
            reopened
                .get_account(account_id)
                .unwrap()
                .unwrap()
                .opening_balance,
            dec!(250)
        );
    }

    #[test]
    fn opening_the_same_file_twice_does_not_fail_on_the_second_migration_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        SqliteStore::open(&path).unwrap();
        SqliteStore::open(&path).unwrap();
    }

    #[test]
    fn a_saved_entry_round_trips_with_its_tags() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).unwrap();
        let entry = sample_entry(account.id);
        store.save_entry(entry.clone()).unwrap();
        let read_back = store.get_entry(entry.id).unwrap().unwrap();
        assert_eq!(read_back.amount, dec!(-20));
        assert_eq!(read_back.description, "Groceries");
        let mut tags = read_back.tags.clone();
        tags.sort();
        assert_eq!(tags, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn saving_an_entry_with_a_duplicate_tag_deduplicates_rather_than_erroring() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).unwrap();
        let mut entry = sample_entry(account.id);
        entry.tags = vec!["food".to_string(), "food".to_string()];
        store.save_entry(entry.clone()).unwrap();
        let read_back = store.get_entry(entry.id).unwrap().unwrap();
        assert_eq!(read_back.tags, vec!["food".to_string()]);
    }

    #[test]
    fn entries_for_account_returns_only_that_accounts_entries() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let eur = Currency::new("EUR").unwrap();
        let a = Account::new("A", eur.clone(), AccountKind::Own, dec!(0));
        let b = Account::new("B", eur, AccountKind::Own, dec!(0));
        store.save_account(a.clone()).unwrap();
        store.save_account(b.clone()).unwrap();
        store.save_entry(sample_entry(a.id)).unwrap();
        store.save_entry(sample_entry(a.id)).unwrap();
        store.save_entry(sample_entry(b.id)).unwrap();
        assert_eq!(store.entries_for_account(a.id).unwrap().len(), 2);
        assert_eq!(store.entries_for_account(b.id).unwrap().len(), 1);
    }

    #[test]
    fn an_account_with_no_entries_returns_an_empty_list() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(
            store
                .entries_for_account(AccountId::generate())
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn entries_for_pot_returns_only_entries_tagged_to_that_pot() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).unwrap();
        let pot_id = PotId::generate();
        let mut tagged = sample_entry(account.id);
        tagged.pot_id = Some(pot_id);
        store.save_entry(tagged).unwrap();
        store.save_entry(sample_entry(account.id)).unwrap();
        assert_eq!(store.entries_for_pot(pot_id).unwrap().len(), 1);
    }

    #[test]
    fn a_pot_with_no_tagged_entries_returns_an_empty_list() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.entries_for_pot(PotId::generate()).unwrap().len(), 0);
    }

    #[test]
    fn resaving_an_entry_replaces_its_tags_rather_than_appending() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).unwrap();
        let mut entry = sample_entry(account.id);
        store.save_entry(entry.clone()).unwrap();
        entry.tags = vec!["only-this-one".to_string()];
        store.save_entry(entry.clone()).unwrap();
        assert_eq!(
            store.get_entry(entry.id).unwrap().unwrap().tags,
            vec!["only-this-one".to_string()]
        );
    }

    #[test]
    fn a_full_ledger_balance_works_against_sqlite_storage() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(100))
            .unwrap();
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "Groceries")
            .unwrap();
        assert_eq!(ledger.account_balance(account.id), Ok(dec!(80)));
    }

    #[test]
    fn saved_parts_can_be_read_back_for_their_entry() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).unwrap();
        let entry = sample_entry(account.id);
        store.save_entry(entry.clone()).unwrap();
        let parts = vec![
            EntryPart {
                id: EntryPartId::generate(),
                entry_id: entry.id,
                amount: dec!(-15),
                category: Some("Loan".to_string()),
                transfer_account_id: None,
            },
            EntryPart {
                id: EntryPartId::generate(),
                entry_id: entry.id,
                amount: dec!(-5),
                category: Some("Gifts".to_string()),
                transfer_account_id: None,
            },
        ];
        store.save_entry_parts(entry.id, parts.clone()).unwrap();
        let mut read_back = store.parts_for_entry(entry.id).unwrap();
        read_back.sort_by_key(|p| p.amount);
        let mut expected = parts;
        expected.sort_by_key(|p| p.amount);
        assert_eq!(read_back, expected);
    }

    #[test]
    fn an_entry_with_no_parts_returns_an_empty_list() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.parts_for_entry(EntryId::generate()).unwrap().len(), 0);
    }

    #[test]
    fn resaving_parts_replaces_the_old_set() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        store.save_account(account.clone()).unwrap();
        let entry = sample_entry(account.id);
        store.save_entry(entry.clone()).unwrap();
        store
            .save_entry_parts(
                entry.id,
                vec![EntryPart {
                    id: EntryPartId::generate(),
                    entry_id: entry.id,
                    amount: dec!(-20),
                    category: None,
                    transfer_account_id: None,
                }],
            )
            .unwrap();
        store
            .save_entry_parts(
                entry.id,
                vec![EntryPart {
                    id: EntryPartId::generate(),
                    entry_id: entry.id,
                    amount: dec!(-10),
                    category: None,
                    transfer_account_id: None,
                }],
            )
            .unwrap();
        assert_eq!(store.parts_for_entry(entry.id).unwrap().len(), 1);
    }

    #[test]
    fn splitting_a_stored_entry_works_through_the_ledger() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger
            .open_account("Checking", eur, AccountKind::Own, dec!(0))
            .unwrap();
        let entry = ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "Mixed")
            .unwrap();
        let parts = ledger
            .split_entry(
                entry.id,
                vec![(dec!(-15), Some("Loan".to_string())), (dec!(-5), None)],
            )
            .unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(ledger.parts_for_entry(entry.id).unwrap().len(), 2);
    }

    #[test]
    fn a_saved_pot_can_be_read_back() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let pot = Pot {
            id: PotId::generate(),
            name: "Emergency fund".to_string(),
            currency: Currency::new("EUR").unwrap(),
            target: Some(dec!(2000)),
            priority: Some(1),
        };
        store.save_pot(pot.clone()).unwrap();
        assert_eq!(store.get_pot(pot.id), Ok(Some(pot)));
    }

    #[test]
    fn a_pot_with_no_target_or_priority_round_trips_as_none() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let pot = Pot {
            id: PotId::generate(),
            name: "Siblings".to_string(),
            currency: Currency::new("EUR").unwrap(),
            target: None,
            priority: None,
        };
        store.save_pot(pot.clone()).unwrap();
        let read_back = store.get_pot(pot.id).unwrap().unwrap();
        assert_eq!(read_back.target, None);
        assert_eq!(read_back.priority, None);
    }

    #[test]
    fn all_pots_lists_every_saved_pot() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let eur = Currency::new("EUR").unwrap();
        store
            .save_pot(Pot {
                id: PotId::generate(),
                name: "A".to_string(),
                currency: eur.clone(),
                target: None,
                priority: None,
            })
            .unwrap();
        store
            .save_pot(Pot {
                id: PotId::generate(),
                name: "B".to_string(),
                currency: eur,
                target: None,
                priority: None,
            })
            .unwrap();
        assert_eq!(store.all_pots().unwrap().len(), 2);
    }

    #[test]
    fn allocations_for_a_pot_with_none_is_empty() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(
            store.allocations_for_pot(PotId::generate()).unwrap().len(),
            0
        );
    }

    #[test]
    fn saved_allocations_can_be_read_back_for_their_pot() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let pot = Pot {
            id: PotId::generate(),
            name: "A".to_string(),
            currency: Currency::new("EUR").unwrap(),
            target: None,
            priority: None,
        };
        store.save_pot(pot.clone()).unwrap();
        let allocation = Allocation {
            id: AllocationId::generate(),
            pot_id: pot.id,
            amount: dec!(100),
            date: a_date(),
            note: None,
        };
        store.save_allocation(allocation.clone()).unwrap();
        assert_eq!(store.allocations_for_pot(pot.id), Ok(vec![allocation]));
    }

    #[test]
    fn deleting_a_pot_removes_it_from_all_pots() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let pot = Pot {
            id: PotId::generate(),
            name: "A".to_string(),
            currency: Currency::new("EUR").unwrap(),
            target: None,
            priority: None,
        };
        store.save_pot(pot.clone()).unwrap();
        store.delete_pot(pot.id).unwrap();
        assert_eq!(store.get_pot(pot.id), Ok(None));
        assert_eq!(store.all_pots().unwrap().len(), 0);
    }

    #[test]
    fn deleting_a_pots_allocations_leaves_other_pots_untouched() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let eur = Currency::new("EUR").unwrap();
        let pot = Pot {
            id: PotId::generate(),
            name: "A".to_string(),
            currency: eur.clone(),
            target: None,
            priority: None,
        };
        let other_pot = Pot {
            id: PotId::generate(),
            name: "B".to_string(),
            currency: eur,
            target: None,
            priority: None,
        };
        store.save_pot(pot.clone()).unwrap();
        store.save_pot(other_pot.clone()).unwrap();
        store
            .save_allocation(Allocation {
                id: AllocationId::generate(),
                pot_id: pot.id,
                amount: dec!(100),
                date: a_date(),
                note: None,
            })
            .unwrap();
        let kept = Allocation {
            id: AllocationId::generate(),
            pot_id: other_pot.id,
            amount: dec!(50),
            date: a_date(),
            note: None,
        };
        store.save_allocation(kept.clone()).unwrap();

        store.delete_allocations_for_pot(pot.id).unwrap();

        assert_eq!(store.allocations_for_pot(pot.id).unwrap().len(), 0);
        assert_eq!(store.allocations_for_pot(other_pot.id), Ok(vec![kept]));
    }

    #[test]
    fn allocating_and_general_savings_work_through_the_ledger_on_sqlite() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        ledger
            .open_account("Checking", eur.clone(), AccountKind::Own, dec!(100))
            .unwrap();
        let pot = ledger
            .open_pot("Emergency fund", eur.clone(), None, None)
            .unwrap();
        ledger.allocate_to_pot(pot.id, dec!(100), a_date()).unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(100)));
        assert_eq!(ledger.general_savings(&eur), Ok(dec!(0)));
        let over = ledger.allocate_to_pot(pot.id, dec!(0.01), a_date());
        assert_eq!(over, Err(LedgerError::GeneralSavingsWouldGoNegative(eur)));
    }

    #[test]
    fn a_saved_valuation_can_be_read_back() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new(
            "An ETF position",
            Currency::new("EUR").unwrap(),
            AccountKind::Investment,
            dec!(1000),
        );
        store.save_account(account.clone()).unwrap();
        let valuation = Valuation {
            id: ValuationId::generate(),
            account_id: account.id,
            date: a_date(),
            old_value: dec!(1000),
            new_value: dec!(1042),
            category: "Investment gain".to_string(),
        };
        store.save_valuation(valuation.clone()).unwrap();
        assert_eq!(
            store.valuations_for_account(account.id),
            Ok(vec![valuation])
        );
    }

    #[test]
    fn an_account_with_no_valuations_returns_an_empty_list() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(
            store
                .valuations_for_account(AccountId::generate())
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn updating_current_value_persists_through_sqlite() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let etf = ledger
            .open_account("An ETF position", eur, AccountKind::Investment, dec!(1000))
            .unwrap();
        let valuation = ledger
            .update_current_value(etf.id, dec!(1042), "Investment gain", a_date())
            .unwrap();
        assert_eq!(valuation.gain(), dec!(42));
        assert_eq!(ledger.current_value(etf.id), Ok(dec!(1042)));
    }

    #[test]
    fn a_cross_currency_transfer_persists_correctly_through_sqlite() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let ngn = Currency::new("NGN").unwrap();
        let a = ledger
            .open_account("A", eur, AccountKind::Own, dec!(200))
            .unwrap();
        let b = ledger
            .open_account("B", ngn, AccountKind::Own, dec!(0))
            .unwrap();
        ledger
            .transfer(crate::transfer::Transfer::new(
                crate::transfer::TransferLeg::new(a.id, dec!(200)),
                crate::transfer::TransferLeg::new(b.id, dec!(370000)),
                a_date(),
                "move",
            ))
            .unwrap();
        assert_eq!(ledger.account_balance(a.id), Ok(dec!(0)));
        assert_eq!(ledger.account_balance(b.id), Ok(dec!(370000)));
    }
}
