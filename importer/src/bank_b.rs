//! BankB: a PDF statement, read by shelling out to `pdftotext -raw`. Every
//! rule about this format lives here.
//!
//! In the extracted text each row starts with a signed amount ("- 1,000.00
//! SEPA ..."), followed by its item text, then its value date and booking
//! date, each split across two lines ("03-03-" then "2026"), then free-form
//! reference lines. A page break inserts footer text and a repeated table
//! header between rows, never inside the item text before the dates.

use crate::{BankReader, DateRange, ImportError, ParseResult, ParsedRow};
use chrono::NaiveDate;
use ledger_core::BankState;
use rust_decimal::Decimal;
use std::io::Write;
use std::process::{Command, Stdio};
use std::str::FromStr;

pub struct BankB;

impl BankReader for BankB {
    fn read(&self, bytes: &[u8]) -> Result<ParseResult, ImportError> {
        parse_text(&pdftotext(bytes)?)
    }
}

/// Runs `pdftotext -raw -enc UTF-8 - -`, feeding the PDF on stdin.
fn pdftotext(bytes: &[u8]) -> Result<String, ImportError> {
    let mut child = Command::new("pdftotext")
        .args(["-raw", "-enc", "UTF-8", "-", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ImportError::PdfToText(format!("could not start pdftotext: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| ImportError::PdfToText("could not open pdftotext's input".to_string()))?;
    // Written from another thread so a large PDF can't deadlock against
    // pdftotext filling its output pipe before it has read all its input.
    let input = bytes.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child
        .wait_with_output()
        .map_err(|e| ImportError::PdfToText(format!("pdftotext did not finish: {e}")))?;
    // A write error just means pdftotext stopped reading early; its exit
    // status below says whether that was a failure.
    let _ = writer.join();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ImportError::PdfToText(format!(
            "pdftotext exited with {}: {}",
            output.status,
            stderr.trim()
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if text.trim().is_empty() {
        return Err(ImportError::PdfToText(
            "pdftotext produced no text".to_string(),
        ));
    }
    Ok(text)
}

const HEADER_PREFIX: &str = "Account statement from ";
const OPENING_PREFIX: &str = "Previous balance as at ";
const CLOSING_LINE: &str = "New balance";
/// A column label printed before the reference text, not part of it.
const REFERENCE_LABEL: &str = "Payment Reference/E2E-Ref.";

fn parse_error(message: impl Into<String>) -> ImportError {
    ImportError::Parse(message.into())
}

/// "+ 1,250.00 ..." or "- 0.04 ..." into the signed amount and the rest of
/// the line. Amounts use "," for thousands and "." for decimals.
fn signed_amount(line: &str) -> Option<(Decimal, &str)> {
    let (negative, rest) = if let Some(rest) = line.strip_prefix("- ") {
        (true, rest)
    } else {
        (false, line.strip_prefix("+ ")?)
    };
    let (number, tail) = rest.split_once(' ').unwrap_or((rest, ""));
    let digits_ok = number
        .chars()
        .all(|c| c.is_ascii_digit() || c == ',' || c == '.');
    let (_, decimals) = number.rsplit_once('.')?;
    if !digits_ok || decimals.len() != 2 {
        return None;
    }
    let value = Decimal::from_str(&number.replace(',', "")).ok()?;
    Some((if negative { -value } else { value }, tail))
}

/// The "DD-MM-" first half of a split date.
fn is_day_month(line: &str) -> bool {
    // Format: "DD-MM-" (6 bytes, digits at 0,1,3,4, dashes at 2,5)
    matches!(line.as_bytes(), [d1, d2, b'-', m1, m2, b'-']
        if d1.is_ascii_digit() && d2.is_ascii_digit() && m1.is_ascii_digit() && m2.is_ascii_digit())
}

fn split_date(day_month: &str, year: &str) -> Result<NaiveDate, ImportError> {
    NaiveDate::parse_from_str(&format!("{day_month}{year}"), "%d-%m-%Y")
        .map_err(|_| parse_error(format!("{day_month:?} {year:?} is not a date")))
}

fn dotted_date(s: &str) -> Result<NaiveDate, ImportError> {
    NaiveDate::parse_from_str(s, "%d.%m.%Y")
        .map_err(|_| parse_error(format!("{s:?} is not a date")))
}

/// The balance on the line after `label`: "+ 1,250.00 EUR ...".
fn balance_after(lines: &[&str], label_at: usize, label: &str) -> Result<Decimal, ImportError> {
    lines
        .get(label_at + 1)
        .and_then(|l| signed_amount(l))
        .map(|(amount, _)| amount)
        .ok_or_else(|| parse_error(format!("no amount after {label:?}")))
}

pub(crate) fn parse_text(text: &str) -> Result<ParseResult, ImportError> {
    let lines: Vec<&str> = text
        .split(['\n', '\x0c'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    // "Account statement from 01.03.2026 to 31.03.2026". An account
    // settlement, or anything else, has no such line.
    let (start, end) = lines
        .iter()
        .find_map(|l| l.strip_prefix(HEADER_PREFIX)?.split_once(" to "))
        .ok_or(ImportError::NotAStatement)?;
    let date_range = DateRange {
        start: dotted_date(start.trim())?,
        end: dotted_date(end.trim())?,
    };

    let opening_at = lines
        .iter()
        .position(|l| l.starts_with(OPENING_PREFIX))
        .ok_or_else(|| parse_error("no previous balance line"))?;
    let opening_balance = balance_after(&lines, opening_at, OPENING_PREFIX)?;
    let closing_at = lines
        .iter()
        .position(|l| *l == CLOSING_LINE)
        .filter(|&at| at > opening_at)
        .ok_or_else(|| parse_error("no new balance line"))?;
    let closing_balance = balance_after(&lines, closing_at, CLOSING_LINE)?;

    let mut rows = Vec::new();
    let mut i = opening_at + 2;
    while i < closing_at {
        let Some((amount, first_text)) = signed_amount(lines[i]) else {
            // Reference lines, page footers and repeated table headers.
            i += 1;
            continue;
        };
        let mut description: Vec<&str> = vec![first_text];
        i += 1;
        while i < closing_at && !is_day_month(lines[i]) {
            if signed_amount(lines[i]).is_some() {
                return Err(parse_error(format!("row {:?} has no dates", lines[i - 1])));
            }
            description.push(lines[i]);
            i += 1;
        }
        // Value date then booking date, each as "DD-MM-" and "YYYY".
        if i + 3 >= closing_at || !is_day_month(lines[i + 2]) {
            return Err(parse_error(format!(
                "a row of {amount} has no value and booking dates"
            )));
        }
        let booking_date = split_date(lines[i + 2], lines[i + 3])?;
        i += 4;
        let description = description
            .into_iter()
            .filter(|part| !part.is_empty() && *part != REFERENCE_LABEL)
            .collect::<Vec<_>>()
            .join(" ");
        rows.push(ParsedRow {
            date: booking_date,
            time: None,
            amount,
            description,
            bank_state: BankState::Completed,
        });
    }

    Ok(ParseResult {
        date_range,
        rows,
        reverted_candidates: Vec::new(),
        opening_balance,
        closing_balance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DateRange, ImportError, ParsedRow};
    use chrono::NaiveDate;
    use ledger_core::BankState;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    /// Shaped like `pdftotext -raw` output of a two-page statement: each
    /// date is split across two lines ("03-03-" then "2026"), and a page
    /// break drops footer text and a repeated table header between rows.
    const STATEMENT: &str = "March 31, 2026
Account statement from 01.03.2026 to 31.03.2026
Account holder: Jane Example
Previous balance as at 28.02.2026 IBAN of Page Statement
+ 1,250.00 EUR XX00 0000 0000 0000 0000 00 1 1 2
Credit Debit Item Value Booking
date
- 1,000.00 SEPA Überweisung an
Sam Landlord
02-03-
2026
03-03-
2026
IBAN XX00 1111 2222 3333 4444 55
BIC EXAMPLEXXX
Payment Reference/E2E-Ref.
March rent
- 12.34 Kartenzahlung
Payment Reference/E2E-Ref.
05-03-
2026
05-03-
2026
Corner Grocer//Townsville/XX 04-03-2026T18:04:42 Folgenr. 09
Example Bank AG
Some Town
Jane Example
1 Example Street
Telephone (0000) 0000-0
0000000001 / 00000000 / 20260401\x0cCredit Debit Item Value Booking
date
+ 2,500.00 SEPA Überweisung von
Example Employer GmbH
25-03-
2026
25-03-
2026
Payment Reference/E2E-Ref.
Salary 03/2026
- 0.04 Payment Reference/E2E-Ref.
Balance of settlement items
31-03-
2026
31-03-
2026
New balance
+ 2,737.62 EUR
Account number
0000000 00
Important notes
Please raise any objections without delay.
";

    fn date(m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, d).unwrap()
    }

    fn row(d: NaiveDate, amount: Decimal, description: &str) -> ParsedRow {
        ParsedRow {
            date: d,
            time: None,
            amount,
            description: description.to_string(),
            bank_state: BankState::Completed,
        }
    }

    #[test]
    fn a_statement_parses_into_rows_dated_by_booking_date() {
        let statement = parse_text(STATEMENT).unwrap();
        assert_eq!(
            statement.rows,
            vec![
                row(
                    date(3, 3),
                    dec!(-1000.00),
                    "SEPA Überweisung an Sam Landlord"
                ),
                row(date(3, 5), dec!(-12.34), "Kartenzahlung"),
                row(
                    date(3, 25),
                    dec!(2500.00),
                    "SEPA Überweisung von Example Employer GmbH"
                ),
                row(date(3, 31), dec!(-0.04), "Balance of settlement items"),
            ]
        );
        assert!(statement.reverted_candidates.is_empty());
    }

    #[test]
    fn the_date_range_and_balances_come_from_the_statement_header_and_footer() {
        let statement = parse_text(STATEMENT).unwrap();
        assert_eq!(
            statement.date_range,
            DateRange {
                start: date(3, 1),
                end: date(3, 31)
            }
        );
        assert_eq!(statement.opening_balance, dec!(1250.00));
        assert_eq!(statement.closing_balance, dec!(2737.62));
    }

    #[test]
    fn a_negative_opening_balance_keeps_its_sign() {
        let text = STATEMENT
            .replace("+ 1,250.00 EUR XX00", "- 18.04 EUR XX00")
            .replace("+ 2,737.62 EUR", "+ 1,469.58 EUR");
        let statement = parse_text(&text).unwrap();
        assert_eq!(statement.opening_balance, dec!(-18.04));
        assert!(crate::Importer::new(Stub(statement)).parse(b"").is_ok());
    }

    struct Stub(crate::ParseResult);

    impl crate::BankReader for Stub {
        fn read(&self, _bytes: &[u8]) -> Result<crate::ParseResult, ImportError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn the_sample_statement_passes_its_balance_check() {
        let statement = parse_text(STATEMENT).unwrap();
        assert!(crate::Importer::new(Stub(statement)).parse(b"").is_ok());
    }

    #[test]
    fn an_account_settlement_is_not_a_statement() {
        let text = "June 30, 2026
Account settlement
Account holder: Jane Example
Settlement period Currency BIC (SWIFT) IBAN Page
Credit Debit Item
- 0.04 active account
Balance of settlement items
- 0.04 EUR
";
        assert_eq!(parse_text(text), Err(ImportError::NotAStatement));
    }

    #[test]
    fn a_statement_missing_its_balances_or_row_dates_is_a_parse_error() {
        let no_opening = STATEMENT.replace("Previous balance as at 28.02.2026", "Something else");
        assert!(matches!(
            parse_text(&no_opening),
            Err(ImportError::Parse(_))
        ));
        let no_closing = STATEMENT.replace("New balance", "Something else");
        assert!(matches!(
            parse_text(&no_closing),
            Err(ImportError::Parse(_))
        ));
        let no_dates = STATEMENT.replace("31-03-\n2026\n31-03-\n2026\n", "");
        assert!(matches!(parse_text(&no_dates), Err(ImportError::Parse(_))));
    }
}
