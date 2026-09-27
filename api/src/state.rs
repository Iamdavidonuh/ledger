use crate::error::AppError;
use ledger_core::{Ledger, SqliteStore};
use std::sync::{Arc, Mutex, MutexGuard};

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

    pub fn lock(&self) -> Result<MutexGuard<'_, Ledger<SqliteStore>>, AppError> {
        self.ledger.lock().map_err(|_| AppError::internal("the ledger lock was poisoned"))
    }
}
