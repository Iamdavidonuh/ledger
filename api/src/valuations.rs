use crate::error::AppError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use chrono::NaiveDate;
use ledger_core::Valuation;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct UpdateValueRequest {
    pub new_value: Decimal,
    pub category: String,
    pub date: NaiveDate,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/accounts/:id/current-value", post(update_current_value))
}

async fn update_current_value(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateValueRequest>,
) -> Result<Json<Valuation>, AppError> {
    let mut ledger = state
        .ledger
        .lock()
        .map_err(|_| AppError::internal("the ledger lock was poisoned"))?;
    let valuation = ledger.update_current_value(id, req.new_value, &req.category, req.date)?;
    Ok(Json(valuation))
}
