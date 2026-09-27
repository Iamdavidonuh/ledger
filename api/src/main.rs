// Every handler locks the shared Ledger mutex with `.expect("...poisoned")`,
// the standard idiom for a poisoned mutex (only reachable if another thread
// panicked while holding the lock), and a few handlers use `.expect(...)`
// for "this id was just confirmed to exist by an earlier call in the same
// handler" invariants. Both document a real invariant rather than hiding a
// normal failure, the same reasoning ledger-core's sqlite_store.rs uses,
// just spread across this whole crate instead of one file, since the
// mutex-lock pattern is inherent to every handler here.
#![allow(clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod accounts;
mod entries;
mod error;
mod pots;
mod state;
mod transfers;
mod valuations;

use axum::Router;
use ledger_core::SqliteStore;
use state::AppState;
use std::path::PathBuf;

async fn health() -> &'static str {
    "ok"
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", axum::routing::get(health))
        .merge(accounts::router())
        .merge(entries::router())
        .merge(pots::router())
        .merge(transfers::router())
        .merge(valuations::router())
        .with_state(state)
}

#[tokio::main]
async fn main() {
    let db_path = std::env::var("LEDGER_DB_PATH").unwrap_or_else(|_| "ledger.sqlite3".to_string());
    let store = SqliteStore::open(&PathBuf::from(db_path)).expect("opening the database should not fail");
    let state = AppState::new(store);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("binding the port should not fail");
    axum::serve(listener, app(state)).await.expect("serving should not fail");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState::new(SqliteStore::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let response = app(test_state())
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_full_flow_of_transfer_allocate_and_a_refused_overdraw_works_end_to_end() {
        let state = test_state();

        let a = state
            .ledger
            .lock()
            .unwrap()
            .open_account(
                "Checking",
                ledger_core::Currency::new("EUR").unwrap(),
                ledger_core::AccountKind::Own,
                rust_decimal_macros::dec!(1000),
            )
            .id;
        let b = state
            .ledger
            .lock()
            .unwrap()
            .open_account(
                "Savings",
                ledger_core::Currency::new("EUR").unwrap(),
                ledger_core::AccountKind::Own,
                rust_decimal_macros::dec!(0),
            )
            .id;

        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/transfers")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"from_account_id": a, "to_account_id": b, "date": "2026-01-15", "amount_sent": "400", "amount_received": "400", "description": "move"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let pot = state
            .ledger
            .lock()
            .unwrap()
            .open_pot("Emergency fund", ledger_core::Currency::new("EUR").unwrap(), None, None)
            .id;

        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/pots/{pot}/allocations"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({"amount": "1000", "date": "2026-01-15"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/pots/{pot}/allocations"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({"amount": "0.01", "date": "2026-01-15"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
