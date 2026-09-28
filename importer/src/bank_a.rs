//! BankA: a CSV export with `Type,Product,Started Date,Completed Date,
//! Description,Amount,Fee,Currency,State,Balance` columns. Every rule about
//! this format lives here.

use crate::{BankReader, DateRange, ImportError, ParseResult, ParsedRow, RevertedCandidate};
use chrono::{NaiveDate, NaiveDateTime};
use ledger_core::BankState;
use rust_decimal::Decimal;
use std::str::FromStr;

pub struct BankA;

const DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

struct CsvRow {
    started: NaiveDateTime,
    completed: Option<NaiveDateTime>,
    description: String,
    amount: Decimal,
    fee: Decimal,
    state: BankAState,
    balance: Option<Decimal>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BankAState {
    Completed,
    Pending,
    Reverted,
}

fn parse_error(line: usize, message: impl std::fmt::Display) -> ImportError {
    ImportError::Parse(format!("line {line}: {message}"))
}

fn column(headers: &csv::StringRecord, name: &str) -> Result<usize, ImportError> {
    headers
        .iter()
        .position(|h| h.trim() == name)
        .ok_or_else(|| ImportError::Parse(format!("missing the {name:?} column")))
}

fn decimal(line: usize, name: &str, value: &str) -> Result<Decimal, ImportError> {
    Decimal::from_str(value.trim())
        .map_err(|_| parse_error(line, format!("{name} {value:?} is not a number")))
}

fn datetime(line: usize, name: &str, value: &str) -> Result<NaiveDateTime, ImportError> {
    NaiveDateTime::parse_from_str(value.trim(), DATETIME_FORMAT)
        .map_err(|_| parse_error(line, format!("{name} {value:?} is not a date and time")))
}

fn read_rows(bytes: &[u8]) -> Result<Vec<CsvRow>, ImportError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|e| ImportError::Parse(e.to_string()))?
        .clone();
    let started_col = column(&headers, "Started Date")?;
    let completed_col = column(&headers, "Completed Date")?;
    let description_col = column(&headers, "Description")?;
    let amount_col = column(&headers, "Amount")?;
    let fee_col = column(&headers, "Fee")?;
    let state_col = column(&headers, "State")?;
    let balance_col = column(&headers, "Balance")?;

    let mut rows = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let line = index + 2;
        let record = record.map_err(|e| parse_error(line, e))?;
        let field = |col: usize| record.get(col).unwrap_or("").trim();
        let state = match field(state_col) {
            "COMPLETED" => BankAState::Completed,
            "PENDING" => BankAState::Pending,
            "REVERTED" => BankAState::Reverted,
            other => return Err(parse_error(line, format!("unknown State {other:?}"))),
        };
        let completed = match field(completed_col) {
            "" => None,
            v => Some(datetime(line, "Completed Date", v)?),
        };
        let balance = match field(balance_col) {
            "" => None,
            v => Some(decimal(line, "Balance", v)?),
        };
        rows.push(CsvRow {
            started: datetime(line, "Started Date", field(started_col))?,
            completed,
            description: field(description_col).to_string(),
            amount: decimal(line, "Amount", field(amount_col))?,
            fee: decimal(line, "Fee", field(fee_col))?,
            state,
            balance,
        });
    }
    Ok(rows)
}

/// Opening and closing balance from the running Balance column of the
/// Completed rows, ordered by Completed Date (the balance only moves when a
/// row settles). The opening balance backs the earliest row's own Amount and
/// Fee out of its balance, since that balance already has both applied.
fn balances(rows: &[CsvRow]) -> Result<(Decimal, Decimal), ImportError> {
    let mut completed: Vec<(NaiveDateTime, &CsvRow)> = Vec::new();
    for row in rows.iter().filter(|r| r.state == BankAState::Completed) {
        let at = row.completed.ok_or_else(|| {
            ImportError::Parse("a COMPLETED row has no Completed Date".to_string())
        })?;
        completed.push((at, row));
    }
    completed.sort_by_key(|(at, _)| *at);
    let (Some((_, first)), Some((_, last))) = (completed.first(), completed.last()) else {
        return Err(ImportError::Parse(
            "the file has no COMPLETED rows to check a balance against".to_string(),
        ));
    };
    let missing = || ImportError::Parse("a COMPLETED row has no Balance".to_string());
    let opening = first.balance.ok_or_else(missing)? - first.amount + first.fee;
    let closing = last.balance.ok_or_else(missing)?;
    Ok((opening, closing))
}

impl BankReader for BankA {
    fn read(&self, bytes: &[u8]) -> Result<ParseResult, ImportError> {
        let rows = read_rows(bytes)?;
        let (opening_balance, closing_balance) = balances(&rows)?;
        let start = rows.iter().map(|r| r.started.date()).min();
        let end = rows.iter().map(|r| r.started.date()).max();
        let (Some(start), Some(end)) = (start, end) else {
            return Err(ImportError::Parse("the file has no rows".to_string()));
        };

        let mut parsed = Vec::new();
        let mut reverted_candidates = Vec::new();
        for row in &rows {
            if row.amount.is_zero() && row.fee.is_zero() {
                continue;
            }
            let date: NaiveDate = row.started.date();
            let time = row.started.time();
            // A fee is a positive cost; it becomes its own row at -Fee.
            let mut parts = vec![(row.amount, row.description.clone())];
            if !row.fee.is_zero() {
                parts.push((-row.fee, format!("Fee on: {}", row.description)));
            }
            for (amount, description) in parts {
                let bank_state = match row.state {
                    BankAState::Reverted => {
                        reverted_candidates.push(RevertedCandidate { date, time, amount });
                        continue;
                    }
                    BankAState::Completed => BankState::Completed,
                    BankAState::Pending => BankState::Pending,
                };
                parsed.push(ParsedRow {
                    date,
                    time: Some(time),
                    amount,
                    description,
                    bank_state,
                });
            }
        }
        Ok(ParseResult {
            date_range: DateRange { start, end },
            rows: parsed,
            reverted_candidates,
            opening_balance,
            closing_balance,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BankReader, ImportError, Importer, ParsedRow, RevertedCandidate};
    use chrono::{NaiveDate, NaiveTime};
    use ledger_core::BankState;
    use rust_decimal_macros::dec;

    const HEADER: &str =
        "Type,Product,Started Date,Completed Date,Description,Amount,Fee,Currency,State,Balance\n";

    fn csv(rows: &[&str]) -> Vec<u8> {
        let mut s = HEADER.to_string();
        for r in rows {
            s.push_str(r);
            s.push('\n');
        }
        s.into_bytes()
    }

    fn date(m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, d).unwrap()
    }

    fn time(h: u32, mi: u32, s: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, mi, s).unwrap()
    }

    #[test]
    fn a_completed_row_becomes_a_normal_row_dated_by_its_started_date() {
        let bytes = csv(&[
            "Card Payment,Current,2026-03-01 23:40:00,2026-03-02 02:10:00,Night Kiosk,-10.00,0.00,EUR,COMPLETED,90.00",
        ]);
        let statement = BankA.read(&bytes).unwrap();
        assert_eq!(
            statement.rows,
            vec![ParsedRow {
                date: date(3, 1),
                time: Some(time(23, 40, 0)),
                amount: dec!(-10.00),
                description: "Night Kiosk".to_string(),
                bank_state: BankState::Completed,
            }]
        );
        assert!(statement.reverted_candidates.is_empty());
    }

    #[test]
    fn a_nonzero_fee_is_split_into_its_own_row() {
        let bytes = csv(&[
            "Card Payment,Current,2026-03-01 12:00:00,2026-03-01 12:00:01,Book Shop,-28.00,0.30,EUR,COMPLETED,71.70",
        ]);
        let rows = BankA.read(&bytes).unwrap().rows;
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0].amount, rows[0].description.as_str()),
            (dec!(-28.00), "Book Shop")
        );
        assert_eq!(
            (rows[1].amount, rows[1].description.as_str()),
            (dec!(-0.30), "Fee on: Book Shop")
        );
        assert_eq!(
            (rows[1].date, rows[1].time, rows[1].bank_state),
            (rows[0].date, rows[0].time, rows[0].bank_state)
        );
    }

    #[test]
    fn a_zero_amount_charge_with_a_fee_keeps_both_rows() {
        let bytes = csv(&[
            "Charge,Current,2026-03-02 08:00:00,2026-03-02 08:00:01,Card Delivery Fee,0.00,5.99,EUR,COMPLETED,94.01",
        ]);
        let rows = BankA.read(&bytes).unwrap().rows;
        let got: Vec<_> = rows
            .iter()
            .map(|r| (r.amount, r.description.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (dec!(0.00), "Card Delivery Fee"),
                (dec!(-5.99), "Fee on: Card Delivery Fee")
            ]
        );
    }

    #[test]
    fn a_row_zero_on_both_amount_and_fee_is_dropped() {
        let bytes = csv(&[
            "Card Payment,Current,2026-03-01 12:00:00,2026-03-01 12:00:01,Real,-1.00,0.00,EUR,COMPLETED,99.00",
            "Card Refund,Current,2026-03-02 12:00:00,2026-03-02 12:00:01,Nothing,0.00,0.00,EUR,COMPLETED,99.00",
        ]);
        let rows = BankA.read(&bytes).unwrap().rows;
        assert_eq!(
            rows.iter()
                .map(|r| r.description.as_str())
                .collect::<Vec<_>>(),
            vec!["Real"]
        );
    }

    #[test]
    fn a_pending_row_is_queued_as_pending() {
        let bytes = csv(&[
            "Card Payment,Current,2026-03-01 12:00:00,2026-03-01 12:00:01,Real,-1.00,0.00,EUR,COMPLETED,99.00",
            "Card Payment,Current,2026-03-03 19:00:00,,Pending Cafe,-6.50,0.00,EUR,PENDING,",
        ]);
        let rows = BankA.read(&bytes).unwrap().rows;
        assert_eq!(rows[1].bank_state, BankState::Pending);
        assert_eq!(
            (rows[1].date, rows[1].time),
            (date(3, 3), Some(time(19, 0, 0)))
        );
    }

    #[test]
    fn a_reverted_row_is_a_candidate_not_a_row_and_its_fee_is_a_second_candidate() {
        let bytes = csv(&[
            "Card Payment,Current,2026-03-01 12:00:00,2026-03-01 12:00:01,Real,-1.00,0.00,EUR,COMPLETED,99.00",
            "Card Payment,Current,2026-03-04 12:00:00,,Corner Bakery,-4.20,0.10,EUR,REVERTED,",
        ]);
        let statement = BankA.read(&bytes).unwrap();
        assert_eq!(statement.rows.len(), 1);
        assert_eq!(
            statement.reverted_candidates,
            vec![
                RevertedCandidate {
                    date: date(3, 4),
                    time: time(12, 0, 0),
                    amount: dec!(-4.20)
                },
                RevertedCandidate {
                    date: date(3, 4),
                    time: time(12, 0, 0),
                    amount: dec!(-0.10)
                },
            ]
        );
    }

    #[test]
    fn a_quoted_description_with_commas_does_not_shift_the_other_columns() {
        let bytes = csv(&[
            "Transfer,Current,2026-03-03 02:00:00,2026-03-03 13:00:00,\"Radio licence, TV, extras\",-18.36,0.00,EUR,COMPLETED,81.64",
        ]);
        let rows = BankA.read(&bytes).unwrap().rows;
        assert_eq!(rows[0].description, "Radio licence, TV, extras");
        assert_eq!(rows[0].amount, dec!(-18.36));
    }

    #[test]
    fn a_byte_order_mark_before_the_header_is_ignored() {
        let mut bytes = "\u{feff}".as_bytes().to_vec();
        bytes.extend(csv(&[
            "Card Payment,Current,2026-03-01 12:00:00,2026-03-01 12:00:01,Real,-1.00,0.00,EUR,COMPLETED,99.00",
        ]));
        assert_eq!(BankA.read(&bytes).unwrap().rows.len(), 1);
    }

    #[test]
    fn an_unknown_state_is_a_parse_error() {
        let bytes = csv(&[
            "Card Payment,Current,2026-03-01 12:00:00,2026-03-01 12:00:01,Real,-1.00,0.00,EUR,COMPLETED,99.00",
            "Card Payment,Current,2026-03-02 12:00:00,,Odd,-1.00,0.00,EUR,DECLINED,",
        ]);
        assert!(matches!(BankA.read(&bytes), Err(ImportError::Parse(_))));
    }

    #[test]
    fn a_file_with_no_completed_rows_is_a_parse_error() {
        let bytes =
            csv(&["Card Payment,Current,2026-03-02 12:00:00,,Pending,-1.00,0.00,EUR,PENDING,"]);
        assert!(matches!(BankA.read(&bytes), Err(ImportError::Parse(_))));
    }

    #[test]
    fn a_file_that_is_not_a_statement_csv_is_a_parse_error() {
        assert!(matches!(
            BankA.read(b"%PDF-1.4 not a csv"),
            Err(ImportError::Parse(_))
        ));
        assert!(matches!(
            BankA.read(b"a,b\n1,2\n"),
            Err(ImportError::Parse(_))
        ));
        let bad_amount = csv(&["Card Payment,Current,2026-03-01 12:00:00,2026-03-01 12:00:01,X,abc,0.00,EUR,COMPLETED,1.00"]);
        assert!(matches!(
            BankA.read(&bad_amount),
            Err(ImportError::Parse(_))
        ));
    }

    #[test]
    fn balances_come_from_completed_rows_ordered_by_completed_date_not_file_order() {
        // File order is deliberately not Completed Date order here, and the
        // earliest Started Date (a pending row) is neither first nor last.
        let bytes = csv(&[
            "Card Payment,Current,2026-03-05 09:00:00,2026-03-05 09:00:01,Middle,-5.00,0.00,EUR,COMPLETED,185.00",
            "Deposit,Current,2026-03-09 09:00:00,2026-03-09 09:00:01,Last,15.00,0.00,EUR,COMPLETED,200.00",
            "Card Payment,Current,2026-02-27 20:00:00,,Earliest start,-2.00,0.00,EUR,PENDING,",
            "Card Payment,Current,2026-02-28 23:40:00,2026-03-01 02:10:00,First,-10.00,0.00,EUR,COMPLETED,190.00",
            "Card Payment,Current,2026-03-10 18:00:00,,Latest start,-3.00,0.00,EUR,REVERTED,",
        ]);
        let statement = BankA.read(&bytes).unwrap();
        assert_eq!(statement.opening_balance, dec!(200.00));
        assert_eq!(statement.closing_balance, dec!(200.00));
        assert_eq!(statement.date_range.start, date(2, 27));
        assert_eq!(statement.date_range.end, date(3, 10));
        assert!(Importer::new(BankA).parse(&bytes).is_ok());
    }

    #[test]
    fn the_opening_balance_backs_out_the_first_rows_amount_and_fee() {
        // The balance column already has both the amount and the fee
        // deducted: 150.00 - 10.00 - 0.50 = 139.50.
        let bytes = csv(&[
            "Card Payment,Current,2026-03-01 09:00:00,2026-03-01 09:00:01,Kiosk,-10.00,0.50,EUR,COMPLETED,139.50",
            "Deposit,Current,2026-03-02 09:00:00,2026-03-02 09:00:01,Pay,60.50,0.00,EUR,COMPLETED,200.00",
        ]);
        let statement = BankA.read(&bytes).unwrap();
        assert_eq!(statement.opening_balance, dec!(150.00));
        assert_eq!(statement.closing_balance, dec!(200.00));
    }

    #[test]
    fn the_balance_check_counts_each_completed_rows_amount_minus_its_fee() {
        let good = csv(&[
            "Card Payment,Current,2026-03-01 09:00:00,2026-03-01 09:00:01,Kiosk,-10.00,0.50,EUR,COMPLETED,139.50",
            "Charge,Current,2026-03-02 09:00:00,2026-03-02 09:00:01,Card Delivery Fee,0.00,5.99,EUR,COMPLETED,133.51",
            "Card Payment,Current,2026-03-03 09:00:00,,Pending,-99.00,0.00,EUR,PENDING,",
        ]);
        assert!(Importer::new(BankA).parse(&good).is_ok());
        // A closing balance that ignores the second row's fee does not add up.
        let bad = csv(&[
            "Card Payment,Current,2026-03-01 09:00:00,2026-03-01 09:00:01,Kiosk,-10.00,0.50,EUR,COMPLETED,139.50",
            "Charge,Current,2026-03-02 09:00:00,2026-03-02 09:00:01,Card Delivery Fee,0.00,5.99,EUR,COMPLETED,139.50",
        ]);
        assert_eq!(
            Importer::new(BankA).parse(&bad),
            Err(ImportError::BalanceCheckFailed {
                expected: dec!(139.50),
                actual: dec!(133.51)
            })
        );
    }
}
