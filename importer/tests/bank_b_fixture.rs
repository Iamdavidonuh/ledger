#![allow(clippy::unwrap_used)]

//! These run the real `pdftotext` binary (poppler) against small synthetic
//! PDFs laid out like the bank's statement: made-up names and amounts, each
//! date split across two text lines, and a page break between rows.

use chrono::NaiveDate;
use importer::{BankB, ImportError, Importer};
use rust_decimal_macros::dec;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn a_statement_pdf_parses_and_passes_its_balance_check() {
    let result = Importer::new(BankB).parse(&fixture("bank_b_statement.pdf")).unwrap();
    let rows: Vec<(NaiveDate, rust_decimal::Decimal, &str)> =
        result.rows.iter().map(|r| (r.date, r.amount, r.description.as_str())).collect();
    let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
    assert_eq!(
        rows,
        vec![
            (d(3, 3), dec!(-1000.00), "SEPA Überweisung an Sam Landlord"),
            (d(3, 5), dec!(-12.34), "Kartenzahlung"),
            (d(3, 25), dec!(2500.00), "SEPA Überweisung von Example Employer GmbH"),
            (d(3, 31), dec!(-0.04), "Balance of settlement items"),
        ]
    );
    assert_eq!(result.opening_balance, dec!(1250.00));
    assert_eq!(result.closing_balance, dec!(2737.62));
    assert_eq!((result.date_range.start, result.date_range.end), (d(3, 1), d(3, 31)));
}

#[test]
fn an_account_settlement_pdf_is_rejected_as_not_a_statement() {
    assert_eq!(Importer::new(BankB).parse(&fixture("bank_b_settlement.pdf")), Err(ImportError::NotAStatement));
}

#[test]
fn bytes_that_are_not_a_pdf_are_a_pdftotext_error() {
    assert!(matches!(Importer::new(BankB).parse(b"not a pdf at all"), Err(ImportError::PdfToText(_))));
}
