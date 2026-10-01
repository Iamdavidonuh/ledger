use crate::error::{AppError, AppJson};
use crate::state::WithLedger;
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

pub fn router<S: WithLedger>() -> Router<S> {
    Router::new().route(
        "/accounts/:id/current-value",
        post(update_current_value::<S>),
    )
}

async fn update_current_value<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<AccountId>,
    AppJson(req): AppJson<UpdateValueRequest>,
) -> Result<Json<Valuation>, AppError> {
    let valuation = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                ledger
                    .update_current_value(id, req.new_value, &req.category, req.date)
                    .await
            })
        })
        .await?;
    Ok(Json(valuation))
}
