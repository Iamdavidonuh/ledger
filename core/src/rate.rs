use crate::Currency;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

mod cache;
mod fetcher;

pub use cache::{CachedRate, InMemoryRateCache};
pub use fetcher::{CoinGeckoClient, FrankfurterClient, RateFetcher};

#[derive(Debug, Clone)]
pub struct ExchangeRate {
    pub from: Currency,
    pub to: Currency,
    pub rate: Decimal,
    pub fetched_at: DateTime<Utc>,
}

impl ExchangeRate {
    pub fn new(from: Currency, to: Currency, rate: Decimal) -> Self {
        Self {
            from,
            to,
            rate,
            fetched_at: Utc::now(),
        }
    }

    pub fn convert(&self, amount: Decimal) -> Decimal {
        amount * self.rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn converts_amount_using_rate() {
        let eur = Currency::new("EUR").unwrap();
        let usd = Currency::new("USD").unwrap();
        let rate = ExchangeRate::new(eur, usd, dec!(1.1));

        assert_eq!(rate.convert(dec!(100)), dec!(110.0));
    }
}
