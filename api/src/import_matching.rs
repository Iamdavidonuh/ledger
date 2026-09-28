//! Turns a parsed statement into queue rows and match rows, against the
//! account's existing entries. Pure logic over data already fetched: it
//! never reads or writes the ledger itself.

use chrono::NaiveTime;
use importer::ParseResult;
use ledger_core::{BankState, Entry, Import, ImportQueueRow, ImportQueueRowMatch, QueueRowKind};
use uuid::Uuid;

pub struct Staged {
    pub rows: Vec<ImportQueueRow>,
    pub matches: Vec<ImportQueueRowMatch>,
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
fn resembles(row: &ImportQueueRow, date: chrono::NaiveDate, time: Option<NaiveTime>, amount: rust_decimal::Decimal) -> bool {
    !row.amount.is_zero() && row.amount == amount && row.date == date && times_compatible(row.time, time)
}

fn match_row(queue_row_id: Uuid, matched_entry_id: Option<Uuid>, matched_queue_row_id: Option<Uuid>) -> ImportQueueRowMatch {
    ImportQueueRowMatch { id: Uuid::new_v4(), queue_row_id, matched_entry_id, matched_queue_row_id }
}

/// `entries` is every entry in the import's account; only non-voided ones
/// inside the statement's date range are matched against.
pub fn build_queue(import: &Import, parsed: &ParseResult, entries: &[Entry]) -> Staged {
    let range = parsed.date_range;
    let candidates: Vec<&Entry> = entries
        .iter()
        .filter(|e| !e.is_voided() && e.date >= range.start && e.date <= range.end)
        .collect();

    let new_row = |kind, date, time, amount, description: &str, bank_state| ImportQueueRow {
        id: Uuid::new_v4(),
        import_id: import.id,
        kind,
        date,
        time,
        amount,
        currency: import.currency.clone(),
        description: description.to_string(),
        bank_state,
        category: None,
    };

    let mut rows = Vec::new();
    let mut matches = Vec::new();

    let normal: Vec<ImportQueueRow> = parsed
        .rows
        .iter()
        .map(|r| new_row(QueueRowKind::Normal, r.date, r.time, r.amount, &r.description, r.bank_state))
        .collect();
    for row in &normal {
        for entry in candidates.iter().filter(|e| resembles(row, e.date, e.time, e.amount)) {
            matches.push(match_row(row.id, Some(entry.id), None));
        }
    }
    // Only Pending rows are matched against each other: the balance check
    // already vouches for every Completed row being real.
    let pending: Vec<&ImportQueueRow> = normal.iter().filter(|r| r.bank_state == BankState::Pending).collect();
    for (i, a) in pending.iter().enumerate() {
        for b in &pending[i + 1..] {
            if resembles(a, b.date, b.time, b.amount) {
                matches.push(match_row(a.id, None, Some(b.id)));
                matches.push(match_row(b.id, None, Some(a.id)));
            }
        }
    }
    rows.extend(normal);

    for c in &parsed.reverted_candidates {
        let row = new_row(QueueRowKind::RevertedCandidate, c.date, Some(c.time), c.amount, "", BankState::Reverted);
        let suggestion = candidates
            .iter()
            .filter(|e| e.time == Some(c.time) && e.date == c.date && e.amount == c.amount)
            .map(|e| e.id)
            .min();
        if let Some(entry_id) = suggestion {
            matches.push(match_row(row.id, Some(entry_id), None));
        }
        rows.push(row);
    }

    Staged { rows, matches }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime, Utc};
    use importer::{DateRange, ParseResult, ParsedRow, RevertedCandidate};
    use ledger_core::{BankState, Currency, Entry, EntrySource, Import, QueueRowKind};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, d).unwrap()
    }

    fn at(h: u32) -> Option<NaiveTime> {
        NaiveTime::from_hms_opt(h, 0, 0)
    }

    fn import() -> Import {
        Import {
            id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
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
            id: Uuid::new_v4(),
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

    fn row(date: NaiveDate, time: Option<NaiveTime>, amount: Decimal, bank_state: BankState) -> ParsedRow {
        ParsedRow { date, time, amount, description: "r".to_string(), bank_state }
    }

    fn parsed(rows: Vec<ParsedRow>, reverted: Vec<RevertedCandidate>) -> ParseResult {
        ParseResult {
            date_range: DateRange { start: day(1), end: day(20) },
            rows,
            reverted_candidates: reverted,
            opening_balance: dec!(0),
            closing_balance: dec!(0),
        }
    }

    fn entry_matches(staged: &Staged, row_index: usize) -> Vec<Uuid> {
        let id = staged.rows[row_index].id;
        staged.matches.iter().filter(|m| m.queue_row_id == id).filter_map(|m| m.matched_entry_id).collect()
    }

    fn row_matches(staged: &Staged, row_index: usize) -> Vec<Uuid> {
        let id = staged.rows[row_index].id;
        staged.matches.iter().filter(|m| m.queue_row_id == id).filter_map(|m| m.matched_queue_row_id).collect()
    }

    #[test]
    fn queue_rows_carry_the_imports_currency_and_the_rows_own_facts() {
        let imp = import();
        let staged = build_queue(&imp, &parsed(vec![row(day(2), at(9), dec!(-3), BankState::Pending)], vec![]), &[]);
        let r = &staged.rows[0];
        assert_eq!((r.import_id, r.kind, r.currency.clone()), (imp.id, QueueRowKind::Normal, imp.currency.clone()));
        assert_eq!((r.date, r.time, r.amount, r.bank_state), (day(2), at(9), dec!(-3), BankState::Pending));
        assert_eq!(r.description, "r");
        assert_eq!(r.category, None);
    }

    #[test]
    fn same_date_and_amount_with_different_times_do_not_match() {
        let imp = import();
        let e = entry(&imp, day(2), at(9), dec!(-3));
        let staged = build_queue(&imp, &parsed(vec![row(day(2), at(10), dec!(-3), BankState::Completed)], vec![]), &[e]);
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn same_date_and_amount_with_no_time_on_either_match() {
        let imp = import();
        let e = entry(&imp, day(2), None, dec!(-3));
        let staged = build_queue(&imp, &parsed(vec![row(day(2), None, dec!(-3), BankState::Completed)], vec![]), std::slice::from_ref(&e));
        assert_eq!(entry_matches(&staged, 0), vec![e.id]);
    }

    #[test]
    fn a_time_on_only_one_side_still_matches() {
        let imp = import();
        let e = entry(&imp, day(2), None, dec!(-3));
        let staged = build_queue(&imp, &parsed(vec![row(day(2), at(9), dec!(-3), BankState::Completed)], vec![]), std::slice::from_ref(&e));
        assert_eq!(entry_matches(&staged, 0), vec![e.id]);
    }

    #[test]
    fn a_different_date_amount_or_sign_does_not_match() {
        let imp = import();
        let entries = vec![entry(&imp, day(3), None, dec!(-3)), entry(&imp, day(2), None, dec!(-4)), entry(&imp, day(2), None, dec!(3))];
        let staged = build_queue(&imp, &parsed(vec![row(day(2), None, dec!(-3), BankState::Completed)], vec![]), &entries);
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn a_zero_amount_row_is_never_matched() {
        let imp = import();
        let e = entry(&imp, day(2), None, dec!(0));
        let rows = vec![row(day(2), None, dec!(0), BankState::Pending), row(day(2), None, dec!(0), BankState::Pending)];
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
            &parsed(vec![row(day(2), None, dec!(-3), BankState::Completed)], vec![RevertedCandidate { date: day(2), time: at(9).unwrap(), amount: dec!(-3) }]),
            &[e],
        );
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn an_entry_outside_the_statements_date_range_is_not_matched() {
        let imp = import();
        let mut p = parsed(vec![row(day(2), None, dec!(-3), BankState::Completed)], vec![]);
        p.date_range = DateRange { start: day(5), end: day(20) };
        let staged = build_queue(&imp, &p, &[entry(&imp, day(2), None, dec!(-3))]);
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn a_row_matching_several_entries_records_every_one() {
        let imp = import();
        let (a, b) = (entry(&imp, day(2), None, dec!(-3)), entry(&imp, day(2), at(7), dec!(-3)));
        let staged = build_queue(&imp, &parsed(vec![row(day(2), None, dec!(-3), BankState::Completed)], vec![]), &[a.clone(), b.clone()]);
        let mut got = entry_matches(&staged, 0);
        got.sort();
        let mut want = vec![a.id, b.id];
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn two_pending_rows_with_the_same_date_and_amount_match_each_other_both_ways() {
        let imp = import();
        let rows = vec![row(day(2), at(9), dec!(-3), BankState::Pending), row(day(2), at(9), dec!(-3), BankState::Pending)];
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
        let rows = vec![row(day(2), None, dec!(-3), BankState::Pending), row(day(2), None, dec!(-3), BankState::Pending)];
        let staged = build_queue(&imp, &parsed(rows, vec![]), std::slice::from_ref(&e));
        assert_eq!(entry_matches(&staged, 0), vec![e.id]);
        assert_eq!(row_matches(&staged, 0), vec![staged.rows[1].id]);
    }

    #[test]
    fn a_reverted_candidate_matches_an_entry_with_the_same_date_time_and_amount() {
        let imp = import();
        let e = entry(&imp, day(2), at(9), dec!(-3));
        let c = RevertedCandidate { date: day(2), time: at(9).unwrap(), amount: dec!(-3) };
        let staged = build_queue(&imp, &parsed(vec![], vec![c]), std::slice::from_ref(&e));
        let r = &staged.rows[0];
        assert_eq!((r.kind, r.description.as_str(), r.time), (QueueRowKind::RevertedCandidate, "", at(9)));
        assert_eq!(entry_matches(&staged, 0), vec![e.id]);
    }

    #[test]
    fn a_reverted_candidate_does_not_match_an_entry_without_a_time_but_is_still_queued() {
        let imp = import();
        let e = entry(&imp, day(2), None, dec!(-3));
        let c = RevertedCandidate { date: day(2), time: at(9).unwrap(), amount: dec!(-3) };
        let staged = build_queue(&imp, &parsed(vec![], vec![c]), &[e]);
        assert_eq!(staged.rows.len(), 1);
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn an_unmatched_reverted_candidate_is_still_queued() {
        let imp = import();
        let c = RevertedCandidate { date: day(2), time: at(9).unwrap(), amount: dec!(-3) };
        let staged = build_queue(&imp, &parsed(vec![], vec![c]), &[]);
        assert_eq!(staged.rows.len(), 1);
        assert!(staged.matches.is_empty());
    }

    #[test]
    fn a_candidate_matching_several_entries_suggests_the_lowest_entry_id() {
        let imp = import();
        let entries: Vec<Entry> = (0..3).map(|_| entry(&imp, day(2), at(9), dec!(-3))).collect();
        let lowest = entries.iter().map(|e| e.id).min().unwrap();
        let c = RevertedCandidate { date: day(2), time: at(9).unwrap(), amount: dec!(-3) };
        let staged = build_queue(&imp, &parsed(vec![], vec![c]), &entries);
        assert_eq!(entry_matches(&staged, 0), vec![lowest]);
    }
}
