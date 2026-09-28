use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use importer::ImportError;
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
    /// A statement file that failed to parse or balance. It comes from the
    /// importer, outside any ledger call, so it is never a LedgerError.
    Import(ImportError),
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

impl From<ImportError> for AppError {
    fn from(e: ImportError) -> Self {
        AppError::Import(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, json!({ "error": message })),
            AppError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": message })),
            AppError::Ledger(
                e @ (LedgerError::AccountNotFound(_)
                | LedgerError::EntryNotFound(_)
                | LedgerError::PotNotFound(_)
                | LedgerError::ImportNotFound(_)
                | LedgerError::QueueRowNotFound(_)),
            ) => (StatusCode::NOT_FOUND, json!({ "error": e.to_string() })),
            AppError::Ledger(e @ LedgerError::Storage(_)) => {
                (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": e.to_string() }))
            }
            AppError::Ledger(
                e @ LedgerError::BulkAcceptBlocked { suspicious_count, reverted_candidate_count },
            ) => (
                StatusCode::CONFLICT,
                json!({
                    "error": e.to_string(),
                    "suspicious_count": suspicious_count,
                    "reverted_candidate_count": reverted_candidate_count,
                }),
            ),
            AppError::Ledger(
                e @ (LedgerError::IncompleteImportExists | LedgerError::NoSuggestedMatch | LedgerError::WrongQueueRowKind),
            ) => (StatusCode::CONFLICT, json!({ "error": e.to_string() })),
            AppError::Ledger(e) => (StatusCode::UNPROCESSABLE_ENTITY, json!({ "error": e.to_string() })),
            AppError::Import(e @ ImportError::BalanceCheckFailed { expected, actual }) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({ "error": e.to_string(), "expected": expected, "actual": actual }),
            ),
            AppError::Import(e) => (StatusCode::UNPROCESSABLE_ENTITY, json!({ "error": e.to_string() })),
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    async fn respond(e: AppError) -> (StatusCode, serde_json::Value) {
        let response = e.into_response();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn import_and_queue_row_not_found_are_404() {
        for e in [LedgerError::ImportNotFound(Uuid::new_v4()), LedgerError::QueueRowNotFound(Uuid::new_v4())] {
            assert_eq!(respond(AppError::from(e)).await.0, StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn review_queue_conflicts_are_409() {
        for e in [LedgerError::IncompleteImportExists, LedgerError::NoSuggestedMatch, LedgerError::WrongQueueRowKind] {
            assert_eq!(respond(AppError::from(e)).await.0, StatusCode::CONFLICT);
        }
    }

    #[tokio::test]
    async fn a_blocked_bulk_accept_is_409_with_its_counts_on_the_body() {
        let (status, body) = respond(AppError::from(LedgerError::BulkAcceptBlocked {
            suspicious_count: 2,
            reverted_candidate_count: 1,
        }))
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["suspicious_count"], 2);
        assert_eq!(body["reverted_candidate_count"], 1);
        assert!(body["error"].is_string());
    }

    #[tokio::test]
    async fn a_transfer_to_self_is_422() {
        assert_eq!(
            respond(AppError::from(LedgerError::TransferToSelfNotAllowed)).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[tokio::test]
    async fn an_import_error_is_422() {
        for e in [
            ImportError::Parse("bad".to_string()),
            ImportError::PdfToText("missing".to_string()),
            ImportError::NotAStatement,
        ] {
            assert_eq!(respond(AppError::from(e)).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        }
    }

    #[tokio::test]
    async fn a_failed_balance_check_carries_both_figures_as_fields() {
        let (status, body) =
            respond(AppError::from(ImportError::BalanceCheckFailed { expected: dec!(90.00), actual: dec!(88.00) })).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["expected"], "90.00");
        assert_eq!(body["actual"], "88.00");
        assert!(body["error"].is_string());
    }
}
