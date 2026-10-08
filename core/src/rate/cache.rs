use crate::Currency;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct CachedRate {
    pub rate: Decimal,
    pub fetched_at: DateTime<Utc>,
    pub ttl_hours: u64,
}

impl CachedRate {
    pub fn new(rate: Decimal, ttl_hours: u64) -> Self {
        Self {
            rate,
            fetched_at: Utc::now(),
            ttl_hours,
        }
    }

    pub fn is_expired(&self) -> bool {
        let age = Utc::now() - self.fetched_at;
        age > Duration::hours(self.ttl_hours as i64)
    }
}

pub struct InMemoryRateCache {
    cache: Arc<RwLock<HashMap<(String, String), CachedRate>>>,
}

impl InMemoryRateCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get(&self, from: &Currency, to: &Currency) -> Option<CachedRate> {
        let cache = self.cache.read().await;
        let key = (from.code().to_string(), to.code().to_string());
        cache.get(&key).cloned()
    }

    pub async fn set(&self, from: &Currency, to: &Currency, rate: Decimal, ttl_hours: u64) {
        let mut cache = self.cache.write().await;
        let key = (from.code().to_string(), to.code().to_string());
        cache.insert(key, CachedRate::new(rate, ttl_hours));
    }

    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

impl Default for InMemoryRateCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn stores_and_retrieves_cached_rate() {
        let cache = InMemoryRateCache::new();
        let eur = Currency::new("EUR").unwrap();
        let usd = Currency::new("USD").unwrap();

        cache.set(&eur, &usd, dec!(1.1), 6).await;

        let cached = cache.get(&eur, &usd).await.unwrap();
        assert_eq!(cached.rate, dec!(1.1));
        assert_eq!(cached.ttl_hours, 6);
        assert!(!cached.is_expired());
    }

    #[tokio::test]
    async fn returns_none_for_missing_rate() {
        let cache = InMemoryRateCache::new();
        let eur = Currency::new("EUR").unwrap();
        let gbp = Currency::new("GBP").unwrap();

        assert!(cache.get(&eur, &gbp).await.is_none());
    }

    #[tokio::test]
    async fn clear_removes_all_entries() {
        let cache = InMemoryRateCache::new();
        let eur = Currency::new("EUR").unwrap();
        let usd = Currency::new("USD").unwrap();

        cache.set(&eur, &usd, dec!(1.1), 6).await;
        cache.clear().await;

        assert!(cache.get(&eur, &usd).await.is_none());
    }

    #[test]
    fn detects_expired_rate() {
        let rate = CachedRate {
            rate: dec!(1.1),
            fetched_at: Utc::now() - Duration::hours(7),
            ttl_hours: 6,
        };

        assert!(rate.is_expired());
    }

    #[test]
    fn detects_fresh_rate() {
        let rate = CachedRate::new(dec!(1.1), 6);
        assert!(!rate.is_expired());
    }
}
