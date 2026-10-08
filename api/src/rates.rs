use crate::error::AppError;
use crate::state::WithRates;
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use ledger_core::{Currency, ExchangeRate};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct GetRateQuery {
    pub from: String,
    pub to: String,
}

#[derive(serde::Serialize)]
pub struct RateResponse {
    pub from: String,
    pub to: String,
    pub rate: String,
    pub fetched_at: String,
}

pub fn router<S: WithRates>() -> Router<S> {
    Router::new()
        .route("/rates", get(get_rate::<S>))
        .route("/rates/refresh", post(refresh_rates::<S>))
}

async fn get_rate<S: WithRates>(
    State(state): State<S>,
    Query(query): Query<GetRateQuery>,
) -> Result<Json<RateResponse>, AppError> {
    let from = Currency::new(&query.from)
        .map_err(|e| AppError::BadRequest(format!("Invalid from currency: {}", e)))?;
    let to = Currency::new(&query.to)
        .map_err(|e| AppError::BadRequest(format!("Invalid to currency: {}", e)))?;

    let rate = state.get_rate(&from, &to).await?;

    Ok(Json(RateResponse {
        from: rate.from.code().to_string(),
        to: rate.to.code().to_string(),
        rate: rate.rate.to_string(),
        fetched_at: rate.fetched_at.to_rfc3339(),
    }))
}

async fn refresh_rates<S: WithRates>(State(state): State<S>) -> Result<(), AppError> {
    state.clear_rate_cache().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::TestState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        router::<TestState>().with_state(TestState::new())
    }

    #[tokio::test]
    async fn get_rate_returns_400_for_invalid_currency() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/rates?from=INVALID_CODE_TOO_LONG&to=USD")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn refresh_rates_clears_cache() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/rates/refresh")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
