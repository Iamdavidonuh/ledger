use crate::error::AppError;
use ledger_core::{Ledger, LedgerError, LedgerStore, PgStore};
use sqlx::PgPool;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(test)]
use ledger_core::InMemoryStore;
#[cfg(test)]
use tokio::sync::Mutex;

/// Shared interface implemented by both `AppState` (production, pool-backed)
/// and `TestState` (tests, in-memory). Handlers are generic over this trait.
pub trait WithLedger: Clone + Send + Sync + 'static {
    type Store: LedgerStore;

    fn with_ledger<F, T>(
        &self,
        f: F,
    ) -> impl Future<Output = Result<T, AppError>> + Send + '_
    where
        for<'a> F: FnOnce(
            &'a mut Ledger<Self::Store>,
        ) -> Pin<Box<dyn Future<Output = Result<T, LedgerError>> + Send + 'a>>,
        F: Send + 'static,
        T: Send + 'static;
}

/// Production state: holds a pool and acquires one connection per request.
#[derive(Clone)]
pub struct AppState {
    pool: Arc<PgPool>,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        AppState { pool: Arc::new(pool) }
    }
}

impl WithLedger for AppState {
    type Store = PgStore;

    async fn with_ledger<F, T>(&self, f: F) -> Result<T, AppError>
    where
        for<'a> F: FnOnce(
            &'a mut Ledger<PgStore>,
        ) -> Pin<Box<dyn Future<Output = Result<T, LedgerError>> + Send + 'a>>,
        F: Send + 'static,
        T: Send + 'static,
    {
        let store = PgStore::acquire(&self.pool).await.map_err(AppError::from)?;
        let mut ledger = Ledger::new(store);
        f(&mut ledger).await.map_err(AppError::from)
    }
}

/// Test state: wraps an `InMemoryStore` behind a mutex. No database needed.
#[cfg(test)]
#[derive(Clone)]
pub struct TestState {
    pub ledger: Arc<Mutex<Ledger<InMemoryStore>>>,
}

#[cfg(test)]
impl TestState {
    pub fn new() -> Self {
        TestState {
            ledger: Arc::new(Mutex::new(Ledger::new(InMemoryStore::default()))),
        }
    }
}

#[cfg(test)]
impl WithLedger for TestState {
    type Store = InMemoryStore;

    async fn with_ledger<F, T>(&self, f: F) -> Result<T, AppError>
    where
        for<'a> F: FnOnce(
            &'a mut Ledger<InMemoryStore>,
        ) -> Pin<Box<dyn Future<Output = Result<T, LedgerError>> + Send + 'a>>,
        F: Send + 'static,
        T: Send + 'static,
    {
        let mut guard = self.ledger.lock().await;
        f(&mut guard).await.map_err(AppError::from)
    }
}
