#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! Pure bank statement parsing: bytes in, rows and balance figures out. No
//! HTTP, no database, no knowledge of existing entries. Each bank has its
//! own `BankReader`; `Importer` runs the logic they share.

pub mod bank_a;
pub mod bank_b;

pub use bank_a::BankA;
pub use bank_b::BankB;
pub use ledger_core::BankState;

use chrono::{NaiveDate, NaiveTime};
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

/// One statement line headed for the review queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRow {
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    pub amount: Decimal,
    pub description: String,
    pub bank_state: BankState,
}

/// A line the bank reversed: matched later against existing entries by
/// date, time and amount, never queued as a normal row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevertedCandidate {
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub amount: Decimal,
}

/// What `BankReader::read` returns: the raw parsed data from the file.
/// `Importer::parse` wraps it, runs the balance check on top, and returns
/// the same type once it passes — there is no partial or pre-check variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseResult {
    pub date_range: DateRange,
    pub rows: Vec<ParsedRow>,
    pub reverted_candidates: Vec<RevertedCandidate>,
    pub opening_balance: Decimal,
    pub closing_balance: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    #[error("the file could not be read as a statement: {0}")]
    Parse(String),
    #[error("pdftotext could not extract the PDF's text: {0}")]
    PdfToText(String),
    #[error("this PDF is not an account statement")]
    NotAStatement,
    #[error("the balance check failed: the statement closes at {expected}, but its rows add up to {actual}")]
    BalanceCheckFailed { expected: Decimal, actual: Decimal },
}

pub trait BankReader {
    fn read(&self, bytes: &[u8]) -> Result<ParseResult, ImportError>;
}

pub struct Importer<T: BankReader> {
    reader: T,
}

impl<T: BankReader> Importer<T> {
    pub fn new(reader: T) -> Self {
        Importer { reader }
    }

    /// Reads the file, then checks the opening balance plus every
    /// Completed row equals the closing balance. Pending rows and reverted
    /// candidates are not part of the bank's own arithmetic, so they are
    /// left out of the sum.
    pub fn parse(&self, bytes: &[u8]) -> Result<ParseResult, ImportError> {
        let parsed = self.reader.read(bytes)?;
        let completed: Decimal = parsed
            .rows
            .iter()
            .filter(|r| r.bank_state == BankState::Completed)
            .map(|r| r.amount)
            .sum();
        let actual = parsed.opening_balance + completed;
        if actual != parsed.closing_balance {
            return Err(ImportError::BalanceCheckFailed { expected: parsed.closing_balance, actual });
        }
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    struct Stub(Result<ParseResult, ImportError>);

    impl BankReader for Stub {
        fn read(&self, _bytes: &[u8]) -> Result<ParseResult, ImportError> {
            self.0.clone()
        }
    }

    fn row(amount: Decimal, bank_state: BankState) -> ParsedRow {
        ParsedRow {
            date: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            time: None,
            amount,
            description: "x".to_string(),
            bank_state,
        }
    }

    fn statement(rows: Vec<ParsedRow>, opening: Decimal, closing: Decimal) -> ParseResult {
        let d = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        ParseResult {
            date_range: DateRange { start: d, end: d },
            rows,
            reverted_candidates: vec![RevertedCandidate {
                date: d,
                time: NaiveTime::from_hms_opt(1, 0, 0).unwrap(),
                amount: dec!(-1000),
            }],
            opening_balance: opening,
            closing_balance: closing,
        }
    }

    #[test]
    fn a_statement_whose_completed_rows_reach_the_closing_balance_parses() {
        let rows = vec![row(dec!(-10), BankState::Completed), row(dec!(25.5), BankState::Completed)];
        let result = Importer::new(Stub(Ok(statement(rows.clone(), dec!(100), dec!(115.5))))).parse(b"").unwrap();
        assert_eq!(result.rows, rows);
        assert_eq!(result.opening_balance, dec!(100));
        assert_eq!(result.closing_balance, dec!(115.5));
        assert_eq!(result.reverted_candidates.len(), 1);
    }

    #[test]
    fn pending_rows_and_reverted_candidates_are_left_out_of_the_balance_check() {
        let rows = vec![row(dec!(-10), BankState::Completed), row(dec!(-99), BankState::Pending)];
        let result = Importer::new(Stub(Ok(statement(rows, dec!(100), dec!(90))))).parse(b"");
        assert!(result.is_ok());
    }

    #[test]
    fn a_statement_that_does_not_add_up_fails_with_both_figures() {
        let rows = vec![row(dec!(-10), BankState::Completed)];
        let result = Importer::new(Stub(Ok(statement(rows, dec!(100), dec!(80))))).parse(b"");
        assert_eq!(
            result,
            Err(ImportError::BalanceCheckFailed { expected: dec!(80), actual: dec!(90) })
        );
    }

    #[test]
    fn a_reader_error_comes_straight_back() {
        let result = Importer::new(Stub(Err(ImportError::NotAStatement))).parse(b"");
        assert_eq!(result, Err(ImportError::NotAStatement));
    }
}
