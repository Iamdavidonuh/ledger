#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod accounts;
pub mod cors;
mod entries;
pub mod error;
mod import_matching;
mod imports;
mod pots;
mod rates;
pub mod state;
mod transfers;
mod valuations;

use axum::Router;
use state::{WithLedger, WithRates};

pub fn app<S: WithLedger + WithRates>(state: S) -> Router {
    Router::new()
        .route("/health", axum::routing::get(health))
        .merge(accounts::router::<S>())
        .merge(entries::router::<S>())
        .merge(imports::router::<S>())
        .merge(pots::router::<S>())
        .merge(rates::router::<S>())
        .merge(transfers::router::<S>())
        .merge(valuations::router::<S>())
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
