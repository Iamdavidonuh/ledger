//! The bank statement import endpoints. Parsing happens here via the
//! importer crate and matching via import_matching; every write goes
//! through one Ledger method, which owns its own transaction.

use crate::error::{AppError, AppJson};
use crate::import_matching::{build_queue, new_import};
use crate::state::WithLedger;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use importer::{BankA, BankB, ImportError, Importer, ParseResult};
use ledger_core::{
    AccountId, Entry, Import, ImportId, ImportQueueRow, LedgerError, QueueRowId, QueueRowView,
};
use rust_decimal::Decimal;
use std::str::FromStr;

/// Axum's 2 MB default is too small for a full year's CSV export. This is
/// a starting figure, to be confirmed against the deployment's memory limit.
const UPLOAD_LIMIT_BYTES: usize = 10 * 1024 * 1024;

#[derive(Clone, Copy)]
enum BankType {
    BankA,
    BankB,
}

impl FromStr for BankType {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, AppError> {
        match value {
            "BankA" => Ok(BankType::BankA),
            "BankB" => Ok(BankType::BankB),
            other => Err(AppError::bad_request(format!(
                "bank_type must be BankA or BankB, not {other:?}"
            ))),
        }
    }
}

impl BankType {
    fn read(self, bytes: &[u8]) -> Result<ParseResult, ImportError> {
        match self {
            BankType::BankA => Importer::new(BankA).parse(bytes),
            BankType::BankB => Importer::new(BankB).parse(bytes),
        }
    }
}

struct Upload {
    account_id: AccountId,
    bank_type: BankType,
    file_name: String,
    bytes: Vec<u8>,
}

async fn read_upload(mut multipart: Multipart) -> Result<Upload, AppError> {
    let bad = |e: axum::extract::multipart::MultipartError| AppError::bad_request(e.to_string());
    let (mut account_id, mut bank_type, mut file) = (None, None, None);
    while let Some(field) = multipart.next_field().await.map_err(bad)? {
        match field.name() {
            Some("account_id") => {
                let text = field.text().await.map_err(bad)?;
                account_id = Some(
                    text.trim()
                        .parse::<AccountId>()
                        .map_err(|_| AppError::bad_request("account_id must be a UUID"))?,
                );
            }
            Some("bank_type") => bank_type = Some(field.text().await.map_err(bad)?.trim().parse()?),
            Some("file") => {
                let file_name = field
                    .file_name()
                    .filter(|n| !n.is_empty())
                    .unwrap_or("unknown")
                    .to_string();
                file = Some((file_name, field.bytes().await.map_err(bad)?.to_vec()));
            }
            _ => {}
        }
    }
    let account_id = account_id.ok_or_else(|| AppError::bad_request("account_id is required"))?;
    let bank_type = bank_type.ok_or_else(|| AppError::bad_request("bank_type is required"))?;
    let (file_name, bytes) = file.ok_or_else(|| AppError::bad_request("file is required"))?;
    Ok(Upload {
        account_id,
        bank_type,
        file_name,
        bytes,
    })
}

#[derive(serde::Serialize)]
pub struct ImportResult {
    pub import_id: ImportId,
    pub total_rows: usize,
    pub reverted_candidate_count: usize,
    pub suspicious_count: usize,
}

#[derive(serde::Serialize)]
pub struct ImportSummary {
    pub id: ImportId,
    pub file_name: String,
    pub uploaded_at: DateTime<Utc>,
    pub rows_read: i64,
    pub completed: bool,
}

#[derive(serde::Deserialize)]
pub struct AcceptRequest {
    pub category: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct AcceptAsTransferRequest {
    pub other_account_id: AccountId,
    pub other_amount: Option<Decimal>,
}

#[derive(serde::Deserialize)]
pub struct SetCategoryRequest {
    pub category: Option<String>,
}

#[derive(serde::Serialize)]
pub struct TransferResult {
    pub out_entry: Entry,
    pub in_entry: Entry,
}

#[derive(serde::Serialize)]
pub struct Accepted {
    pub accepted: usize,
}

#[derive(serde::Serialize)]
pub struct Discarded {
    pub discarded: usize,
}

pub fn router<S: WithLedger>() -> Router<S> {
    Router::new()
        .route(
            "/imports",
            post(upload::<S>)
                .get(list_imports::<S>)
                .layer(DefaultBodyLimit::max(UPLOAD_LIMIT_BYTES)),
        )
        .route("/imports/:id", get(get_import::<S>))
        .route("/imports/:id/queue", get(get_queue::<S>))
        .route("/imports/:id/queue/accept-all", post(accept_all::<S>))
        .route("/imports/:id/queue/discard-all", post(discard_all::<S>))
        .route(
            "/imports/:id/queue/:row_id",
            axum::routing::patch(set_category::<S>),
        )
        .route("/imports/:id/queue/:row_id/accept", post(accept::<S>))
        .route(
            "/imports/:id/queue/:row_id/accept-as-transfer",
            post(accept_as_transfer::<S>),
        )
        .route(
            "/imports/:id/queue/:row_id/resolve-revert",
            post(resolve_revert::<S>),
        )
        .route("/imports/:id/queue/:row_id/discard", post(discard::<S>))
}

async fn upload<S: WithLedger>(
    State(state): State<S>,
    multipart: Multipart,
) -> Result<Json<ImportResult>, AppError> {
    let upload = read_upload(multipart).await?;
    let account_id = upload.account_id;

    // Fail fast before parsing; stage_import checks again right before
    // saving, since another upload could stage in between.
    let account = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                let account = ledger
                    .account(account_id)
                    .await?
                    .ok_or(LedgerError::AccountNotFound(account_id))?;
                if ledger
                    .incomplete_import_for_account(account_id)
                    .await?
                    .is_some()
                {
                    return Err(LedgerError::IncompleteImportExists);
                }
                Ok(account)
            })
        })
        .await?;

    // Parsing can run pdftotext, a blocking subprocess.
    let (bank_type, bytes) = (upload.bank_type, upload.bytes);
    let parsed = tokio::task::spawn_blocking(move || bank_type.read(&bytes))
        .await
        .map_err(|_| AppError::internal("the statement parser panicked"))??;

    let file_name = upload.file_name;
    let result = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                let entries = ledger.entries(account_id).await?;
                let import = new_import(&account, file_name, &parsed);
                let staged = build_queue(&import, &parsed, &entries);
                let result = ImportResult {
                    import_id: import.id,
                    total_rows: staged.rows.len(),
                    reverted_candidate_count: parsed.reverted_candidates.len(),
                    suspicious_count: staged.suspicious_count(),
                };
                ledger
                    .stage_import(import, staged.rows, staged.matches)
                    .await?;
                Ok(result)
            })
        })
        .await?;
    Ok(Json(result))
}

async fn list_imports<S: WithLedger>(
    State(state): State<S>,
) -> Result<Json<Vec<ImportSummary>>, AppError> {
    let imports = state
        .with_ledger(|ledger| Box::pin(async move { ledger.imports().await }))
        .await?;
    Ok(Json(
        imports
            .into_iter()
            .map(|i| ImportSummary {
                id: i.id,
                file_name: i.file_name,
                uploaded_at: i.uploaded_at,
                rows_read: i.rows_read,
                completed: i.completed,
            })
            .collect(),
    ))
}

async fn get_import<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<ImportId>,
) -> Result<Json<Import>, AppError> {
    let import = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                ledger
                    .import(id)
                    .await?
                    .ok_or(LedgerError::ImportNotFound(id))
            })
        })
        .await?;
    Ok(Json(import))
}

async fn get_queue<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<ImportId>,
) -> Result<Json<Vec<QueueRowView>>, AppError> {
    Ok(Json(
        state
            .with_ledger(move |ledger| Box::pin(async move { ledger.import_queue(id).await }))
            .await?,
    ))
}

async fn accept_all<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<ImportId>,
) -> Result<Json<Accepted>, AppError> {
    let accepted = state
        .with_ledger(move |ledger| Box::pin(async move { ledger.bulk_accept_import(id).await }))
        .await?;
    Ok(Json(Accepted { accepted }))
}

async fn discard_all<S: WithLedger>(
    State(state): State<S>,
    Path(id): Path<ImportId>,
) -> Result<Json<Discarded>, AppError> {
    let discarded = state
        .with_ledger(move |ledger| Box::pin(async move { ledger.discard_import(id).await }))
        .await?;
    Ok(Json(Discarded { discarded }))
}

/// The body is optional: an empty one means "no explicit category".
async fn accept<S: WithLedger>(
    State(state): State<S>,
    Path((id, row_id)): Path<(ImportId, QueueRowId)>,
    body: Bytes,
) -> Result<Json<Entry>, AppError> {
    let category = if body.is_empty() {
        None
    } else {
        serde_json::from_slice::<AcceptRequest>(&body)
            .map_err(|e| AppError::bad_request(e.to_string()))?
            .category
    };
    Ok(Json(
        state
            .with_ledger(move |ledger| {
                Box::pin(async move { ledger.accept_queue_row(id, row_id, category).await })
            })
            .await?,
    ))
}

async fn accept_as_transfer<S: WithLedger>(
    State(state): State<S>,
    Path((id, row_id)): Path<(ImportId, QueueRowId)>,
    AppJson(req): AppJson<AcceptAsTransferRequest>,
) -> Result<Json<TransferResult>, AppError> {
    let (out_entry, in_entry) = state
        .with_ledger(move |ledger| {
            Box::pin(async move {
                ledger
                    .accept_queue_row_as_transfer(
                        id,
                        row_id,
                        req.other_account_id,
                        req.other_amount,
                    )
                    .await
            })
        })
        .await?;
    Ok(Json(TransferResult {
        out_entry,
        in_entry,
    }))
}

async fn resolve_revert<S: WithLedger>(
    State(state): State<S>,
    Path((id, row_id)): Path<(ImportId, QueueRowId)>,
) -> Result<Json<Entry>, AppError> {
    Ok(Json(
        state
            .with_ledger(move |ledger| {
                Box::pin(async move { ledger.resolve_reverted_candidate(id, row_id).await })
            })
            .await?,
    ))
}

async fn set_category<S: WithLedger>(
    State(state): State<S>,
    Path((id, row_id)): Path<(ImportId, QueueRowId)>,
    AppJson(req): AppJson<SetCategoryRequest>,
) -> Result<Json<ImportQueueRow>, AppError> {
    Ok(Json(
        state
            .with_ledger(move |ledger| {
                Box::pin(async move {
                    ledger
                        .set_queue_row_category(id, row_id, req.category)
                        .await
                })
            })
            .await?,
    ))
}

async fn discard<S: WithLedger>(
    State(state): State<S>,
    Path((id, row_id)): Path<(ImportId, QueueRowId)>,
) -> Result<StatusCode, AppError> {
    state
        .with_ledger(move |ledger| {
            Box::pin(async move { ledger.discard_queue_row(id, row_id).await })
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app;
    use crate::state::TestState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use ledger_core::{
        AccountKind, BankState, Currency, Entry, EntryId, Import, ImportQueueRow, QueueRowView,
        RowReview,
    };
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use tower::ServiceExt;

    fn test_state() -> crate::state::TestState {
        crate::state::TestState::new()
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/../importer/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    async fn an_account(
        state: &crate::state::TestState,
        currency: &str,
        opening: Decimal,
    ) -> AccountId {
        state
            .ledger
            .lock()
            .await
            .open_account(
                "Card",
                Currency::new(currency).unwrap(),
                AccountKind::Own,
                opening,
            )
            .await
            .unwrap()
            .id
    }

    const BOUNDARY: &str = "test-boundary-7MA4YWxkTrZu0gW";

    fn multipart(
        account_id: Option<AccountId>,
        bank_type: Option<&str>,
        file: Option<(&str, Vec<u8>)>,
    ) -> Request<Body> {
        let mut body = Vec::new();
        let mut text = |name: &str, value: &str| {
            body.extend(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").bytes());
        };
        if let Some(id) = account_id {
            text("account_id", &id.to_string());
        }
        if let Some(bank) = bank_type {
            text("bank_type", bank);
        }
        if let Some((file_name, bytes)) = file {
            body.extend(
                format!(
                    "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .bytes(),
            );
            body.extend(bytes);
            body.extend(b"\r\n");
        }
        body.extend(format!("--{BOUNDARY}--\r\n").bytes());
        Request::builder()
            .method("POST")
            .uri("/imports")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(Body::from(body))
            .unwrap()
    }

    async fn send(
        state: &crate::state::TestState,
        request: Request<Body>,
    ) -> (StatusCode, serde_json::Value) {
        let response = app(state.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, json)
    }

    async fn upload(
        state: &crate::state::TestState,
        account_id: AccountId,
        bank_type: &str,
        file_name: &str,
    ) -> (StatusCode, serde_json::Value) {
        send(
            state,
            multipart(
                Some(account_id),
                Some(bank_type),
                Some((file_name, fixture(file_name))),
            ),
        )
        .await
    }

    fn json_request(method: &str, uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn get(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    async fn queue(state: &crate::state::TestState, import_id: &str) -> Vec<QueueRowView> {
        let (status, body) = send(state, get(&format!("/imports/{import_id}/queue"))).await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_value(body).unwrap()
    }

    fn import_id(body: &serde_json::Value) -> String {
        body["import_id"].as_str().unwrap().to_string()
    }

    async fn imports_saved(state: &crate::state::TestState) -> usize {
        state.ledger.lock().await.imports().await.unwrap().len()
    }

    async fn balance(state: &crate::state::TestState, account_id: AccountId) -> Decimal {
        state
            .ledger
            .lock()
            .await
            .account_balance(account_id)
            .await
            .unwrap()
    }

    // ---- upload ----

    #[tokio::test]
    async fn uploading_a_bank_a_statement_stages_its_queue() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(150)).await;
        let (status, body) = upload(&state, account, "BankA", "bank_a_statement.csv").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total_rows"], 9);
        assert_eq!(body["reverted_candidate_count"], 1);
        assert_eq!(body["suspicious_count"], 0);
        let id = import_id(&body);
        let rows = queue(&state, &id).await;
        assert_eq!(rows.len(), 9);
        assert!(rows
            .windows(2)
            .all(|w| (w[0].row.date, w[0].row.time) <= (w[1].row.date, w[1].row.time)));
        let pending: Vec<&QueueRowView> = rows
            .iter()
            .filter(|r| {
                r.row
                    .normal()
                    .is_some_and(|detail| detail.bank_state == BankState::Pending)
            })
            .collect();
        assert_eq!(pending.len(), 1);
        assert_eq!(description_of(&pending[0].row), "Pending Cafe");
        assert!(rows
            .iter()
            .any(|r| description_of(&r.row) == "Radio licence, TV, extras"));
        assert!(state
            .ledger
            .lock()
            .await
            .entries(account)
            .await
            .unwrap()
            .is_empty());

        let (status, record) = send(&state, get(&format!("/imports/{id}"))).await;
        assert_eq!(status, StatusCode::OK);
        let record: Import = serde_json::from_value(record).unwrap();
        assert_eq!(record.file_name, "bank_a_statement.csv");
        assert_eq!(
            (record.opening_balance, record.closing_balance),
            (dec!(150.00), dec!(311.35))
        );
        assert_eq!(record.currency, Currency::new("EUR").unwrap());
        assert!(!record.completed);

        let (status, list) = send(&state, get("/imports")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list[0]["id"], id.as_str());
        assert_eq!(list[0]["file_name"], "bank_a_statement.csv");
        assert_eq!(list[0]["completed"], false);
        assert!(list[0]["rows_read"].is_number());
        assert!(list[0]["uploaded_at"].is_string());
    }

    #[tokio::test]
    async fn uploading_a_bank_b_statement_stages_its_queue() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(1250)).await;
        let (status, body) = upload(&state, account, "BankB", "bank_b_statement.pdf").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total_rows"], 4);
        assert_eq!(queue(&state, &import_id(&body)).await.len(), 4);
    }

    #[tokio::test]
    async fn an_account_settlement_pdf_is_rejected_and_nothing_is_saved() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(0)).await;
        let (status, _) = upload(&state, account, "BankB", "bank_b_settlement.pdf").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(imports_saved(&state).await, 0);
    }

    #[tokio::test]
    async fn a_failed_balance_check_returns_both_figures_and_saves_nothing() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(0)).await;
        let (status, body) = upload(&state, account, "BankA", "bank_a_bad_balance.csv").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["expected"], "90.00");
        assert_eq!(body["actual"], "88.00");
        assert_eq!(imports_saved(&state).await, 0);
    }

    #[tokio::test]
    async fn the_wrong_bank_type_for_a_file_just_fails_to_parse() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(0)).await;
        let (status, _) = send(
            &state,
            multipart(
                Some(account),
                Some("BankA"),
                Some(("x.pdf", fixture("bank_b_statement.pdf"))),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(imports_saved(&state).await, 0);
    }

    #[tokio::test]
    async fn uploading_to_an_unknown_account_is_404() {
        let state = test_state();
        let (status, _) = upload(
            &state,
            AccountId::generate(),
            "BankA",
            "bank_a_statement.csv",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(imports_saved(&state).await, 0);
    }

    #[tokio::test]
    async fn a_malformed_upload_request_is_a_bad_request() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(0)).await;
        let file = || Some(("f.csv", fixture("bank_a_statement.csv")));
        assert_eq!(
            send(&state, multipart(Some(account), Some("BankC"), file()))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            send(&state, multipart(None, Some("BankA"), file())).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            send(&state, multipart(Some(account), None, file())).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            send(&state, multipart(Some(account), Some("BankA"), None))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(imports_saved(&state).await, 0);
    }

    #[tokio::test]
    async fn a_second_upload_while_one_is_incomplete_is_409_and_allowed_once_it_completes() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(0)).await;
        let (_, body) = upload(&state, account, "BankA", "bank_a_statement.csv").await;
        let (status, _) = upload(&state, account, "BankA", "bank_a_statement.csv").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(imports_saved(&state).await, 1);
        let (status, discarded) = send(
            &state,
            json_request(
                "POST",
                &format!("/imports/{}/queue/discard-all", import_id(&body)),
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(discarded["discarded"], 9);
        let (status, _) = upload(&state, account, "BankA", "bank_a_statement.csv").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_file_larger_than_axums_2mb_default_is_accepted() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(0)).await;
        let mut csv = fixture("bank_a_bad_balance.csv");
        csv.extend(vec![b'\n'; 3 * 1024 * 1024]);
        let (status, _) = send(
            &state,
            multipart(Some(account), Some("BankA"), Some(("big.csv", csv))),
        )
        .await;
        // Parsed (and then refused on its balance), not refused on size.
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    // ---- matching at upload ----

    /// An entry with no time, the way a manual entry is recorded.
    async fn an_untimed_entry(
        state: &crate::state::TestState,
        account: AccountId,
        day: u32,
        amount: Decimal,
    ) -> Entry {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, day).unwrap();
        state
            .ledger
            .lock()
            .await
            .record_manual_entry(account, date, amount, "earlier")
            .await
            .unwrap()
    }

    /// An entry with a time, which only an import produces: stages a
    /// one-row statement and accepts it, as an earlier upload would have.
    async fn an_earlier_imported_entry(
        state: &TestState,
        account: AccountId,
        started: &str,
        amount: &str,
    ) -> Entry {
        let csv = format!(
            "Type,Product,Started Date,Completed Date,Description,Amount,Fee,Currency,State,Balance\n\
             Card Payment,Current,{started},{started},Corner Bakery,{amount},0.00,EUR,COMPLETED,{amount}\n"
        );
        let (status, body) = send(
            state,
            multipart(
                Some(account),
                Some("BankA"),
                Some(("earlier.csv", csv.into_bytes())),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let id = import_id(&body);
        let row = queue(state, &id).await.remove(0);
        let (_, entry) = send(
            state,
            json_request(
                "POST",
                &format!("/imports/{id}/queue/{}/accept", row.row.id),
                serde_json::json!({}),
            ),
        )
        .await;
        serde_json::from_value(entry).unwrap()
    }

    #[tokio::test]
    async fn a_reverted_row_matching_a_timed_entry_is_suggested_not_applied() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(0)).await;
        let earlier =
            an_earlier_imported_entry(&state, account, "2026-03-04 12:00:00", "-4.20").await;
        let (_, body) = upload(&state, account, "BankA", "bank_a_statement.csv").await;
        let rows = queue(&state, &import_id(&body)).await;
        let candidate = the_reverted_candidate(&rows);
        assert_eq!(suggested_entry_id(candidate), Some(earlier.id));
        let still = state.ledger.lock().await.entries(account).await.unwrap();
        assert_eq!(still[0].bank_state, BankState::Completed);
    }

    #[tokio::test]
    async fn a_reverted_row_against_an_untimed_or_absent_entry_is_queued_with_no_suggestion() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(0)).await;
        an_untimed_entry(&state, account, 4, dec!(-4.20)).await;
        let (_, body) = upload(&state, account, "BankA", "bank_a_statement.csv").await;
        assert_eq!(body["reverted_candidate_count"], 1);
        let rows = queue(&state, &import_id(&body)).await;
        let candidate = the_reverted_candidate(&rows);
        assert_eq!(suggested_entry_id(candidate), None);
        let (status, _) = send(
            &state,
            json_request(
                "POST",
                &format!(
                    "/imports/{}/queue/{}/resolve-revert",
                    import_id(&body),
                    candidate.row.id
                ),
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn a_row_resembling_an_existing_entry_is_flagged_suspicious() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(0)).await;
        let earlier = an_untimed_entry(&state, account, 6, dec!(200.00)).await;
        let (_, body) = upload(&state, account, "BankA", "bank_a_statement.csv").await;
        assert_eq!(body["suspicious_count"], 1);
        let rows = queue(&state, &import_id(&body)).await;
        let flagged: Vec<&QueueRowView> = rows
            .iter()
            .filter(|r| {
                matches!(
                    r.review,
                    RowReview::Normal {
                        suspicious: true,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(flagged.len(), 1);
        assert!(matches!(
            &flagged[0].review,
            RowReview::Normal { matched_entry_ids, .. } if *matched_entry_ids == vec![earlier.id]
        ));
        let (status, blocked) = send(
            &state,
            json_request(
                "POST",
                &format!("/imports/{}/queue/accept-all", import_id(&body)),
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(blocked["suspicious_count"], 1);
        assert_eq!(blocked["reverted_candidate_count"], 1);
    }

    #[tokio::test]
    async fn rows_in_another_currency_account_are_never_matched() {
        let state = test_state();
        let naira = an_account(&state, "NGN", dec!(0)).await;
        let euro = an_account(&state, "EUR", dec!(0)).await;
        an_untimed_entry(&state, euro, 2, dec!(-12.00)).await;
        let (status, body) = upload(&state, naira, "BankA", "bank_a_statement_ngn.csv").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["suspicious_count"], 0);
        let rows = queue(&state, &import_id(&body)).await;
        assert!(rows
            .iter()
            .all(|r| r.row.currency == Currency::new("NGN").unwrap()));
    }

    // ---- review actions ----

    async fn staged_bank_a(
        state: &crate::state::TestState,
    ) -> (AccountId, String, Vec<QueueRowView>) {
        let account = an_account(state, "EUR", dec!(150)).await;
        let (_, body) = upload(state, account, "BankA", "bank_a_statement.csv").await;
        let id = import_id(&body);
        let rows = queue(state, &id).await;
        (account, id, rows)
    }

    fn description_of(row: &ImportQueueRow) -> &str {
        row.normal()
            .map_or("", |detail| detail.description.as_str())
    }

    fn category_of(row: &ImportQueueRow) -> Option<String> {
        row.normal().and_then(|detail| detail.category.clone())
    }

    fn row_described<'a>(rows: &'a [QueueRowView], description: &str) -> &'a QueueRowView {
        rows.iter()
            .find(|r| description_of(&r.row) == description)
            .unwrap()
    }

    fn the_reverted_candidate(rows: &[QueueRowView]) -> &QueueRowView {
        rows.iter().find(|r| r.row.is_reverted_candidate()).unwrap()
    }

    fn suggested_entry_id(view: &QueueRowView) -> Option<EntryId> {
        match view.review {
            RowReview::RevertedCandidate { suggested_entry_id } => suggested_entry_id,
            RowReview::Normal { .. } => None,
        }
    }

    #[tokio::test]
    async fn accepting_a_row_with_a_category_saves_that_entry() {
        let state = test_state();
        let (account, id, rows) = staged_bank_a(&state).await;
        let row = row_described(&rows, "Night Kiosk");
        let (status, entry) = send(
            &state,
            json_request(
                "POST",
                &format!("/imports/{id}/queue/{}/accept", row.row.id),
                serde_json::json!({"category": "Snacks"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let entry: Entry = serde_json::from_value(entry).unwrap();
        assert_eq!(entry.category, Some("Snacks".to_string()));
        assert_eq!(entry.account_id, account);
        assert_eq!(queue(&state, &id).await.len(), 8);
    }

    #[tokio::test]
    async fn accepting_with_an_empty_body_is_allowed_and_a_malformed_body_is_400() {
        let state = test_state();
        let (_, id, rows) = staged_bank_a(&state).await;
        let accept = |row: &QueueRowView, body: &'static str| {
            Request::builder()
                .method("POST")
                .uri(format!("/imports/{id}/queue/{}/accept", row.row.id))
                .body(Body::from(body))
                .unwrap()
        };
        let (status, _) = send(&state, accept(row_described(&rows, "Night Kiosk"), "")).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(
            &state,
            accept(row_described(&rows, "Corner Bakery"), "{not json"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(queue(&state, &id).await.len(), 8);
    }

    #[tokio::test]
    async fn a_patched_category_persists_and_is_used_on_accept() {
        let state = test_state();
        let (_, id, rows) = staged_bank_a(&state).await;
        let row = row_described(&rows, "Corner Bakery");
        let (status, patched) = send(
            &state,
            json_request(
                "PATCH",
                &format!("/imports/{id}/queue/{}", row.row.id),
                serde_json::json!({"category": "Food"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let patched: ImportQueueRow = serde_json::from_value(patched).unwrap();
        assert_eq!(category_of(&patched), Some("Food".to_string()));
        assert_eq!(
            category_of(&row_described(&queue(&state, &id).await, "Corner Bakery").row),
            Some("Food".to_string())
        );
        let (_, entry) = send(
            &state,
            json_request(
                "POST",
                &format!("/imports/{id}/queue/{}/accept", row.row.id),
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(entry["category"], "Food");
    }

    #[tokio::test]
    async fn a_row_id_from_another_import_is_404() {
        let state = test_state();
        let (_, id, _) = staged_bank_a(&state).await;
        let (_, _, other_rows) = staged_bank_a(&state).await;
        let foreign = other_rows[0].row.id;
        let (status, _) = send(
            &state,
            json_request(
                "PATCH",
                &format!("/imports/{id}/queue/{foreign}"),
                serde_json::json!({"category": "x"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(
            &state,
            json_request(
                "PATCH",
                &format!("/imports/{id}/queue/{}", QueueRowId::generate()),
                serde_json::json!({"category": "x"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(
            &state,
            json_request(
                "POST",
                &format!("/imports/{id}/queue/{foreign}/accept"),
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn actions_on_the_wrong_kind_of_row_are_409() {
        let state = test_state();
        let (_, id, rows) = staged_bank_a(&state).await;
        let candidate = the_reverted_candidate(&rows).row.id;
        let normal = row_described(&rows, "Night Kiosk").row.id;
        let other = an_account(&state, "EUR", dec!(0)).await;
        for (method, uri, body) in [
            (
                "POST",
                format!("/imports/{id}/queue/{candidate}/accept"),
                serde_json::json!({}),
            ),
            (
                "POST",
                format!("/imports/{id}/queue/{candidate}/accept-as-transfer"),
                serde_json::json!({"other_account_id": other}),
            ),
            (
                "PATCH",
                format!("/imports/{id}/queue/{candidate}"),
                serde_json::json!({"category": "x"}),
            ),
            (
                "POST",
                format!("/imports/{id}/queue/{normal}/resolve-revert"),
                serde_json::json!({}),
            ),
        ] {
            assert_eq!(
                send(&state, json_request(method, &uri, body)).await.0,
                StatusCode::CONFLICT,
                "{method} {uri}"
            );
        }
    }

    #[tokio::test]
    async fn accepting_as_a_transfer_moves_money_to_the_other_account() {
        let state = test_state();
        let (account, id, rows) = staged_bank_a(&state).await;
        let savings = an_account(&state, "EUR", dec!(0)).await;
        let row = row_described(&rows, "Payment from Jane Example").row.id;
        let (status, body) = send(
            &state,
            json_request(
                "POST",
                &format!("/imports/{id}/queue/{row}/accept-as-transfer"),
                serde_json::json!({"other_account_id": savings}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let out_entry: Entry = serde_json::from_value(body["out_entry"].clone()).unwrap();
        let in_entry: Entry = serde_json::from_value(body["in_entry"].clone()).unwrap();
        assert_eq!(
            (out_entry.account_id, out_entry.amount),
            (savings, dec!(-200.00))
        );
        assert_eq!(
            (in_entry.account_id, in_entry.amount),
            (account, dec!(200.00))
        );
        let (status, _) = send(
            &state,
            json_request(
                "POST",
                &format!(
                    "/imports/{id}/queue/{}/accept-as-transfer",
                    row_described(&rows, "Night Kiosk").row.id
                ),
                serde_json::json!({"other_account_id": account}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let (status, _) = send(
            &state,
            json_request(
                "POST",
                &format!(
                    "/imports/{id}/queue/{}/accept-as-transfer",
                    row_described(&rows, "Night Kiosk").row.id
                ),
                serde_json::json!({"other_account_id": AccountId::generate()}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn discarding_a_row_removes_it_without_an_entry() {
        let state = test_state();
        let (account, id, rows) = staged_bank_a(&state).await;
        let (status, _) = send(
            &state,
            json_request(
                "POST",
                &format!("/imports/{id}/queue/{}/discard", rows[0].row.id),
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(queue(&state, &id).await.len(), 8);
        assert!(state
            .ledger
            .lock()
            .await
            .entries(account)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn discard_all_on_an_unknown_import_is_404() {
        let state = test_state();
        let (status, _) = send(
            &state,
            json_request(
                "POST",
                &format!("/imports/{}/queue/discard-all", ImportId::generate()),
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(&state, get(&format!("/imports/{}", ImportId::generate()))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(
            &state,
            get(&format!("/imports/{}/queue", ImportId::generate())),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_transfer_from_an_account_to_itself_is_422() {
        let state = test_state();
        let account = an_account(&state, "EUR", dec!(100)).await;
        let (status, _) = send(
            &state,
            json_request(
                "POST",
                "/transfers",
                serde_json::json!({"from_account_id": account, "to_account_id": account, "date": "2026-03-01", "amount_sent": "5", "amount_received": "5", "description": "x"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    // ---- whole system ----

    #[tokio::test]
    async fn importing_and_accepting_every_fixture_statement_reaches_the_statements_balances() {
        let state = test_state();
        let card = an_account(&state, "EUR", dec!(150.00)).await;
        let current = an_account(&state, "EUR", dec!(1250.00)).await;

        let (_, body) = upload(&state, card, "BankA", "bank_a_statement.csv").await;
        let id = import_id(&body);
        // The one reverted row has nothing to revert: discard it, then bulk accept.
        let candidate = the_reverted_candidate(&queue(&state, &id).await).clone();
        send(
            &state,
            json_request(
                "POST",
                &format!("/imports/{id}/queue/{}/discard", candidate.row.id),
                serde_json::json!({}),
            ),
        )
        .await;
        let (status, accepted) = send(
            &state,
            json_request(
                "POST",
                &format!("/imports/{id}/queue/accept-all"),
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(accepted["accepted"], 8);
        // The statement's closing balance, plus the one still-pending row.
        assert_eq!(balance(&state, card).await, dec!(311.35) - dec!(6.50));

        let (_, body) = upload(&state, current, "BankB", "bank_b_statement.pdf").await;
        let id = import_id(&body);
        let (_, accepted) = send(
            &state,
            json_request(
                "POST",
                &format!("/imports/{id}/queue/accept-all"),
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(accepted["accepted"], 4);
        assert_eq!(balance(&state, current).await, dec!(2737.62));

        let (_, list) = send(&state, get("/imports")).await;
        assert!(list
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["completed"] == true));
    }
}
