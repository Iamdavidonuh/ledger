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
