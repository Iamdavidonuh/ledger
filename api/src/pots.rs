use crate::error::{AppError, AppJson};
use crate::state::AppState;
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

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pots", post(open_pot).get(list_pots))
        .route("/pots/:id", get(get_pot).delete(delete_pot))
        .route("/pots/:id/allocations", post(allocate))
}

async fn open_pot(
    State(state): State<AppState>,
    AppJson(req): AppJson<OpenPotRequest>,
) -> Result<Json<Pot>, AppError> {
    let pot = state
        .with_ledger(move |ledger| {
            ledger.open_pot(&req.name, req.currency, req.target, req.priority)
        })
        .await?;
    Ok(Json(pot))
}

async fn list_pots(State(state): State<AppState>) -> Result<Json<Vec<PotWithBalance>>, AppError> {
    let with_balances = state
        .with_ledger(|ledger| {
            ledger
                .pots()?
                .into_iter()
                .map(|pot| {
                    let balance = ledger.pot_balance(pot.id)?;
                    Ok(PotWithBalance { pot, balance })
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .await?;
    Ok(Json(with_balances))
}

async fn get_pot(
    State(state): State<AppState>,
    Path(id): Path<PotId>,
) -> Result<Json<PotWithBalance>, AppError> {
    let with_balance = state
        .with_ledger(move |ledger| {
            let pot = ledger.pot(id)?.ok_or(LedgerError::PotNotFound(id))?;
            let balance = ledger.pot_balance(id)?;
            Ok(PotWithBalance { pot, balance })
        })
        .await?;
    Ok(Json(with_balance))
}

async fn delete_pot(
    State(state): State<AppState>,
    Path(id): Path<PotId>,
) -> Result<StatusCode, AppError> {
    state
        .with_ledger(move |ledger| ledger.delete_pot(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn allocate(
    State(state): State<AppState>,
    Path(id): Path<PotId>,
    AppJson(req): AppJson<AllocateRequest>,
) -> Result<Json<PotWithBalance>, AppError> {
    let with_balance = state
        .with_ledger(move |ledger| {
            ledger.allocate_to_pot(id, req.amount, req.date)?;
            let pot = ledger.pot(id)?.ok_or(LedgerError::PotNotFound(id))?;
            let balance = ledger.pot_balance(id)?;
            Ok(PotWithBalance { pot, balance })
        })
        .await?;
    Ok(Json(with_balance))
}
