use crate::error::AppError;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use chrono::NaiveDate;
use ledger_core::Entry;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct TransferRequest {
    pub from_account_id: Uuid,
    pub to_account_id: Uuid,
    pub date: NaiveDate,
    pub amount_sent: Decimal,
    pub amount_received: Decimal,
    pub description: String,
}

#[derive(serde::Serialize)]
pub struct TransferResponse {
    pub out_entry: Entry,
    pub in_entry: Entry,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/transfers", post(create_transfer))
}

async fn create_transfer(
    State(state): State<AppState>,
    Json(req): Json<TransferRequest>,
) -> Result<Json<TransferResponse>, AppError> {
    let mut ledger = state
        .ledger
        .lock()
        .map_err(|_| AppError::internal("the ledger lock was poisoned"))?;
    let (out_entry, in_entry) = ledger.transfer(
        req.from_account_id,
        req.to_account_id,
        req.date,
        req.amount_sent,
        req.amount_received,
        &req.description,
    )?;
    Ok(Json(TransferResponse { out_entry, in_entry }))
}
