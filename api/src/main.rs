#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod accounts;
mod entries;
mod error;
mod import_matching;
mod imports;
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
        .merge(imports::router())
        .merge(pots::router())
        .merge(transfers::router())
        .merge(valuations::router())
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = std::env::var("LEDGER_DB_PATH").unwrap_or_else(|_| "ledger.sqlite3".to_string());
    let store = SqliteStore::open(&PathBuf::from(db_path))?;
    let state = AppState::new(store);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app(state)).await?;
    Ok(())
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
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
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
            .unwrap()
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
            .unwrap()
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
            .open_pot(
                "Emergency fund",
                ledger_core::Currency::new("EUR").unwrap(),
                None,
                None,
            )
            .unwrap()
            .id;

        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/pots/{pot}/allocations"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"amount": "1000", "date": "2026-01-15"}).to_string(),
                    ))
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
                    .body(Body::from(
                        serde_json::json!({"amount": "0.01", "date": "2026-01-15"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn deleting_a_pot_through_the_api_returns_its_balance_to_general_savings() {
        let state = test_state();
        let eur = ledger_core::Currency::new("EUR").unwrap();
        let pot = {
            let mut ledger = state.ledger.lock().unwrap();
            ledger
                .open_account(
                    "Checking",
                    eur.clone(),
                    ledger_core::AccountKind::Own,
                    rust_decimal_macros::dec!(500),
                )
                .unwrap();
            ledger
                .open_pot("Trip fund", eur.clone(), None, None)
                .unwrap()
                .id
        };

        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/pots/{pot}/allocations"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"amount": "200", "date": "2026-01-15"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/pots/{pot}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        assert_eq!(
            state.ledger.lock().unwrap().general_savings(&eur),
            Ok(rust_decimal_macros::dec!(500))
        );

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/pots/{pot}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
