use crate::error::AppError;
use ledger_core::{Ledger, LedgerError, SqliteStore};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub ledger: Arc<Mutex<Ledger<SqliteStore>>>,
}

impl AppState {
    pub fn new(store: SqliteStore) -> Self {
        AppState {
            ledger: Arc::new(Mutex::new(Ledger::new(store))),
        }
    }

    /// Runs `f` on Tokio's blocking-task pool rather than an async worker
    /// thread. rusqlite is a synchronous C binding with no async I/O of its
    /// own, so calling it directly from an async handler would hold a
    /// worker thread (and, since std::sync::Mutex::lock() also blocks
    /// rather than yielding, every handler queued behind it) for as long as
    /// the disk I/O takes; enough concurrent requests could starve the
    /// whole runtime, including unrelated requests that never touch the
    /// ledger.
    pub async fn with_ledger<F, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Ledger<SqliteStore>) -> Result<T, LedgerError> + Send + 'static,
        T: Send + 'static,
    {
        let ledger = self.ledger.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = ledger
                .lock()
                .map_err(|_| AppError::internal("the ledger lock was poisoned"))?;
            f(&mut guard).map_err(AppError::from)
        })
        .await
        .map_err(|_| AppError::internal("the ledger task panicked"))?
    }
}
