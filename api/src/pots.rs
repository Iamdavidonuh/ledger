use crate::error::AppError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::NaiveDate;
use ledger_core::{Currency, Pot};
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct OpenPotRequest {
    pub name: String,
    pub currency: String,
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
        .route("/pots/:id", get(get_pot))
        .route("/pots/:id/allocations", post(allocate))
}

async fn open_pot(State(state): State<AppState>, Json(req): Json<OpenPotRequest>) -> Result<Json<Pot>, AppError> {
    let currency = Currency::new(&req.currency).map_err(|e| AppError::bad_request(e.to_string()))?;
    let mut ledger = state.ledger.lock().expect("the ledger mutex should not be poisoned");
    let pot = ledger.open_pot(&req.name, currency, req.target, req.priority);
    Ok(Json(pot))
}

async fn list_pots(State(state): State<AppState>) -> Json<Vec<PotWithBalance>> {
    let ledger = state.ledger.lock().expect("the ledger mutex should not be poisoned");
    let with_balances = ledger
        .pots()
        .into_iter()
        .map(|pot| {
            let balance = ledger.pot_balance(pot.id).expect("an id from pots() always exists");
            PotWithBalance { pot, balance }
        })
        .collect();
    Json(with_balances)
}

async fn get_pot(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<PotWithBalance>, AppError> {
    let ledger = state.ledger.lock().expect("the ledger mutex should not be poisoned");
    let balance = ledger.pot_balance(id)?;
    let pot = ledger.pots().into_iter().find(|p| p.id == id).expect("pot_balance succeeded, so this id exists");
    Ok(Json(PotWithBalance { pot, balance }))
}

async fn allocate(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<AllocateRequest>,
) -> Result<Json<PotWithBalance>, AppError> {
    let mut ledger = state.ledger.lock().expect("the ledger mutex should not be poisoned");
    ledger.allocate_to_pot(id, req.amount, req.date)?;
    let balance = ledger.pot_balance(id)?;
    let pot = ledger.pots().into_iter().find(|p| p.id == id).expect("allocate_to_pot succeeded, so this id exists");
    Ok(Json(PotWithBalance { pot, balance }))
}
