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
use sqlx::PgPool;
use state::{AppState, WithLedger};

async fn health() -> &'static str {
    "ok"
}

/// BankB reads a PDF by shelling out to `pdftotext`; a missing binary would
/// otherwise surface as a confusing failure deep inside someone's first
/// BankB upload, with nothing pointing at the actual problem (the deployed
/// image is missing it). A warning at startup, not a hard exit: BankA
/// imports, accounts, entries, pots and everything else this API does are
/// unrelated to pdftotext and stay fully usable even with BankB degraded.
fn check_pdftotext_is_available() {
    let found = std::process::Command::new("pdftotext")
        .arg("-v")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok();
    if !found {
        eprintln!(
            "warning: pdftotext is not on PATH; BankB (PDF) imports will fail. \
             Everything else is unaffected. Install poppler-utils, or fix the \
             Docker image if this is deployed."
        );
    }
}

pub fn app<S: WithLedger>(state: S) -> Router {
    Router::new()
        .route("/health", axum::routing::get(health))
        .merge(accounts::router::<S>())
        .merge(entries::router::<S>())
        .merge(imports::router::<S>())
        .merge(pots::router::<S>())
        .merge(transfers::router::<S>())
        .merge(valuations::router::<S>())
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    check_pdftotext_is_available();
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/ledger".to_string());
    let pool = PgPool::connect(&database_url).await?;
    sqlx::migrate!("../core/migrations").run(&pool).await?;
    let state = AppState::new(pool);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use ledger_core::{AccountKind, Currency};
    use rust_decimal_macros::dec;
    use state::TestState;
    use tower::ServiceExt;

    fn test_state() -> TestState {
        TestState::new()
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

        let a = state.ledger.lock().await
            .open_account("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(1000))
            .await.unwrap().id;
        let b = state.ledger.lock().await
            .open_account("Savings", Currency::new("EUR").unwrap(), AccountKind::Own, dec!(0))
            .await.unwrap().id;

        let response = app(state.clone())
            .oneshot(
                Request::builder().method("POST").uri("/transfers")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({"from_account_id": a, "to_account_id": b, "date": "2026-01-15", "amount_sent": "400", "amount_received": "400", "description": "move"}).to_string()))
                    .unwrap(),
            )
            .await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let pot = state.ledger.lock().await
            .open_pot("Emergency fund", Currency::new("EUR").unwrap(), None, None)
            .await.unwrap().id;

        let response = app(state.clone())
            .oneshot(
                Request::builder().method("POST").uri(format!("/pots/{pot}/allocations"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({"amount": "1000", "date": "2026-01-15"}).to_string()))
                    .unwrap(),
            )
            .await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app(state)
            .oneshot(
                Request::builder().method("POST").uri(format!("/pots/{pot}/allocations"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({"amount": "0.01", "date": "2026-01-15"}).to_string()))
                    .unwrap(),
            )
            .await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn deleting_a_pot_through_the_api_returns_its_balance_to_general_savings() {
        let state = test_state();
        let eur = Currency::new("EUR").unwrap();
        let pot = {
            let mut ledger = state.ledger.lock().await;
            ledger.open_account("Checking", eur.clone(), AccountKind::Own, dec!(500)).await.unwrap();
            ledger.open_pot("Trip fund", eur.clone(), None, None).await.unwrap().id
        };

        let response = app(state.clone())
            .oneshot(
                Request::builder().method("POST").uri(format!("/pots/{pot}/allocations"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({"amount": "200", "date": "2026-01-15"}).to_string()))
                    .unwrap(),
            )
            .await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app(state.clone())
            .oneshot(
                Request::builder().method("DELETE").uri(format!("/pots/{pot}"))
                    .body(Body::empty()).unwrap(),
            )
            .await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        assert_eq!(
            state.ledger.lock().await.general_savings(&eur).await,
            Ok(dec!(500))
        );

        let response = app(state)
            .oneshot(Request::builder().uri(format!("/pots/{pot}")).body(Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
