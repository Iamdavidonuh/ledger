//! API integration tests against a real Postgres instance.
//!
//! Skipped automatically when DATABASE_URL is not set, so `cargo test`
//! in a plain dev environment still passes cleanly.
//!
//! Run with:
//!   docker compose -f docker-compose.test.yml run --rm test

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use ledger_api::app;

use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

/// A live `AppState` backed by a clean Postgres database. Returns `None`
/// when DATABASE_URL is not set so tests skip gracefully.
async fn test_state() -> Option<ledger_api::state::AppState> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPool::connect(&url).await.ok()?;
    sqlx::migrate!("../core/migrations").run(&pool).await.ok()?;
    sqlx::query(
        "TRUNCATE accounts, entries, entry_tags, entry_parts, pots, allocations, \
         valuations, imports, import_queue_rows, import_queue_row_matches CASCADE",
    )
    .execute(&pool)
    .await
    .ok()?;
    Some(ledger_api::state::AppState::new(pool))
}

/// Sends one HTTP request through the full Axum router and returns the
/// response status and parsed JSON body. The state is cloned internally
/// so the caller retains it for further calls.
async fn send_api(
    state: &ledger_api::state::AppState,
    request: Request<Body>,
) -> (StatusCode, Value) {
    let response = app(state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

/// Builds a JSON POST request.
fn post(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Builds a JSON PATCH request.
fn patch(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("PATCH")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Builds a GET request.
fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

/// Builds a DELETE request.
fn delete(uri: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

/// Creates a ledger account via the API and returns its id string.
async fn create_account(
    state: &ledger_api::state::AppState,
    name: &str,
    currency: &str,
    kind: &str,
    opening_balance: &str,
) -> String {
    let (status, body) = send_api(
        state,
        post(
            "/accounts",
            json!({ "name": name, "currency": currency, "kind": kind, "opening_balance": opening_balance }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create_account failed: {body}");
    body["id"].as_str().unwrap().to_string()
}

/// Records a manual entry and returns the entry JSON.
async fn record_entry(
    state: &ledger_api::state::AppState,
    account_id: &str,
    kind: &str,
    amount: &str,
    description: &str,
) -> Value {
    let (status, body) = send_api(
        state,
        post(
            "/entries",
            json!({
                "account_id": account_id,
                "date": "2026-01-15",
                "kind": kind,
                "amount": amount,
                "description": description
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "record_entry failed: {body}");
    body
}

/// Reads a fixture file from the importer test fixtures directory.
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../importer/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

const BOUNDARY: &str = "test-boundary-7MA4YWxkTrZu0gW";

/// Builds a multipart/form-data POST /imports request.
fn multipart_upload(
    account_id: &str,
    bank_type: &str,
    file_name: &str,
    file_bytes: Vec<u8>,
) -> Request<Body> {
    let mut body: Vec<u8> = Vec::new();
    let mut text = |name: &str, value: &str| {
        body.extend(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .bytes(),
        );
    };
    text("account_id", account_id);
    text("bank_type", bank_type);
    body.extend(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .bytes(),
    );
    body.extend(file_bytes);
    body.extend(b"\r\n");
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

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_returns_ok() {
    let Some(state) = test_state().await else {
        return;
    };
    let (status, _) = send_api(&state, get("/health")).await;
    assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_and_get_account() {
    let Some(state) = test_state().await else {
        return;
    };
    let id = create_account(&state, "Checking", "EUR", "own", "100").await;

    let (status, body) = send_api(&state, get(&format!("/accounts/{id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Checking");
    assert_eq!(body["balance"], "100");
}

#[tokio::test]
async fn list_accounts_includes_balances() {
    let Some(state) = test_state().await else {
        return;
    };
    create_account(&state, "EUR Account", "EUR", "own", "200").await;
    create_account(&state, "NGN Account", "NGN", "own", "50000").await;

    let (status, body) = send_api(&state, get("/accounts")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn unknown_account_is_not_found() {
    let Some(state) = test_state().await else {
        return;
    };
    let (status, _) = send_api(&state, get(&format!("/accounts/{}", Uuid::new_v4()))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn invalid_currency_is_bad_request() {
    let Some(state) = test_state().await else {
        return;
    };
    let (status, _) = send_api(
        &state,
        post(
            "/accounts",
            json!({ "name": "Bad", "currency": "!", "kind": "own", "opening_balance": "0" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Entries
// ---------------------------------------------------------------------------

#[tokio::test]
async fn record_expense_and_income_update_balance() {
    let Some(state) = test_state().await else {
        return;
    };
    let account_id = create_account(&state, "Checking", "EUR", "own", "0").await;

    record_entry(&state, &account_id, "income", "500", "Salary").await;
    record_entry(&state, &account_id, "expense", "50", "Groceries").await;

    let (_, body) = send_api(&state, get(&format!("/accounts/{account_id}"))).await;
    assert_eq!(body["balance"], "450");
}

#[tokio::test]
async fn list_entries_filters_by_account() {
    let Some(state) = test_state().await else {
        return;
    };
    let a = create_account(&state, "A", "EUR", "own", "0").await;
    let b = create_account(&state, "B", "EUR", "own", "0").await;

    record_entry(&state, &a, "expense", "10", "x").await;
    record_entry(&state, &a, "expense", "20", "y").await;
    record_entry(&state, &b, "expense", "5", "z").await;

    let (status, body) = send_api(&state, get(&format!("/entries?account_id={a}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn negative_amount_is_bad_request() {
    let Some(state) = test_state().await else {
        return;
    };
    let account_id = create_account(&state, "Checking", "EUR", "own", "0").await;
    let (status, _) = send_api(
        &state,
        post(
            "/entries",
            json!({ "account_id": account_id, "date": "2026-01-15", "kind": "expense", "amount": "-10", "description": "x" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn void_entry_removes_it_from_balance() {
    let Some(state) = test_state().await else {
        return;
    };
    let account_id = create_account(&state, "Checking", "EUR", "own", "0").await;
    let entry = record_entry(&state, &account_id, "expense", "100", "Oops").await;
    let entry_id = entry["id"].as_str().unwrap();

    let (status, _) = send_api(
        &state,
        post(
            &format!("/entries/{entry_id}/void"),
            json!({ "reason": "mistake" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, body) = send_api(&state, get(&format!("/accounts/{account_id}"))).await;
    assert_eq!(body["balance"], "0");
}

#[tokio::test]
async fn update_entry_metadata_persists() {
    let Some(state) = test_state().await else {
        return;
    };
    let account_id = create_account(&state, "Checking", "EUR", "own", "0").await;
    let entry = record_entry(&state, &account_id, "expense", "30", "Coffee").await;
    let entry_id = entry["id"].as_str().unwrap();

    let (status, body) = send_api(
        &state,
        patch(
            &format!("/entries/{entry_id}"),
            json!({ "category": "Food", "tags": ["morning"], "note": "nice" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["category"], "Food");
    assert_eq!(body["note"], "nice");
}

#[tokio::test]
async fn confirm_then_edit_amount_is_locked() {
    let Some(state) = test_state().await else {
        return;
    };
    let account_id = create_account(&state, "Checking", "EUR", "own", "0").await;
    let entry = record_entry(&state, &account_id, "expense", "10", "x").await;
    let entry_id = entry["id"].as_str().unwrap();

    send_api(
        &state,
        post(&format!("/entries/{entry_id}/confirm"), json!({})),
    )
    .await;

    let (status, _) = send_api(
        &state,
        patch(
            &format!("/entries/{entry_id}/amount"),
            json!({ "kind": "expense", "amount": "5" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// ---------------------------------------------------------------------------
// Pots
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_pot_and_allocate() {
    let Some(state) = test_state().await else {
        return;
    };
    create_account(&state, "Savings", "EUR", "own", "1000").await;

    let (status, pot) = send_api(
        &state,
        post(
            "/pots",
            json!({ "name": "Trip", "currency": "EUR", "target": "500" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let pot_id = pot["id"].as_str().unwrap();

    let (status, body) = send_api(
        &state,
        post(
            &format!("/pots/{pot_id}/allocations"),
            json!({ "amount": "300", "date": "2026-01-15" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["balance"], "300");
}

#[tokio::test]
async fn allocating_more_than_savings_is_unprocessable() {
    let Some(state) = test_state().await else {
        return;
    };
    create_account(&state, "Savings", "EUR", "own", "100").await;

    let (_, pot) = send_api(
        &state,
        post("/pots", json!({ "name": "Big", "currency": "EUR" })),
    )
    .await;
    let pot_id = pot["id"].as_str().unwrap();

    let (status, _) = send_api(
        &state,
        post(
            &format!("/pots/{pot_id}/allocations"),
            json!({ "amount": "101", "date": "2026-01-15" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn delete_pot_returns_balance_to_savings() {
    let Some(state) = test_state().await else {
        return;
    };
    let account_id = create_account(&state, "Savings", "EUR", "own", "500").await;

    let (_, pot) = send_api(
        &state,
        post("/pots", json!({ "name": "Trip", "currency": "EUR" })),
    )
    .await;
    let pot_id = pot["id"].as_str().unwrap();

    send_api(
        &state,
        post(
            &format!("/pots/{pot_id}/allocations"),
            json!({ "amount": "200", "date": "2026-01-15" }),
        ),
    )
    .await;

    let (status, _) = send_api(&state, delete(&format!("/pots/{pot_id}"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, body) = send_api(&state, get(&format!("/accounts/{account_id}"))).await;
    assert_eq!(body["balance"], "500");

    let (status, _) = send_api(&state, get(&format!("/pots/{pot_id}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_pots_shows_balances() {
    let Some(state) = test_state().await else {
        return;
    };
    create_account(&state, "Savings", "EUR", "own", "1000").await;

    send_api(
        &state,
        post("/pots", json!({ "name": "A", "currency": "EUR" })),
    )
    .await;
    send_api(
        &state,
        post("/pots", json!({ "name": "B", "currency": "EUR" })),
    )
    .await;

    let (status, body) = send_api(&state, get("/pots")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// Transfers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn transfer_moves_money_between_accounts() {
    let Some(state) = test_state().await else {
        return;
    };
    let from = create_account(&state, "Checking", "EUR", "own", "1000").await;
    let to = create_account(&state, "Savings", "EUR", "own", "0").await;

    let (status, body) = send_api(
        &state,
        post(
            "/transfers",
            json!({
                "from_account_id": from,
                "to_account_id": to,
                "date": "2026-01-15",
                "amount_sent": "400",
                "amount_received": "400",
                "description": "move"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["out_entry"]["id"].is_string());
    assert!(body["in_entry"]["id"].is_string());

    let (_, from_body) = send_api(&state, get(&format!("/accounts/{from}"))).await;
    let (_, to_body) = send_api(&state, get(&format!("/accounts/{to}"))).await;
    assert_eq!(from_body["balance"], "600");
    assert_eq!(to_body["balance"], "400");
}

#[tokio::test]
async fn transfer_to_self_is_unprocessable() {
    let Some(state) = test_state().await else {
        return;
    };
    let account = create_account(&state, "Checking", "EUR", "own", "100").await;

    let (status, _) = send_api(
        &state,
        post(
            "/transfers",
            json!({
                "from_account_id": account,
                "to_account_id": account,
                "date": "2026-01-15",
                "amount_sent": "50",
                "amount_received": "50",
                "description": "oops"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn cross_currency_transfer_requires_different_amounts() {
    let Some(state) = test_state().await else {
        return;
    };
    let eur = create_account(&state, "EUR", "EUR", "own", "200").await;
    let ngn = create_account(&state, "NGN", "NGN", "own", "0").await;

    // Same amount on a cross-currency transfer is refused.
    let (status, _) = send_api(
        &state,
        post(
            "/transfers",
            json!({
                "from_account_id": eur,
                "to_account_id": ngn,
                "date": "2026-01-15",
                "amount_sent": "100",
                "amount_received": "100",
                "description": "send"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Explicit conversion rate works.
    let (status, _) = send_api(
        &state,
        post(
            "/transfers",
            json!({
                "from_account_id": eur,
                "to_account_id": ngn,
                "date": "2026-01-15",
                "amount_sent": "100",
                "amount_received": "170000",
                "description": "send"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Valuations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn update_current_value_records_valuation() {
    let Some(state) = test_state().await else {
        return;
    };
    let account = create_account(&state, "ETF", "EUR", "investment", "1000").await;

    let (status, body) = send_api(
        &state,
        post(
            &format!("/accounts/{account}/current-value"),
            json!({ "new_value": "1200", "category": "Investment gain", "date": "2026-01-15" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["old_value"], "1000");
    assert_eq!(body["new_value"], "1200");
    assert_eq!(body["category"], "Investment gain");
}

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

#[tokio::test]
async fn upload_bank_a_statement_stages_queue() {
    let Some(state) = test_state().await else {
        return;
    };
    let account = create_account(&state, "Card", "EUR", "own", "150").await;

    let (status, body) = send_api(
        &state,
        multipart_upload(
            &account,
            "BankA",
            "bank_a_statement.csv",
            fixture("bank_a_statement.csv"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_rows"], 9);
    assert_eq!(body["reverted_candidate_count"], 1);

    let import_id = body["import_id"].as_str().unwrap();
    let (status, queue) = send_api(&state, get(&format!("/imports/{import_id}/queue"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(queue.as_array().unwrap().len(), 9);
}

#[tokio::test]
async fn upload_to_unknown_account_is_not_found() {
    let Some(state) = test_state().await else {
        return;
    };
    let (status, _) = send_api(
        &state,
        multipart_upload(
            &Uuid::new_v4().to_string(),
            "BankA",
            "bank_a_statement.csv",
            fixture("bank_a_statement.csv"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn failed_balance_check_returns_422_and_saves_nothing() {
    let Some(state) = test_state().await else {
        return;
    };
    let account = create_account(&state, "Card", "EUR", "own", "0").await;

    let (status, body) = send_api(
        &state,
        multipart_upload(
            &account,
            "BankA",
            "bank_a_bad_balance.csv",
            fixture("bank_a_bad_balance.csv"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["expected"], "90.00");
    assert_eq!(body["actual"], "88.00");

    let (_, imports) = send_api(&state, get("/imports")).await;
    assert_eq!(imports.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn second_upload_while_incomplete_is_conflict() {
    let Some(state) = test_state().await else {
        return;
    };
    let account = create_account(&state, "Card", "EUR", "own", "0").await;

    send_api(
        &state,
        multipart_upload(
            &account,
            "BankA",
            "bank_a_statement.csv",
            fixture("bank_a_statement.csv"),
        ),
    )
    .await;

    let (status, _) = send_api(
        &state,
        multipart_upload(
            &account,
            "BankA",
            "bank_a_statement.csv",
            fixture("bank_a_statement.csv"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn accept_all_clean_rows_completes_import() {
    let Some(state) = test_state().await else {
        return;
    };
    let account = create_account(&state, "Card", "EUR", "own", "150").await;

    let (_, body) = send_api(
        &state,
        multipart_upload(
            &account,
            "BankA",
            "bank_a_statement.csv",
            fixture("bank_a_statement.csv"),
        ),
    )
    .await;
    let import_id = body["import_id"].as_str().unwrap();

    // Discard the reverted candidate row first. Deserialized into the real
    // type rather than matched as a JSON string, so this can't drift from
    // RowDetail's actual variant names or serde representation again.
    let (_, queue) = send_api(&state, get(&format!("/imports/{import_id}/queue"))).await;
    let queue: Vec<ledger_core::QueueRowView> = serde_json::from_value(queue).unwrap();
    let candidate = queue
        .iter()
        .find(|row| row.row.is_reverted_candidate())
        .unwrap();
    let candidate_id = candidate.row.id.to_string();
    send_api(
        &state,
        post(
            &format!("/imports/{import_id}/queue/{candidate_id}/discard"),
            json!({}),
        ),
    )
    .await;

    // Now bulk-accept the remaining 8 rows.
    let (status, accepted) = send_api(
        &state,
        post(&format!("/imports/{import_id}/queue/accept-all"), json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(accepted["accepted"], 8);

    // Import is marked completed.
    let (_, import) = send_api(&state, get(&format!("/imports/{import_id}"))).await;
    assert!(import["completed"].as_bool().unwrap());

    // Entries landed in the account.
    let (_, entries) = send_api(&state, get(&format!("/entries?account_id={account}"))).await;
    assert_eq!(entries.as_array().unwrap().len(), 8);
}

#[tokio::test]
async fn discard_all_completes_import_with_no_entries() {
    let Some(state) = test_state().await else {
        return;
    };
    let account = create_account(&state, "Card", "EUR", "own", "0").await;

    let (_, body) = send_api(
        &state,
        multipart_upload(
            &account,
            "BankA",
            "bank_a_statement.csv",
            fixture("bank_a_statement.csv"),
        ),
    )
    .await;
    let import_id = body["import_id"].as_str().unwrap();

    let (status, result) = send_api(
        &state,
        post(
            &format!("/imports/{import_id}/queue/discard-all"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["discarded"], 9);

    let (_, import) = send_api(&state, get(&format!("/imports/{import_id}"))).await;
    assert!(import["completed"].as_bool().unwrap());

    let (_, entries) = send_api(&state, get(&format!("/entries?account_id={account}"))).await;
    assert!(entries.as_array().unwrap().is_empty());
}
