use crate::account::{Account, AccountKind};
use crate::currency::Currency;
use crate::entry::{BankState, Entry, EntryMetadata, EntrySource};
use crate::error::LedgerError;
use crate::id::{AccountId, EntryId, ImportId, QueueRowId};
use crate::import::{
    Import, ImportQueueRow, ImportQueueRowMatch, NormalDetail, QueueRowView, RowDetail, RowReview,
};
use crate::ledger::Ledger;
use crate::pot::Pot;
use crate::store::{InMemoryStore, LedgerStore};
use crate::SqliteStore;
use chrono::{NaiveDate, NaiveTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 3, d).unwrap()
}

fn at(h: u32, m: u32) -> Option<NaiveTime> {
    NaiveTime::from_hms_opt(h, m, 0)
}

fn eur() -> Currency {
    Currency::new("EUR").unwrap()
}

fn setup() -> (Ledger<InMemoryStore>, Account) {
    let mut ledger = Ledger::new(InMemoryStore::default());
    let account = ledger
        .open_account("Card", eur(), AccountKind::Own, dec!(100))
        .unwrap();
    (ledger, account)
}

fn an_import(account: &Account) -> Import {
    Import {
        id: ImportId::generate(),
        account_id: account.id,
        currency: account.currency.clone(),
        file_name: "statement.csv".to_string(),
        uploaded_at: Utc::now(),
        rows_read: 0,
        opening_balance: dec!(0),
        closing_balance: dec!(0),
        completed: false,
    }
}

fn normal(
    import: &Import,
    date: NaiveDate,
    time: Option<NaiveTime>,
    amount: Decimal,
    description: &str,
) -> ImportQueueRow {
    ImportQueueRow {
        id: QueueRowId::generate(),
        import_id: import.id,
        date,
        time,
        amount,
        currency: import.currency.clone(),
        detail: RowDetail::Normal(NormalDetail {
            description: description.to_string(),
            bank_state: BankState::Completed,
            category: None,
        }),
    }
}

/// `row` (a Normal row) with a different bank state.
fn in_state(row: ImportQueueRow, bank_state: BankState) -> ImportQueueRow {
    let detail = row.normal().cloned().expect("a Normal row");
    ImportQueueRow {
        detail: RowDetail::Normal(NormalDetail {
            bank_state,
            ..detail
        }),
        ..row
    }
}

fn pending(
    import: &Import,
    date: NaiveDate,
    time: Option<NaiveTime>,
    amount: Decimal,
    description: &str,
) -> ImportQueueRow {
    in_state(
        normal(import, date, time, amount, description),
        BankState::Pending,
    )
}

fn reverted_candidate(
    import: &Import,
    date: NaiveDate,
    time: Option<NaiveTime>,
    amount: Decimal,
) -> ImportQueueRow {
    ImportQueueRow {
        detail: RowDetail::RevertedCandidate,
        ..normal(import, date, time, amount, "")
    }
}

fn to_entry(row: &ImportQueueRow, entry_id: EntryId) -> ImportQueueRowMatch {
    ImportQueueRowMatch::to_entry(row.id, entry_id)
}

fn to_row(row: &ImportQueueRow, other: &ImportQueueRow) -> ImportQueueRowMatch {
    ImportQueueRowMatch::to_queue_row(row.id, other.id)
}

/// Stages one import holding `rows` and `matches` and returns it.
fn stage<S: LedgerStore>(
    ledger: &mut Ledger<S>,
    account: &Account,
    build: impl FnOnce(&Import) -> (Vec<ImportQueueRow>, Vec<ImportQueueRowMatch>),
) -> (Import, Vec<ImportQueueRow>) {
    let import = an_import(account);
    let (rows, matches) = build(&import);
    ledger
        .stage_import(import.clone(), rows.clone(), matches)
        .unwrap();
    (import, rows)
}

fn an_entry_with_category<S: LedgerStore>(
    ledger: &mut Ledger<S>,
    account: &Account,
    description: &str,
    category: Option<&str>,
) -> Entry {
    let entry = ledger
        .record_manual_entry(account.id, day(1), dec!(-1), description)
        .unwrap();
    ledger
        .update_entry_metadata(
            entry.id,
            EntryMetadata {
                category: category.map(str::to_string),
                ..EntryMetadata::default()
            },
        )
        .unwrap()
}

fn tagged_to(pot: &Pot) -> EntryMetadata {
    EntryMetadata {
        pot_id: Some(pot.id),
        ..EntryMetadata::default()
    }
}

fn a_timed_entry<S: LedgerStore>(
    ledger: &mut Ledger<S>,
    account: &Account,
    amount: Decimal,
) -> Entry {
    let mut entry = ledger
        .record_manual_entry(account.id, day(5), amount, "Shop")
        .unwrap();
    entry.time = at(10, 0);
    ledger.store.save_entry(entry.clone()).unwrap();
    entry
}

fn queue<S: LedgerStore>(ledger: &Ledger<S>, import: &Import) -> Vec<crate::import::QueueRowView> {
    ledger.import_queue(import.id).unwrap()
}

/// What a Normal row in the queue is suspicious of: the entries and the
/// sibling rows it matched.
fn matched(view: &QueueRowView) -> (Vec<EntryId>, Vec<QueueRowId>) {
    match &view.review {
        RowReview::Normal {
            matched_entry_ids,
            matched_queue_row_ids,
            ..
        } => (matched_entry_ids.clone(), matched_queue_row_ids.clone()),
        RowReview::RevertedCandidate { .. } => panic!("expected a Normal row"),
    }
}

fn is_suspicious(view: &QueueRowView) -> bool {
    matches!(
        view.review,
        RowReview::Normal {
            suspicious: true,
            ..
        }
    )
}

fn suggested_category(view: &QueueRowView) -> Option<String> {
    match &view.review {
        RowReview::Normal {
            suggested_category, ..
        } => suggested_category.clone(),
        RowReview::RevertedCandidate { .. } => panic!("expected a Normal row"),
    }
}

fn is_completed<S: LedgerStore>(ledger: &Ledger<S>, import: &Import) -> bool {
    ledger.import(import.id).unwrap().unwrap().completed
}

// ---- stage_import ----

#[test]
fn staging_saves_the_import_and_its_queue_without_touching_the_ledger() {
    let (mut ledger, account) = setup();
    let existing = a_timed_entry(&mut ledger, &account, dec!(-5));
    let (import, rows) = stage(&mut ledger, &account, |i| {
        let a = normal(i, day(5), at(10, 0), dec!(-5), "Shop");
        let r = reverted_candidate(i, day(5), at(10, 0), dec!(-5));
        let matches = vec![to_entry(&a, existing.id), to_entry(&r, existing.id)];
        (vec![a, r], matches)
    });
    assert_eq!(ledger.import(import.id), Ok(Some(import.clone())));
    assert_eq!(queue(&ledger, &import).len(), 2);
    assert_eq!(ledger.entries(account.id).unwrap(), vec![existing.clone()]);
    assert_eq!(
        ledger
            .store
            .get_entry(existing.id)
            .unwrap()
            .unwrap()
            .bank_state,
        BankState::Completed
    );
    assert_eq!(rows.len(), 2);
}

#[test]
fn staging_is_refused_while_the_account_has_an_incomplete_import() {
    let (mut ledger, account) = setup();
    stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(1), None, dec!(-1), "x")], vec![])
    });
    let second = an_import(&account);
    assert_eq!(
        ledger.stage_import(
            second.clone(),
            vec![normal(&second, day(2), None, dec!(-1), "y")],
            vec![]
        ),
        Err(LedgerError::IncompleteImportExists)
    );
    assert_eq!(ledger.import(second.id), Ok(None));
}

#[test]
fn staging_succeeds_when_the_accounts_only_import_is_completed() {
    let (mut ledger, account) = setup();
    let (first, _) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(1), None, dec!(-1), "x")], vec![])
    });
    ledger.discard_import(first.id).unwrap();
    let second = an_import(&account);
    ledger.stage_import(second.clone(), vec![], vec![]).unwrap();
    assert!(ledger.import(second.id).unwrap().is_some());
}

#[test]
fn a_failed_stage_writes_nothing_at_all() {
    let mut ledger = Ledger::new(SqliteStore::open_in_memory().unwrap());
    let account = ledger
        .open_account("Card", eur(), AccountKind::Own, dec!(0))
        .unwrap();
    let import = an_import(&account);
    let row = normal(&import, day(1), None, dec!(-1), "x");
    // A match pointing at nothing breaks the store's exactly-one-target
    // rule, so the save fails part way through the transaction.
    let broken = ImportQueueRowMatch::to_entry(row.id, EntryId::generate());
    // Use a non-existent entry_id so the FK constraint fails.
    assert!(ledger
        .stage_import(import.clone(), vec![row.clone()], vec![broken])
        .is_err());
    assert_eq!(ledger.import(import.id), Ok(None));
    assert_eq!(ledger.store.get_queue_row(row.id), Ok(None));
    assert_eq!(ledger.incomplete_import_for_account(account.id), Ok(None));
    assert!(ledger.entries(account.id).unwrap().is_empty());
}

// ---- reads ----

#[test]
fn the_queue_view_shows_suspicion_matches_and_suggestions() {
    let (mut ledger, account) = setup();
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food"));
    let existing = a_timed_entry(&mut ledger, &account, dec!(-5));
    let (import, rows) = stage(&mut ledger, &account, |i| {
        let clean = normal(i, day(2), None, dec!(-3), "Bakery");
        let p1 = pending(i, day(5), at(10, 0), dec!(-5), "Shop");
        let p2 = pending(i, day(5), at(10, 0), dec!(-5), "Shop");
        let rc = reverted_candidate(i, day(6), at(9, 0), dec!(-7));
        let matches = vec![
            to_entry(&p1, existing.id),
            to_row(&p1, &p2),
            to_row(&p2, &p1),
            to_entry(&rc, existing.id),
        ];
        (vec![clean, p1, p2, rc], matches)
    });
    let view = queue(&ledger, &import);
    let find = |id: QueueRowId| view.iter().find(|v| v.row.id == id).unwrap();
    assert_eq!(
        find(rows[0].id).review,
        RowReview::Normal {
            suspicious: false,
            matched_entry_ids: vec![],
            matched_queue_row_ids: vec![],
            suggested_category: Some("Food".to_string()),
        }
    );
    assert_eq!(
        find(rows[1].id).review,
        RowReview::Normal {
            suspicious: true,
            matched_entry_ids: vec![existing.id],
            matched_queue_row_ids: vec![rows[2].id],
            suggested_category: None,
        }
    );
    assert_eq!(
        find(rows[2].id).review,
        RowReview::Normal {
            suspicious: true,
            matched_entry_ids: vec![],
            matched_queue_row_ids: vec![rows[1].id],
            suggested_category: None,
        }
    );
    assert_eq!(
        find(rows[3].id).review,
        RowReview::RevertedCandidate {
            suggested_entry_id: Some(existing.id)
        }
    );
    assert_eq!(
        view.iter().map(|v| v.row.date).collect::<Vec<_>>(),
        vec![day(2), day(5), day(5), day(6)]
    );
}

#[test]
fn the_queue_of_an_unknown_import_is_not_found() {
    let (ledger, _) = setup();
    let id = ImportId::generate();
    assert_eq!(
        ledger.import_queue(id),
        Err(LedgerError::ImportNotFound(id))
    );
}

#[test]
fn imports_lists_every_import() {
    let (mut ledger, account) = setup();
    let (import, _) = stage(&mut ledger, &account, |_| (vec![], vec![]));
    assert_eq!(ledger.imports(), Ok(vec![import]));
}

// ---- accept ----

#[test]
fn accepting_a_row_saves_it_as_a_locked_imported_entry_and_removes_it() {
    let (mut ledger, account) = setup();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        let row = pending(i, day(3), at(23, 15), dec!(-9.99), "Cinema, popcorn");
        (
            vec![row, normal(i, day(4), None, dec!(-1), "other")],
            vec![],
        )
    });
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, Some("Fun".to_string()))
        .unwrap();
    assert_eq!(entry.account_id, account.id);
    assert_eq!(entry.date, day(3));
    assert_eq!(entry.time, at(23, 15));
    assert_eq!(entry.amount, dec!(-9.99));
    assert_eq!(entry.currency, eur());
    assert_eq!(entry.description, "Cinema, popcorn");
    assert_eq!(entry.category, Some("Fun".to_string()));
    assert_eq!(entry.source, EntrySource::Imported);
    assert_eq!(entry.bank_state, BankState::Pending);
    assert_eq!(ledger.store.get_entry(entry.id), Ok(Some(entry.clone())));
    assert_eq!(ledger.store.get_queue_row(rows[0].id), Ok(None));
    assert!(!is_completed(&ledger, &import));
    assert_eq!(
        ledger.edit_manual_entry_amount(entry.id, dec!(-1)),
        Err(LedgerError::EntryLocked)
    );
}

#[test]
fn resolving_the_last_row_completes_the_import() {
    let (mut ledger, account) = setup();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(1), None, dec!(-1), "a"),
                normal(i, day(2), None, dec!(-2), "b"),
            ],
            vec![],
        )
    });
    ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .unwrap();
    assert!(!is_completed(&ledger, &import));
    ledger.discard_queue_row(import.id, rows[1].id).unwrap();
    assert!(is_completed(&ledger, &import));
    assert_eq!(ledger.incomplete_import_for_account(account.id), Ok(None));
}

#[test]
fn an_explicit_category_beats_a_patched_one_and_a_suggestion() {
    let (mut ledger, account) = setup();
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food"));
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-3), "Bakery")], vec![])
    });
    ledger
        .set_queue_row_category(import.id, rows[0].id, Some("Treats".to_string()))
        .unwrap();
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, Some("Gifts".to_string()))
        .unwrap();
    assert_eq!(entry.category, Some("Gifts".to_string()));
}

#[test]
fn a_patched_category_beats_a_suggestion_when_accept_omits_one() {
    let (mut ledger, account) = setup();
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food"));
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-3), "Bakery")], vec![])
    });
    ledger
        .set_queue_row_category(import.id, rows[0].id, Some("Treats".to_string()))
        .unwrap();
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .unwrap();
    assert_eq!(entry.category, Some("Treats".to_string()));
}

#[test]
fn unanimous_history_suggests_a_category_ignoring_uncategorised_entries() {
    let (mut ledger, account) = setup();
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food"));
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food"));
    an_entry_with_category(&mut ledger, &account, "Bakery", None);
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-3), "Bakery")], vec![])
    });
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .unwrap();
    assert_eq!(entry.category, Some("Food".to_string()));
}

#[test]
fn mixed_history_suggests_nothing() {
    let (mut ledger, account) = setup();
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food"));
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Treats"));
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-3), "Bakery")], vec![])
    });
    assert_eq!(suggested_category(&queue(&ledger, &import)[0]), None);
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .unwrap();
    assert_eq!(entry.category, None);
}

#[test]
fn history_from_another_account_suggests_nothing() {
    let (mut ledger, account) = setup();
    let other = ledger
        .open_account("Other", eur(), AccountKind::Own, dec!(0))
        .unwrap();
    an_entry_with_category(&mut ledger, &other, "Bakery", Some("Food"));
    let (import, _) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-3), "Bakery")], vec![])
    });
    assert_eq!(suggested_category(&queue(&ledger, &import)[0]), None);
}

#[test]
fn a_suggestion_reflects_a_row_accepted_earlier_in_the_same_session() {
    let (mut ledger, account) = setup();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(2), None, dec!(-3), "Bakery"),
                normal(i, day(3), None, dec!(-4), "Bakery"),
            ],
            vec![],
        )
    });
    ledger
        .accept_queue_row(import.id, rows[0].id, Some("Food".to_string()))
        .unwrap();
    let second = ledger
        .accept_queue_row(import.id, rows[1].id, None)
        .unwrap();
    assert_eq!(second.category, Some("Food".to_string()));
}

#[test]
fn a_voided_entrys_category_does_not_drive_a_suggestion() {
    let (mut ledger, account) = setup();
    let old = ledger
        .record_manual_entry(account.id, day(1), dec!(-3), "Bakery")
        .unwrap();
    ledger
        .update_entry_metadata(
            old.id,
            EntryMetadata {
                category: Some("Food".to_string()),
                ..EntryMetadata::default()
            },
        )
        .unwrap();
    ledger.void_entry(old.id, "wrong account").unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-4), "Bakery")], vec![])
    });
    let accepted = ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .unwrap();
    assert_eq!(accepted.category, None);
}

#[test]
fn accept_needs_a_normal_row_in_that_import() {
    let (mut ledger, account) = setup();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![reverted_candidate(i, day(1), at(1, 0), dec!(-1))],
            vec![],
        )
    });
    assert_eq!(
        ledger.accept_queue_row(import.id, rows[0].id, None),
        Err(LedgerError::WrongQueueRowKind)
    );
    assert_eq!(
        ledger.accept_queue_row_as_transfer(import.id, rows[0].id, AccountId::generate(), None),
        Err(LedgerError::WrongQueueRowKind)
    );
    assert_eq!(
        ledger.set_queue_row_category(import.id, rows[0].id, None),
        Err(LedgerError::WrongQueueRowKind)
    );
    let missing_import = ImportId::generate();
    assert_eq!(
        ledger.accept_queue_row(missing_import, rows[0].id, None),
        Err(LedgerError::ImportNotFound(missing_import))
    );
    let missing_row = QueueRowId::generate();
    assert_eq!(
        ledger.accept_queue_row(import.id, missing_row, None),
        Err(LedgerError::QueueRowNotFound(missing_row))
    );
    assert_eq!(queue(&ledger, &import).len(), 1);
}

#[test]
fn a_row_id_from_a_different_import_is_not_found() {
    let (mut ledger, account) = setup();
    let other_account = ledger
        .open_account("Other", eur(), AccountKind::Own, dec!(0))
        .unwrap();
    let (import, _) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(1), None, dec!(-1), "a")], vec![])
    });
    let (_, other_rows) = stage(&mut ledger, &other_account, |i| {
        (vec![normal(i, day(1), None, dec!(-1), "b")], vec![])
    });
    let foreign = other_rows[0].id;
    assert_eq!(
        ledger.accept_queue_row(import.id, foreign, None),
        Err(LedgerError::QueueRowNotFound(foreign))
    );
    assert_eq!(
        ledger.set_queue_row_category(import.id, foreign, None),
        Err(LedgerError::QueueRowNotFound(foreign))
    );
    assert_eq!(
        ledger.discard_queue_row(import.id, foreign),
        Err(LedgerError::QueueRowNotFound(foreign))
    );
    assert_eq!(
        ledger.resolve_reverted_candidate(import.id, foreign),
        Err(LedgerError::QueueRowNotFound(foreign))
    );
    assert!(ledger.store.get_queue_row(foreign).unwrap().is_some());
}

// ---- sibling matches ----

fn two_matched_pending_rows(
    ledger: &mut Ledger<InMemoryStore>,
    account: &Account,
) -> (Import, Vec<ImportQueueRow>) {
    stage(ledger, account, |i| {
        let a = pending(i, day(5), at(10, 0), dec!(-5), "Shop");
        let b = pending(i, day(5), at(10, 0), dec!(-5), "Shop");
        let matches = vec![to_row(&a, &b), to_row(&b, &a)];
        (vec![a, b], matches)
    })
}

#[test]
fn accepting_one_of_two_matched_rows_repoints_the_sibling_at_the_new_entry() {
    let (mut ledger, account) = setup();
    let (import, rows) = two_matched_pending_rows(&mut ledger, &account);
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .unwrap();
    let view = queue(&ledger, &import);
    assert_eq!(view.len(), 1);
    assert!(is_suspicious(&view[0]));
    assert_eq!(matched(&view[0]), (vec![entry.id], vec![]));
    assert!(ledger
        .store
        .queue_row_matches_referencing(rows[0].id)
        .unwrap()
        .is_empty());
}

#[test]
fn accepting_as_transfer_repoints_the_sibling_at_this_accounts_entry() {
    let (mut ledger, account) = setup();
    let savings = ledger
        .open_account("Savings", eur(), AccountKind::Own, dec!(0))
        .unwrap();
    let (import, rows) = two_matched_pending_rows(&mut ledger, &account);
    let (out_entry, _) = ledger
        .accept_queue_row_as_transfer(import.id, rows[0].id, savings.id, None)
        .unwrap();
    assert_eq!(out_entry.account_id, account.id);
    let view = queue(&ledger, &import);
    assert_eq!(matched(&view[0]).0, vec![out_entry.id]);
}

#[test]
fn discarding_one_of_two_matched_rows_clears_the_pair_but_not_the_siblings_other_match() {
    let (mut ledger, account) = setup();
    let existing = a_timed_entry(&mut ledger, &account, dec!(-5));
    let (import, rows) = stage(&mut ledger, &account, |i| {
        let a = pending(i, day(5), at(10, 0), dec!(-5), "Shop");
        let b = pending(i, day(5), at(10, 0), dec!(-5), "Shop");
        let c = pending(i, day(6), None, dec!(-1), "x");
        let matches = vec![
            to_row(&a, &b),
            to_row(&b, &a),
            to_entry(&b, existing.id),
            to_row(&c, &a),
        ];
        (vec![a, b, c], matches)
    });
    ledger.discard_queue_row(import.id, rows[0].id).unwrap();
    let view = queue(&ledger, &import);
    let b = view.iter().find(|v| v.row.id == rows[1].id).unwrap();
    assert!(is_suspicious(b));
    assert_eq!(matched(b), (vec![existing.id], vec![]));
    let c = view.iter().find(|v| v.row.id == rows[2].id).unwrap();
    assert!(!is_suspicious(c));
    assert!(ledger.entries(account.id).unwrap().len() == 1);
}

// ---- accept as transfer ----

#[test]
fn an_outgoing_row_accepted_as_transfer_sends_from_this_account() {
    let (mut ledger, account) = setup();
    let savings = ledger
        .open_account("Savings", eur(), AccountKind::Own, dec!(0))
        .unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![normal(i, day(2), None, dec!(-40), "To savings")],
            vec![],
        )
    });
    let (out_entry, in_entry) = ledger
        .accept_queue_row_as_transfer(import.id, rows[0].id, savings.id, None)
        .unwrap();
    assert_eq!(
        (out_entry.account_id, out_entry.amount),
        (account.id, dec!(-40))
    );
    assert_eq!(
        (in_entry.account_id, in_entry.amount),
        (savings.id, dec!(40))
    );
    assert_eq!(out_entry.date, day(2));
    assert_eq!(out_entry.description, "To savings");
    assert_eq!(ledger.account_balance(account.id), Ok(dec!(60)));
    assert_eq!(ledger.account_balance(savings.id), Ok(dec!(40)));
    assert_eq!(ledger.store.get_queue_row(rows[0].id), Ok(None));
    assert!(is_completed(&ledger, &import));
}

#[test]
fn an_incoming_row_accepted_as_transfer_sends_from_the_other_account() {
    let (mut ledger, account) = setup();
    let savings = ledger
        .open_account("Savings", eur(), AccountKind::Own, dec!(100))
        .unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![normal(i, day(2), None, dec!(25), "From savings")],
            vec![],
        )
    });
    let (out_entry, in_entry) = ledger
        .accept_queue_row_as_transfer(import.id, rows[0].id, savings.id, None)
        .unwrap();
    assert_eq!(
        (out_entry.account_id, out_entry.amount),
        (savings.id, dec!(-25))
    );
    assert_eq!(
        (in_entry.account_id, in_entry.amount),
        (account.id, dec!(25))
    );
}

#[test]
fn a_cross_currency_transfer_uses_the_given_other_amount_on_the_other_side() {
    let (mut ledger, account) = setup();
    let naira = ledger
        .open_account(
            "Naira",
            Currency::new("NGN").unwrap(),
            AccountKind::Own,
            dec!(0),
        )
        .unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(2), None, dec!(-100), "Send"),
                normal(i, day(3), None, dec!(50), "Receive"),
            ],
            vec![],
        )
    });
    let (out_entry, in_entry) = ledger
        .accept_queue_row_as_transfer(import.id, rows[0].id, naira.id, Some(dec!(170000)))
        .unwrap();
    assert_eq!(
        (out_entry.account_id, out_entry.amount),
        (account.id, dec!(-100))
    );
    assert_eq!(
        (in_entry.account_id, in_entry.amount),
        (naira.id, dec!(170000))
    );
    let (out_entry, in_entry) = ledger
        .accept_queue_row_as_transfer(import.id, rows[1].id, naira.id, Some(dec!(85000)))
        .unwrap();
    assert_eq!(
        (out_entry.account_id, out_entry.amount),
        (naira.id, dec!(-85000))
    );
    assert_eq!(
        (in_entry.account_id, in_entry.amount),
        (account.id, dec!(50))
    );
}

#[test]
fn accept_as_transfer_applies_the_same_validation_as_transfer() {
    let (mut ledger, account) = setup();
    let savings = ledger
        .open_account("Savings", eur(), AccountKind::Own, dec!(0))
        .unwrap();
    let naira = ledger
        .open_account(
            "Naira",
            Currency::new("NGN").unwrap(),
            AccountKind::Own,
            dec!(0),
        )
        .unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(2), None, dec!(-100), "Send"),
                normal(i, day(3), None, dec!(0), "Nothing"),
            ],
            vec![],
        )
    });
    let row = rows[0].id;
    assert_eq!(
        ledger.accept_queue_row_as_transfer(import.id, row, naira.id, None),
        Err(LedgerError::CrossCurrencyAmountRequired)
    );
    assert_eq!(
        ledger.accept_queue_row_as_transfer(import.id, row, savings.id, Some(dec!(99))),
        Err(LedgerError::CrossCurrencyAmountRequired)
    );
    assert_eq!(
        ledger.accept_queue_row_as_transfer(import.id, rows[1].id, savings.id, None),
        Err(LedgerError::TransferAmountMustBePositive)
    );
    let missing = AccountId::generate();
    assert_eq!(
        ledger.accept_queue_row_as_transfer(import.id, row, missing, None),
        Err(LedgerError::AccountNotFound(missing))
    );
    assert_eq!(
        ledger.accept_queue_row_as_transfer(import.id, row, account.id, None),
        Err(LedgerError::TransferToSelfNotAllowed)
    );
    assert_eq!(queue(&ledger, &import).len(), 2);
    assert!(ledger.entries(account.id).unwrap().is_empty());
}

#[test]
fn a_transfer_from_a_timed_pending_row_is_an_ordinary_manual_transfer() {
    let (mut ledger, account) = setup();
    let savings = ledger
        .open_account("Savings", eur(), AccountKind::Own, dec!(0))
        .unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![pending(i, day(2), at(8, 30), dec!(-40), "x")], vec![])
    });
    let (out_entry, in_entry) = ledger
        .accept_queue_row_as_transfer(import.id, rows[0].id, savings.id, None)
        .unwrap();
    for e in [out_entry, in_entry] {
        assert_eq!(e.time, None);
        assert_eq!(e.bank_state, BankState::Completed);
        assert_eq!(e.source, EntrySource::Manual);
        assert_eq!(e.category, None);
    }
}

// ---- resolve reverted candidate ----

fn a_candidate_matching<S: LedgerStore>(
    ledger: &mut Ledger<S>,
    account: &Account,
    entry: &Entry,
) -> (Import, ImportQueueRow) {
    let (import, rows) = stage(ledger, account, |i| {
        let rc = reverted_candidate(i, entry.date, entry.time, entry.amount);
        let m = to_entry(&rc, entry.id);
        (vec![rc], vec![m])
    });
    (import, rows[0].clone())
}

#[test]
fn resolving_a_matched_candidate_reverts_its_entry_and_clears_the_row() {
    let (mut ledger, account) = setup();
    let entry = a_timed_entry(&mut ledger, &account, dec!(-30));
    let (import, rc) = a_candidate_matching(&mut ledger, &account, &entry);
    let reverted = ledger.resolve_reverted_candidate(import.id, rc.id).unwrap();
    assert_eq!(reverted.bank_state, BankState::Reverted);
    assert_eq!(
        ledger
            .store
            .get_entry(entry.id)
            .unwrap()
            .unwrap()
            .bank_state,
        BankState::Reverted
    );
    assert_eq!(ledger.account_balance(account.id), Ok(dec!(100)));
    assert_eq!(ledger.store.get_queue_row(rc.id), Ok(None));
    assert!(ledger
        .store
        .queue_row_matches_referencing(rc.id)
        .unwrap()
        .is_empty());
    assert!(is_completed(&ledger, &import));
}

#[test]
fn resolving_an_unmatched_candidate_is_refused_and_leaves_it_queued() {
    let (mut ledger, account) = setup();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![reverted_candidate(i, day(1), at(1, 0), dec!(-1))],
            vec![],
        )
    });
    assert_eq!(
        ledger.resolve_reverted_candidate(import.id, rows[0].id),
        Err(LedgerError::NoSuggestedMatch)
    );
    assert_eq!(
        ledger.store.get_queue_row(rows[0].id),
        Ok(Some(rows[0].clone()))
    );
}

#[test]
fn resolving_a_normal_row_is_the_wrong_kind() {
    let (mut ledger, account) = setup();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(1), None, dec!(-1), "a")], vec![])
    });
    assert_eq!(
        ledger.resolve_reverted_candidate(import.id, rows[0].id),
        Err(LedgerError::WrongQueueRowKind)
    );
}

#[test]
fn resolving_against_an_already_reverted_entry_succeeds_without_rechecking_the_pot() {
    let (mut ledger, account) = setup();
    let pot = ledger.open_pot("Trip", eur(), None, None).unwrap();
    let income = a_timed_entry(&mut ledger, &account, dec!(50));
    ledger
        .update_entry_metadata(income.id, tagged_to(&pot))
        .unwrap();
    let spend = ledger
        .record_manual_entry(account.id, day(6), dec!(-40), "Hotel")
        .unwrap();
    ledger
        .update_entry_metadata(spend.id, tagged_to(&pot))
        .unwrap();
    let mut already = ledger.store.get_entry(income.id).unwrap().unwrap();
    already.bank_state = BankState::Reverted;
    ledger.store.save_entry(already.clone()).unwrap();
    let (import, rc) = a_candidate_matching(&mut ledger, &account, &already);
    assert_eq!(
        ledger.resolve_reverted_candidate(import.id, rc.id),
        Ok(already.clone())
    );
    assert_eq!(ledger.store.get_entry(income.id), Ok(Some(already)));
    assert_eq!(ledger.store.get_queue_row(rc.id), Ok(None));
    assert!(ledger
        .store
        .queue_row_matches_referencing(rc.id)
        .unwrap()
        .is_empty());
}

#[test]
fn resolving_is_refused_when_reverting_would_push_a_pot_negative() {
    let (mut ledger, account) = setup();
    let pot = ledger.open_pot("Trip", eur(), None, None).unwrap();
    let income = a_timed_entry(&mut ledger, &account, dec!(50));
    let income = ledger
        .update_entry_metadata(income.id, tagged_to(&pot))
        .unwrap();
    let spend = ledger
        .record_manual_entry(account.id, day(6), dec!(-40), "Hotel")
        .unwrap();
    ledger
        .update_entry_metadata(spend.id, tagged_to(&pot))
        .unwrap();
    let (import, rc) = a_candidate_matching(&mut ledger, &account, &income);
    assert_eq!(
        ledger.resolve_reverted_candidate(import.id, rc.id),
        Err(LedgerError::PotWouldGoNegative)
    );
    assert_eq!(ledger.store.get_entry(income.id), Ok(Some(income)));
    assert_eq!(ledger.pot_balance(pot.id), Ok(dec!(10)));
    assert_eq!(ledger.store.get_queue_row(rc.id), Ok(Some(rc.clone())));
    assert_eq!(
        ledger
            .store
            .queue_row_matches_referencing(rc.id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn resolving_against_a_voided_entry_is_refused_and_leaves_the_row() {
    let (mut ledger, account) = setup();
    let entry = a_timed_entry(&mut ledger, &account, dec!(-30));
    let (import, rc) = a_candidate_matching(&mut ledger, &account, &entry);
    ledger.void_entry(entry.id, "duplicate").unwrap();
    assert_eq!(
        ledger.resolve_reverted_candidate(import.id, rc.id),
        Err(LedgerError::AlreadyVoided)
    );
    assert_eq!(ledger.store.get_queue_row(rc.id), Ok(Some(rc)));
}

#[test]
fn discarding_a_matched_candidate_does_not_revert_its_entry() {
    let (mut ledger, account) = setup();
    let entry = a_timed_entry(&mut ledger, &account, dec!(-30));
    let (import, rc) = a_candidate_matching(&mut ledger, &account, &entry);
    ledger.discard_queue_row(import.id, rc.id).unwrap();
    assert_eq!(ledger.store.get_entry(entry.id), Ok(Some(entry)));
    assert!(ledger
        .store
        .queue_row_matches_referencing(rc.id)
        .unwrap()
        .is_empty());
    assert!(is_completed(&ledger, &import));
}

// ---- bulk accept ----

#[test]
fn bulk_accept_is_blocked_while_suspicious_or_candidate_rows_remain() {
    let (mut ledger, account) = setup();
    let existing = a_timed_entry(&mut ledger, &account, dec!(-5));
    let (import, rows) = stage(&mut ledger, &account, |i| {
        let clean = normal(i, day(1), None, dec!(-1), "a");
        let suspicious = normal(i, day(5), at(10, 0), dec!(-5), "Shop");
        let matched_rc = reverted_candidate(i, day(5), at(10, 0), dec!(-5));
        let unmatched_rc = reverted_candidate(i, day(7), at(1, 0), dec!(-9));
        let matches = vec![
            to_entry(&suspicious, existing.id),
            to_entry(&matched_rc, existing.id),
        ];
        (vec![clean, suspicious, matched_rc, unmatched_rc], matches)
    });
    assert_eq!(
        ledger.bulk_accept_import(import.id),
        Err(LedgerError::BulkAcceptBlocked {
            suspicious_count: 1,
            reverted_candidate_count: 2
        })
    );
    ledger.discard_queue_row(import.id, rows[1].id).unwrap();
    assert_eq!(
        ledger.bulk_accept_import(import.id),
        Err(LedgerError::BulkAcceptBlocked {
            suspicious_count: 0,
            reverted_candidate_count: 2
        })
    );
    ledger.discard_queue_row(import.id, rows[3].id).unwrap();
    assert_eq!(
        ledger.bulk_accept_import(import.id),
        Err(LedgerError::BulkAcceptBlocked {
            suspicious_count: 0,
            reverted_candidate_count: 1
        })
    );
    assert!(ledger.store.get_queue_row(rows[2].id).unwrap().is_some());
    assert_eq!(ledger.entries(account.id).unwrap().len(), 1);
    ledger
        .resolve_reverted_candidate(import.id, rows[2].id)
        .unwrap();
    assert_eq!(ledger.bulk_accept_import(import.id), Ok(1));
    assert!(is_completed(&ledger, &import));
}

#[test]
fn bulk_accept_saves_every_clean_row_with_patched_categories_then_suggestions() {
    let (mut ledger, account) = setup();
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food"));
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(2), None, dec!(-3), "Bakery"),
                normal(i, day(3), None, dec!(-4), "Bakery"),
                normal(i, day(4), None, dec!(-5), "Other"),
            ],
            vec![],
        )
    });
    ledger
        .set_queue_row_category(import.id, rows[1].id, Some("Treats".to_string()))
        .unwrap();
    assert_eq!(ledger.bulk_accept_import(import.id), Ok(3));
    let entries = ledger.entries(account.id).unwrap();
    assert_eq!(entries.len(), 4); // 1 pre-existing + 3 accepted
    let accepted: Vec<_> = entries
        .iter()
        .filter(|e| e.source == EntrySource::Imported)
        .collect();
    assert_eq!(accepted.len(), 3);
    // row[0] gets "Food" from history; row[1] was patched to "Treats"
    let bakery_rows: Vec<_> = accepted
        .iter()
        .filter(|e| e.description == "Bakery")
        .collect();
    assert_eq!(bakery_rows.len(), 2);
    let mut bakery_cats: Vec<_> = bakery_rows.iter().map(|e| e.category.clone()).collect();
    bakery_cats.sort();
    assert_eq!(
        bakery_cats,
        vec![Some("Food".to_string()), Some("Treats".to_string())]
    );
    let other = accepted.iter().find(|e| e.description == "Other").unwrap();
    assert_eq!(other.category, None);
    assert!(is_completed(&ledger, &import));
}

#[test]
fn bulk_accept_on_an_empty_queue_completes_the_import() {
    let (mut ledger, account) = setup();
    let (import, _) = stage(&mut ledger, &account, |_| (vec![], vec![]));
    assert_eq!(ledger.bulk_accept_import(import.id), Ok(0));
    assert!(is_completed(&ledger, &import));
}

#[test]
fn discard_import_removes_all_rows_and_marks_it_complete() {
    let (mut ledger, account) = setup();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(1), None, dec!(-1), "a"),
                normal(i, day(2), None, dec!(-2), "b"),
            ],
            vec![],
        )
    });
    assert_eq!(ledger.discard_import(import.id), Ok(2));
    assert!(is_completed(&ledger, &import));
    assert!(ledger.store.get_queue_row(rows[0].id).unwrap().is_none());
    assert!(ledger.store.get_queue_row(rows[1].id).unwrap().is_none());
    assert!(ledger.entries(account.id).unwrap().is_empty());
    assert_eq!(ledger.incomplete_import_for_account(account.id), Ok(None));
}
