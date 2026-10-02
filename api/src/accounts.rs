use crate::error::{AppError, AppJson};
use crate::state::WithLedger;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use ledger_core::{Account, AccountId, AccountKind, Currency, LedgerError};
use rust_decimal::Decimal;

#[derive(serde::Deserialize)]
pub struct CreateAccountRequest {
    pub name: String,
    pub currency: Currency,
    pub kind: AccountKind,
    pub opening_balance: Decimal,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AccountWithBalance {
    #[serde(flatten)]
    pub account: Account,
    pub balance: Decimal,
}

pub fn router<S: WithLedger>() -> Router<S> {
    Router::new()
        .route(
            "/accounts",
            post(create_account::<S>).get(list_accounts::<S>),
        )
        .route("/accounts/:id", get(get_account::<S>))
        .route("/accounts/:id/archive", post(archive_account::<S>))
        .route("/accounts/:id/unarchive", post(unarchive_account::<S>))
}

async fn create_account<S: WithLedger>(
    State(state): State<S>,
    AppJson(req): AppJson<CreateAccountRequest>,
) -> Result<Json<Account>, AppError> {
    let account = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                ledger
                    .open_account(&req.name, req.currency, req.kind, req.opening_balance)
                    .await
            })
        })
        .await?;
    Ok(Json(account))
}

async fn list_accounts<S: WithLedger>(
    State(state): State<S>,
) -> Result<Json<Vec<AccountWithBalance>>, AppError> {
    let with_balances = state
        .with_ledger(|ledger| {
            Box::pin(async move {
                let accounts = ledger.accounts().await?;
                let mut result = Vec::with_capacity(accounts.len());
                for account in accounts {
                    let balance = ledger.account_balance(account.id).await?;
                    result.push(AccountWithBalance { account, balance });
                }
                Ok(result)
            })
        })
        .await?;
    Ok(Json(with_balances))
}

async fn get_account<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<AccountId>,
) -> Result<Json<AccountWithBalance>, AppError> {
    let with_balance = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                let account = ledger
                    .account(id)
                    .await?
                    .ok_or(LedgerError::AccountNotFound(id))?;
                let balance = ledger.account_balance(id).await?;
                Ok(AccountWithBalance { account, balance })
            })
        })
        .await?;
    Ok(Json(with_balance))
}

async fn archive_account<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<AccountId>,
) -> Result<Json<Account>, AppError> {
    let account = state
        .with_ledger(move |ledger| {
            Box::pin(async move { ledger.set_account_archived(id, true).await })
        })
        .await?;
    Ok(Json(account))
}

async fn unarchive_account<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<AccountId>,
) -> Result<Json<Account>, AppError> {
    let account = state
        .with_ledger(move |ledger| {
            Box::pin(async move { ledger.set_account_archived(id, false).await })
        })
        .await?;
    Ok(Json(account))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_state() -> crate::state::TestState {
        crate::state::TestState::new()
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
            .await
            .open_account(
                "Checking",
                Currency::new("EUR").unwrap(),
                AccountKind::Own,
                rust_decimal_macros::dec!(50),
            )
            .await
            .unwrap();
        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/accounts")
                    .body(Body::empty())
                    .unwrap(),
            )
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
                    .uri(format!("/accounts/{}", AccountId::generate()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn archiving_then_unarchiving_an_account_round_trips() {
        let state = test_state();
        let id = state
            .ledger
            .lock()
            .await
            .open_account(
                "Checking",
                Currency::new("EUR").unwrap(),
                AccountKind::Own,
                rust_decimal_macros::dec!(0),
            )
            .await
            .unwrap()
            .id;

        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/accounts/{id}/archive"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let account: Account = serde_json::from_slice(&body).unwrap();
        assert!(account.archived);

        let response = app(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/accounts/{id}/unarchive"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let account: Account = serde_json::from_slice(&body).unwrap();
        assert!(!account.archived);
    }

    #[tokio::test]
    async fn archiving_an_unknown_account_is_not_found() {
        let response = app(test_state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/accounts/{}/archive", AccountId::generate()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
