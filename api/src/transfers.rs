use crate::error::{AppError, AppJson};
use crate::state::AppState;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use chrono::NaiveDate;
use ledger_core::{AccountId, Entry, Transfer, TransferLeg};
use rust_decimal::Decimal;

#[derive(serde::Deserialize)]
pub struct TransferRequest {
    pub from_account_id: AccountId,
    pub to_account_id: AccountId,
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
    AppJson(req): AppJson<TransferRequest>,
) -> Result<Json<TransferResponse>, AppError> {
    let (out_entry, in_entry) = state
        .with_ledger(move |ledger| {
            ledger.transfer(Transfer::new(
                TransferLeg::new(req.from_account_id, req.amount_sent),
                TransferLeg::new(req.to_account_id, req.amount_received),
                req.date,
                req.description,
            ))
        })
        .await?;
    Ok(Json(TransferResponse {
        out_entry,
        in_entry,
    }))
}
