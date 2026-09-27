use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use ledger_core::LedgerError;
use serde_json::json;

/// One error type for every handler in this crate. A handler either hits a
/// LedgerError (converted automatically via `?`, since Ledger's own rules
/// decide not-found versus a blocked action), rejects malformed input
/// itself before ever calling the Ledger via `AppError::bad_request`, or
/// hits a genuine internal failure (the mutex was poisoned by an earlier
/// panic) via `AppError::internal`. Nothing in this crate panics or
/// `.expect()`s on a request; every failure path returns a response.
pub enum AppError {
    BadRequest(String),
    Internal(String),
    Ledger(LedgerError),
}

impl AppError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        AppError::BadRequest(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        AppError::Internal(message.into())
    }
}

impl From<LedgerError> for AppError {
    fn from(e: LedgerError) -> Self {
        AppError::Ledger(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            AppError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
            AppError::Ledger(
                e @ (LedgerError::AccountNotFound(_) | LedgerError::EntryNotFound(_) | LedgerError::PotNotFound(_)),
            ) => (StatusCode::NOT_FOUND, e.to_string()),
            AppError::Ledger(e) => (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
