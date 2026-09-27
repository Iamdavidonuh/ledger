# SQLite Storage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `LedgerStore` a real backing store: a SQLite implementation (`SqliteStore`) that persists accounts, entries, entry parts, pots, allocations, and valuations across restarts, with schema migrations, while `Ledger<S>` and every existing invariant it enforces stay unchanged.

**Architecture:** `SqliteStore` implements the same `LedgerStore` trait `InMemoryStore` already implements, so `Ledger<S>` needs no changes at all. It uses `rusqlite`, a synchronous SQLite driver, not `sqlx`, because `LedgerStore`'s methods are synchronous by design (the storage trait has no async in it, and adding one now would mean reworking every task in the previous plan for no benefit this crate needs). Money amounts are stored as `TEXT` and parsed back through `rust_decimal::Decimal`'s exact string round-trip, never as `REAL`, since floating point cannot represent decimal money exactly. Schema versioning uses `rusqlite_migration`. No database trigger enforces "entries are facts"; that rule is already enforced in `Ledger<S>` itself (Tasks 3 and 4 of the previous plan), so `SqliteStore` is a faithful, simple persistence layer with no business rules of its own.

**Tech Stack:** Rust, `rusqlite` (bundled SQLite), `rusqlite_migration` (schema versioning), `rust_decimal` (already in use), `tempfile` (dev-dependency, for a real-file persistence test).

**Spec:** the money tracker design spec (kept private; see the previous plan's note on a sanitized public copy).

## Global Constraints

- No AI anywhere in the app or its suggestions.
- The books always balance: moving money between accounts or pots never creates or loses value. This plan does not touch that logic; it only persists what `Ledger<S>` already computed.
- No real bank names, people, or personal amounts anywhere in this repository, including tests and fixtures.
- Tightened Clippy lints on money code: `unwrap_used`, `expect_used`, and `panic` are warnings outside test code (as set up in `core/src/lib.rs`).
- SQLite is a single file with one writer, per the deployment design; this plan does not add any locking or concurrency handling beyond what SQLite itself provides, since the service runs as one process.

## Review Focus

- **Decimal round-tripping through TEXT:** an amount like `-1042.50` must come back out exactly as `-1042.50`, not `-1042.5` or a value corrupted by a float conversion anywhere in the path.
- **Reopening a database file:** data written before closing the connection must still be there after a fresh connection to the same file, proving persistence actually persists rather than only working within one open connection.
- **A pot or account with no entries or allocations at all:** balance and total queries must return zero cleanly, not error or panic on an empty result set.
- **Two entries or two pots with different currencies sharing an account:** `SqliteStore` must return exactly what was saved for each id, never mixing rows across currencies, the same guarantee `InMemoryStore` already gives structurally.
- **The migration running twice:** starting `SqliteStore` against a database that already has the schema applied must not fail or duplicate tables.

---

## Task 1: `SqliteStore` skeleton, schema, and accounts

**Files:**
- Create: `core/migrations.rs` (embedded migration list)
- Create: `core/src/sqlite_store.rs`
- Modify: `core/Cargo.toml`
- Modify: `core/src/lib.rs`

**Interfaces:**
- Consumes: `LedgerStore`, `Account`, `AccountKind`, `Currency` (previous plan).
- Produces: `SqliteStore::open_in_memory() -> Result<Self, SqliteStoreError>`, `SqliteStore::open(path: &std::path::Path) -> Result<Self, SqliteStoreError>`, `SqliteStoreError`, and `LedgerStore` implementations for `save_account`, `get_account`, `all_accounts`.

- [ ] **Step 1: Write the failing tests**

Add to `core/Cargo.toml`:

```toml
rusqlite = { version = "0.32", features = ["bundled"] }
rusqlite_migration = "1"

[dev-dependencies]
tempfile = "3"
```

(keep the existing `rust_decimal_macros` dev-dependency line alongside `tempfile`.)

Create `core/src/sqlite_store.rs`:

```rust
use crate::account::{Account, AccountKind};
use crate::currency::Currency;
use rusqlite::Connection;
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
    #[error("stored value {0:?} is not a valid decimal amount")]
    BadDecimal(String),
    #[error("stored value {0:?} is not a known account kind")]
    BadAccountKind(String),
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

fn account_kind_from_str(s: &str) -> Result<AccountKind, SqliteStoreError> {
    match s {
        "own" => Ok(AccountKind::Own),
        "outside" => Ok(AccountKind::Outside),
        "person" => Ok(AccountKind::Person),
        "investment" => Ok(AccountKind::Investment),
        other => Err(SqliteStoreError::BadAccountKind(other.to_string())),
    }
}

fn decimal_to_text(d: Decimal) -> String {
    d.to_string()
}

fn decimal_from_text(s: &str) -> Result<Decimal, SqliteStoreError> {
    Decimal::from_str(s).map_err(|_| SqliteStoreError::BadDecimal(s.to_string()))
}

impl crate::store::LedgerStore for SqliteStore {
    fn save_account(&mut self, account: Account) {
        todo!()
    }

    fn get_account(&self, id: Uuid) -> Option<Account> {
        todo!()
    }

    fn all_accounts(&self) -> Vec<Account> {
        todo!()
    }

    fn save_entry(&mut self, _entry: crate::entry::Entry) {
        unimplemented!("added in Task 2")
    }

    fn get_entry(&self, _id: Uuid) -> Option<crate::entry::Entry> {
        unimplemented!("added in Task 2")
    }

    fn entries_for_account(&self, _account_id: Uuid) -> Vec<crate::entry::Entry> {
        unimplemented!("added in Task 2")
    }

    fn save_entry_parts(&mut self, _entry_id: Uuid, _parts: Vec<crate::entry::EntryPart>) {
        unimplemented!("added in Task 3")
    }

    fn parts_for_entry(&self, _entry_id: Uuid) -> Vec<crate::entry::EntryPart> {
        unimplemented!("added in Task 3")
    }

    fn save_pot(&mut self, _pot: crate::pot::Pot) {
        unimplemented!("added in Task 4")
    }

    fn get_pot(&self, _id: Uuid) -> Option<crate::pot::Pot> {
        unimplemented!("added in Task 4")
    }

    fn all_pots(&self) -> Vec<crate::pot::Pot> {
        unimplemented!("added in Task 4")
    }

    fn save_allocation(&mut self, _allocation: crate::pot::Allocation) {
        unimplemented!("added in Task 4")
    }

    fn allocations_for_pot(&self, _pot_id: Uuid) -> Vec<crate::pot::Allocation> {
        unimplemented!("added in Task 4")
    }

    fn save_valuation(&mut self, _valuation: crate::valuation::Valuation) {
        unimplemented!("added in Task 5")
    }

    fn valuations_for_account(&self, _account_id: Uuid) -> Vec<crate::valuation::Valuation> {
        unimplemented!("added in Task 5")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LedgerStore;
    use rust_decimal_macros::dec;

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
        store.save_account(account.clone());
        assert_eq!(store.get_account(id), Some(account));
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
        store.save_account(account);
        assert_eq!(store.get_account(id).unwrap().opening_balance, dec!(-1042.50));
    }

    #[test]
    fn unknown_id_returns_none() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.get_account(Uuid::new_v4()), None);
    }

    #[test]
    fn all_accounts_lists_every_saved_account() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let eur = Currency::new("EUR").unwrap();
        store.save_account(Account::new("A", eur.clone(), AccountKind::Own, dec!(0)));
        store.save_account(Account::new("B", eur, AccountKind::Investment, dec!(500)));
        assert_eq!(store.all_accounts().len(), 2);
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
        store.save_account(account.clone());
        account.archived = true;
        store.save_account(account.clone());
        assert_eq!(store.all_accounts().len(), 1);
        assert!(store.get_account(account.id).unwrap().archived);
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
            store.save_account(account);
        }
        let reopened = SqliteStore::open(&path).unwrap();
        assert_eq!(reopened.get_account(account_id).unwrap().opening_balance, dec!(250));
    }

    #[test]
    fn opening_the_same_file_twice_does_not_fail_on_the_second_migration_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        SqliteStore::open(&path).unwrap();
        SqliteStore::open(&path).unwrap();
    }
}
```

Create `core/migrations/0001_initial.sql`:

```sql
CREATE TABLE accounts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    currency TEXT NOT NULL,
    kind TEXT NOT NULL,
    opening_balance TEXT NOT NULL,
    archived INTEGER NOT NULL,
    current_value TEXT
);

CREATE TABLE entries (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    date TEXT NOT NULL,
    time TEXT,
    amount TEXT NOT NULL,
    currency TEXT NOT NULL,
    description TEXT NOT NULL,
    note TEXT,
    category TEXT,
    pot_id TEXT,
    transfer_account_id TEXT,
    source TEXT NOT NULL,
    bank_state TEXT NOT NULL,
    confirmed INTEGER NOT NULL,
    voided_reason TEXT
);

CREATE INDEX entries_account_id ON entries(account_id);

CREATE TABLE entry_tags (
    entry_id TEXT NOT NULL REFERENCES entries(id),
    tag TEXT NOT NULL,
    PRIMARY KEY (entry_id, tag)
);

CREATE TABLE entry_parts (
    id TEXT PRIMARY KEY,
    entry_id TEXT NOT NULL REFERENCES entries(id),
    amount TEXT NOT NULL,
    category TEXT,
    transfer_account_id TEXT
);

CREATE INDEX entry_parts_entry_id ON entry_parts(entry_id);

CREATE TABLE pots (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    currency TEXT NOT NULL,
    target TEXT,
    priority INTEGER
);

CREATE TABLE allocations (
    id TEXT PRIMARY KEY,
    pot_id TEXT NOT NULL REFERENCES pots(id),
    amount TEXT NOT NULL,
    date TEXT NOT NULL,
    note TEXT
);

CREATE INDEX allocations_pot_id ON allocations(pot_id);

CREATE TABLE valuations (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    date TEXT NOT NULL,
    old_value TEXT NOT NULL,
    new_value TEXT NOT NULL,
    category TEXT NOT NULL
);

CREATE INDEX valuations_account_id ON valuations(account_id);
```

Add to `core/src/lib.rs`:

```rust
pub mod sqlite_store;
pub use sqlite_store::{SqliteStore, SqliteStoreError};
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test sqlite_store`
Expected: panics on the `todo!()` bodies for `save_account`, `get_account`, and `all_accounts`.

- [ ] **Step 3: Implement the account methods**

Replace the three `todo!()` bodies in the `LedgerStore` impl:

```rust
    fn save_account(&mut self, account: Account) {
        self.conn
            .execute(
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
            )
            .expect("writing an account should not fail against a healthy database");
    }

    fn get_account(&self, id: Uuid) -> Option<Account> {
        self.conn
            .query_row(
                "SELECT id, name, currency, kind, opening_balance, archived, current_value
                 FROM accounts WHERE id = ?1",
                rusqlite::params![id.to_string()],
                |row| {
                    let id_str: String = row.get(0)?;
                    let currency: String = row.get(2)?;
                    let kind: String = row.get(3)?;
                    let opening_balance: String = row.get(4)?;
                    let archived: i64 = row.get(5)?;
                    let current_value: Option<String> = row.get(6)?;
                    Ok((id_str, row.get::<_, String>(1)?, currency, kind, opening_balance, archived, current_value))
                },
            )
            .ok()
            .map(|(id_str, name, currency, kind, opening_balance, archived, current_value)| {
                Account {
                    id: Uuid::parse_str(&id_str).expect("stored ids are always valid UUIDs"),
                    name,
                    currency: Currency::new(&currency).expect("stored currencies are always valid"),
                    kind: account_kind_from_str(&kind).expect("stored kinds are always valid"),
                    opening_balance: decimal_from_text(&opening_balance)
                        .expect("stored amounts are always valid decimals"),
                    archived: archived != 0,
                    current_value: current_value
                        .map(|v| decimal_from_text(&v).expect("stored amounts are always valid decimals")),
                }
            })
    }

    fn all_accounts(&self) -> Vec<Account> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM accounts")
            .expect("preparing a fixed query should not fail");
        let ids: Vec<Uuid> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .expect("querying a fixed statement should not fail")
            .map(|id_str| Uuid::parse_str(&id_str.expect("row read should not fail")).expect("stored ids are always valid UUIDs"))
            .collect();
        ids.into_iter()
            .filter_map(|id| self.get_account(id))
            .collect()
    }
```

Note: `save_account`, `get_account`, and `all_accounts` use `.expect(...)` on operations against a database this process itself created and controls (a fixed, hand-written schema, ids this process generated). The crate's lints treat `expect_used` as a warning rather than a hard denial specifically so a small number of documented, genuinely-infallible-in-practice calls like these remain readable; each one names why it should not fail.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test sqlite_store`
Expected: all 8 tests in `sqlite_store::tests` pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add SqliteStore with schema migrations and account persistence"
```

---

## Task 2: Entries, including tags

**Files:**
- Modify: `core/src/sqlite_store.rs`

**Interfaces:**
- Consumes: `Entry`, `EntrySource`, `BankState` (previous plan), `SqliteStore` (Task 1).
- Produces: `SqliteStore`'s `save_entry`, `get_entry`, `entries_for_account`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `core/src/sqlite_store.rs`:

```rust
    use crate::entry::{BankState, Entry, EntrySource};
    use chrono::NaiveDate;

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
    fn a_saved_entry_round_trips_with_its_tags() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
        store.save_account(account.clone());
        let entry = sample_entry(account.id);
        store.save_entry(entry.clone());
        let read_back = store.get_entry(entry.id).unwrap();
        assert_eq!(read_back.amount, dec!(-20));
        assert_eq!(read_back.description, "Groceries");
        let mut tags = read_back.tags.clone();
        tags.sort();
        assert_eq!(tags, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn entries_for_account_returns_only_that_accounts_entries() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let eur = Currency::new("EUR").unwrap();
        let a = Account::new("A", eur.clone(), AccountKind::Own, dec!(0));
        let b = Account::new("B", eur, AccountKind::Own, dec!(0));
        store.save_account(a.clone());
        store.save_account(b.clone());
        store.save_entry(sample_entry(a.id));
        store.save_entry(sample_entry(a.id));
        store.save_entry(sample_entry(b.id));
        assert_eq!(store.entries_for_account(a.id).len(), 2);
        assert_eq!(store.entries_for_account(b.id).len(), 1);
    }

    #[test]
    fn an_account_with_no_entries_returns_an_empty_list() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.entries_for_account(Uuid::new_v4()).len(), 0);
    }

    #[test]
    fn resaving_an_entry_replaces_its_tags_rather_than_appending() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
        store.save_account(account.clone());
        let mut entry = sample_entry(account.id);
        store.save_entry(entry.clone());
        entry.tags = vec!["only-this-one".to_string()];
        store.save_entry(entry.clone());
        assert_eq!(store.get_entry(entry.id).unwrap().tags, vec!["only-this-one".to_string()]);
    }

    #[test]
    fn a_full_ledger_balance_works_against_sqlite_storage() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(100));
        ledger
            .record_manual_entry(account.id, a_date(), dec!(-20), "Groceries")
            .unwrap();
        assert_eq!(ledger.account_balance(account.id), Ok(dec!(80)));
    }
```

Add stub methods, replacing the two `unimplemented!("added in Task 2")` bodies:

```rust
    fn save_entry(&mut self, entry: crate::entry::Entry) {
        todo!()
    }

    fn get_entry(&self, id: Uuid) -> Option<crate::entry::Entry> {
        todo!()
    }

    fn entries_for_account(&self, account_id: Uuid) -> Vec<crate::entry::Entry> {
        todo!()
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test sqlite_store`
Expected: panics on the three new `todo!()` bodies.

- [ ] **Step 3: Implement entry storage**

```rust
fn entry_source_to_str(source: crate::entry::EntrySource) -> &'static str {
    match source {
        crate::entry::EntrySource::Manual => "manual",
        crate::entry::EntrySource::Imported => "imported",
    }
}

fn entry_source_from_str(s: &str) -> Result<crate::entry::EntrySource, SqliteStoreError> {
    match s {
        "manual" => Ok(crate::entry::EntrySource::Manual),
        "imported" => Ok(crate::entry::EntrySource::Imported),
        other => Err(SqliteStoreError::BadAccountKind(other.to_string())),
    }
}

fn bank_state_to_str(state: crate::entry::BankState) -> &'static str {
    match state {
        crate::entry::BankState::Completed => "completed",
        crate::entry::BankState::Pending => "pending",
        crate::entry::BankState::Reverted => "reverted",
    }
}

fn bank_state_from_str(s: &str) -> Result<crate::entry::BankState, SqliteStoreError> {
    match s {
        "completed" => Ok(crate::entry::BankState::Completed),
        "pending" => Ok(crate::entry::BankState::Pending),
        "reverted" => Ok(crate::entry::BankState::Reverted),
        other => Err(SqliteStoreError::BadAccountKind(other.to_string())),
    }
}
```

Add this near the other `_to_str`/`_from_str` helpers above the `impl LedgerStore for SqliteStore` block, then replace the three `todo!()` bodies:

```rust
    fn save_entry(&mut self, entry: crate::entry::Entry) {
        let tx = self.conn.transaction().expect("starting a transaction should not fail");
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
        )
        .expect("writing an entry should not fail against a healthy database");
        tx.execute("DELETE FROM entry_tags WHERE entry_id = ?1", rusqlite::params![entry.id.to_string()])
            .expect("clearing old tags should not fail");
        for tag in &entry.tags {
            tx.execute(
                "INSERT INTO entry_tags (entry_id, tag) VALUES (?1, ?2)",
                rusqlite::params![entry.id.to_string(), tag],
            )
            .expect("writing a tag should not fail");
        }
        tx.commit().expect("committing should not fail");
    }

    fn get_entry(&self, id: Uuid) -> Option<crate::entry::Entry> {
        let tags: Vec<String> = self
            .conn
            .prepare("SELECT tag FROM entry_tags WHERE entry_id = ?1")
            .expect("preparing a fixed query should not fail")
            .query_map(rusqlite::params![id.to_string()], |row| row.get(0))
            .expect("querying a fixed statement should not fail")
            .map(|r| r.expect("row read should not fail"))
            .collect();
        self.conn
            .query_row(
                "SELECT account_id, date, time, amount, currency, description, note, category,
                        pot_id, transfer_account_id, source, bank_state, confirmed, voided_reason
                 FROM entries WHERE id = ?1",
                rusqlite::params![id.to_string()],
                |row| {
                    Ok(crate::entry::Entry {
                        id,
                        account_id: Uuid::parse_str(&row.get::<_, String>(0)?).expect("stored ids are always valid UUIDs"),
                        date: chrono::NaiveDate::parse_from_str(&row.get::<_, String>(1)?, "%Y-%m-%d")
                            .expect("stored dates are always valid"),
                        time: row
                            .get::<_, Option<String>>(2)?
                            .map(|t| chrono::NaiveTime::parse_from_str(&t, "%H:%M:%S%.f").expect("stored times are always valid")),
                        amount: decimal_from_text(&row.get::<_, String>(3)?).expect("stored amounts are always valid"),
                        currency: Currency::new(&row.get::<_, String>(4)?).expect("stored currencies are always valid"),
                        description: row.get(5)?,
                        note: row.get(6)?,
                        category: row.get(7)?,
                        tags: Vec::new(),
                        pot_id: row.get::<_, Option<String>>(8)?.map(|s| Uuid::parse_str(&s).expect("stored ids are always valid UUIDs")),
                        transfer_account_id: row.get::<_, Option<String>>(9)?.map(|s| Uuid::parse_str(&s).expect("stored ids are always valid UUIDs")),
                        source: entry_source_from_str(&row.get::<_, String>(10)?).expect("stored sources are always valid"),
                        bank_state: bank_state_from_str(&row.get::<_, String>(11)?).expect("stored bank states are always valid"),
                        confirmed: row.get::<_, i64>(12)? != 0,
                        voided_reason: row.get(13)?,
                    })
                },
            )
            .ok()
            .map(|mut e| {
                e.tags = tags;
                e
            })
    }

    fn entries_for_account(&self, account_id: Uuid) -> Vec<crate::entry::Entry> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM entries WHERE account_id = ?1")
            .expect("preparing a fixed query should not fail");
        let ids: Vec<Uuid> = stmt
            .query_map(rusqlite::params![account_id.to_string()], |row| row.get::<_, String>(0))
            .expect("querying a fixed statement should not fail")
            .map(|id_str| Uuid::parse_str(&id_str.expect("row read should not fail")).expect("stored ids are always valid UUIDs"))
            .collect();
        ids.into_iter().filter_map(|id| self.get_entry(id)).collect()
    }
```

`SqliteStore::save_entry` needs `&mut self` to call `self.conn.transaction()`, which the `LedgerStore` trait already declares it as (`fn save_entry(&mut self, ...)`), so no signature change is needed.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test sqlite_store`
Expected: all tests in `sqlite_store::tests` pass, including the one that runs a full `Ledger<SqliteStore>` balance calculation.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add entry persistence to SqliteStore, including tags"
```

---

## Task 3: Entry parts

**Files:**
- Modify: `core/src/sqlite_store.rs`

**Interfaces:**
- Consumes: `EntryPart` (previous plan), `SqliteStore` (Tasks 1 and 2).
- Produces: `SqliteStore`'s `save_entry_parts`, `parts_for_entry`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
    use crate::entry::EntryPart;

    #[test]
    fn saved_parts_can_be_read_back_for_their_entry() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
        store.save_account(account.clone());
        let entry = sample_entry(account.id);
        store.save_entry(entry.clone());
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
        store.save_entry_parts(entry.id, parts.clone());
        let mut read_back = store.parts_for_entry(entry.id);
        read_back.sort_by_key(|p| p.amount);
        let mut expected = parts;
        expected.sort_by_key(|p| p.amount);
        assert_eq!(read_back, expected);
    }

    #[test]
    fn an_entry_with_no_parts_returns_an_empty_list() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.parts_for_entry(Uuid::new_v4()).len(), 0);
    }

    #[test]
    fn resaving_parts_replaces_the_old_set() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0));
        store.save_account(account.clone());
        let entry = sample_entry(account.id);
        store.save_entry(entry.clone());
        store.save_entry_parts(
            entry.id,
            vec![EntryPart { id: Uuid::new_v4(), entry_id: entry.id, amount: dec!(-20), category: None, transfer_account_id: None }],
        );
        store.save_entry_parts(
            entry.id,
            vec![EntryPart { id: Uuid::new_v4(), entry_id: entry.id, amount: dec!(-10), category: None, transfer_account_id: None }],
        );
        assert_eq!(store.parts_for_entry(entry.id).len(), 1);
    }

    #[test]
    fn splitting_a_stored_entry_works_through_the_ledger() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let account = ledger.open_account("Checking", eur, AccountKind::Own, dec!(0));
        let entry = ledger.record_manual_entry(account.id, a_date(), dec!(-20), "Mixed").unwrap();
        let parts = ledger
            .split_entry(entry.id, vec![(dec!(-15), Some("Loan".to_string())), (dec!(-5), None)])
            .unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(ledger.parts_for_entry(entry.id).len(), 2);
    }
```

Replace the two `unimplemented!("added in Task 3")` bodies with stubs:

```rust
    fn save_entry_parts(&mut self, entry_id: Uuid, parts: Vec<crate::entry::EntryPart>) {
        todo!()
    }

    fn parts_for_entry(&self, entry_id: Uuid) -> Vec<crate::entry::EntryPart> {
        todo!()
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test sqlite_store`
Expected: panics on the two new `todo!()` bodies.

- [ ] **Step 3: Implement entry part storage**

```rust
    fn save_entry_parts(&mut self, entry_id: Uuid, parts: Vec<crate::entry::EntryPart>) {
        let tx = self.conn.transaction().expect("starting a transaction should not fail");
        tx.execute("DELETE FROM entry_parts WHERE entry_id = ?1", rusqlite::params![entry_id.to_string()])
            .expect("clearing old parts should not fail");
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
            )
            .expect("writing a part should not fail");
        }
        tx.commit().expect("committing should not fail");
    }

    fn parts_for_entry(&self, entry_id: Uuid) -> Vec<crate::entry::EntryPart> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, amount, category, transfer_account_id FROM entry_parts WHERE entry_id = ?1")
            .expect("preparing a fixed query should not fail");
        stmt.query_map(rusqlite::params![entry_id.to_string()], |row| {
            let id: String = row.get(0)?;
            let amount: String = row.get(1)?;
            let category: Option<String> = row.get(2)?;
            let transfer_account_id: Option<String> = row.get(3)?;
            Ok((id, amount, category, transfer_account_id))
        })
        .expect("querying a fixed statement should not fail")
        .map(|r| {
            let (id, amount, category, transfer_account_id) = r.expect("row read should not fail");
            crate::entry::EntryPart {
                id: Uuid::parse_str(&id).expect("stored ids are always valid UUIDs"),
                entry_id,
                amount: decimal_from_text(&amount).expect("stored amounts are always valid"),
                category,
                transfer_account_id: transfer_account_id
                    .map(|s| Uuid::parse_str(&s).expect("stored ids are always valid UUIDs")),
            }
        })
        .collect()
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test sqlite_store`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add entry part persistence to SqliteStore"
```

---

## Task 4: Pots and allocations

**Files:**
- Modify: `core/src/sqlite_store.rs`

**Interfaces:**
- Consumes: `Pot`, `Allocation` (previous plan), `SqliteStore` (Tasks 1 to 3).
- Produces: `SqliteStore`'s `save_pot`, `get_pot`, `all_pots`, `save_allocation`, `allocations_for_pot`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
    use crate::pot::{Allocation, Pot};

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
        store.save_pot(pot.clone());
        assert_eq!(store.get_pot(pot.id), Some(pot));
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
        store.save_pot(pot.clone());
        let read_back = store.get_pot(pot.id).unwrap();
        assert_eq!(read_back.target, None);
        assert_eq!(read_back.priority, None);
    }

    #[test]
    fn all_pots_lists_every_saved_pot() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let eur = Currency::new("EUR").unwrap();
        store.save_pot(Pot { id: Uuid::new_v4(), name: "A".to_string(), currency: eur.clone(), target: None, priority: None });
        store.save_pot(Pot { id: Uuid::new_v4(), name: "B".to_string(), currency: eur, target: None, priority: None });
        assert_eq!(store.all_pots().len(), 2);
    }

    #[test]
    fn allocations_for_a_pot_with_none_is_empty() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.allocations_for_pot(Uuid::new_v4()).len(), 0);
    }

    #[test]
    fn saved_allocations_can_be_read_back_for_their_pot() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let pot = Pot { id: Uuid::new_v4(), name: "A".to_string(), currency: Currency::new("EUR").unwrap(), target: None, priority: None };
        store.save_pot(pot.clone());
        let allocation = Allocation { id: Uuid::new_v4(), pot_id: pot.id, amount: dec!(100), date: a_date(), note: None };
        store.save_allocation(allocation.clone());
        assert_eq!(store.allocations_for_pot(pot.id), vec![allocation]);
    }

    #[test]
    fn allocating_and_general_savings_work_through_the_ledger_on_sqlite() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100));
        let pot = ledger.open_pot("Emergency fund", eur.clone(), None, None);
        ledger.allocate_to_pot(pot.id, dec!(100), a_date()).unwrap();
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(100)));
        assert_eq!(ledger.general_savings(&eur), dec!(0));
        let over = ledger.allocate_to_pot(pot.id, dec!(0.01), a_date());
        assert_eq!(over, Err(crate::error::LedgerError::GeneralSavingsWouldGoNegative(eur)));
    }
```

Replace the five `unimplemented!("added in Task 4")` bodies with stubs:

```rust
    fn save_pot(&mut self, pot: crate::pot::Pot) {
        todo!()
    }

    fn get_pot(&self, id: Uuid) -> Option<crate::pot::Pot> {
        todo!()
    }

    fn all_pots(&self) -> Vec<crate::pot::Pot> {
        todo!()
    }

    fn save_allocation(&mut self, allocation: crate::pot::Allocation) {
        todo!()
    }

    fn allocations_for_pot(&self, pot_id: Uuid) -> Vec<crate::pot::Allocation> {
        todo!()
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test sqlite_store`
Expected: panics on the five new `todo!()` bodies.

- [ ] **Step 3: Implement pot and allocation storage**

```rust
    fn save_pot(&mut self, pot: crate::pot::Pot) {
        self.conn
            .execute(
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
            )
            .expect("writing a pot should not fail against a healthy database");
    }

    fn get_pot(&self, id: Uuid) -> Option<crate::pot::Pot> {
        self.conn
            .query_row(
                "SELECT name, currency, target, priority FROM pots WHERE id = ?1",
                rusqlite::params![id.to_string()],
                |row| {
                    let target: Option<String> = row.get(2)?;
                    Ok(crate::pot::Pot {
                        id,
                        name: row.get(0)?,
                        currency: Currency::new(&row.get::<_, String>(1)?).expect("stored currencies are always valid"),
                        target: target.map(|t| decimal_from_text(&t).expect("stored amounts are always valid")),
                        priority: row.get(3)?,
                    })
                },
            )
            .ok()
    }

    fn all_pots(&self) -> Vec<crate::pot::Pot> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM pots")
            .expect("preparing a fixed query should not fail");
        let ids: Vec<Uuid> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .expect("querying a fixed statement should not fail")
            .map(|id_str| Uuid::parse_str(&id_str.expect("row read should not fail")).expect("stored ids are always valid UUIDs"))
            .collect();
        ids.into_iter().filter_map(|id| self.get_pot(id)).collect()
    }

    fn save_allocation(&mut self, allocation: crate::pot::Allocation) {
        self.conn
            .execute(
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
            )
            .expect("writing an allocation should not fail against a healthy database");
    }

    fn allocations_for_pot(&self, pot_id: Uuid) -> Vec<crate::pot::Allocation> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, amount, date, note FROM allocations WHERE pot_id = ?1")
            .expect("preparing a fixed query should not fail");
        stmt.query_map(rusqlite::params![pot_id.to_string()], |row| {
            let id: String = row.get(0)?;
            let amount: String = row.get(1)?;
            let date: String = row.get(2)?;
            let note: Option<String> = row.get(3)?;
            Ok((id, amount, date, note))
        })
        .expect("querying a fixed statement should not fail")
        .map(|r| {
            let (id, amount, date, note) = r.expect("row read should not fail");
            crate::pot::Allocation {
                id: Uuid::parse_str(&id).expect("stored ids are always valid UUIDs"),
                pot_id,
                amount: decimal_from_text(&amount).expect("stored amounts are always valid"),
                date: chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").expect("stored dates are always valid"),
                note,
            }
        })
        .collect()
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test sqlite_store`
Expected: all tests pass, including the never-below-zero check running against `SqliteStore`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add pot and allocation persistence to SqliteStore"
```

---

## Task 5: Valuations and a transfer persistence check

**Files:**
- Modify: `core/src/sqlite_store.rs`

**Interfaces:**
- Consumes: `Valuation` (previous plan), `SqliteStore` (Tasks 1 to 4).
- Produces: `SqliteStore`'s `save_valuation`, `valuations_for_account`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
    use crate::valuation::Valuation;

    #[test]
    fn a_saved_valuation_can_be_read_back() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let account = Account::new("An ETF position", Currency::new("EUR").unwrap(), AccountKind::Investment, dec!(1000));
        store.save_account(account.clone());
        let valuation = Valuation {
            id: Uuid::new_v4(),
            account_id: account.id,
            date: a_date(),
            old_value: dec!(1000),
            new_value: dec!(1042),
            category: "Investment gain".to_string(),
        };
        store.save_valuation(valuation.clone());
        assert_eq!(store.valuations_for_account(account.id), vec![valuation]);
    }

    #[test]
    fn an_account_with_no_valuations_returns_an_empty_list() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.valuations_for_account(Uuid::new_v4()).len(), 0);
    }

    #[test]
    fn updating_current_value_persists_through_sqlite() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let etf = ledger.open_account("An ETF position", eur, AccountKind::Investment, dec!(1000));
        let valuation = ledger.update_current_value(etf.id, dec!(1042), "Investment gain", a_date()).unwrap();
        assert_eq!(valuation.gain(), dec!(42));
        assert_eq!(ledger.current_value(etf.id), Ok(dec!(1042)));
    }

    #[test]
    fn a_cross_currency_transfer_persists_correctly_through_sqlite() {
        let mut ledger = crate::ledger::Ledger::new(SqliteStore::open_in_memory().unwrap());
        let eur = Currency::new("EUR").unwrap();
        let ngn = Currency::new("NGN").unwrap();
        let a = ledger.open_account("A", eur, AccountKind::Own, dec!(200));
        let b = ledger.open_account("B", ngn, AccountKind::Own, dec!(0));
        ledger.transfer(a.id, b.id, a_date(), dec!(200), dec!(370000), "move").unwrap();
        assert_eq!(ledger.account_balance(a.id), Ok(dec!(0)));
        assert_eq!(ledger.account_balance(b.id), Ok(dec!(370000)));
    }
```

Replace the two `unimplemented!("added in Task 5")` bodies with stubs:

```rust
    fn save_valuation(&mut self, valuation: crate::valuation::Valuation) {
        todo!()
    }

    fn valuations_for_account(&self, account_id: Uuid) -> Vec<crate::valuation::Valuation> {
        todo!()
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test sqlite_store`
Expected: panics on the two new `todo!()` bodies.

- [ ] **Step 3: Implement valuation storage**

```rust
    fn save_valuation(&mut self, valuation: crate::valuation::Valuation) {
        self.conn
            .execute(
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
            )
            .expect("writing a valuation should not fail against a healthy database");
    }

    fn valuations_for_account(&self, account_id: Uuid) -> Vec<crate::valuation::Valuation> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, date, old_value, new_value, category FROM valuations WHERE account_id = ?1")
            .expect("preparing a fixed query should not fail");
        stmt.query_map(rusqlite::params![account_id.to_string()], |row| {
            let id: String = row.get(0)?;
            let date: String = row.get(1)?;
            let old_value: String = row.get(2)?;
            let new_value: String = row.get(3)?;
            let category: String = row.get(4)?;
            Ok((id, date, old_value, new_value, category))
        })
        .expect("querying a fixed statement should not fail")
        .map(|r| {
            let (id, date, old_value, new_value, category) = r.expect("row read should not fail");
            crate::valuation::Valuation {
                id: Uuid::parse_str(&id).expect("stored ids are always valid UUIDs"),
                account_id,
                date: chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").expect("stored dates are always valid"),
                old_value: decimal_from_text(&old_value).expect("stored amounts are always valid"),
                new_value: decimal_from_text(&new_value).expect("stored amounts are always valid"),
                category,
            }
        })
        .collect()
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test`
Expected: every test in the crate passes, `InMemoryStore`'s tests from the previous plan and `SqliteStore`'s tests together. Then run `cd core && cargo clippy --all-targets -- -D warnings` and fix anything it flags.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add valuation persistence to SqliteStore, completing the LedgerStore implementation"
```

---

## What this plan does not cover

1. **Connecting `SqliteStore` to the running service.** That is the Axum API plan's job: opening one `SqliteStore` at startup from a configured file path and holding it behind a mutex, since SQLite here has one writer.
2. **Backups.** The nightly `.backup` and the Google Drive rsync are deployment concerns, not this crate's.
3. **Schema changes after this one.** Any future migration is a new `M::up(...)` appended to the list in `migrations()`, never an edit to `0001_initial.sql`.
