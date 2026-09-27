# Ledger Core Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the core money engine as a standalone Rust library: accounts, entries, pots and allocations, transfers (including loans as transfers to a person account), and investment/loan valuations, with every invariant from the design spec enforced and tested. No database, no HTTP API, no importers yet.

**Architecture:** A `Ledger<S: LedgerStore>` engine, generic over a storage trait, tested against an in-memory store. Money is `rust_decimal::Decimal`. Currency is a small validated newtype, not a fixed enum, since the app must support any fiat or crypto currency. All test data is generic (no real bank names, people or amounts), since this repository is public.

**Tech Stack:** Rust, `rust_decimal` (money), `uuid` (ids), `chrono` (dates), `thiserror` (errors), `serde` (serialization, for later API use).

**Spec:** the money tracker design spec (kept private; see the note at the end of this plan about a sanitized public copy).

## Global Constraints

- No AI anywhere in the app or its suggestions.
- Entries from a bank are facts: locked once imported. A manual entry is a draft the user can fix until a statement confirms it.
- The books always balance: moving money between accounts or pots never creates or loses value.
- Accounts may be negative (an overdraft, or a loan you owe someone).
- Only pots and General savings are held to a floor of zero; nothing else is.
- No real bank names, people, or personal amounts anywhere in this repository, including tests and fixtures.
- Tightened Clippy lints on money code: `unwrap_used`, `expect_used`, and `panic` are warnings, not silent.

## Review Focus

- **Voiding one side of a transfer:** the other side must stay exactly as it is, never auto-voided, since the spec says the app warns rather than cascades.
- **Allocating to a pot at the exact boundary:** allocating down to precisely zero general savings must succeed; one cent more must be refused. An off-by-one here silently breaks the "never below zero" rule.
- **Editing a confirmed or imported entry:** must be refused, or "entries are facts" has no teeth.
- **Entry parts that do not sum to the entry's amount:** must be refused, not silently accepted with a mismatch, since a mismatch alone would let money vanish.
- **A cross-currency transfer given no amount received:** must be refused rather than silently treating the sent amount as also received, since that would misstate the receiving account's balance in its own currency.

---

## Task 1: Workspace scaffold and the Currency type

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `core/Cargo.toml`
- Create: `core/src/lib.rs`
- Create: `core/src/currency.rs`
- Modify: `README.md`
- Create: `.gitignore`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: `Currency::new(&str) -> Result<Currency, CurrencyError>`, `Currency::code(&self) -> &str`. Later tasks use `Currency` as a field type and compare it with `==`.

- [ ] **Step 1: Write the failing tests**

Create `core/src/currency.rs`:

```rust
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Currency(String);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CurrencyError {
    #[error("currency code cannot be empty")]
    Empty,
    #[error("currency code must be 2 to 10 letters or digits, got {0:?}")]
    InvalidFormat(String),
}

impl Currency {
    pub fn new(code: &str) -> Result<Self, CurrencyError> {
        todo!()
    }

    pub fn code(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_plain_fiat_code() {
        let c = Currency::new("EUR").unwrap();
        assert_eq!(c.code(), "EUR");
    }

    #[test]
    fn accepts_a_crypto_code_of_any_length_in_range() {
        let c = Currency::new("SOL").unwrap();
        assert_eq!(c.code(), "SOL");
    }

    #[test]
    fn normalizes_lowercase_to_uppercase() {
        let c = Currency::new("eur").unwrap();
        assert_eq!(c.code(), "EUR");
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let c = Currency::new("  eur  ").unwrap();
        assert_eq!(c.code(), "EUR");
    }

    #[test]
    fn rejects_an_empty_code() {
        assert_eq!(Currency::new(""), Err(CurrencyError::Empty));
        assert_eq!(Currency::new("   "), Err(CurrencyError::Empty));
    }

    #[test]
    fn rejects_a_code_that_is_too_short_or_too_long() {
        assert!(matches!(Currency::new("E"), Err(CurrencyError::InvalidFormat(_))));
        assert!(matches!(
            Currency::new("WAYTOOLONGCODE"),
            Err(CurrencyError::InvalidFormat(_))
        ));
    }

    #[test]
    fn rejects_a_code_with_symbols() {
        assert!(matches!(Currency::new("EU-R"), Err(CurrencyError::InvalidFormat(_))));
    }

    #[test]
    fn two_currencies_with_the_same_code_are_equal() {
        assert_eq!(Currency::new("EUR").unwrap(), Currency::new("eur").unwrap());
    }
}
```

Create `core/src/lib.rs`:

```rust
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod currency;

pub use currency::{Currency, CurrencyError};
```

The crate-wide lints in `Cargo.toml` (`unwrap_used`, `expect_used`, `panic` as warnings) apply to test code too unless relaxed. The line above keeps them strict for production code while allowing the plain, readable `.unwrap()` calls that the tests in this plan use throughout.

Create `core/Cargo.toml`:

```toml
[package]
name = "ledger-core"
version = "0.1.0"
edition = "2021"

[dependencies]
rust_decimal = { version = "1", features = ["serde-with-str"] }
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
thiserror = "2"

[dev-dependencies]
rust_decimal_macros = "1"

[lints.clippy]
unwrap_used = "warn"
expect_used = "warn"
panic = "warn"
```

Create `Cargo.toml` (workspace root):

```toml
[workspace]
resolver = "2"
members = ["core"]
```

The repo already has a `.gitignore` covering `/target`, frontend build output, secrets, and OS and editor noise. Cargo.lock is committed, not ignored, since this is a deployable application rather than a library, and a committed lockfile keeps builds reproducible.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test`
Expected: compile error, `todo!()` panics, or the crate fails to build because `Currency::new` is unimplemented.

- [ ] **Step 3: Implement `Currency::new`**

Replace the `todo!()` body in `core/src/currency.rs` with:

```rust
    pub fn new(code: &str) -> Result<Self, CurrencyError> {
        let upper = code.trim().to_uppercase();
        if upper.is_empty() {
            return Err(CurrencyError::Empty);
        }
        let len_ok = (2..=10).contains(&upper.len());
        let chars_ok = upper.chars().all(|c| c.is_ascii_alphanumeric());
        if !len_ok || !chars_ok {
            return Err(CurrencyError::InvalidFormat(code.to_string()));
        }
        Ok(Currency(upper))
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test`
Expected: all 8 tests in `currency::tests` pass.

- [ ] **Step 5: Write a generic README and commit**

Replace `README.md` at the repo root with:

```markdown
# ledger

A personal, multi-currency money tracker. Accounts, pots, loans, investments,
and bank statement imports, running as a small self-hosted service.

This project is a work in progress.
```

```bash
git add -A
git commit -m "Scaffold workspace and add the Currency type"
```

---

## Task 2: Account model and the storage trait

**Files:**
- Create: `core/src/account.rs`
- Create: `core/src/store.rs`
- Modify: `core/src/lib.rs`

**Interfaces:**
- Consumes: `Currency` (Task 1).
- Produces: `AccountKind` (`Own`, `Outside`, `Person`, `Investment`), `Account { id, name, currency, kind, opening_balance, archived }`, `Account::new(name, currency, kind, opening_balance) -> Account`, the `LedgerStore` trait (`save_account`, `get_account`, `all_accounts`), and `InMemoryStore`. Later tasks add more methods to `LedgerStore` and more fields to `InMemoryStore`.

- [ ] **Step 1: Write the failing tests**

Create `core/src/account.rs`:

```rust
use crate::currency::Currency;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AccountKind {
    Own,
    Outside,
    Person,
    Investment,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: Uuid,
    pub name: String,
    pub currency: Currency,
    pub kind: AccountKind,
    pub opening_balance: Decimal,
    pub archived: bool,
}

impl Account {
    pub fn new(
        name: impl Into<String>,
        currency: Currency,
        kind: AccountKind,
        opening_balance: Decimal,
    ) -> Self {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn a_new_account_gets_a_unique_id_and_starts_unarchived() {
        let eur = Currency::new("EUR").unwrap();
        let a = Account::new("Checking", eur.clone(), AccountKind::Own, dec!(0));
        let b = Account::new("Checking", eur, AccountKind::Own, dec!(0));
        assert_ne!(a.id, b.id);
        assert!(!a.archived);
        assert_eq!(a.opening_balance, dec!(0));
    }
}
```

Create `core/src/store.rs`:

```rust
use crate::account::Account;
use std::collections::HashMap;
use uuid::Uuid;

pub trait LedgerStore {
    fn save_account(&mut self, account: Account);
    fn get_account(&self, id: Uuid) -> Option<Account>;
    fn all_accounts(&self) -> Vec<Account>;
}

#[derive(Default)]
pub struct InMemoryStore {
    accounts: HashMap<Uuid, Account>,
}

impl LedgerStore for InMemoryStore {
    fn save_account(&mut self, account: Account) {
        self.accounts.insert(account.id, account);
    }

    fn get_account(&self, id: Uuid) -> Option<Account> {
        self.accounts.get(&id).cloned()
    }

    fn all_accounts(&self) -> Vec<Account> {
        self.accounts.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountKind;
    use crate::currency::Currency;
    use rust_decimal_macros::dec;

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
        store.save_account(account.clone());
        assert_eq!(store.get_account(id), Some(account));
    }

    #[test]
    fn unknown_id_returns_none() {
        let store = InMemoryStore::default();
        assert_eq!(store.get_account(Uuid::new_v4()), None);
    }

    #[test]
    fn all_accounts_lists_every_saved_account() {
        let mut store = InMemoryStore::default();
        let eur = Currency::new("EUR").unwrap();
        store.save_account(Account::new("A", eur.clone(), AccountKind::Own, dec!(0)));
        store.save_account(Account::new("B", eur, AccountKind::Own, dec!(0)));
        assert_eq!(store.all_accounts().len(), 2);
    }
}
```

Add to `core/src/lib.rs`:

```rust
pub mod account;
pub mod store;

pub use account::{Account, AccountKind};
pub use store::{InMemoryStore, LedgerStore};
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test`
Expected: `Account::new` panics on `todo!()`.

- [ ] **Step 3: Implement `Account::new`**

Replace the `todo!()` body in `core/src/account.rs`:

```rust
    pub fn new(
        name: impl Into<String>,
        currency: Currency,
        kind: AccountKind,
        opening_balance: Decimal,
    ) -> Self {
        Account {
            id: Uuid::new_v4(),
            name: name.into(),
            currency,
            kind,
            opening_balance,
            archived: false,
        }
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test`
Expected: all tests in `account::tests` and `store::tests` pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add the Account model and the LedgerStore trait"
```

---

## Task 3: Entries and account balance

**Files:**
- Create: `core/src/entry.rs`
- Create: `core/src/error.rs`
- Create: `core/src/ledger.rs`
- Modify: `core/src/store.rs`
- Modify: `core/src/lib.rs`

**Interfaces:**
- Consumes: `Account`, `AccountKind`, `Currency`, `LedgerStore`, `InMemoryStore` (Tasks 1 and 2).
- Produces: `EntrySource` (`Manual`, `Imported`), `BankState` (`Completed`, `Pending`, `Reverted`), `Entry { id, account_id, date, time, amount, currency, description, note, category, tags, pot_id, transfer_account_id, source, bank_state, confirmed, voided_reason }`, `Entry::is_voided(&self) -> bool`, `LedgerError`, `Ledger<S>::new(store) -> Ledger<S>`, `Ledger<S>::open_account(name, currency, kind, opening_balance) -> Account`, `Ledger<S>::record_manual_entry(account_id, date, amount, description) -> Result<Entry, LedgerError>`, `Ledger<S>::account_balance(account_id) -> Result<Decimal, LedgerError>`.

- [ ] **Step 1: Write the failing tests**

Create `core/src/entry.rs`:

```rust
use crate::currency::Currency;
use chrono::{NaiveDate, NaiveTime};
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EntrySource {
    Manual,
    Imported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BankState {
    Completed,
    Pending,
    Reverted,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub id: Uuid,
    pub account_id: Uuid,
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    pub amount: Decimal,
    pub currency: Currency,
    pub description: String,
    pub note: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub pot_id: Option<Uuid>,
    pub transfer_account_id: Option<Uuid>,
    pub source: EntrySource,
    pub bank_state: BankState,
    pub confirmed: bool,
    pub voided_reason: Option<String>,
}

impl Entry {
    pub fn is_voided(&self) -> bool {
        self.voided_reason.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> Entry {
        Entry {
            id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
            date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            time: None,
            amount: Decimal::ZERO,
            currency: Currency::new("EUR").unwrap(),
            description: "test".to_string(),
            note: None,
            category: None,
            tags: Vec::new(),
            pot_id: None,
            transfer_account_id: None,
            source: EntrySource::Manual,
            bank_state: BankState::Completed,
            confirmed: false,
            voided_reason: None,
        }
    }

    #[test]
    fn a_fresh_entry_is_not_voided() {
        assert!(!sample_entry().is_voided());
    }

    #[test]
    fn an_entry_with_a_reason_is_voided() {
        let mut e = sample_entry();
        e.voided_reason = Some("typo".to_string());
        assert!(e.is_voided());
    }
}
```

Create `core/src/error.rs`:

```rust
use crate::currency::Currency;
use uuid::Uuid;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LedgerError {
    #[error("account {0} not found")]
    AccountNotFound(Uuid),
    #[error("entry {0} not found")]
    EntryNotFound(Uuid),
    #[error("pot {0} not found")]
    PotNotFound(Uuid),
    #[error("cannot edit a locked field on this entry")]
    EntryLocked,
    #[error("entry is already voided")]
    AlreadyVoided,
    #[error("a void reason is required")]
    VoidReasonRequired,
    #[error("entry parts must sum to the entry amount")]
    PartsDoNotSumToAmount,
    #[error("allocation would leave general savings below zero in {0}")]
    GeneralSavingsWouldGoNegative(Currency),
    #[error("allocation would leave the pot below zero")]
    PotWouldGoNegative,
    #[error("a cross-currency transfer needs an explicit amount received")]
    CrossCurrencyAmountRequired,
}
```

Create `core/src/ledger.rs`:

```rust
use crate::account::{Account, AccountKind};
use crate::currency::Currency;
use crate::entry::{BankState, Entry, EntrySource};
use crate::error::LedgerError;
use crate::store::LedgerStore;
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
        todo!()
    }

    pub fn account_balance(&self, account_id: Uuid) -> Result<Decimal, LedgerError> {
        todo!()
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
        assert_eq!(
            ledger.account_balance(Uuid::new_v4()),
            Err(LedgerError::AccountNotFound(Uuid::nil()))
                .map_err(|_| LedgerError::AccountNotFound(Uuid::nil()))
                .or(Err(LedgerError::AccountNotFound(Uuid::nil())))
        );
    }

    #[test]
    fn recording_against_an_unknown_account_is_an_error() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let result = ledger.record_manual_entry(Uuid::new_v4(), a_date(), dec!(10), "x");
        assert!(matches!(result, Err(LedgerError::AccountNotFound(_))));
    }
}
```

Note: the `the_balance_of_an_unknown_account_is_an_error` test above only checks the error variant shape, not a specific id; simplify it in Step 3 review if it reads awkwardly (it will be tidied in Task 3 Step 3 below).

Add to `core/src/store.rs`, inside the `LedgerStore` trait and `InMemoryStore`:

```rust
    fn save_entry(&mut self, entry: crate::entry::Entry);
    fn get_entry(&self, id: Uuid) -> Option<crate::entry::Entry>;
    fn entries_for_account(&self, account_id: Uuid) -> Vec<crate::entry::Entry>;
```

and to the `InMemoryStore` struct add an `entries: HashMap<Uuid, crate::entry::Entry>` field plus matching impl methods (mirroring the account methods, filtering `entries_for_account` by `e.account_id == account_id`).

Add to `core/src/lib.rs`:

```rust
pub mod entry;
pub mod error;
pub mod ledger;

pub use entry::{BankState, Entry, EntrySource};
pub use error::LedgerError;
pub use ledger::Ledger;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test`
Expected: compile errors from the two `todo!()` bodies and the unfinished `store.rs` additions.

- [ ] **Step 3: Implement the entry storage, recording, and balance**

In `core/src/store.rs`, add the field and methods:

```rust
use crate::entry::Entry;
// (keep the existing `use crate::account::Account;` and `use std::collections::HashMap;` and `use uuid::Uuid;`)

pub trait LedgerStore {
    fn save_account(&mut self, account: Account);
    fn get_account(&self, id: Uuid) -> Option<Account>;
    fn all_accounts(&self) -> Vec<Account>;
    fn save_entry(&mut self, entry: Entry);
    fn get_entry(&self, id: Uuid) -> Option<Entry>;
    fn entries_for_account(&self, account_id: Uuid) -> Vec<Entry>;
}

#[derive(Default)]
pub struct InMemoryStore {
    accounts: HashMap<Uuid, Account>,
    entries: HashMap<Uuid, Entry>,
}

impl LedgerStore for InMemoryStore {
    fn save_account(&mut self, account: Account) {
        self.accounts.insert(account.id, account);
    }

    fn get_account(&self, id: Uuid) -> Option<Account> {
        self.accounts.get(&id).cloned()
    }

    fn all_accounts(&self) -> Vec<Account> {
        self.accounts.values().cloned().collect()
    }

    fn save_entry(&mut self, entry: Entry) {
        self.entries.insert(entry.id, entry);
    }

    fn get_entry(&self, id: Uuid) -> Option<Entry> {
        self.entries.get(&id).cloned()
    }

    fn entries_for_account(&self, account_id: Uuid) -> Vec<Entry> {
        self.entries
            .values()
            .filter(|e| e.account_id == account_id)
            .cloned()
            .collect()
    }
}
```

In `core/src/ledger.rs`, replace the two `todo!()` bodies:

```rust
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
```

Simplify the awkward test in `core/src/ledger.rs`'s test module to:

```rust
    #[test]
    fn the_balance_of_an_unknown_account_is_an_error() {
        let ledger = Ledger::new(InMemoryStore::default());
        assert!(matches!(
            ledger.account_balance(Uuid::new_v4()),
            Err(LedgerError::AccountNotFound(_))
        ));
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test`
Expected: all tests in `entry::tests`, `store::tests`, and `ledger::tests` pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add entries, LedgerError, and account balance calculation"
```

---

## Task 4: Manual entry editing rules and void

**Files:**
- Modify: `core/src/ledger.rs`

**Interfaces:**
- Consumes: `Entry`, `EntrySource`, `LedgerError` (Task 3).
- Produces: `Ledger<S>::edit_manual_entry_amount(entry_id, new_amount) -> Result<Entry, LedgerError>`, `Ledger<S>::confirm_entry(entry_id) -> Result<Entry, LedgerError>`, `Ledger<S>::void_entry(entry_id, reason) -> Result<Entry, LedgerError>`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `core/src/ledger.rs`:

```rust
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
```

Add empty method stubs (with `todo!()`) to the `impl<S: LedgerStore> Ledger<S>` block:

```rust
    pub fn edit_manual_entry_amount(
        &mut self,
        entry_id: Uuid,
        new_amount: Decimal,
    ) -> Result<Entry, LedgerError> {
        todo!()
    }

    pub fn confirm_entry(&mut self, entry_id: Uuid) -> Result<Entry, LedgerError> {
        todo!()
    }

    pub fn void_entry(&mut self, entry_id: Uuid, reason: &str) -> Result<Entry, LedgerError> {
        todo!()
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test`
Expected: panics on the three new `todo!()` bodies.

- [ ] **Step 3: Implement editing, confirming, and voiding**

Replace the three `todo!()` bodies:

```rust
    pub fn edit_manual_entry_amount(
        &mut self,
        entry_id: Uuid,
        new_amount: Decimal,
    ) -> Result<Entry, LedgerError> {
        let mut entry = self
            .store
            .get_entry(entry_id)
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
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
```

`Entry` needs `PartialEq` for `EntrySource`, which it already derives (Task 3), so `entry.source != EntrySource::Manual` compiles as is.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test`
Expected: all tests pass, including the five new ones.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add manual entry editing, confirming, and void with a reason"
```

**Note on coverage:** `edit_manual_entry_amount` already refuses any entry whose `source` is not `EntrySource::Manual`, so an imported entry is locked immediately, the same as a confirmed manual one, with no separate code path. There is no test for that specific case in this task, because nothing in this plan can yet create an `Imported` entry: that only happens once an importer exists. The importer plan must add a test that an imported entry is locked on arrival, exercising this same check.

---

## Task 5: Entry parts (splits)

**Files:**
- Modify: `core/src/entry.rs`
- Modify: `core/src/store.rs`
- Modify: `core/src/ledger.rs`

**Interfaces:**
- Consumes: `Entry`, `LedgerError`, `LedgerStore` (Tasks 3 and 4).
- Produces: `EntryPart { id, entry_id, amount, category, transfer_account_id }`, `Ledger<S>::split_entry(entry_id, parts: Vec<(Decimal, Option<String>)>) -> Result<Vec<EntryPart>, LedgerError>`, `Ledger<S>::parts_for_entry(entry_id) -> Vec<EntryPart>`.

- [ ] **Step 1: Write the failing tests**

Add to `core/src/entry.rs`:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EntryPart {
    pub id: Uuid,
    pub entry_id: Uuid,
    pub amount: Decimal,
    pub category: Option<String>,
    pub transfer_account_id: Option<Uuid>,
}
```

Add to `core/src/store.rs`, in the trait and `InMemoryStore`:

```rust
    fn save_entry_parts(&mut self, entry_id: Uuid, parts: Vec<crate::entry::EntryPart>);
    fn parts_for_entry(&self, entry_id: Uuid) -> Vec<crate::entry::EntryPart>;
```

with a new `entry_parts: HashMap<Uuid, Vec<crate::entry::EntryPart>>` field on `InMemoryStore`, `save_entry_parts` inserting by `entry_id`, and `parts_for_entry` returning a clone of that entry's vector or an empty vector.

Add to the `tests` module in `core/src/ledger.rs`:

```rust
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
        let result = ledger.split_entry(
            entry.id,
            vec![(dec!(-300), None), (dec!(-100), None)],
        );
        assert_eq!(result, Err(LedgerError::PartsDoNotSumToAmount));
        assert!(ledger.parts_for_entry(entry.id).is_empty());
    }
```

Add a stub method to `impl<S: LedgerStore> Ledger<S>`:

```rust
    pub fn split_entry(
        &mut self,
        entry_id: Uuid,
        parts: Vec<(Decimal, Option<String>)>,
    ) -> Result<Vec<crate::entry::EntryPart>, LedgerError> {
        todo!()
    }

    pub fn parts_for_entry(&self, entry_id: Uuid) -> Vec<crate::entry::EntryPart> {
        self.store.parts_for_entry(entry_id)
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test`
Expected: panic on `split_entry`'s `todo!()`.

- [ ] **Step 3: Implement `split_entry`**

```rust
    pub fn split_entry(
        &mut self,
        entry_id: Uuid,
        parts: Vec<(Decimal, Option<String>)>,
    ) -> Result<Vec<crate::entry::EntryPart>, LedgerError> {
        let entry = self
            .store
            .get_entry(entry_id)
            .ok_or(LedgerError::EntryNotFound(entry_id))?;
        let sum: Decimal = parts.iter().map(|(amount, _)| *amount).sum();
        if sum != entry.amount {
            return Err(LedgerError::PartsDoNotSumToAmount);
        }
        let built: Vec<crate::entry::EntryPart> = parts
            .into_iter()
            .map(|(amount, category)| crate::entry::EntryPart {
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add entry parts with a sum-to-amount invariant"
```

---

## Task 6: Pots and allocations, with the never-below-zero invariant

**Files:**
- Create: `core/src/pot.rs`
- Modify: `core/src/store.rs`
- Modify: `core/src/ledger.rs`
- Modify: `core/src/lib.rs`

**Interfaces:**
- Consumes: `Currency`, `AccountKind`, `LedgerError`, `Ledger<S>::account_balance` (Tasks 1 to 3).
- Produces: `Pot { id, name, currency, target, priority }`, `Allocation { id, pot_id, amount, date, note }`, `Ledger<S>::open_pot(name, currency, target, priority) -> Pot`, `Ledger<S>::pot_balance(pot_id) -> Result<Decimal, LedgerError>`, `Ledger<S>::general_savings(&currency) -> Decimal`, `Ledger<S>::allocate_to_pot(pot_id, amount, date) -> Result<Allocation, LedgerError>`.

- [ ] **Step 1: Write the failing tests**

Create `core/src/pot.rs`:

```rust
use crate::currency::Currency;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pot {
    pub id: Uuid,
    pub name: String,
    pub currency: Currency,
    pub target: Option<Decimal>,
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Allocation {
    pub id: Uuid,
    pub pot_id: Uuid,
    pub amount: Decimal,
    pub date: NaiveDate,
    pub note: Option<String>,
}
```

Add to `core/src/store.rs`, in the trait and `InMemoryStore`:

```rust
    fn save_pot(&mut self, pot: crate::pot::Pot);
    fn get_pot(&self, id: Uuid) -> Option<crate::pot::Pot>;
    fn all_pots(&self) -> Vec<crate::pot::Pot>;
    fn save_allocation(&mut self, allocation: crate::pot::Allocation);
    fn allocations_for_pot(&self, pot_id: Uuid) -> Vec<crate::pot::Allocation>;
```

with `pots: HashMap<Uuid, crate::pot::Pot>` and `allocations: HashMap<Uuid, crate::pot::Allocation>` fields, and matching impls (mirroring accounts and entries).

Add to `core/src/lib.rs`:

```rust
pub mod pot;
pub use pot::{Allocation, Pot};
```

Add to the `tests` module in `core/src/ledger.rs`:

```rust
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
        ledger
            .allocate_to_pot(pot.id, dec!(100), a_date())
            .unwrap();
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
        assert_eq!(
            result,
            Err(LedgerError::GeneralSavingsWouldGoNegative(eur))
        );
        assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(0)));
    }

    #[test]
    fn unallocating_more_than_a_pot_holds_is_refused() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100));
        let pot = ledger.open_pot("Emergency fund", eur, None, None);
        ledger
            .allocate_to_pot(pot.id, dec!(50), a_date())
            .unwrap();
        let result = ledger.allocate_to_pot(pot.id, dec!(-60), a_date());
        assert_eq!(result, Err(LedgerError::PotWouldGoNegative));
    }

    #[test]
    fn unallocating_moves_money_back_to_general_savings() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(100));
        let pot = ledger.open_pot("Emergency fund", eur.clone(), None, None);
        ledger
            .allocate_to_pot(pot.id, dec!(100), a_date())
            .unwrap();
        ledger
            .allocate_to_pot(pot.id, dec!(-40), a_date())
            .unwrap();
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
```

Add stub methods to `impl<S: LedgerStore> Ledger<S>`:

```rust
    pub fn open_pot(
        &mut self,
        name: &str,
        currency: Currency,
        target: Option<Decimal>,
        priority: Option<i32>,
    ) -> crate::pot::Pot {
        let pot = crate::pot::Pot {
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
        todo!()
    }

    pub fn general_savings(&self, currency: &Currency) -> Decimal {
        todo!()
    }

    pub fn allocate_to_pot(
        &mut self,
        pot_id: Uuid,
        amount: Decimal,
        date: NaiveDate,
    ) -> Result<crate::pot::Allocation, LedgerError> {
        todo!()
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test`
Expected: panics on the three `todo!()` bodies.

- [ ] **Step 3: Implement pot balance, general savings, and allocation**

```rust
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
            .map(|a| self.account_balance(a.id).unwrap_or(Decimal::ZERO))
            .sum()
    }

    fn pots_total(&self, currency: &Currency) -> Decimal {
        self.store
            .all_pots()
            .into_iter()
            .filter(|p| &p.currency == currency)
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
    ) -> Result<crate::pot::Allocation, LedgerError> {
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
        let allocation = crate::pot::Allocation {
            id: Uuid::new_v4(),
            pot_id,
            amount,
            date,
            note: None,
        };
        self.store.save_allocation(allocation.clone());
        Ok(allocation)
    }
```

`Account` and `Pot` need `Currency` to support `&Currency == &Currency`, which already works since `Currency` derives `PartialEq`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test`
Expected: all tests pass, including the boundary case at exactly zero.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add pots and allocations with the never-below-zero invariant"
```

---

## Task 7: Transfers, including loans as transfers to a person account

**Files:**
- Modify: `core/src/ledger.rs`

**Interfaces:**
- Consumes: `Entry`, `AccountKind`, `LedgerError` (Tasks 2 to 4).
- Produces: `Ledger<S>::transfer(from_account, to_account, date, amount_sent, amount_received, description) -> Result<(Entry, Entry), LedgerError>`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `core/src/ledger.rs`:

```rust
    #[test]
    fn a_same_currency_transfer_moves_the_same_amount_both_ways() {
        let mut ledger = Ledger::new(InMemoryStore::default());
        let eur = Currency::new("EUR").unwrap();
        let a = ledger.open_account("A", eur.clone(), AccountKind::Own, dec!(100));
        let b = ledger.open_account("B", eur.clone(), AccountKind::Own, dec!(0));
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
        // Guards against silently treating the sent amount as also received
        // just because the caller forgot to convert it.
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
        ledger.void_entry(out_entry.id, "recorded against the wrong account").unwrap();
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
```

Add a stub method to `impl<S: LedgerStore> Ledger<S>`:

```rust
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
        todo!()
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test`
Expected: panic on `transfer`'s `todo!()`.

- [ ] **Step 3: Implement `transfer`**

```rust
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
```

Note the second new check: a same-currency transfer with `amount_sent != amount_received` is also refused, since that would create or destroy value between two accounts in the same currency, which the spec's conservation rule forbids just as much as a mislabelled cross-currency one.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test`
Expected: all tests pass. In particular, `lending_money_is_a_transfer_to_a_person_account_and_can_make_it_negative` and `an_own_account_can_go_negative` pass with no special-casing beyond `transfer` itself, confirming loans need no separate mechanism.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add transfers between accounts, with conservation and loans as person accounts"
```

---

## Task 8: Investment and loan valuations

**Files:**
- Create: `core/src/valuation.rs`
- Modify: `core/src/account.rs`
- Modify: `core/src/store.rs`
- Modify: `core/src/ledger.rs`
- Modify: `core/src/lib.rs`

**Interfaces:**
- Consumes: `Account`, `AccountKind`, `LedgerError`, `Ledger<S>::account_balance` (Tasks 2, 3, 6).
- Produces: `Valuation { id, account_id, date, old_value, new_value, category }`, `Valuation::gain(&self) -> Decimal`, `Ledger<S>::update_current_value(account_id, new_value, category, date) -> Result<Valuation, LedgerError>`, `Ledger<S>::current_value(account_id) -> Result<Decimal, LedgerError>`.

- [ ] **Step 1: Write the failing tests**

Create `core/src/valuation.rs`:

```rust
use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Valuation {
    pub id: Uuid,
    pub account_id: Uuid,
    pub date: NaiveDate,
    pub old_value: Decimal,
    pub new_value: Decimal,
    pub category: String,
}

impl Valuation {
    pub fn gain(&self) -> Decimal {
        self.new_value - self.old_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_is_the_difference_between_old_and_new_value() {
        use rust_decimal_macros::dec;
        let v = Valuation {
            id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
            date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            old_value: dec!(1000),
            new_value: dec!(1042),
            category: "Investment gain".to_string(),
        };
        assert_eq!(v.gain(), dec!(42));
    }

    #[test]
    fn a_loss_is_a_negative_gain() {
        use rust_decimal_macros::dec;
        let v = Valuation {
            id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
            date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            old_value: dec!(1000),
            new_value: dec!(950),
            category: "Investment gain".to_string(),
        };
        assert_eq!(v.gain(), dec!(-50));
    }
}
```

In `core/src/account.rs`, add a field to `Account` and set it in `Account::new`:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: Uuid,
    pub name: String,
    pub currency: Currency,
    pub kind: AccountKind,
    pub opening_balance: Decimal,
    pub archived: bool,
    pub current_value: Option<Decimal>,
}

impl Account {
    pub fn new(
        name: impl Into<String>,
        currency: Currency,
        kind: AccountKind,
        opening_balance: Decimal,
    ) -> Self {
        Account {
            id: Uuid::new_v4(),
            name: name.into(),
            currency,
            kind,
            opening_balance,
            archived: false,
            current_value: None,
        }
    }
}
```

This does not break the Task 2 tests: they read fields off the returned `Account` (`a.archived`, `a.opening_balance`), never build one with a struct literal, so adding a field changes nothing for them.

Add to `core/src/store.rs`, in the trait and `InMemoryStore`:

```rust
    fn save_valuation(&mut self, valuation: crate::valuation::Valuation);
    fn valuations_for_account(&self, account_id: Uuid) -> Vec<crate::valuation::Valuation>;
```

with a `valuations: HashMap<Uuid, Vec<crate::valuation::Valuation>>` field and matching impls.

Add to `core/src/lib.rs`:

```rust
pub mod valuation;
pub use valuation::Valuation;
```

Add to the `tests` module in `core/src/ledger.rs`:

```rust
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
```

Add stub methods to `impl<S: LedgerStore> Ledger<S>`:

```rust
    pub fn current_value(&self, account_id: Uuid) -> Result<Decimal, LedgerError> {
        todo!()
    }

    pub fn update_current_value(
        &mut self,
        account_id: Uuid,
        new_value: Decimal,
        category: &str,
        date: NaiveDate,
    ) -> Result<crate::valuation::Valuation, LedgerError> {
        todo!()
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd core && cargo test`
Expected: panics on the two `todo!()` bodies.

- [ ] **Step 3: Implement current value tracking and valuations**

```rust
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
    ) -> Result<crate::valuation::Valuation, LedgerError> {
        let mut account = self
            .store
            .get_account(account_id)
            .ok_or(LedgerError::AccountNotFound(account_id))?;
        let old_value = self.current_value(account_id)?;
        let valuation = crate::valuation::Valuation {
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd core && cargo test`
Expected: every test in the crate passes. Run `cd core && cargo clippy -- -D warnings` too, and fix anything it flags (in particular, no `unwrap()` or `expect()` should exist outside test code).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add investment and loan current value tracking with valuation entries"
```

---

## What this plan does not cover

Follow-up plans, once this one is merged:

1. **SQLite storage.** A second `LedgerStore` implementation backed by SQLite (`sqlx`), with migrations, replacing `InMemoryStore` in production while keeping every test in this plan passing unchanged against the new implementation.
2. **The Axum API.** A thin HTTP layer over `Ledger<S>`, one route per screen action in the design spec.
3. **Importers.** Two bank statement readers (CSV and PDF shapes), each named generically per the spec's privacy rule, each ending with a balance check against the statement.
4. **The Svelte frontend.** The screens already designed, calling the API.
5. **Deployment.** The Dockerfile, Helm chart, and CI, per the spec's deployment section.

## A note on the spec

This plan's tests use only generic, made-up data. The full design spec, which this plan implements, currently contains real personal financial figures as examples throughout (real balances, real dates, real people). Before any of that spec is copied into this public repository, it needs a pass to replace those examples with generic ones, the same way this plan's tests are generic. Until then, I'd keep the spec itself in a private location and treat this plan, and the repository, as the only public-facing artifacts.
