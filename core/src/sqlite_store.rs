use crate::account::{Account, AccountKind};
use crate::currency::Currency;
use crate::entry::{BankState, Entry, EntryPart, EntrySource};
use crate::error::LedgerError;
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
    Migrations::new(vec![M::up(include_str!("../migrations/0001_initial.sql"))])
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
        other => Err(LedgerError::Storage(format!("unknown account kind {other:?} stored in the database"))),
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
        other => Err(LedgerError::Storage(format!("unknown entry source {other:?} stored in the database"))),
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
        other => Err(LedgerError::Storage(format!("unknown bank state {other:?} stored in the database"))),
    }
}

fn decimal_to_text(d: Decimal) -> String {
    d.to_string()
}

fn decimal_from_text(s: &str) -> Result<Decimal, LedgerError> {
    Decimal::from_str(s).map_err(|_| LedgerError::Storage(format!("stored value {s:?} is not a valid decimal amount")))
}

fn uuid_from_text(s: &str) -> Result<Uuid, LedgerError> {
    Uuid::parse_str(s).map_err(|_| LedgerError::Storage(format!("stored value {s:?} is not a valid UUID")))
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

impl SqliteStore {
    fn tags_for_entry(&self, entry_id: Uuid) -> Result<Vec<String>, LedgerError> {
        let mut stmt = self.conn.prepare("SELECT tag FROM entry_tags WHERE entry_id = ?1")?;
        let tags = stmt
            .query_map(rusqlite::params![entry_id.to_string()], |row| row.get::<_, String>(0))?
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
                decimal_to_text(account.opening_balance),
                account.archived as i64,
                account.current_value.map(decimal_to_text),
            ],
        )?;
        Ok(())
    }

    fn get_account(&self, id: Uuid) -> Result<Option<Account>, LedgerError> {
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
        let mut stmt = self.conn.prepare("SELECT id FROM accounts")?;
        let ids: Vec<Uuid> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .map(|id_str| -> Result<Uuid, LedgerError> { uuid_from_text(&id_str?) })
            .collect::<Result<Vec<Uuid>, LedgerError>>()?;
        ids.into_iter()
            .map(|id| {
                self.get_account(id)?
                    .ok_or_else(|| LedgerError::Storage(format!("account {id} listed by id but could not be read back")))
            })
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
                decimal_to_text(entry.amount),
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
        tx.execute("DELETE FROM entry_tags WHERE entry_id = ?1", rusqlite::params![entry.id.to_string()])?;
        let mut unique_tags = entry.tags.clone();
        unique_tags.sort();
        unique_tags.dedup();
        for tag in &unique_tags {
            tx.execute(
                "INSERT INTO entry_tags (entry_id, tag) VALUES (?1, ?2)",
                rusqlite::params![entry.id.to_string(), tag],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn get_entry(&self, id: Uuid) -> Result<Option<Entry>, LedgerError> {
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
            account_id: uuid_from_text(&account_id)?,
            date: date_from_text(&date)?,
            time: time.map(|t| time_from_text(&t)).transpose()?,
            amount: decimal_from_text(&amount)?,
            currency: currency_from_text(&currency)?,
            description,
            note,
            category,
            tags,
            pot_id: pot_id.map(|s| uuid_from_text(&s)).transpose()?,
            transfer_account_id: transfer_account_id.map(|s| uuid_from_text(&s)).transpose()?,
            source: entry_source_from_str(&source)?,
            bank_state: bank_state_from_str(&bank_state)?,
            confirmed: confirmed != 0,
            voided_reason,
        }))
    }

    fn entries_for_account(&self, account_id: Uuid) -> Result<Vec<Entry>, LedgerError> {
        let mut stmt = self.conn.prepare("SELECT id FROM entries WHERE account_id = ?1")?;
        let ids: Vec<Uuid> = stmt
            .query_map(rusqlite::params![account_id.to_string()], |row| row.get::<_, String>(0))?
            .map(|id_str| -> Result<Uuid, LedgerError> { uuid_from_text(&id_str?) })
            .collect::<Result<Vec<Uuid>, LedgerError>>()?;
        ids.into_iter()
            .map(|id| {
                self.get_entry(id)?
                    .ok_or_else(|| LedgerError::Storage(format!("entry {id} listed by id but could not be read back")))
            })
            .collect()
    }

    fn save_entry_parts(&mut self, entry_id: Uuid, parts: Vec<EntryPart>) -> Result<(), LedgerError> {
        let tx = self.conn.savepoint()?;
        tx.execute("DELETE FROM entry_parts WHERE entry_id = ?1", rusqlite::params![entry_id.to_string()])?;
        for part in &parts {
            tx.execute(
                "INSERT INTO entry_parts (id, entry_id, amount, category, transfer_account_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    part.id.to_string(),
                    entry_id.to_string(),
                    decimal_to_text(part.amount),
                    part.category,
                    part.transfer_account_id.map(|id| id.to_string()),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn parts_for_entry(&self, entry_id: Uuid) -> Result<Vec<EntryPart>, LedgerError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, amount, category, transfer_account_id FROM entry_parts WHERE entry_id = ?1")?;
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
                    id: uuid_from_text(&id)?,
                    entry_id,
                    amount: decimal_from_text(&amount)?,
                    category,
                    transfer_account_id: transfer_account_id.map(|s| uuid_from_text(&s)).transpose()?,
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
                pot.target.map(decimal_to_text),
                pot.priority,
            ],
        )?;
        Ok(())
    }

    fn get_pot(&self, id: Uuid) -> Result<Option<Pot>, LedgerError> {
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
        let mut stmt = self.conn.prepare("SELECT id FROM pots")?;
        let ids: Vec<Uuid> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .map(|id_str| -> Result<Uuid, LedgerError> { uuid_from_text(&id_str?) })
            .collect::<Result<Vec<Uuid>, LedgerError>>()?;
        ids.into_iter()
            .map(|id| {
                self.get_pot(id)?
                    .ok_or_else(|| LedgerError::Storage(format!("pot {id} listed by id but could not be read back")))
            })
            .collect()
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
                decimal_to_text(allocation.amount),
                allocation.date.to_string(),
                allocation.note,
            ],
        )?;
        Ok(())
    }

    fn allocations_for_pot(&self, pot_id: Uuid) -> Result<Vec<Allocation>, LedgerError> {
        let mut stmt = self.conn.prepare("SELECT id, amount, date, note FROM allocations WHERE pot_id = ?1")?;
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
                    id: uuid_from_text(&id)?,
                    pot_id,
                    amount: decimal_from_text(&amount)?,
                    date: date_from_text(&date)?,
                    note,
                })
            })
            .collect()
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
                decimal_to_text(valuation.old_value),
                decimal_to_text(valuation.new_value),
                valuation.category,
            ],
        )?;
        Ok(())
    }

    fn valuations_for_account(&self, account_id: Uuid) -> Result<Vec<Valuation>, LedgerError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, date, old_value, new_value, category FROM valuations WHERE account_id = ?1")?;
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
                    id: uuid_from_text(&id)?,
                    account_id,
                    date: date_from_text(&date)?,
                    old_value: decimal_from_text(&old_value)?,
                    new_value: decimal_from_text(&new_value)?,
                    category,
                })
            })
            .collect()
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
                let _ = self.conn.execute_batch("ROLLBACK TO SAVEPOINT ledger_txn; RELEASE SAVEPOINT ledger_txn");
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
    use crate::valuation::Valuation;
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    fn a_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
    }

    fn sample_entry(account_id: Uuid) -> Entry {
        Entry {
            id: Uuid::new_v4(),
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
        assert_eq!(store.get_account(id).unwrap().unwrap().opening_balance, dec!(-1042.50));
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
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
        store.save_account(account.clone()).unwrap();
        let mut entry = sample_entry(account.id);
        entry.time = Some(chrono::NaiveTime::from_hms_opt(14, 32, 7).unwrap());
        store.save_entry(entry.clone()).unwrap();
        assert_eq!(store.get_entry(entry.id).unwrap().unwrap().time, entry.time);
    }

    #[test]
    fn an_entry_with_a_time_including_fractional_seconds_round_trips_exactly() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
        store.save_account(account.clone()).unwrap();
        let mut entry = sample_entry(account.id);
        entry.time = Some(chrono::NaiveTime::from_hms_milli_opt(14, 32, 7, 250).unwrap());
        store.save_entry(entry.clone()).unwrap();
        assert_eq!(store.get_entry(entry.id).unwrap().unwrap().time, entry.time);
    }

    #[test]
    fn saving_a_valuation_with_a_reused_id_returns_an_error_rather_than_silently_overwriting() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("An ETF position", Currency::new("EUR").unwrap(), AccountKind::Investment, dec!(1000));
        store.save_account(account.clone()).unwrap();
        let valuation = Valuation {
            id: Uuid::new_v4(),
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
        assert_eq!(store.get_account(Uuid::new_v4()), Ok(None));
    }

    #[test]
    fn all_accounts_lists_every_saved_account() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let eur = Currency::new("EUR").unwrap();
        store.save_account(Account::new("A", eur.clone(), AccountKind::Own, dec!(0))).unwrap();
        store.save_account(Account::new("B", eur, AccountKind::Investment, dec!(500))).unwrap();
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
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
        let account_id = account.id;
        let result = store.transaction(|store| -> Result<(), LedgerError> {
            store.save_account(account.clone())?;
            Err(LedgerError::Storage("simulated mid-transaction failure".to_string()))
        });
        assert!(result.is_err());
        assert_eq!(store.get_account(account_id).unwrap(), None);
    }

    #[test]
    fn a_successful_transaction_commits_every_write_it_made() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
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
        assert_eq!(reopened.get_account(account_id).unwrap().unwrap().opening_balance, dec!(250));
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
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
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
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
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
        assert_eq!(store.entries_for_account(Uuid::new_v4()).unwrap().len(), 0);
    }

    #[test]
    fn resaving_an_entry_replaces_its_tags_rather_than_appending() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
        store.save_account(account.clone()).unwrap();
        let mut entry = sample_entry(account.id);
        store.save_entry(entry.clone()).unwrap();
        entry.tags = vec!["only-this-one".to_string()];
        store.save_entry(entry.clone()).unwrap();
        assert_eq!(store.get_entry(entry.id).unwrap().unwrap().tags, vec!["only-this-one".to_string()]);
    }

    #[test]
    fn a_full_ledger_balance_works_against_sqlite_storage() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(100)).unwrap();
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "Groceries")
            .unwrap();
        assert_eq!(ledger.account_balance(account.id), Ok(dec!(80)));
    }

    #[test]
    fn saved_parts_can_be_read_back_for_their_entry() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
        store.save_account(account.clone()).unwrap();
        let entry = sample_entry(account.id);
        store.save_entry(entry.clone()).unwrap();
        let parts = vec![
            EntryPart {
                id: Uuid::new_v4(),
                entry_id: entry.id,
                amount: dec!(-15),
                category: Some("Loan".to_string()),
                transfer_account_id: None,
            },
            EntryPart {
                id: Uuid::new_v4(),
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
        assert_eq!(store.parts_for_entry(Uuid::new_v4()).unwrap().len(), 0);
    }

    #[test]
    fn resaving_parts_replaces_the_old_set() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
        store.save_account(account.clone()).unwrap();
        let entry = sample_entry(account.id);
        store.save_entry(entry.clone()).unwrap();
        store
            .save_entry_parts(
                entry.id,
                vec![EntryPart { id: Uuid::new_v4(), entry_id: entry.id, amount: dec!(-20), category: None, transfer_account_id: None }],
            )
            .unwrap();
        store
            .save_entry_parts(
                entry.id,
                vec![EntryPart { id: Uuid::new_v4(), entry_id: entry.id, amount: dec!(-10), category: None, transfer_account_id: None }],
            )
            .unwrap();
        assert_eq!(store.parts_for_entry(entry.id).unwrap().len(), 1);
    }

    #[test]
    fn splitting_a_stored_entry_works_through_the_ledger() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0)).unwrap();
        let entry = ledger.record_manual_entry(account.id, a_date(), dec!(-20), "Mixed").unwrap();
        let parts = ledger
            .split_entry(entry.id, vec![(dec!(-15), Some("Loan".to_string())), (dec!(-5), None)])
            .unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(ledger.parts_for_entry(entry.id).unwrap().len(), 2);
    }

    #[test]
    fn a_saved_pot_can_be_read_back() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let pot = Pot {
            id: Uuid::new_v4(),
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
            id: Uuid::new_v4(),
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
        store.save_pot(Pot { id: Uuid::new_v4(), name: "A".to_string(), currency: eur.clone(), target: None, priority: None }).unwrap();
        store.save_pot(Pot { id: Uuid::new_v4(), name: "B".to_string(), currency: eur, target: None, priority: None }).unwrap();
        assert_eq!(store.all_pots().unwrap().len(), 2);
    }

    #[test]
    fn allocations_for_a_pot_with_none_is_empty() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.allocations_for_pot(Uuid::new_v4()).unwrap().len(), 0);
    }

    #[test]
    fn saved_allocations_can_be_read_back_for_their_pot() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let pot = Pot { id: Uuid::new_v4(), name: "A".to_string(), currency: Currency::new("EUR").unwrap(), target: None, priority: None };
        store.save_pot(pot.clone()).unwrap();
        let allocation = Allocation { id: Uuid::new_v4(), pot_id: pot.id, amount: dec!(100), date: a_date(), note: None };
        store.save_allocation(allocation.clone()).unwrap();
        assert_eq!(store.allocations_for_pot(pot.id), Ok(vec![allocation]));
    }

    #[test]
    fn allocating_and_general_savings_work_through_the_ledger_on_sqlite() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100)).unwrap();
        let pot = ledger.open_pot("Emergency fund", eur.clone(), None, None).unwrap();
        ledger.allocate_to_pot(pot.id, dec!(100), a_date()).unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(100)));
        assert_eq!(ledger.general_savings(&eur), Ok(dec!(0)));
        let over = ledger.allocate_to_pot(pot.id, dec!(0.01), a_date());
        assert_eq!(over, Err(LedgerError::GeneralSavingsWouldGoNegative(eur)));
    }

    #[test]
    fn a_saved_valuation_can_be_read_back() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("An ETF position", Currency::new("EUR").unwrap(), AccountKind::Investment, dec!(1000));
        store.save_account(account.clone()).unwrap();
        let valuation = Valuation {
            id: Uuid::new_v4(),
            account_id: account.id,
            date: a_date(),
            old_value: dec!(1000),
            new_value: dec!(1042),
            category: "Investment gain".to_string(),
        };
        store.save_valuation(valuation.clone()).unwrap();
        assert_eq!(store.valuations_for_account(account.id), Ok(vec![valuation]));
    }

    #[test]
    fn an_account_with_no_valuations_returns_an_empty_list() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.valuations_for_account(Uuid::new_v4()).unwrap().len(), 0);
    }

    #[test]
    fn updating_current_value_persists_through_sqlite() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let etf = ledger.open_account("An ETF position", eur, AccountKind::Investment, dec!(1000)).unwrap();
        let valuation = ledger.update_current_value(etf.id, dec!(1042), "Investment gain", a_date()).unwrap();
        assert_eq!(valuation.gain(), dec!(42));
        assert_eq!(ledger.current_value(etf.id), Ok(dec!(1042)));
    }

    #[test]
    fn a_cross_currency_transfer_persists_correctly_through_sqlite() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let ngn = Currency::new("NGN").unwrap();
        let a = ledger.open_account("A", eur, AccountKind::Own, dec!(200)).unwrap();
        let b = ledger.open_account("B", ngn, AccountKind::Own, dec!(0)).unwrap();
        ledger.transfer(a.id, b.id, a_date(), dec!(200), dec!(370000), "move").unwrap();
        assert_eq!(ledger.account_balance(a.id), Ok(dec!(0)));
        assert_eq!(ledger.account_balance(b.id), Ok(dec!(370000)));
    }
}
