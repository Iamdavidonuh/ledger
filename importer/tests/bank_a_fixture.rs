#![allow(clippy::unwrap_used)]

use chrono::NaiveDate;
use importer::{BankA, BankState, ImportError, Importer};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn the_sample_statement_parses_into_the_expected_rows_and_balances() {
    let result = Importer::new(BankA)
        .parse(&fixture("bank_a_statement.csv"))
        .unwrap();
    let rows: Vec<(String, Decimal, &str, BankState)> = result
        .rows
        .iter()
        .map(|r| {
            (
                format!("{} {}", r.date, r.time.unwrap()),
                r.amount,
                r.description.as_str(),
                r.bank_state,
            )
        })
        .collect();
    use BankState::{Completed, Pending};
    assert_eq!(
        rows,
        vec![
            (
                "2026-02-28 23:40:00".to_string(),
                dec!(-10.00),
                "Night Kiosk",
                Completed
            ),
            (
                "2026-02-28 23:40:00".to_string(),
                dec!(-0.50),
                "Fee on: Night Kiosk",
                Completed
            ),
            (
                "2026-03-02 08:00:00".to_string(),
                dec!(0.00),
                "Card Delivery Fee",
                Completed
            ),
            (
                "2026-03-02 08:00:00".to_string(),
                dec!(-5.99),
                "Fee on: Card Delivery Fee",
                Completed
            ),
            (
                "2026-03-03 02:00:00".to_string(),
                dec!(-18.36),
                "Radio licence, TV, extras",
                Completed
            ),
            (
                "2026-03-04 12:30:00".to_string(),
                dec!(-3.80),
                "Corner Bakery",
                Completed
            ),
            (
                "2026-03-06 09:00:00".to_string(),
                dec!(200.00),
                "Payment from Jane Example",
                Completed
            ),
            (
                "2026-03-07 19:00:00".to_string(),
                dec!(-6.50),
                "Pending Cafe",
                Pending
            ),
        ]
    );
    assert_eq!(result.reverted_candidates.len(), 1);
    assert_eq!(result.reverted_candidates[0].amount, dec!(-4.20));
    assert_eq!(result.opening_balance, dec!(150.00));
    assert_eq!(result.closing_balance, dec!(311.35));
    assert_eq!(
        result.date_range.start,
        NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()
    );
    assert_eq!(
        result.date_range.end,
        NaiveDate::from_ymd_opt(2026, 3, 7).unwrap()
    );
}

#[test]
fn a_statement_whose_balance_does_not_add_up_is_rejected_with_both_figures() {
    assert_eq!(
        Importer::new(BankA).parse(&fixture("bank_a_bad_balance.csv")),
        Err(ImportError::BalanceCheckFailed {
            expected: dec!(90.00),
            actual: dec!(88.00)
        })
    );
}
