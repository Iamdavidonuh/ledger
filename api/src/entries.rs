use crate::error::AppError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{patch, post};
use axum::{Json, Router};
use chrono::NaiveDate;
use ledger_core::{AccountId, Entry, EntryId, EntryMetadata, PotId};
use rust_decimal::Decimal;

#[derive(serde::Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Expense,
    Income,
}

impl EntryKind {
    fn signed(self, amount: Decimal) -> Decimal {
        match self {
            EntryKind::Expense => -amount,
            EntryKind::Income => amount,
        }
    }
}

fn require_non_negative(amount: Decimal) -> Result<(), AppError> {
    if amount < Decimal::ZERO {
        return Err(AppError::bad_request(
            "amount must be zero or positive; use kind to say expense or income",
        ));
    }
    Ok(())
}

#[derive(serde::Deserialize)]
pub struct RecordEntryRequest {
    pub account_id: AccountId,
    pub date: NaiveDate,
    pub kind: EntryKind,
    pub amount: Decimal,
    pub description: String,
}

#[derive(serde::Deserialize)]
pub struct AccountIdQuery {
    pub account_id: AccountId,
}

#[derive(serde::Deserialize)]
pub struct UpdateEntryMetadataRequest {
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub pot_id: Option<PotId>,
}

#[derive(serde::Deserialize)]
pub struct EditAmountRequest {
    pub kind: EntryKind,
    pub amount: Decimal,
}

#[derive(serde::Deserialize)]
pub struct VoidRequest {
    pub reason: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/entries", post(record_entry).get(list_entries))
        .route("/entries/:id", patch(update_metadata))
        .route("/entries/:id/amount", patch(edit_amount))
        .route("/entries/:id/void", post(void_entry))
        .route("/entries/:id/confirm", post(confirm_entry))
}

async fn record_entry(
    State(state): State<AppState>,
    Json(req): Json<RecordEntryRequest>,
) -> Result<Json<Entry>, AppError> {
    require_non_negative(req.amount)?;
    let entry = state
        .with_ledger(move |ledger| {
            ledger.record_manual_entry(
                req.account_id,
                req.date,
                req.kind.signed(req.amount),
                &req.description,
            )
        })
        .await?;
    Ok(Json(entry))
}

async fn list_entries(
    State(state): State<AppState>,
    Query(q): Query<AccountIdQuery>,
) -> Result<Json<Vec<Entry>>, AppError> {
    let entries = state
        .with_ledger(move |ledger| ledger.entries(q.account_id))
        .await?;
    Ok(Json(entries))
}

async fn update_metadata(
    State(state): State<AppState>,
    Path(id): Path<EntryId>,
    Json(req): Json<UpdateEntryMetadataRequest>,
) -> Result<Json<Entry>, AppError> {
    let entry = state
        .with_ledger(move |ledger| {
            ledger.update_entry_metadata(
                id,
                EntryMetadata {
                    category: req.category,
                    tags: req.tags,
                    note: req.note,
                    pot_id: req.pot_id,
                },
            )
        })
        .await?;
    Ok(Json(entry))
}

async fn edit_amount(
    State(state): State<AppState>,
    Path(id): Path<EntryId>,
    Json(req): Json<EditAmountRequest>,
) -> Result<Json<Entry>, AppError> {
    require_non_negative(req.amount)?;
    let entry = state
        .with_ledger(move |ledger| ledger.edit_manual_entry_amount(id, req.kind.signed(req.amount)))
        .await?;
    Ok(Json(entry))
}

async fn void_entry(
    State(state): State<AppState>,
    Path(id): Path<EntryId>,
    Json(req): Json<VoidRequest>,
) -> Result<Json<Entry>, AppError> {
    let entry = state
        .with_ledger(move |ledger| ledger.void_entry(id, &req.reason))
        .await?;
    Ok(Json(entry))
}

async fn confirm_entry(
    State(state): State<AppState>,
    Path(id): Path<EntryId>,
) -> Result<Json<Entry>, AppError> {
    let entry = state
        .with_ledger(move |ledger| ledger.confirm_entry(id))
        .await?;
    Ok(Json(entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use ledger_core::{AccountKind, Currency, SqliteStore};
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState::new(SqliteStore::open_in_memory().unwrap())
    }

    fn an_account(state: &AppState) -> AccountId {
        state
            .ledger
            .lock()
            .unwrap()
            .open_account(
                "Checking",
                Currency::new("EUR").unwrap(),
                AccountKind::Own,
                rust_decimal_macros::dec!(0),
            )
            .unwrap()
            .id
    }

    async fn post_json(
        state: AppState,
        uri: &str,
        body: serde_json::Value,
    ) -> axum::response::Response {
        app(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn recording_an_expense_stores_it_negative() {
        let state = test_state();
        let account_id = an_account(&state);
        let response = post_json(
            state,
            "/entries",
            serde_json::json!({"account_id": account_id, "date": "2026-01-15", "kind": "expense", "amount": "20", "description": "Groceries"}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let entry: Entry = serde_json::from_slice(&body).unwrap();
        assert_eq!(entry.amount, rust_decimal_macros::dec!(-20));
    }

    #[tokio::test]
    async fn a_manually_recorded_entry_goes_into_the_ledger_and_moves_the_balance() {
        let state = test_state();
        let account_id = an_account(&state);
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": account_id, "date": "2026-01-15", "kind": "income", "amount": "75", "description": "Salary"}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let recorded: Entry = serde_json::from_slice(&body).unwrap();

        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.entries(account_id).unwrap(), vec![recorded]);
        assert_eq!(
            ledger.account_balance(account_id),
            Ok(rust_decimal_macros::dec!(75))
        );
    }

    #[tokio::test]
    async fn a_negative_amount_in_the_request_is_a_bad_request() {
        let state = test_state();
        let account_id = an_account(&state);
        let response = post_json(
            state,
            "/entries",
            serde_json::json!({"account_id": account_id, "date": "2026-01-15", "kind": "expense", "amount": "-20", "description": "x"}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn listing_entries_filters_by_account() {
        let state = test_state();
        let a = an_account(&state);
        post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": a, "date": "2026-01-15", "kind": "expense", "amount": "1", "description": "x"}),
        )
        .await;
        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/entries?account_id={a}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let entries: Vec<Entry> = serde_json::from_slice(&body).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn editing_the_amount_of_a_confirmed_entry_is_unprocessable() {
        let state = test_state();
        let account_id = an_account(&state);
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": account_id, "date": "2026-01-15", "kind": "expense", "amount": "20", "description": "x"}),
        )
        .await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let entry: Entry = serde_json::from_slice(&body).unwrap();
        post_json(
            state.clone(),
            &format!("/entries/{}/confirm", entry.id),
            serde_json::json!({}),
        )
        .await;
        let response = app(state)
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/entries/{}/amount", entry.id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"kind": "expense", "amount": "5"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn voiding_an_entry_then_editing_it_is_unprocessable() {
        let state = test_state();
        let account_id = an_account(&state);
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": account_id, "date": "2026-01-15", "kind": "expense", "amount": "20", "description": "x"}),
        )
        .await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let entry: Entry = serde_json::from_slice(&body).unwrap();
        post_json(
            state.clone(),
            &format!("/entries/{}/void", entry.id),
            serde_json::json!({"reason": "typed the wrong amount"}),
        )
        .await;
        let response = app(state)
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/entries/{}", entry.id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"category": "Groceries"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn metadata_can_be_set_then_cleared_back_to_none() {
        let state = test_state();
        let account_id = an_account(&state);
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": account_id, "date": "2026-01-15", "kind": "expense", "amount": "20", "description": "x"}),
        )
        .await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let entry: Entry = serde_json::from_slice(&body).unwrap();

        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/entries/{}", entry.id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"category": "Groceries", "tags": ["a"], "note": "n"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let set: Entry = serde_json::from_slice(&body).unwrap();
        assert_eq!(set.category, Some("Groceries".to_string()));
        assert_eq!(set.tags, vec!["a".to_string()]);
        assert_eq!(set.note, Some("n".to_string()));

        let response = app(state)
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/entries/{}", entry.id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"category": null, "tags": [], "note": null, "pot_id": null}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let cleared: Entry = serde_json::from_slice(&body).unwrap();
        assert_eq!(cleared.category, None);
        assert!(cleared.tags.is_empty());
        assert_eq!(cleared.note, None);
        assert_eq!(cleared.pot_id, None);
    }
}
