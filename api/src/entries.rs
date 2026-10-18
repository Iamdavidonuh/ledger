use crate::error::{AppError, AppJson};
use crate::state::WithLedger;
use axum::extract::{Path, Query, State};
use axum::routing::{get, patch, post};
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
    pub transfer_account_id: Option<AccountId>,
    #[serde(default)]
    pub clear_transfer: bool,
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

#[derive(serde::Deserialize)]
pub struct SplitEntryRequest {
    pub parts: Vec<SplitPart>,
}

#[derive(serde::Deserialize)]
pub struct SplitPart {
    pub amount: Decimal,
    pub category: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub struct SplitPartResponse {
    pub id: String,
    pub amount: String,
    pub category: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct MarkAsTransferRequest {
    pub transfer_account_id: AccountId,
}

pub fn router<S: WithLedger>() -> Router<S> {
    Router::new()
        .route("/entries", post(record_entry::<S>).get(list_entries::<S>))
        .route("/entries/:id", patch(update_metadata::<S>))
        .route("/entries/:id/amount", patch(edit_amount::<S>))
        .route("/entries/:id/void", post(void_entry::<S>))
        .route("/entries/:id/revert", post(revert_entry::<S>))
        .route("/entries/:id/confirm", post(confirm_entry::<S>))
        .route("/entries/:id/split", post(split_entry::<S>))
        .route("/entries/:id/parts", get(get_entry_parts::<S>))
        .route("/entries/:id/mark-as-transfer", post(mark_as_transfer::<S>))
}

async fn record_entry<S: WithLedger>(
    State(state): State<S>,
    AppJson(req): AppJson<RecordEntryRequest>,
) -> Result<Json<Entry>, AppError> {
    require_non_negative(req.amount)?;
    let entry = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                ledger
                    .record_manual_entry(
                        req.account_id,
                        req.date,
                        req.kind.signed(req.amount),
                        &req.description,
                    )
                    .await
            })
        })
        .await?;
    Ok(Json(entry))
}

async fn list_entries<S: WithLedger>(
    State(state): State<S>,
    Query(q): Query<AccountIdQuery>,
) -> Result<Json<Vec<Entry>>, AppError> {
    let entries = state
        .with_ledger(move |ledger| Box::pin(async move { ledger.entries(q.account_id).await }))
        .await?;
    Ok(Json(entries))
}

async fn update_metadata<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<EntryId>,
    AppJson(req): AppJson<UpdateEntryMetadataRequest>,
) -> Result<Json<Entry>, AppError> {
    let entry = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                let entry = ledger
                    .update_entry_metadata(
                        id,
                        EntryMetadata {
                            category: req.category,
                            tags: req.tags,
                            note: req.note,
                            pot_id: req.pot_id,
                        },
                    )
                    .await?;
                // Transfer changes are applied after metadata so both can be
                // updated in one request. clear_transfer wins over a new id.
                if req.clear_transfer {
                    ledger.unmark_entry_transfer(id).await
                } else if let Some(transfer_account_id) = req.transfer_account_id {
                    ledger.mark_entry_as_transfer(id, transfer_account_id).await
                } else {
                    Ok(entry)
                }
            })
        })
        .await?;
    Ok(Json(entry))
}

async fn edit_amount<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<EntryId>,
    AppJson(req): AppJson<EditAmountRequest>,
) -> Result<Json<Entry>, AppError> {
    require_non_negative(req.amount)?;
    let entry = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                ledger
                    .edit_manual_entry_amount(id, req.kind.signed(req.amount))
                    .await
            })
        })
        .await?;
    Ok(Json(entry))
}

async fn void_entry<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<EntryId>,
    AppJson(req): AppJson<VoidRequest>,
) -> Result<Json<Entry>, AppError> {
    let entry = state
        .with_ledger(move |ledger| {
            Box::pin(async move { ledger.void_entry(id, &req.reason).await })
        })
        .await?;
    Ok(Json(entry))
}

async fn revert_entry<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<EntryId>,
) -> Result<Json<Entry>, AppError> {
    let entry = state
        .with_ledger(move |ledger| Box::pin(async move { ledger.revert_entry(id).await }))
        .await?;
    Ok(Json(entry))
}

async fn confirm_entry<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<EntryId>,
) -> Result<Json<Entry>, AppError> {
    let entry = state
        .with_ledger(move |ledger| Box::pin(async move { ledger.confirm_entry(id).await }))
        .await?;
    Ok(Json(entry))
}

async fn split_entry<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<EntryId>,
    AppJson(req): AppJson<SplitEntryRequest>,
) -> Result<Json<Vec<SplitPartResponse>>, AppError> {
    let parts: Vec<(Decimal, Option<String>)> = req
        .parts
        .into_iter()
        .map(|p| (p.amount, p.category))
        .collect();
    let entry_parts = state
        .with_ledger(move |ledger| {
            Box::pin(async move { ledger.split_entry(id, parts).await })
        })
        .await?;
    
    let response = entry_parts
        .into_iter()
        .map(|p| SplitPartResponse {
            id: p.id.to_string(),
            amount: p.amount.to_string(),
            category: p.category,
        })
        .collect();
    
    Ok(Json(response))
}

async fn get_entry_parts<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<EntryId>,
) -> Result<Json<Vec<SplitPartResponse>>, AppError> {
    let parts = state
        .with_ledger(move |ledger| Box::pin(async move { ledger.parts_for_entry(id).await }))
        .await?;
    
    let response = parts
        .into_iter()
        .map(|p| SplitPartResponse {
            id: p.id.to_string(),
            amount: p.amount.to_string(),
            category: p.category,
        })
        .collect();
    
    Ok(Json(response))
}

async fn mark_as_transfer<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<EntryId>,
    AppJson(req): AppJson<MarkAsTransferRequest>,
) -> Result<Json<Entry>, AppError> {
    let entry = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                ledger
                    .mark_entry_as_transfer(id, req.transfer_account_id)
                    .await
            })
        })
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
    use ledger_core::{AccountKind, BankState, Currency};
    use tower::ServiceExt;

    fn test_state() -> crate::state::TestState {
        crate::state::TestState::new()
    }

    async fn an_account(state: &crate::state::TestState) -> AccountId {
        state
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
            .id
    }

    async fn post_json(
        state: crate::state::TestState,
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
        let account_id = an_account(&state).await;
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
        let account_id = an_account(&state).await;
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": account_id, "date": "2026-01-15", "kind": "income", "amount": "75", "description": "Salary"}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let recorded: Entry = serde_json::from_slice(&body).unwrap();

        let mut ledger = state.ledger.lock().await;
        assert_eq!(ledger.entries(account_id).await.unwrap(), vec![recorded]);
        assert_eq!(
            ledger.account_balance(account_id).await,
            Ok(rust_decimal_macros::dec!(75))
        );
    }

    #[tokio::test]
    async fn a_negative_amount_in_the_request_is_a_bad_request() {
        let state = test_state();
        let account_id = an_account(&state).await;
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
        let a = an_account(&state).await;
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
        let account_id = an_account(&state).await;
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
        let account_id = an_account(&state).await;
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

    async fn a_recorded_entry(state: &crate::state::TestState, account_id: AccountId) -> Entry {
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": account_id, "date": "2026-01-15", "kind": "expense", "amount": "20", "description": "x"}),
        )
        .await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn reverting_an_entry_marks_it_reverted_and_returns_it_to_the_balance() {
        let state = test_state();
        let account_id = an_account(&state).await;
        let entry = a_recorded_entry(&state, account_id).await;
        let response = post_json(
            state.clone(),
            &format!("/entries/{}/revert", entry.id),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let reverted: Entry = serde_json::from_slice(&body).unwrap();
        assert_eq!(reverted.bank_state, BankState::Reverted);
        assert_eq!(
            state.ledger.lock().await.account_balance(account_id).await,
            Ok(rust_decimal_macros::dec!(0))
        );
    }

    #[tokio::test]
    async fn reverting_an_entry_twice_is_unprocessable() {
        let state = test_state();
        let account_id = an_account(&state).await;
        let entry = a_recorded_entry(&state, account_id).await;
        let uri = format!("/entries/{}/revert", entry.id);
        post_json(state.clone(), &uri, serde_json::json!({})).await;
        let response = post_json(state, &uri, serde_json::json!({})).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn reverting_a_voided_entry_is_unprocessable() {
        let state = test_state();
        let account_id = an_account(&state).await;
        let entry = a_recorded_entry(&state, account_id).await;
        post_json(
            state.clone(),
            &format!("/entries/{}/void", entry.id),
            serde_json::json!({"reason": "typed the wrong amount"}),
        )
        .await;
        let response = post_json(
            state,
            &format!("/entries/{}/revert", entry.id),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn reverting_a_missing_entry_is_not_found() {
        let response = post_json(
            test_state(),
            &format!("/entries/{}/revert", EntryId::generate()),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn metadata_can_be_set_then_cleared_back_to_none() {
        let state = test_state();
        let account_id = an_account(&state).await;
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

    #[tokio::test]
    async fn splitting_an_entry_into_parts_stores_them_and_they_can_be_retrieved() {
        let state = test_state();
        let account_id = an_account(&state).await;
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": account_id, "date": "2026-01-15", "kind": "expense", "amount": "100", "description": "Groceries"}),
        )
        .await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let entry: Entry = serde_json::from_slice(&body).unwrap();

        let response = post_json(
            state.clone(),
            &format!("/entries/{}/split", entry.id),
            serde_json::json!({"parts": [
                {"amount": "-60", "category": "Food"},
                {"amount": "-40", "category": "Household"}
            ]}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let parts: Vec<SplitPartResponse> = serde_json::from_slice(&body).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].amount, "-60");
        assert_eq!(parts[0].category, Some("Food".to_string()));
        assert_eq!(parts[1].amount, "-40");
        assert_eq!(parts[1].category, Some("Household".to_string()));

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/entries/{}/parts", entry.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let retrieved: Vec<SplitPartResponse> = serde_json::from_slice(&body).unwrap();
        assert_eq!(retrieved.len(), parts.len());
        assert_eq!(retrieved[0].amount, parts[0].amount);
        assert_eq!(retrieved[1].amount, parts[1].amount);
    }

    #[tokio::test]
    async fn splitting_a_voided_entry_is_unprocessable() {
        let state = test_state();
        let account_id = an_account(&state).await;
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": account_id, "date": "2026-01-15", "kind": "expense", "amount": "100", "description": "x"}),
        )
        .await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let entry: Entry = serde_json::from_slice(&body).unwrap();

        post_json(
            state.clone(),
            &format!("/entries/{}/void", entry.id),
            serde_json::json!({"reason": "test"}),
        )
        .await;

        let response = post_json(
            state,
            &format!("/entries/{}/split", entry.id),
            serde_json::json!({"parts": [{"amount": "-100", "category": null}]}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn splitting_with_parts_that_dont_sum_to_entry_amount_is_unprocessable() {
        let state = test_state();
        let account_id = an_account(&state).await;
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": account_id, "date": "2026-01-15", "kind": "expense", "amount": "100", "description": "x"}),
        )
        .await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let entry: Entry = serde_json::from_slice(&body).unwrap();

        let response = post_json(
            state,
            &format!("/entries/{}/split", entry.id),
            serde_json::json!({"parts": [
                {"amount": "-60", "category": null},
                {"amount": "-30", "category": null}
            ]}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn marking_an_entry_as_transfer_links_it_to_another_account() {
        let state = test_state();
        let checking = an_account(&state).await;
        let savings = an_account(&state).await;
        
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": checking, "date": "2026-01-15", "kind": "expense", "amount": "100", "description": "Transfer to savings"}),
        )
        .await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let entry: Entry = serde_json::from_slice(&body).unwrap();
        assert_eq!(entry.transfer_account_id, None);

        let response = post_json(
            state,
            &format!("/entries/{}/mark-as-transfer", entry.id),
            serde_json::json!({"transfer_account_id": savings}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let updated: Entry = serde_json::from_slice(&body).unwrap();
        assert_eq!(updated.transfer_account_id, Some(savings));
    }

    #[tokio::test]
    async fn marking_a_voided_entry_as_transfer_is_unprocessable() {
        let state = test_state();
        let checking = an_account(&state).await;
        let savings = an_account(&state).await;
        
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": checking, "date": "2026-01-15", "kind": "expense", "amount": "100", "description": "x"}),
        )
        .await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let entry: Entry = serde_json::from_slice(&body).unwrap();

        post_json(
            state.clone(),
            &format!("/entries/{}/void", entry.id),
            serde_json::json!({"reason": "mistake"}),
        )
        .await;

        let response = post_json(
            state,
            &format!("/entries/{}/mark-as-transfer", entry.id),
            serde_json::json!({"transfer_account_id": savings}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn marking_entry_as_transfer_to_same_account_is_unprocessable() {
        let state = test_state();
        let account = an_account(&state).await;
        
        let response = post_json(
            state.clone(),
            "/entries",
            serde_json::json!({"account_id": account, "date": "2026-01-15", "kind": "expense", "amount": "100", "description": "x"}),
        )
        .await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let entry: Entry = serde_json::from_slice(&body).unwrap();

        let response = post_json(
            state,
            &format!("/entries/{}/mark-as-transfer", entry.id),
            serde_json::json!({"transfer_account_id": account}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
