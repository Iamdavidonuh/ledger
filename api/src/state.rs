use crate::error::AppError;
use ledger_core::{
    Currency, ExchangeRate, InMemoryRateCache, InMemoryStore, Ledger, LedgerError, LedgerStore,
    PgStore, RateFetcher,
};
use sqlx::PgPool;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared interface implemented by both `AppState` (production, pool-backed)
/// and `TestState` (tests, in-memory). Handlers are generic over this trait.
pub trait WithLedger: Clone + Send + Sync + 'static {
    type Store: LedgerStore;

    fn with_ledger<F, T>(&self, f: F) -> impl Future<Output = Result<T, AppError>> + Send + '_
    where
        for<'a> F: FnOnce(
            &'a mut Ledger<Self::Store>,
        )
            -> Pin<Box<dyn Future<Output = Result<T, LedgerError>> + Send + 'a>>,
        F: Send + 'static,
        T: Send + 'static;
}

pub trait WithRates: Clone + Send + Sync + 'static {
    fn get_rate(
        &self,
        from: &Currency,
        to: &Currency,
    ) -> impl Future<Output = Result<ExchangeRate, AppError>> + Send;
    
    fn clear_rate_cache(&self) -> impl Future<Output = ()> + Send;

    fn account_value_in(
        &self,
        account_id: ledger_core::AccountId,
        target_currency: &Currency,
    ) -> impl Future<Output = Result<rust_decimal::Decimal, AppError>> + Send;
}

/// Production state: holds a pool and acquires one connection per request.
#[derive(Clone)]
pub struct AppState {
    pool: Arc<PgPool>,
    rate_cache: Arc<InMemoryRateCache>,
    rate_fetcher: Arc<RateFetcher>,
    rate_ttl_hours: u64,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        let rate_ttl_hours = std::env::var("RATE_CACHE_TTL_HOURS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(6);

        AppState {
            pool: Arc::new(pool),
            rate_cache: Arc::new(InMemoryRateCache::new()),
            rate_fetcher: Arc::new(RateFetcher::new()),
            rate_ttl_hours,
        }
    }
}

impl WithLedger for AppState {
    type Store = PgStore;

    async fn with_ledger<F, T>(&self, f: F) -> Result<T, AppError>
    where
        for<'a> F: FnOnce(
            &'a mut Ledger<PgStore>,
        )
            -> Pin<Box<dyn Future<Output = Result<T, LedgerError>> + Send + 'a>>,
        F: Send + 'static,
        T: Send + 'static,
    {
        let store = PgStore::acquire(&self.pool).await.map_err(AppError::from)?;
        let mut ledger = Ledger::new(store);
        f(&mut ledger).await.map_err(AppError::from)
    }
}

impl WithRates for AppState {
    async fn get_rate(&self, from: &Currency, to: &Currency) -> Result<ExchangeRate, AppError> {
        let store = PgStore::acquire(&self.pool).await.map_err(AppError::from)?;
        let ledger = Ledger::new(store).with_rates(
            self.rate_cache.clone(),
            self.rate_fetcher.clone(),
            self.rate_ttl_hours,
        );
        ledger.get_exchange_rate(from, to).await.map_err(AppError::from)
    }

    async fn clear_rate_cache(&self) {
        self.rate_cache.as_ref().clear().await;
    }

    async fn account_value_in(
        &self,
        account_id: ledger_core::AccountId,
        target_currency: &Currency,
    ) -> Result<rust_decimal::Decimal, AppError> {
        let store = PgStore::acquire(&self.pool).await.map_err(AppError::from)?;
        let mut ledger = Ledger::new(store).with_rates(
            self.rate_cache.clone(),
            self.rate_fetcher.clone(),
            self.rate_ttl_hours,
        );
        ledger
            .account_value_in(account_id, target_currency)
            .await
            .map_err(AppError::from)
    }
}

/// Test state: wraps an `InMemoryStore` behind a mutex. No database needed.
/// Used in unit tests via `app(TestState::new()).oneshot(request)`.
#[derive(Clone)]
pub struct TestState {
    pub ledger: Arc<Mutex<Ledger<InMemoryStore>>>,
    rate_cache: Arc<InMemoryRateCache>,
    rate_fetcher: Arc<RateFetcher>,
}

impl Default for TestState {
    fn default() -> Self {
        TestState {
            ledger: Arc::new(Mutex::new(Ledger::new(InMemoryStore::default()))),
            rate_cache: Arc::new(InMemoryRateCache::new()),
            rate_fetcher: Arc::new(RateFetcher::new()),
        }
    }
}

impl TestState {
    pub fn new() -> Self {
        Self::default()
    }
}

impl WithLedger for TestState {
    type Store = InMemoryStore;

    async fn with_ledger<F, T>(&self, f: F) -> Result<T, AppError>
    where
        for<'a> F: FnOnce(
            &'a mut Ledger<InMemoryStore>,
        )
            -> Pin<Box<dyn Future<Output = Result<T, LedgerError>> + Send + 'a>>,
        F: Send + 'static,
        T: Send + 'static,
    {
        let mut guard = self.ledger.lock().await;
        f(&mut guard).await.map_err(AppError::from)
    }
}

impl WithRates for TestState {
    async fn get_rate(&self, from: &Currency, to: &Currency) -> Result<ExchangeRate, AppError> {
        let store = ledger_core::InMemoryStore::default();
        let ledger = Ledger::new(store).with_rates(
            self.rate_cache.clone(),
            self.rate_fetcher.clone(),
            1, // Short TTL for tests
        );
        ledger.get_exchange_rate(from, to).await.map_err(AppError::from)
    }

    async fn clear_rate_cache(&self) {
        self.rate_cache.as_ref().clear().await;
    }

    async fn account_value_in(
        &self,
        account_id: ledger_core::AccountId,
        target_currency: &Currency,
    ) -> Result<rust_decimal::Decimal, AppError> {
        let mut guard = self.ledger.lock().await;
        // Set up rates on the existing ledger
        guard.rate_cache = Some(self.rate_cache.clone());
        guard.rate_fetcher = Some(self.rate_fetcher.clone());
        guard.rate_ttl_hours = 1;

        guard
            .account_value_in(account_id, target_currency)
            .await
            .map_err(AppError::from)
    }
}
