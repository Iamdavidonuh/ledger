use crate::error::AppError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use ledger_core::{Account, AccountKind, Currency, LedgerError};
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct CreateAccountRequest {
    pub name: String,
    pub currency: String,
    pub kind: AccountKind,
    pub opening_balance: Decimal,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AccountWithBalance {
    #[serde(flatten)]
    pub account: Account,
    pub balance: Decimal,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/accounts", post(create_account).get(list_accounts))
        .route("/accounts/:id", get(get_account))
}

async fn create_account(
    State(state): State<AppState>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<Json<Account>, AppError> {
    let currency = Currency::new(&req.currency).map_err(|e| AppError::bad_request(e.to_string()))?;
    let mut ledger = state.lock()?;
    let account = ledger.open_account(&req.name, currency, req.kind, req.opening_balance)?;
    Ok(Json(account))
}

async fn list_accounts(State(state): State<AppState>) -> Result<Json<Vec<AccountWithBalance>>, AppError> {
    let ledger = state.lock()?;
    let mut with_balances = Vec::new();
    for account in ledger.accounts()? {
        let balance = ledger.account_balance(account.id)?;
        with_balances.push(AccountWithBalance { account, balance });
    }
    Ok(Json(with_balances))
}

async fn get_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AccountWithBalance>, AppError> {
    let ledger = state.lock()?;
    let account = ledger.account(id)?.ok_or(LedgerError::AccountNotFound(id))?;
    let balance = ledger.account_balance(id)?;
    Ok(Json(AccountWithBalance { account, balance }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use ledger_core::SqliteStore;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState::new(SqliteStore::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn creating_an_account_returns_it() {
        let response = app(test_state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/accounts")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "Checking", "currency": "EUR", "kind": "own", "opening_balance": "100"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let account: Account = serde_json::from_slice(&body).unwrap();
        assert_eq!(account.name, "Checking");
    }

    #[tokio::test]
    async fn creating_an_account_with_a_bad_currency_is_a_bad_request() {
        let response = app(test_state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/accounts")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "Checking", "currency": "!", "kind": "own", "opening_balance": "0"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn listing_accounts_includes_the_balance() {
        let state = test_state();
        state
            .ledger
            .lock()
            .unwrap()
            .open_account("Checking", Currency::new("EUR").unwrap(), AccountKind::Own, rust_decimal_macros::dec!(50))
            .unwrap();
        let response = app(state)
            .oneshot(Request::builder().uri("/accounts").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let accounts: Vec<AccountWithBalance> = serde_json::from_slice(&body).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].balance, rust_decimal_macros::dec!(50));
    }

    #[tokio::test]
    async fn getting_an_unknown_account_is_not_found() {
        let response = app(test_state())
            .oneshot(
                Request::builder()
                    .uri(format!("/accounts/{}", Uuid::new_v4()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
