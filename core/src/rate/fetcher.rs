use crate::{Currency, LedgerError};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

pub struct FrankfurterClient {
    http: reqwest::Client,
    base_url: String,
}

impl FrankfurterClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: "https://api.frankfurter.app".to_string(),
        }
    }

    pub async fn get_rate(&self, from: &Currency, to: &Currency) -> Result<Decimal, LedgerError> {
        let url = format!(
            "{}/latest?from={}&to={}",
            self.base_url,
            from.code(),
            to.code()
        );

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| LedgerError::ExchangeRateFetchFailed(format!("Failed to fetch fiat rate: {}", e)))?;

        if !resp.status().is_success() {
            return Err(LedgerError::ExchangeRateFetchFailed(format!(
                "Fiat rate API returned status {}",
                resp.status()
            )));
        }

        let data: FrankfurterResponse = resp.json().await.map_err(|e| {
            LedgerError::ExchangeRateFetchFailed(format!("Failed to parse fiat rate response: {}", e))
        })?;

        let rate_f64 = data
            .rates
            .get(to.code())
            .ok_or_else(|| LedgerError::ExchangeRateFetchFailed(format!("No rate for {}", to.code())))?;

        Decimal::try_from(*rate_f64)
            .map_err(|e| LedgerError::ExchangeRateFetchFailed(format!("Invalid rate value: {}", e)))
    }
}

impl Default for FrankfurterClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct FrankfurterResponse {
    rates: HashMap<String, f64>,
}

pub struct CoinGeckoClient {
    http: reqwest::Client,
    base_url: String,
}

impl CoinGeckoClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: "https://api.coingecko.com/api/v3".to_string(),
        }
    }

    fn coin_id(currency: &Currency) -> &str {
        // Map common crypto codes to CoinGecko IDs
        match currency.code() {
            "BTC" => "bitcoin",
            "ETH" => "ethereum",
            "SOL" => "solana",
            "USDT" => "tether",
            "USDC" => "usd-coin",
            "BNB" => "binancecoin",
            "XRP" => "ripple",
            "ADA" => "cardano",
            "DOGE" => "dogecoin",
            "MATIC" => "matic-network",
            "DOT" => "polkadot",
            "AVAX" => "avalanche-2",
            "LINK" => "chainlink",
            "UNI" => "uniswap",
            "ATOM" => "cosmos",
            "LTC" => "litecoin",
            "BCH" => "bitcoin-cash",
            "XLM" => "stellar",
            "ALGO" => "algorand",
            "VET" => "vechain",
            "FIL" => "filecoin",
            "TRX" => "tron",
            "ETC" => "ethereum-classic",
            "AAVE" => "aave",
            "XMR" => "monero",
            "THETA" => "theta-token",
            "EOS" => "eos",
            "XTZ" => "tezos",
            "EGLD" => "elrond-erd-2",
            _ => currency.code(),
        }
    }

    pub async fn get_rate(&self, from: &Currency, to: &Currency) -> Result<Decimal, LedgerError> {
        if to.code() != "USD" {
            return Err(LedgerError::UnsupportedConversion {
                from: from.code().to_string(),
                to: to.code().to_string(),
            });
        }

        let coin_id = Self::coin_id(from);
        let url = format!(
            "{}/simple/price?ids={}&vs_currencies=usd",
            self.base_url, coin_id
        );

        let resp = self
            .http
            .get(&url)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| LedgerError::ExchangeRateFetchFailed(format!("Failed to fetch crypto rate: {}", e)))?;

        if !resp.status().is_success() {
            return Err(LedgerError::ExchangeRateFetchFailed(format!(
                "Crypto rate API returned status {}",
                resp.status()
            )));
        }

        let data: CoinGeckoResponse = resp.json().await.map_err(|e| {
            LedgerError::ExchangeRateFetchFailed(format!("Failed to parse crypto rate response: {}", e))
        })?;

        let coin_data = data.coins.get(coin_id).ok_or_else(|| {
            LedgerError::ExchangeRateFetchFailed(format!("No price data for coin {}", coin_id))
        })?;

        Decimal::try_from(coin_data.usd)
            .map_err(|e| LedgerError::ExchangeRateFetchFailed(format!("Invalid rate value: {}", e)))
    }
}

impl Default for CoinGeckoClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct CoinGeckoResponse {
    #[serde(flatten)]
    coins: HashMap<String, CoinData>,
}

#[derive(Deserialize)]
struct CoinData {
    usd: f64,
}

// Fallback fiat client using open.er-api.com (free, no API key, covers NGN
// and other currencies Frankfurter does not).
pub struct OpenErClient {
    http: reqwest::Client,
    base_url: String,
}

impl OpenErClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: "https://open.er-api.com/v6".to_string(),
        }
    }

    pub async fn get_rate(&self, from: &Currency, to: &Currency) -> Result<Decimal, LedgerError> {
        let url = format!("{}/latest/{}", self.base_url, from.code());
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| LedgerError::ExchangeRateFetchFailed(format!("OpenER fetch failed: {}", e)))?;

        if !resp.status().is_success() {
            return Err(LedgerError::ExchangeRateFetchFailed(format!(
                "OpenER returned status {}",
                resp.status()
            )));
        }

        let data: OpenErResponse = resp.json().await.map_err(|e| {
            LedgerError::ExchangeRateFetchFailed(format!("OpenER parse failed: {}", e))
        })?;

        if data.result != "success" {
            return Err(LedgerError::ExchangeRateFetchFailed(
                "OpenER returned non-success result".to_string(),
            ));
        }

        let rate_f64 = data
            .rates
            .get(to.code())
            .ok_or_else(|| LedgerError::ExchangeRateFetchFailed(format!("OpenER: no rate for {}", to.code())))?;

        Decimal::try_from(*rate_f64)
            .map_err(|e| LedgerError::ExchangeRateFetchFailed(format!("OpenER: invalid rate value: {}", e)))
    }
}

impl Default for OpenErClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct OpenErResponse {
    result: String,
    rates: HashMap<String, f64>,
}

pub struct RateFetcher {
    fiat_client: FrankfurterClient,
    fiat_fallback: OpenErClient,
    crypto_client: CoinGeckoClient,
}

impl RateFetcher {
    pub fn new() -> Self {
        Self {
            fiat_client: FrankfurterClient::new(),
            fiat_fallback: OpenErClient::new(),
            crypto_client: CoinGeckoClient::new(),
        }
    }

    fn is_retryable(err: &LedgerError) -> bool {
        matches!(err, LedgerError::ExchangeRateFetchFailed(_))
    }

    async fn fetch_with_retry(&self, from: &Currency, to: &Currency) -> Result<Decimal, LedgerError> {
        let mut attempts = 0;
        let max_attempts = 3;

        loop {
            match self.fetch_once(from, to).await {
                Ok(rate) => return Ok(rate),
                Err(e) if attempts < max_attempts && Self::is_retryable(&e) => {
                    attempts += 1;
                    let backoff_ms = 100 * 2u64.pow(attempts);
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn fetch_fiat(&self, from: &Currency, to: &Currency) -> Result<Decimal, LedgerError> {
        // Try Frankfurter first; fall back to open.er-api.com for currencies it
        // does not cover (e.g. NGN).
        match self.fiat_client.get_rate(from, to).await {
            Ok(rate) => return Ok(rate),
            Err(LedgerError::ExchangeRateFetchFailed(_)) => {}
            Err(e) => return Err(e),
        }
        self.fiat_fallback.get_rate(from, to).await
    }

    async fn fetch_once(&self, from: &Currency, to: &Currency) -> Result<Decimal, LedgerError> {
        if from == to {
            return Ok(Decimal::ONE);
        }

        // Try fiat-to-fiat (Frankfurter, then open.er-api.com fallback).
        match self.fetch_fiat(from, to).await {
            Ok(rate) => return Ok(rate),
            Err(LedgerError::ExchangeRateFetchFailed(_)) => {
                // Both fiat sources failed; try crypto path.
            }
            Err(e) => return Err(e),
        }

        // Try crypto-to-USD
        if to.code() == "USD" {
            return self.crypto_client.get_rate(from, to).await;
        }

        // Try crypto-to-fiat via USD.
        // CoinGecko free tier only supports USD conversion. Route through USD:
        // fetch crypto→USD, then USD→fiat, and multiply. Precision loss is negligible
        // for display purposes but compounds in repeated conversions.
        let usd = Currency::new("USD").map_err(|e| LedgerError::ExchangeRateFetchFailed(e.to_string()))?;
        match self.crypto_client.get_rate(from, &usd).await {
            Ok(crypto_to_usd) => {
                let usd_to_fiat = self.fetch_fiat(&usd, to).await?;
                Ok(crypto_to_usd * usd_to_fiat)
            }
            Err(_) => Err(LedgerError::UnsupportedConversion {
                from: from.code().to_string(),
                to: to.code().to_string(),
            }),
        }
    }

    pub async fn fetch(&self, from: &Currency, to: &Currency) -> Result<Decimal, LedgerError> {
        self.fetch_with_retry(from, to).await
    }
}

impl Default for RateFetcher {
    fn default() -> Self {
        Self::new()
    }
}
