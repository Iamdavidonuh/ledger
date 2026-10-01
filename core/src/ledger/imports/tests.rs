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

async fn setup() -> (Ledger<InMemoryStore>, Account) {
    let mut ledger = Ledger::new(InMemoryStore::default());
    let account = ledger
        .open_account("Card", eur(), AccountKind::Own, dec!(100))
        .await
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
async fn stage<S: LedgerStore>(
    ledger: &mut Ledger<S>,
    account: &Account,
    build: impl FnOnce(&Import) -> (Vec<ImportQueueRow>, Vec<ImportQueueRowMatch>),
) -> (Import, Vec<ImportQueueRow>) {
    let import = an_import(account);
    let (rows, matches) = build(&import);
    ledger
        .stage_import(import.clone(), rows.clone(), matches)
        .await
        .unwrap();
    (import, rows)
}

async fn an_entry_with_category<S: LedgerStore>(
    ledger: &mut Ledger<S>,
    account: &Account,
    description: &str,
    category: Option<&str>,
) -> Entry {
    let entry = ledger
        .record_manual_entry(account.id, day(1), dec!(-1), description)
        .await
        .unwrap();
    ledger
        .update_entry_metadata(
            entry.id,
            EntryMetadata {
                category: category.map(str::to_string),
                ..EntryMetadata::default()
            },
        )
        .await
        .unwrap()
}

fn tagged_to(pot: &Pot) -> EntryMetadata {
    EntryMetadata {
        pot_id: Some(pot.id),
        ..EntryMetadata::default()
    }
}

async fn a_timed_entry<S: LedgerStore>(
    ledger: &mut Ledger<S>,
    account: &Account,
    amount: Decimal,
) -> Entry {
    let mut entry = ledger
        .record_manual_entry(account.id, day(5), amount, "Shop")
        .await
        .unwrap();
    entry.time = at(10, 0);
    ledger.store.save_entry(entry.clone()).await.unwrap();
    entry
}

async fn queue<S: LedgerStore>(
    ledger: &mut Ledger<S>,
    import: &Import,
) -> Vec<crate::import::QueueRowView> {
    ledger.import_queue(import.id).await.unwrap()
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

async fn is_completed<S: LedgerStore>(ledger: &mut Ledger<S>, import: &Import) -> bool {
    ledger
        .import(import.id)
        .await
        .unwrap()
        .unwrap()
        .completed
}

// ---- stage_import ----

#[tokio::test]
async fn staging_saves_the_import_and_its_queue_without_touching_the_ledger() {
    let (mut ledger, account) = setup().await;
    let existing = a_timed_entry(&mut ledger, &account, dec!(-5)).await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        let a = normal(i, day(5), at(10, 0), dec!(-5), "Shop");
        let r = reverted_candidate(i, day(5), at(10, 0), dec!(-5));
        let matches = vec![to_entry(&a, existing.id), to_entry(&r, existing.id)];
        (vec![a, r], matches)
    })
    .await;
    assert_eq!(
        ledger.import(import.id).await,
        Ok(Some(import.clone()))
    );
    assert_eq!(queue(&mut ledger, &import).await.len(), 2);
    assert_eq!(
        ledger.entries(account.id).await.unwrap(),
        vec![existing.clone()]
    );
    assert_eq!(
        ledger
            .store
            .get_entry(existing.id)
            .await
            .unwrap()
            .unwrap()
            .bank_state,
        BankState::Completed
    );
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn staging_is_refused_while_the_account_has_an_incomplete_import() {
    let (mut ledger, account) = setup().await;
    stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(1), None, dec!(-1), "x")], vec![])
    })
    .await;
    let second = an_import(&account);
    assert_eq!(
        ledger
            .stage_import(
                second.clone(),
                vec![normal(&second, day(2), None, dec!(-1), "y")],
                vec![]
            )
            .await,
        Err(LedgerError::IncompleteImportExists)
    );
    assert_eq!(ledger.import(second.id).await, Ok(None));
}

#[tokio::test]
async fn staging_succeeds_when_the_accounts_only_import_is_completed() {
    let (mut ledger, account) = setup().await;
    let (first, _) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(1), None, dec!(-1), "x")], vec![])
    })
    .await;
    ledger.discard_import(first.id).await.unwrap();
    let second = an_import(&account);
    ledger
        .stage_import(second.clone(), vec![], vec![])
        .await
        .unwrap();
    assert!(ledger.import(second.id).await.unwrap().is_some());
}

#[tokio::test]
async fn a_failed_stage_writes_nothing_at_all() {
    // Stage a first import so the account already has one incomplete.
    // Then attempt to stage a second: the pre-check fires before any
    // writes, so the second import, its rows and any matches must not
    // appear in the store after the error.
    //
    // The deeper rollback guarantee (mid-write FK failure rolling back
    // a partial transaction) requires a live Postgres and is covered by
    // integration tests against PgStore, not this in-memory suite.
    let (mut ledger, account) = setup().await;
    stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(1), None, dec!(-1), "x")], vec![])
    })
    .await;

    let import = an_import(&account);
    let row = normal(&import, day(2), None, dec!(-1), "y");
    let result = ledger
        .stage_import(import.clone(), vec![row.clone()], vec![])
        .await;

    assert_eq!(result, Err(LedgerError::IncompleteImportExists));
    assert_eq!(ledger.import(import.id).await, Ok(None));
    assert_eq!(ledger.store.get_queue_row(row.id).await, Ok(None));
    assert_eq!(ledger.incomplete_import_for_account(account.id).await.unwrap().map(|i| i.id != import.id), Some(true));
}

// ---- reads ----

#[tokio::test]
async fn the_queue_view_shows_suspicion_matches_and_suggestions() {
    let (mut ledger, account) = setup().await;
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food")).await;
    let existing = a_timed_entry(&mut ledger, &account, dec!(-5)).await;
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
    })
    .await;
    let view = queue(&mut ledger, &import).await;
    let find = |id: QueueRowId| view.iter().find(|v| v.row.id == id).unwrap().clone();
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

#[tokio::test]
async fn the_queue_of_an_unknown_import_is_not_found() {
    let (mut ledger, _) = setup().await;
    let id = ImportId::generate();
    assert_eq!(
        ledger.import_queue(id).await,
        Err(LedgerError::ImportNotFound(id))
    );
}

#[tokio::test]
async fn imports_lists_every_import() {
    let (mut ledger, account) = setup().await;
    let (import, _) = stage(&mut ledger, &account, |_| (vec![], vec![])).await;
    assert_eq!(ledger.imports().await, Ok(vec![import]));
}

// ---- accept ----

#[tokio::test]
async fn accepting_a_row_saves_it_as_a_locked_imported_entry_and_removes_it() {
    let (mut ledger, account) = setup().await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        let row = pending(i, day(3), at(23, 15), dec!(-9.99), "Cinema, popcorn");
        (
            vec![row, normal(i, day(4), None, dec!(-1), "other")],
            vec![],
        )
    })
    .await;
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, Some("Fun".to_string()))
        .await
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
    assert_eq!(
        ledger.store.get_entry(entry.id).await,
        Ok(Some(entry.clone()))
    );
    assert_eq!(ledger.store.get_queue_row(rows[0].id).await, Ok(None));
    assert!(!is_completed(&mut ledger, &import).await);
    assert_eq!(
        ledger.edit_manual_entry_amount(entry.id, dec!(-1)).await,
        Err(LedgerError::EntryLocked)
    );
}

#[tokio::test]
async fn resolving_the_last_row_completes_the_import() {
    let (mut ledger, account) = setup().await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(1), None, dec!(-1), "a"),
                normal(i, day(2), None, dec!(-2), "b"),
            ],
            vec![],
        )
    })
    .await;
    ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .await
        .unwrap();
    assert!(!is_completed(&mut ledger, &import).await);
    ledger
        .discard_queue_row(import.id, rows[1].id)
        .await
        .unwrap();
    assert!(is_completed(&mut ledger, &import).await);
    assert_eq!(
        ledger.incomplete_import_for_account(account.id).await,
        Ok(None)
    );
}

#[tokio::test]
async fn an_explicit_category_beats_a_patched_one_and_a_suggestion() {
    let (mut ledger, account) = setup().await;
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food")).await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-3), "Bakery")], vec![])
    })
    .await;
    ledger
        .set_queue_row_category(import.id, rows[0].id, Some("Treats".to_string()))
        .await
        .unwrap();
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, Some("Gifts".to_string()))
        .await
        .unwrap();
    assert_eq!(entry.category, Some("Gifts".to_string()));
}

#[tokio::test]
async fn a_patched_category_beats_a_suggestion_when_accept_omits_one() {
    let (mut ledger, account) = setup().await;
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food")).await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-3), "Bakery")], vec![])
    })
    .await;
    ledger
        .set_queue_row_category(import.id, rows[0].id, Some("Treats".to_string()))
        .await
        .unwrap();
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .await
        .unwrap();
    assert_eq!(entry.category, Some("Treats".to_string()));
}

#[tokio::test]
async fn unanimous_history_suggests_a_category_ignoring_uncategorised_entries() {
    let (mut ledger, account) = setup().await;
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food")).await;
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food")).await;
    an_entry_with_category(&mut ledger, &account, "Bakery", None).await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-3), "Bakery")], vec![])
    })
    .await;
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .await
        .unwrap();
    assert_eq!(entry.category, Some("Food".to_string()));
}

#[tokio::test]
async fn mixed_history_suggests_nothing() {
    let (mut ledger, account) = setup().await;
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food")).await;
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Treats")).await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-3), "Bakery")], vec![])
    })
    .await;
    assert_eq!(
        suggested_category(&queue(&mut ledger, &import).await[0]),
        None
    );
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .await
        .unwrap();
    assert_eq!(entry.category, None);
}

#[tokio::test]
async fn history_from_another_account_suggests_nothing() {
    let (mut ledger, account) = setup().await;
    let other = ledger
        .open_account("Other", eur(), AccountKind::Own, dec!(0))
        .await
        .unwrap();
    an_entry_with_category(&mut ledger, &other, "Bakery", Some("Food")).await;
    let (import, _) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-3), "Bakery")], vec![])
    })
    .await;
    assert_eq!(
        suggested_category(&queue(&mut ledger, &import).await[0]),
        None
    );
}

#[tokio::test]
async fn a_suggestion_reflects_a_row_accepted_earlier_in_the_same_session() {
    let (mut ledger, account) = setup().await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(2), None, dec!(-3), "Bakery"),
                normal(i, day(3), None, dec!(-4), "Bakery"),
            ],
            vec![],
        )
    })
    .await;
    ledger
        .accept_queue_row(import.id, rows[0].id, Some("Food".to_string()))
        .await
        .unwrap();
    let second = ledger
        .accept_queue_row(import.id, rows[1].id, None)
        .await
        .unwrap();
    assert_eq!(second.category, Some("Food".to_string()));
}

#[tokio::test]
async fn a_voided_entrys_category_does_not_drive_a_suggestion() {
    let (mut ledger, account) = setup().await;
    let old = ledger
        .record_manual_entry(account.id, day(1), dec!(-3), "Bakery")
        .await
        .unwrap();
    ledger
        .update_entry_metadata(
            old.id,
            EntryMetadata {
                category: Some("Food".to_string()),
                ..EntryMetadata::default()
            },
        )
        .await
        .unwrap();
    ledger
        .void_entry(old.id, "wrong account")
        .await
        .unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(2), None, dec!(-4), "Bakery")], vec![])
    })
    .await;
    let accepted = ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .await
        .unwrap();
    assert_eq!(accepted.category, None);
}

#[tokio::test]
async fn accept_needs_a_normal_row_in_that_import() {
    let (mut ledger, account) = setup().await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![reverted_candidate(i, day(1), at(1, 0), dec!(-1))],
            vec![],
        )
    })
    .await;
    assert_eq!(
        ledger.accept_queue_row(import.id, rows[0].id, None).await,
        Err(LedgerError::WrongQueueRowKind)
    );
    assert_eq!(
        ledger
            .accept_queue_row_as_transfer(import.id, rows[0].id, AccountId::generate(), None)
            .await,
        Err(LedgerError::WrongQueueRowKind)
    );
    assert_eq!(
        ledger
            .set_queue_row_category(import.id, rows[0].id, None)
            .await,
        Err(LedgerError::WrongQueueRowKind)
    );
    let missing_import = ImportId::generate();
    assert_eq!(
        ledger
            .accept_queue_row(missing_import, rows[0].id, None)
            .await,
        Err(LedgerError::ImportNotFound(missing_import))
    );
    let missing_row = QueueRowId::generate();
    assert_eq!(
        ledger
            .accept_queue_row(import.id, missing_row, None)
            .await,
        Err(LedgerError::QueueRowNotFound(missing_row))
    );
    assert_eq!(queue(&mut ledger, &import).await.len(), 1);
}

#[tokio::test]
async fn a_row_id_from_a_different_import_is_not_found() {
    let (mut ledger, account) = setup().await;
    let other_account = ledger
        .open_account("Other", eur(), AccountKind::Own, dec!(0))
        .await
        .unwrap();
    let (import, _) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(1), None, dec!(-1), "a")], vec![])
    })
    .await;
    let (_, other_rows) = stage(&mut ledger, &other_account, |i| {
        (vec![normal(i, day(1), None, dec!(-1), "b")], vec![])
    })
    .await;
    let foreign = other_rows[0].id;
    assert_eq!(
        ledger.accept_queue_row(import.id, foreign, None).await,
        Err(LedgerError::QueueRowNotFound(foreign))
    );
    assert_eq!(
        ledger
            .set_queue_row_category(import.id, foreign, None)
            .await,
        Err(LedgerError::QueueRowNotFound(foreign))
    );
    assert_eq!(
        ledger.discard_queue_row(import.id, foreign).await,
        Err(LedgerError::QueueRowNotFound(foreign))
    );
    assert_eq!(
        ledger
            .resolve_reverted_candidate(import.id, foreign)
            .await,
        Err(LedgerError::QueueRowNotFound(foreign))
    );
    assert!(
        ledger
            .store
            .get_queue_row(foreign)
            .await
            .unwrap()
            .is_some()
    );
}

// ---- sibling matches ----

async fn two_matched_pending_rows(
    ledger: &mut Ledger<InMemoryStore>,
    account: &Account,
) -> (Import, Vec<ImportQueueRow>) {
    stage(ledger, account, |i| {
        let a = pending(i, day(5), at(10, 0), dec!(-5), "Shop");
        let b = pending(i, day(5), at(10, 0), dec!(-5), "Shop");
        let matches = vec![to_row(&a, &b), to_row(&b, &a)];
        (vec![a, b], matches)
    })
    .await
}

#[tokio::test]
async fn accepting_one_of_two_matched_rows_repoints_the_sibling_at_the_new_entry() {
    let (mut ledger, account) = setup().await;
    let (import, rows) = two_matched_pending_rows(&mut ledger, &account).await;
    let entry = ledger
        .accept_queue_row(import.id, rows[0].id, None)
        .await
        .unwrap();
    let view = queue(&mut ledger, &import).await;
    assert_eq!(view.len(), 1);
    assert!(is_suspicious(&view[0]));
    assert_eq!(matched(&view[0]), (vec![entry.id], vec![]));
    assert!(
        ledger
            .store
            .queue_row_matches_referencing(rows[0].id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn accepting_as_transfer_repoints_the_sibling_at_this_accounts_entry() {
    let (mut ledger, account) = setup().await;
    let savings = ledger
        .open_account("Savings", eur(), AccountKind::Own, dec!(0))
        .await
        .unwrap();
    let (import, rows) = two_matched_pending_rows(&mut ledger, &account).await;
    let (out_entry, _) = ledger
        .accept_queue_row_as_transfer(import.id, rows[0].id, savings.id, None)
        .await
        .unwrap();
    assert_eq!(out_entry.account_id, account.id);
    let view = queue(&mut ledger, &import).await;
    assert_eq!(matched(&view[0]).0, vec![out_entry.id]);
}

#[tokio::test]
async fn discarding_one_of_two_matched_rows_clears_the_pair_but_not_the_siblings_other_match() {
    let (mut ledger, account) = setup().await;
    let existing = a_timed_entry(&mut ledger, &account, dec!(-5)).await;
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
    })
    .await;
    ledger
        .discard_queue_row(import.id, rows[0].id)
        .await
        .unwrap();
    let view = queue(&mut ledger, &import).await;
    let b = view.iter().find(|v| v.row.id == rows[1].id).unwrap();
    assert!(is_suspicious(b));
    assert_eq!(matched(b), (vec![existing.id], vec![]));
    let c = view.iter().find(|v| v.row.id == rows[2].id).unwrap();
    assert!(!is_suspicious(c));
    assert!(ledger.entries(account.id).await.unwrap().len() == 1);
}

// ---- accept as transfer ----

#[tokio::test]
async fn an_outgoing_row_accepted_as_transfer_sends_from_this_account() {
    let (mut ledger, account) = setup().await;
    let savings = ledger
        .open_account("Savings", eur(), AccountKind::Own, dec!(0))
        .await
        .unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![normal(i, day(2), None, dec!(-40), "To savings")],
            vec![],
        )
    })
    .await;
    let (out_entry, in_entry) = ledger
        .accept_queue_row_as_transfer(import.id, rows[0].id, savings.id, None)
        .await
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
    assert_eq!(
        ledger.account_balance(account.id).await,
        Ok(dec!(60))
    );
    assert_eq!(
        ledger.account_balance(savings.id).await,
        Ok(dec!(40))
    );
    assert_eq!(
        ledger.store.get_queue_row(rows[0].id).await,
        Ok(None)
    );
    assert!(is_completed(&mut ledger, &import).await);
}

#[tokio::test]
async fn an_incoming_row_accepted_as_transfer_sends_from_the_other_account() {
    let (mut ledger, account) = setup().await;
    let savings = ledger
        .open_account("Savings", eur(), AccountKind::Own, dec!(100))
        .await
        .unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![normal(i, day(2), None, dec!(25), "From savings")],
            vec![],
        )
    })
    .await;
    let (out_entry, in_entry) = ledger
        .accept_queue_row_as_transfer(import.id, rows[0].id, savings.id, None)
        .await
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

#[tokio::test]
async fn a_cross_currency_transfer_uses_the_given_other_amount_on_the_other_side() {
    let (mut ledger, account) = setup().await;
    let naira = ledger
        .open_account(
            "Naira",
            Currency::new("NGN").unwrap(),
            AccountKind::Own,
            dec!(0),
        )
        .await
        .unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(2), None, dec!(-100), "Send"),
                normal(i, day(3), None, dec!(50), "Receive"),
            ],
            vec![],
        )
    })
    .await;
    let (out_entry, in_entry) = ledger
        .accept_queue_row_as_transfer(import.id, rows[0].id, naira.id, Some(dec!(170000)))
        .await
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
        .await
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

#[tokio::test]
async fn accept_as_transfer_applies_the_same_validation_as_transfer() {
    let (mut ledger, account) = setup().await;
    let savings = ledger
        .open_account("Savings", eur(), AccountKind::Own, dec!(0))
        .await
        .unwrap();
    let naira = ledger
        .open_account(
            "Naira",
            Currency::new("NGN").unwrap(),
            AccountKind::Own,
            dec!(0),
        )
        .await
        .unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(2), None, dec!(-100), "Send"),
                normal(i, day(3), None, dec!(0), "Nothing"),
            ],
            vec![],
        )
    })
    .await;
    let row = rows[0].id;
    assert_eq!(
        ledger
            .accept_queue_row_as_transfer(import.id, row, naira.id, None)
            .await,
        Err(LedgerError::CrossCurrencyAmountRequired)
    );
    assert_eq!(
        ledger
            .accept_queue_row_as_transfer(import.id, row, savings.id, Some(dec!(99)))
            .await,
        Err(LedgerError::CrossCurrencyAmountRequired)
    );
    assert_eq!(
        ledger
            .accept_queue_row_as_transfer(import.id, rows[1].id, savings.id, None)
            .await,
        Err(LedgerError::TransferAmountMustBePositive)
    );
    let missing = AccountId::generate();
    assert_eq!(
        ledger
            .accept_queue_row_as_transfer(import.id, row, missing, None)
            .await,
        Err(LedgerError::AccountNotFound(missing))
    );
    assert_eq!(
        ledger
            .accept_queue_row_as_transfer(import.id, row, account.id, None)
            .await,
        Err(LedgerError::TransferToSelfNotAllowed)
    );
    assert_eq!(queue(&mut ledger, &import).await.len(), 2);
    assert!(ledger.entries(account.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_transfer_from_a_timed_pending_row_is_an_ordinary_manual_transfer() {
    let (mut ledger, account) = setup().await;
    let savings = ledger
        .open_account("Savings", eur(), AccountKind::Own, dec!(0))
        .await
        .unwrap();
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![pending(i, day(2), at(8, 30), dec!(-40), "x")], vec![])
    })
    .await;
    let (out_entry, in_entry) = ledger
        .accept_queue_row_as_transfer(import.id, rows[0].id, savings.id, None)
        .await
        .unwrap();
    for e in [out_entry, in_entry] {
        assert_eq!(e.time, None);
        assert_eq!(e.bank_state, BankState::Completed);
        assert_eq!(e.source, EntrySource::Manual);
        assert_eq!(e.category, None);
    }
}

// ---- resolve reverted candidate ----

async fn a_candidate_matching<S: LedgerStore>(
    ledger: &mut Ledger<S>,
    account: &Account,
    entry: &Entry,
) -> (Import, ImportQueueRow) {
    let (import, rows) = stage(ledger, account, |i| {
        let rc = reverted_candidate(i, entry.date, entry.time, entry.amount);
        let m = to_entry(&rc, entry.id);
        (vec![rc], vec![m])
    })
    .await;
    (import, rows[0].clone())
}

#[tokio::test]
async fn resolving_a_matched_candidate_reverts_its_entry_and_clears_the_row() {
    let (mut ledger, account) = setup().await;
    let entry = a_timed_entry(&mut ledger, &account, dec!(-30)).await;
    let (import, rc) = a_candidate_matching(&mut ledger, &account, &entry).await;
    let reverted = ledger
        .resolve_reverted_candidate(import.id, rc.id)
        .await
        .unwrap();
    assert_eq!(reverted.bank_state, BankState::Reverted);
    assert_eq!(
        ledger
            .store
            .get_entry(entry.id)
            .await
            .unwrap()
            .unwrap()
            .bank_state,
        BankState::Reverted
    );
    assert_eq!(
        ledger.account_balance(account.id).await,
        Ok(dec!(100))
    );
    assert_eq!(ledger.store.get_queue_row(rc.id).await, Ok(None));
    assert!(
        ledger
            .store
            .queue_row_matches_referencing(rc.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(is_completed(&mut ledger, &import).await);
}

#[tokio::test]
async fn resolving_an_unmatched_candidate_is_refused_and_leaves_it_queued() {
    let (mut ledger, account) = setup().await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![reverted_candidate(i, day(1), at(1, 0), dec!(-1))],
            vec![],
        )
    })
    .await;
    assert_eq!(
        ledger
            .resolve_reverted_candidate(import.id, rows[0].id)
            .await,
        Err(LedgerError::NoSuggestedMatch)
    );
    assert_eq!(
        ledger.store.get_queue_row(rows[0].id).await,
        Ok(Some(rows[0].clone()))
    );
}

#[tokio::test]
async fn resolving_a_normal_row_is_the_wrong_kind() {
    let (mut ledger, account) = setup().await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (vec![normal(i, day(1), None, dec!(-1), "a")], vec![])
    })
    .await;
    assert_eq!(
        ledger
            .resolve_reverted_candidate(import.id, rows[0].id)
            .await,
        Err(LedgerError::WrongQueueRowKind)
    );
}

#[tokio::test]
async fn resolving_against_an_already_reverted_entry_succeeds_without_rechecking_the_pot() {
    let (mut ledger, account) = setup().await;
    let pot = ledger.open_pot("Trip", eur(), None, None).await.unwrap();
    let income = a_timed_entry(&mut ledger, &account, dec!(50)).await;
    ledger
        .update_entry_metadata(income.id, tagged_to(&pot))
        .await
        .unwrap();
    let spend = ledger
        .record_manual_entry(account.id, day(6), dec!(-40), "Hotel")
        .await
        .unwrap();
    ledger
        .update_entry_metadata(spend.id, tagged_to(&pot))
        .await
        .unwrap();
    let mut already = ledger.store.get_entry(income.id).await.unwrap().unwrap();
    already.bank_state = BankState::Reverted;
    ledger.store.save_entry(already.clone()).await.unwrap();
    let (import, rc) = a_candidate_matching(&mut ledger, &account, &already).await;
    assert_eq!(
        ledger
            .resolve_reverted_candidate(import.id, rc.id)
            .await,
        Ok(already.clone())
    );
    assert_eq!(
        ledger.store.get_entry(income.id).await,
        Ok(Some(already))
    );
    assert_eq!(ledger.store.get_queue_row(rc.id).await, Ok(None));
    assert!(
        ledger
            .store
            .queue_row_matches_referencing(rc.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn resolving_is_refused_when_reverting_would_push_a_pot_negative() {
    let (mut ledger, account) = setup().await;
    let pot = ledger.open_pot("Trip", eur(), None, None).await.unwrap();
    let income = a_timed_entry(&mut ledger, &account, dec!(50)).await;
    let income = ledger
        .update_entry_metadata(income.id, tagged_to(&pot))
        .await
        .unwrap();
    let spend = ledger
        .record_manual_entry(account.id, day(6), dec!(-40), "Hotel")
        .await
        .unwrap();
    ledger
        .update_entry_metadata(spend.id, tagged_to(&pot))
        .await
        .unwrap();
    let (import, rc) = a_candidate_matching(&mut ledger, &account, &income).await;
    assert_eq!(
        ledger
            .resolve_reverted_candidate(import.id, rc.id)
            .await,
        Err(LedgerError::PotWouldGoNegative)
    );
    assert_eq!(
        ledger.store.get_entry(income.id).await,
        Ok(Some(income))
    );
    assert_eq!(ledger.pot_balance(pot.id).await, Ok(dec!(10)));
    assert_eq!(
        ledger.store.get_queue_row(rc.id).await,
        Ok(Some(rc.clone()))
    );
    assert_eq!(
        ledger
            .store
            .queue_row_matches_referencing(rc.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn resolving_against_a_voided_entry_is_refused_and_leaves_the_row() {
    let (mut ledger, account) = setup().await;
    let entry = a_timed_entry(&mut ledger, &account, dec!(-30)).await;
    let (import, rc) = a_candidate_matching(&mut ledger, &account, &entry).await;
    ledger.void_entry(entry.id, "duplicate").await.unwrap();
    assert_eq!(
        ledger
            .resolve_reverted_candidate(import.id, rc.id)
            .await,
        Err(LedgerError::AlreadyVoided)
    );
    assert_eq!(ledger.store.get_queue_row(rc.id).await, Ok(Some(rc)));
}

#[tokio::test]
async fn discarding_a_matched_candidate_does_not_revert_its_entry() {
    let (mut ledger, account) = setup().await;
    let entry = a_timed_entry(&mut ledger, &account, dec!(-30)).await;
    let (import, rc) = a_candidate_matching(&mut ledger, &account, &entry).await;
    ledger
        .discard_queue_row(import.id, rc.id)
        .await
        .unwrap();
    assert_eq!(
        ledger.store.get_entry(entry.id).await,
        Ok(Some(entry))
    );
    assert!(
        ledger
            .store
            .queue_row_matches_referencing(rc.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(is_completed(&mut ledger, &import).await);
}

// ---- bulk accept ----

#[tokio::test]
async fn bulk_accept_is_blocked_while_suspicious_or_candidate_rows_remain() {
    let (mut ledger, account) = setup().await;
    let existing = a_timed_entry(&mut ledger, &account, dec!(-5)).await;
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
    })
    .await;
    assert_eq!(
        ledger.bulk_accept_import(import.id).await,
        Err(LedgerError::BulkAcceptBlocked {
            suspicious_count: 1,
            reverted_candidate_count: 2
        })
    );
    ledger
        .discard_queue_row(import.id, rows[1].id)
        .await
        .unwrap();
    assert_eq!(
        ledger.bulk_accept_import(import.id).await,
        Err(LedgerError::BulkAcceptBlocked {
            suspicious_count: 0,
            reverted_candidate_count: 2
        })
    );
    ledger
        .discard_queue_row(import.id, rows[3].id)
        .await
        .unwrap();
    assert_eq!(
        ledger.bulk_accept_import(import.id).await,
        Err(LedgerError::BulkAcceptBlocked {
            suspicious_count: 0,
            reverted_candidate_count: 1
        })
    );
    assert!(
        ledger
            .store
            .get_queue_row(rows[2].id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(ledger.entries(account.id).await.unwrap().len(), 1);
    ledger
        .resolve_reverted_candidate(import.id, rows[2].id)
        .await
        .unwrap();
    assert_eq!(ledger.bulk_accept_import(import.id).await, Ok(1));
    assert!(is_completed(&mut ledger, &import).await);
}

#[tokio::test]
async fn bulk_accept_saves_every_clean_row_with_patched_categories_then_suggestions() {
    let (mut ledger, account) = setup().await;
    an_entry_with_category(&mut ledger, &account, "Bakery", Some("Food")).await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(2), None, dec!(-3), "Bakery"),
                normal(i, day(3), None, dec!(-4), "Bakery"),
                normal(i, day(4), None, dec!(-5), "Other"),
            ],
            vec![],
        )
    })
    .await;
    ledger
        .set_queue_row_category(import.id, rows[1].id, Some("Treats".to_string()))
        .await
        .unwrap();
    assert_eq!(ledger.bulk_accept_import(import.id).await, Ok(3));
    let entries = ledger.entries(account.id).await.unwrap();
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
    assert!(is_completed(&mut ledger, &import).await);
}

#[tokio::test]
async fn bulk_accept_on_an_empty_queue_completes_the_import() {
    let (mut ledger, account) = setup().await;
    let (import, _) = stage(&mut ledger, &account, |_| (vec![], vec![])).await;
    assert_eq!(ledger.bulk_accept_import(import.id).await, Ok(0));
    assert!(is_completed(&mut ledger, &import).await);
}

#[tokio::test]
async fn discard_import_removes_all_rows_and_marks_it_complete() {
    let (mut ledger, account) = setup().await;
    let (import, rows) = stage(&mut ledger, &account, |i| {
        (
            vec![
                normal(i, day(1), None, dec!(-1), "a"),
                normal(i, day(2), None, dec!(-2), "b"),
            ],
            vec![],
        )
    })
    .await;
    assert_eq!(ledger.discard_import(import.id).await, Ok(2));
    assert!(is_completed(&mut ledger, &import).await);
    assert!(
        ledger
            .store
            .get_queue_row(rows[0].id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        ledger
            .store
            .get_queue_row(rows[1].id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(ledger.entries(account.id).await.unwrap().is_empty());
    assert_eq!(
        ledger.incomplete_import_for_account(account.id).await,
        Ok(None)
    );
}
