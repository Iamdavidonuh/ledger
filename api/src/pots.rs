use crate::error::{AppError, AppJson};
use crate::state::WithLedger;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::NaiveDate;
use ledger_core::{Currency, LedgerError, Pot, PotId};
use rust_decimal::Decimal;

#[derive(serde::Deserialize)]
pub struct OpenPotRequest {
    pub name: String,
    pub currency: Currency,
    pub target: Option<Decimal>,
    pub priority: Option<i32>,
}

#[derive(serde::Serialize)]
pub struct PotWithBalance {
    #[serde(flatten)]
    pub pot: Pot,
    pub balance: Decimal,
}

#[derive(serde::Deserialize)]
pub struct AllocateRequest {
    pub amount: Decimal,
    pub date: NaiveDate,
}

pub fn router<S: WithLedger>() -> Router<S> {
    Router::new()
        .route("/pots", post(open_pot::<S>).get(list_pots::<S>))
        .route("/pots/:id", get(get_pot::<S>).delete(delete_pot::<S>))
        .route("/pots/:id/allocations", post(allocate::<S>))
}

async fn open_pot<S: WithLedger>(
    State(state): State<S>,
    AppJson(req): AppJson<OpenPotRequest>,
) -> Result<Json<Pot>, AppError> {
    let pot = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                ledger
                    .open_pot(&req.name, req.currency, req.target, req.priority)
                    .await
            })
        })
        .await?;
    Ok(Json(pot))
}

async fn list_pots<S: WithLedger>(
    State(state): State<S>,
) -> Result<Json<Vec<PotWithBalance>>, AppError> {
    let with_balances = state
        .with_ledger(|ledger| {
            Box::pin(async move {
                let pots = ledger.pots().await?;
                let mut result = Vec::with_capacity(pots.len());
                for pot in pots {
                    let balance = ledger.pot_balance(pot.id).await?;
                    result.push(PotWithBalance { pot, balance });
                }
                Ok(result)
            })
        })
        .await?;
    Ok(Json(with_balances))
}

async fn get_pot<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<PotId>,
) -> Result<Json<PotWithBalance>, AppError> {
    let with_balance = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                let pot = ledger.pot(id).await?.ok_or(LedgerError::PotNotFound(id))?;
                let balance = ledger.pot_balance(id).await?;
                Ok(PotWithBalance { pot, balance })
            })
        })
        .await?;
    Ok(Json(with_balance))
}

async fn delete_pot<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<PotId>,
) -> Result<StatusCode, AppError> {
    state
        .with_ledger(move |ledger| Box::pin(async move { ledger.delete_pot(id).await }))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn allocate<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<PotId>,
    AppJson(req): AppJson<AllocateRequest>,
) -> Result<Json<PotWithBalance>, AppError> {
    let with_balance = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                ledger.allocate_to_pot(id, req.amount, req.date).await?;
                let pot = ledger.pot(id).await?.ok_or(LedgerError::PotNotFound(id))?;
                let balance = ledger.pot_balance(id).await?;
                Ok(PotWithBalance { pot, balance })
            })
        })
        .await?;
    Ok(Json(with_balance))
}
