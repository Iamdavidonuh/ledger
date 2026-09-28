use crate::error::AppError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use chrono::NaiveDate;
use ledger_core::{AccountId, Valuation};
use rust_decimal::Decimal;

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
    Path(id): Path<AccountId>,
    Json(req): Json<UpdateValueRequest>,
) -> Result<Json<Valuation>, AppError> {
    let valuation = state
        .with_ledger(move |ledger| {
            ledger.update_current_value(id, req.new_value, &req.category, req.date)
        })
        .await?;
    Ok(Json(valuation))
}
