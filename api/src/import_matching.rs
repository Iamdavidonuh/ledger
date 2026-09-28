//! Turns a parsed statement into queue rows and match rows, against the
//! account's existing entries. Pure logic over data already fetched: it
//! never reads or writes the ledger itself.

use chrono::{NaiveDate, NaiveTime, Utc};
use importer::{ParseResult, ParsedRow, RevertedCandidate};
use ledger_core::{
    Account, BankState, Entry, EntryId, Import, ImportId, ImportQueueRow, ImportQueueRowMatch,
    NormalDetail, QueueRowId, RowDetail,
};
use rust_decimal::Decimal;

pub struct Staged {
    pub rows: Vec<ImportQueueRow>,
    pub matches: Vec<ImportQueueRowMatch>,
}

impl Staged {
    /// Normal rows that resemble something already in the ledger or in the
    /// same import, and so need a decision before bulk accept can run.
    pub fn suspicious_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.normal().is_some())
            .filter(|row| self.matches.iter().any(|m| m.queue_row_id == row.id))
            .count()
    }
}

/// The import record for a statement that has just passed its balance check.
pub fn new_import(account: &Account, file_name: String, parsed: &ParseResult) -> Import {
    Import {
        id: ImportId::generate(),
        account_id: account.id,
        currency: account.currency.clone(),
        file_name,
        uploaded_at: Utc::now(),
        rows_read: (parsed.rows.len() + parsed.reverted_candidates.len()) as i64,
        opening_balance: parsed.opening_balance,
        closing_balance: parsed.closing_balance,
        completed: false,
    }
}

/// Times only rule a match out when both sides have one and they differ.
fn times_compatible(a: Option<NaiveTime>, b: Option<NaiveTime>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// Same date and amount, times compatible. Equal nonzero amounts already
/// share a sign; a zero amount has no sign and never matches.
fn resembles(
    row: &ImportQueueRow,
    date: NaiveDate,
    time: Option<NaiveTime>,
    amount: Decimal,
) -> bool {
    !row.amount.is_zero()
        && row.amount == amount
        && row.date == date
        && times_compatible(row.time, time)
}

fn normal_row(import: &Import, parsed: &ParsedRow) -> ImportQueueRow {
    ImportQueueRow {
        id: QueueRowId::generate(),
        import_id: import.id,
        date: parsed.date,
        time: parsed.time,
        amount: parsed.amount,
        currency: import.currency.clone(),
        detail: RowDetail::Normal(NormalDetail {
            description: parsed.description.clone(),
            bank_state: parsed.bank_state,
            category: None,
        }),
    }
}

fn reverted_candidate_row(import: &Import, candidate: &RevertedCandidate) -> ImportQueueRow {
    ImportQueueRow {
        id: QueueRowId::generate(),
        import_id: import.id,
        date: candidate.date,
        time: Some(candidate.time),
        amount: candidate.amount,
        currency: import.currency.clone(),
        detail: RowDetail::RevertedCandidate,
    }
}

fn is_pending(row: &ImportQueueRow) -> bool {
    row.normal()
        .is_some_and(|detail| detail.bank_state == BankState::Pending)
}

/// A Normal row is suspicious of every live entry it resembles.
fn matches_against_entries(
    rows: &[ImportQueueRow],
    entries: &[&Entry],
) -> Vec<ImportQueueRowMatch> {
    rows.iter()
        .flat_map(|row| {
            entries
                .iter()
                .filter(|e| resembles(row, e.date, e.time, e.amount))
                .map(|e| ImportQueueRowMatch::to_entry(row.id, e.id))
        })
        .collect()
}

/// Only Pending rows are matched against each other: the balance check
/// already vouches for every Completed row being real. A pair is stored as
/// two match rows, one per direction, so each side is suspicious on its own.
fn matches_among_pending(rows: &[ImportQueueRow]) -> Vec<ImportQueueRowMatch> {
    let pending: Vec<&ImportQueueRow> = rows.iter().filter(|row| is_pending(row)).collect();
    let mut matches = Vec::new();
    for (i, a) in pending.iter().enumerate() {
        for b in &pending[i + 1..] {
            if resembles(a, b.date, b.time, b.amount) {
                matches.push(ImportQueueRowMatch::to_queue_row(a.id, b.id));
                matches.push(ImportQueueRowMatch::to_queue_row(b.id, a.id));
            }
        }
    }
    matches
}

/// The entry a reverted candidate would revert: same date, time and amount,
/// and the entry must have a time. If several match, the lowest id wins.
fn revert_suggestion(candidate: &ImportQueueRow, entries: &[&Entry]) -> Option<EntryId> {
    entries
        .iter()
        .filter(|e| e.time.is_some() && e.time == candidate.time)
        .filter(|e| e.date == candidate.date && e.amount == candidate.amount)
        .map(|e| e.id)
        .min()
}

/// `entries` is every entry in the import's account; only non-voided ones
/// inside the statement's date range are matched against, since a voided
/// entry is no longer a real fact about the account. Reverted entries are
/// left out of duplicate matching for the same reason, but stay in the pool
/// for reverted candidates, so a later, overlapping import can still match
/// an entry an earlier import already reverted (resolving that is a no-op).
pub fn build_queue(import: &Import, parsed: &ParseResult, entries: &[Entry]) -> Staged {
    let range = parsed.date_range;
    let in_range: Vec<&Entry> = entries
        .iter()
        .filter(|e| !e.is_voided() && e.date >= range.start && e.date <= range.end)
        .collect();
    let live: Vec<&Entry> = in_range
        .iter()
        .copied()
        .filter(|e| e.bank_state != BankState::Reverted)
        .collect();

    let normal: Vec<ImportQueueRow> = parsed
        .rows
        .iter()
        .map(|row| normal_row(import, row))
        .collect();
    let candidates: Vec<ImportQueueRow> = parsed
        .reverted_candidates
        .iter()
        .map(|candidate| reverted_candidate_row(import, candidate))
        .collect();

    let mut matches = matches_against_entries(&normal, &live);
    matches.extend(matches_among_pending(&normal));
    matches.extend(candidates.iter().filter_map(|candidate| {
        revert_suggestion(candidate, &in_range)
            .map(|entry_id| ImportQueueRowMatch::to_entry(candidate.id, entry_id))
    }));

    Staged {
        rows: normal.into_iter().chain(candidates).collect(),
        matches,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use importer::DateRange;
    use ledger_core::{AccountId, Currency, EntrySource, MatchTarget};
    use rust_decimal_macros::dec;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, d).unwrap()
    }

    fn at(h: u32) -> Option<NaiveTime> {
        NaiveTime::from_hms_opt(h, 0, 0)
    }

    fn import() -> Import {
        Import {
            id: ImportId::generate(),
            account_id: AccountId::generate(),
            currency: Currency::new("EUR").unwrap(),
            file_name: "f.csv".to_string(),
            uploaded_at: Utc::now(),
            rows_read: 0,
            opening_balance: dec!(0),
            closing_balance: dec!(0),
            completed: false,
        }
    }

    fn entry(import: &Import, date: NaiveDate, time: Option<NaiveTime>, amount: Decimal) -> Entry {
        Entry {
            id: EntryId::generate(),
            account_id: import.account_id,
            date,
            time,
            amount,
            currency: import.currency.clone(),
            description: "e".to_string(),
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

    fn row(
        date: NaiveDate,
        time: Option<NaiveTime>,
        amount: Decimal,
        bank_state: BankState,
    ) -> ParsedRow {
        ParsedRow {
            date,
            time,
            amount,
            description: "r".to_string(),
            bank_state,
        }
    }

    fn parsed(rows: Vec<ParsedRow>, reverted: Vec<RevertedCandidate>) -> ParseResult {
        ParseResult {
            date_range: DateRange {
                start: day(1),
                end: day(20),
            },
            rows,
            reverted_candidates: reverted,
            opening_balance: dec!(0),
            closing_balance: dec!(0),
        }
    }

    fn entry_matches(staged: &Staged, row_index: usize) -> Vec<EntryId> {
        let id = staged.rows[row_index].id;
        staged
            .matches
            .iter()
            .filter(|m| m.queue_row_id == id)
            .filter_map(|m| match m.target {
                MatchTarget::Entry { entry_id } => Some(entry_id),
                MatchTarget::QueueRow { .. } => None,
            })
            .collect()
    }

    fn row_matches(staged: &Staged, row_index: usize) -> Vec<QueueRowId> {
        let id = staged.rows[row_index].id;
        staged
            .matches
            .iter()
            .filter(|m| m.queue_row_id == id)
            .filter_map(|m| match m.target {
                MatchTarget::QueueRow { queue_row_id } => Some(queue_row_id),
                MatchTarget::Entry { .. } => None,
            })
            .collect()
    }

    #[test]
    fn queue_rows_carry_the_imports_currency_and_the_rows_own_facts() {
        let imp = import();
        let staged = build_queue(
            &imp,
            &parsed(
                vec![row(day(2), at(9), dec!(-3), BankState::Pending)],
                vec![],
            ),
            &[],
        );
        let r = &staged.rows[0];
        assert_eq!(
            (r.import_id, r.currency.clone()),
            (imp.id, imp.currency.clone())
        );
        assert_eq!((r.date, r.time, r.amount), (day(2), at(9), dec!(-3)));
        assert_eq!(
            r.detail,
            RowDetail::Normal(NormalDetail {
                description: "r".to_string(),
                bank_state: BankState::Pending,
                category: None,
            })
        );
    }

    #[test]
    fn same_date_and_amount_with_different_times_do_not_match() {
        let imp = import();
        let e = entry(&imp, day(2), at(9), dec!(-3));
        let staged = build_queue(
            &imp,
            &parsed(
                vec![row(day(2), at(10), dec!(-3), BankState::Completed)],
                vec![],
            ),
            &[e],
        );
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn same_date_and_amount_with_no_time_on_either_match() {
        let imp = import();
        let e = entry(&imp, day(2), None, dec!(-3));
        let staged = build_queue(
            &imp,
            &parsed(
                vec![row(day(2), None, dec!(-3), BankState::Completed)],
                vec![],
            ),
            std::slice::from_ref(&e),
        );
        assert_eq!(entry_matches(&staged, 0), vec![e.id]);
    }

    #[test]
    fn a_time_on_only_one_side_still_matches() {
        let imp = import();
        let e = entry(&imp, day(2), None, dec!(-3));
        let staged = build_queue(
            &imp,
            &parsed(
                vec![row(day(2), at(9), dec!(-3), BankState::Completed)],
                vec![],
            ),
            std::slice::from_ref(&e),
        );
        assert_eq!(entry_matches(&staged, 0), vec![e.id]);
    }

    #[test]
    fn a_different_date_amount_or_sign_does_not_match() {
        let imp = import();
        let entries = vec![
            entry(&imp, day(3), None, dec!(-3)),
            entry(&imp, day(2), None, dec!(-4)),
            entry(&imp, day(2), None, dec!(3)),
        ];
        let staged = build_queue(
            &imp,
            &parsed(
                vec![row(day(2), None, dec!(-3), BankState::Completed)],
                vec![],
            ),
            &entries,
        );
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn a_zero_amount_row_is_never_matched() {
        let imp = import();
        let e = entry(&imp, day(2), None, dec!(0));
        let rows = vec![
            row(day(2), None, dec!(0), BankState::Pending),
            row(day(2), None, dec!(0), BankState::Pending),
        ];
        let staged = build_queue(&imp, &parsed(rows, vec![]), &[e]);
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn a_voided_entry_is_never_matched() {
        let imp = import();
        let mut e = entry(&imp, day(2), None, dec!(-3));
        e.voided_reason = Some("dup".to_string());
        let staged = build_queue(
            &imp,
            &parsed(
                vec![row(day(2), None, dec!(-3), BankState::Completed)],
                vec![RevertedCandidate {
                    date: day(2),
                    time: at(9).unwrap(),
                    amount: dec!(-3),
                }],
            ),
            &[e],
        );
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn a_reverted_entry_is_never_flagged_as_a_duplicate() {
        // A Reverted entry no longer counts toward any balance, so a new,
        // unrelated row with the same date and amount is not suspicious.
        let imp = import();
        let mut e = entry(&imp, day(2), at(9), dec!(-3));
        e.bank_state = BankState::Reverted;
        let staged = build_queue(
            &imp,
            &parsed(
                vec![row(day(2), at(9), dec!(-3), BankState::Completed)],
                vec![],
            ),
            &[e],
        );
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn a_reverted_candidate_still_matches_an_entry_that_is_already_reverted() {
        // A later import can overlap an earlier one; resolving the match is
        // idempotent, so it must still be offered.
        let imp = import();
        let mut e = entry(&imp, day(2), at(9), dec!(-3));
        e.bank_state = BankState::Reverted;
        let candidate = RevertedCandidate {
            date: day(2),
            time: at(9).unwrap(),
            amount: dec!(-3),
        };
        let staged = build_queue(&imp, &parsed(vec![], vec![candidate]), &[e.clone()]);
        assert_eq!(entry_matches(&staged, 0), vec![e.id]);
    }

    #[test]
    fn an_entry_outside_the_statements_date_range_is_not_matched() {
        let imp = import();
        let mut p = parsed(
            vec![row(day(2), None, dec!(-3), BankState::Completed)],
            vec![],
        );
        p.date_range = DateRange {
            start: day(5),
            end: day(20),
        };
        let staged = build_queue(&imp, &p, &[entry(&imp, day(2), None, dec!(-3))]);
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn a_row_matching_several_entries_records_every_one() {
        let imp = import();
        let (a, b) = (
            entry(&imp, day(2), None, dec!(-3)),
            entry(&imp, day(2), at(7), dec!(-3)),
        );
        let staged = build_queue(
            &imp,
            &parsed(
                vec![row(day(2), None, dec!(-3), BankState::Completed)],
                vec![],
            ),
            &[a.clone(), b.clone()],
        );
        let mut got = entry_matches(&staged, 0);
        got.sort();
        let mut want = vec![a.id, b.id];
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn two_pending_rows_with_the_same_date_and_amount_match_each_other_both_ways() {
        let imp = import();
        let rows = vec![
            row(day(2), at(9), dec!(-3), BankState::Pending),
            row(day(2), at(9), dec!(-3), BankState::Pending),
        ];
        let staged = build_queue(&imp, &parsed(rows, vec![]), &[]);
        assert_eq!(row_matches(&staged, 0), vec![staged.rows[1].id]);
        assert_eq!(row_matches(&staged, 1), vec![staged.rows[0].id]);
        assert_eq!(staged.matches.len(), 2);
    }

    #[test]
    fn two_completed_rows_or_a_completed_and_a_pending_row_do_not_match_each_other() {
        let imp = import();
        let rows = vec![
            row(day(2), None, dec!(-3), BankState::Completed),
            row(day(2), None, dec!(-3), BankState::Completed),
            row(day(4), None, dec!(-8), BankState::Completed),
            row(day(4), None, dec!(-8), BankState::Pending),
        ];
        let staged = build_queue(&imp, &parsed(rows, vec![]), &[]);
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn a_row_can_match_an_entry_and_a_sibling_at_once() {
        let imp = import();
        let e = entry(&imp, day(2), None, dec!(-3));
        let rows = vec![
            row(day(2), None, dec!(-3), BankState::Pending),
            row(day(2), None, dec!(-3), BankState::Pending),
        ];
        let staged = build_queue(&imp, &parsed(rows, vec![]), std::slice::from_ref(&e));
        assert_eq!(entry_matches(&staged, 0), vec![e.id]);
        assert_eq!(row_matches(&staged, 0), vec![staged.rows[1].id]);
    }

    #[test]
    fn a_reverted_candidate_matches_an_entry_with_the_same_date_time_and_amount() {
        let imp = import();
        let e = entry(&imp, day(2), at(9), dec!(-3));
        let c = RevertedCandidate {
            date: day(2),
            time: at(9).unwrap(),
            amount: dec!(-3),
        };
        let staged = build_queue(&imp, &parsed(vec![], vec![c]), std::slice::from_ref(&e));
        let r = &staged.rows[0];
        assert_eq!((&r.detail, r.time), (&RowDetail::RevertedCandidate, at(9)));
        assert_eq!(entry_matches(&staged, 0), vec![e.id]);
    }

    #[test]
    fn a_reverted_candidate_does_not_match_an_entry_without_a_time_but_is_still_queued() {
        let imp = import();
        let e = entry(&imp, day(2), None, dec!(-3));
        let c = RevertedCandidate {
            date: day(2),
            time: at(9).unwrap(),
            amount: dec!(-3),
        };
        let staged = build_queue(&imp, &parsed(vec![], vec![c]), &[e]);
        assert_eq!(staged.rows.len(), 1);
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn an_unmatched_reverted_candidate_is_still_queued() {
        let imp = import();
        let c = RevertedCandidate {
            date: day(2),
            time: at(9).unwrap(),
            amount: dec!(-3),
        };
        let staged = build_queue(&imp, &parsed(vec![], vec![c]), &[]);
        assert_eq!(staged.rows.len(), 1);
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn a_candidate_matching_several_entries_suggests_the_lowest_entry_id() {
        let imp = import();
        let entries: Vec<Entry> = (0..3)
            .map(|_| entry(&imp, day(2), at(9), dec!(-3)))
            .collect();
        let lowest = entries.iter().map(|e| e.id).min().unwrap();
        let c = RevertedCandidate {
            date: day(2),
            time: at(9).unwrap(),
            amount: dec!(-3),
        };
        let staged = build_queue(&imp, &parsed(vec![], vec![c]), &entries);
        assert_eq!(entry_matches(&staged, 0), vec![lowest]);
    }
}
