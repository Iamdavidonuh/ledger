use ledger_core::{Ledger, SqliteStore};
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
}
