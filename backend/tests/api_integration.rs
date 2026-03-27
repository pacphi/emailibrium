//! HTTP-level integration tests for critical API endpoints.
//!
//! Because `AppState` and `api::routes()` live in the binary crate (`main.rs`)
//! and are not re-exported through the library crate, these tests reconstruct
//! a lightweight Router with handlers that replicate the exact SQL queries and
//! response types from the production code. This validates:
//!
//! - HTTP routing, status codes, and JSON serialization
//! - Database schema correctness (against real, migrated in-memory SQLite)
//! - Request/response contracts for each endpoint
//!
//! Run with: cargo test --test api_integration

use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Test-local types (mirror production response/request shapes)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct TestState {
    pool: SqlitePool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmailResponse {
    id: String,
    account_id: String,
    provider: String,
    subject: String,
    from_addr: String,
    is_read: bool,
    is_starred: bool,
    has_attachments: bool,
    embedding_status: String,
    category: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListEmailsResponse {
    emails: Vec<EmailResponse>,
    total: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ListEmailsParams {
    account_id: Option<String>,
    category: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountResponse {
    id: String,
    provider: String,
    email_address: String,
    is_active: bool,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GdprConsentRequest {
    consent_type: String,
    granted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct GdprConsentDecision {
    id: String,
    consent_type: String,
    granted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct GdprConsentResponse {
    decision: GdprConsentDecision,
}

#[derive(Debug, Serialize, Deserialize)]
struct GdprConsentListResponse {
    decisions: Vec<GdprConsentDecision>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct StartRequest {
    account_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JobResponse {
    job_id: String,
    status: String,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmailCounts {
    total: u64,
    unread: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ErasureRequest {
    confirmation_token: String,
}

// ---------------------------------------------------------------------------
// Test-local handlers (replicate production SQL queries)
// ---------------------------------------------------------------------------

const EMAIL_COLUMNS: &str = "id, account_id, provider, subject, from_addr, \
    is_read, is_starred, has_attachments, embedding_status, category";

fn row_to_response(row: &sqlx::sqlite::SqliteRow) -> EmailResponse {
    EmailResponse {
        id: row.get("id"),
        account_id: row.get("account_id"),
        provider: row.get("provider"),
        subject: row.get("subject"),
        from_addr: row.get("from_addr"),
        is_read: row.get::<bool, _>("is_read"),
        is_starred: row.get::<bool, _>("is_starred"),
        has_attachments: row.get::<bool, _>("has_attachments"),
        embedding_status: row.get("embedding_status"),
        category: row
            .get::<Option<String>, _>("category")
            .unwrap_or_else(|| "Uncategorized".to_string()),
    }
}

async fn list_emails(
    State(state): State<TestState>,
    Query(params): Query<ListEmailsParams>,
) -> Result<Json<ListEmailsResponse>, (StatusCode, String)> {
    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);

    let mut where_parts: Vec<String> = Vec::new();
    if params.account_id.is_some() {
        where_parts.push("account_id = ?".to_string());
    }
    if params.category.is_some() {
        where_parts.push("category = ? COLLATE NOCASE".to_string());
    }
    let where_clause = if where_parts.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_parts.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM emails {where_clause}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(ref v) = params.account_id {
        count_q = count_q.bind(v);
    }
    if let Some(ref v) = params.category {
        count_q = count_q.bind(v);
    }
    let total = count_q
        .fetch_one(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let select_sql = format!(
        "SELECT {EMAIL_COLUMNS} FROM emails {where_clause} ORDER BY received_at DESC LIMIT ? OFFSET ?"
    );
    let mut query = sqlx::query(&select_sql);
    if let Some(ref v) = params.account_id {
        query = query.bind(v);
    }
    if let Some(ref v) = params.category {
        query = query.bind(v);
    }
    query = query.bind(limit).bind(offset);

    let rows = query
        .fetch_all(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let emails = rows.iter().map(row_to_response).collect();
    Ok(Json(ListEmailsResponse { emails, total }))
}

async fn get_email(
    State(state): State<TestState>,
    Path(id): Path<String>,
) -> Result<Json<EmailResponse>, (StatusCode, String)> {
    let sql = format!("SELECT {EMAIL_COLUMNS} FROM emails WHERE id = ?1");
    let row = sqlx::query(&sql)
        .bind(&id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match row {
        Some(r) => Ok(Json(row_to_response(&r))),
        None => Err((StatusCode::NOT_FOUND, "Email not found".to_string())),
    }
}

async fn delete_email(
    State(state): State<TestState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let rows = sqlx::query("DELETE FROM emails WHERE id = ?1")
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Email not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn archive_email(
    State(state): State<TestState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let rows = sqlx::query("UPDATE emails SET labels = 'ARCHIVED' WHERE id = ?1")
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Email not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn star_email(
    State(state): State<TestState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let current: Option<(bool,)> = sqlx::query_as("SELECT is_starred FROM emails WHERE id = ?1")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let new_starred = !current
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Email not found".to_string()))?
        .0;

    sqlx::query("UPDATE emails SET is_starred = ?1 WHERE id = ?2")
        .bind(new_starred)
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn email_counts(
    State(state): State<TestState>,
) -> Result<Json<EmailCounts>, (StatusCode, String)> {
    let (total, unread): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(COUNT(*), 0), \
         COALESCE(SUM(CASE WHEN is_read = 0 THEN 1 ELSE 0 END), 0) \
         FROM emails",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(EmailCounts {
        total: total as u64,
        unread: unread as u64,
    }))
}

async fn list_accounts(
    State(state): State<TestState>,
) -> Result<Json<Vec<AccountResponse>>, (StatusCode, String)> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT id, provider, email_address, status FROM connected_accounts ORDER BY created_at",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let accounts = rows
        .into_iter()
        .map(|(id, provider, email_address, status)| AccountResponse {
            is_active: status == "connected",
            id,
            provider,
            email_address,
            status,
        })
        .collect();

    Ok(Json(accounts))
}

async fn disconnect_account(
    State(state): State<TestState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    uuid::Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid account ID format".to_string(),
        )
    })?;

    let _ = sqlx::query("DELETE FROM emails WHERE account_id = ?1")
        .bind(&id)
        .execute(&state.pool)
        .await;
    let _ = sqlx::query("DELETE FROM sync_state WHERE account_id = ?1")
        .bind(&id)
        .execute(&state.pool)
        .await;

    let rows = sqlx::query(
        "UPDATE connected_accounts SET status = 'disconnected', \
         encrypted_access_token = NULL, encrypted_refresh_token = NULL \
         WHERE id = ?1",
    )
    .bind(&id)
    .execute(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Account not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn record_gdpr_consent(
    State(state): State<TestState>,
    Json(req): Json<GdprConsentRequest>,
) -> Result<Json<GdprConsentResponse>, (StatusCode, String)> {
    let valid_types = ["cloud_ai", "data_export", "analytics", "third_party"];
    if !valid_types.contains(&req.consent_type.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Invalid consent_type '{}'. Must be one of: {}",
                req.consent_type,
                valid_types.join(", ")
            ),
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO consent_decisions (id, consent_type, granted, granted_at, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&id)
    .bind(&req.consent_type)
    .bind(req.granted)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(GdprConsentResponse {
        decision: GdprConsentDecision {
            id,
            consent_type: req.consent_type,
            granted: req.granted,
        },
    }))
}

async fn list_gdpr_consents(
    State(state): State<TestState>,
) -> Result<Json<GdprConsentListResponse>, (StatusCode, String)> {
    let rows: Vec<(String, String, bool)> =
        sqlx::query_as("SELECT id, consent_type, granted FROM consent_decisions ORDER BY created_at")
            .fetch_all(&state.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let decisions = rows
        .into_iter()
        .map(|(id, consent_type, granted)| GdprConsentDecision {
            id,
            consent_type,
            granted,
        })
        .collect();

    Ok(Json(GdprConsentListResponse { decisions }))
}

async fn start_ingestion(
    State(state): State<TestState>,
    Json(req): Json<StartRequest>,
) -> Result<Json<JobResponse>, (StatusCode, String)> {
    let account_id = req.account_id.unwrap_or_else(|| "default".to_string());

    if account_id != "default" {
        let exists: Option<(String,)> =
            sqlx::query_as("SELECT id FROM connected_accounts WHERE id = ?1")
                .bind(&account_id)
                .fetch_optional(&state.pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if exists.is_none() {
            return Err((
                StatusCode::NOT_FOUND,
                format!("Account {account_id} not found"),
            ));
        }
    }

    let job_id = uuid::Uuid::new_v4().to_string();
    Ok(Json(JobResponse {
        job_id,
        status: "started".to_string(),
        message: format!("Ingestion started for account {account_id}"),
    }))
}

async fn health_check() -> StatusCode {
    StatusCode::OK
}

async fn readiness_check(State(state): State<TestState>) -> StatusCode {
    match sqlx::query("SELECT 1").execute(&state.pool).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn erase_user_data(
    State(state): State<TestState>,
    Json(body): Json<ErasureRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if body.confirmation_token != "CONFIRM_ERASE_ALL_DATA" {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid confirmation token".to_string(),
        ));
    }

    let _ = sqlx::query("DELETE FROM emails")
        .execute(&state.pool)
        .await;
    let _ = sqlx::query("DELETE FROM consent_decisions")
        .execute(&state.pool)
        .await;

    Ok(StatusCode::OK)
}

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

async fn setup_test_db() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create in-memory SQLite pool");

    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&pool)
        .await
        .unwrap();

    // Use raw_sql to execute multi-statement migration scripts in one call.
    let migrations: &[&str] = &[
        include_str!("../migrations/001_initial_schema.sql"),
        include_str!("../migrations/002_ai_consent.sql"),
        include_str!("../migrations/003_ai_metadata.sql"),
        include_str!("../migrations/004_accounts.sql"),
        include_str!("../migrations/006_ingestion_checkpoints.sql"),
        include_str!("../migrations/007_per_user_learning.sql"),
        include_str!("../migrations/008_cloud_api_audit.sql"),
        include_str!("../migrations/009_ab_tests.sql"),
        include_str!("../migrations/010_gdpr_consent.sql"),
        include_str!("../migrations/011_sync_queue.sql"),
        include_str!("../migrations/012_rules.sql"),
        include_str!("../migrations/014_attachments.sql"),
    ];

    for sql in migrations {
        sqlx::raw_sql(sql)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("Migration failed: {e}\nSQL:\n{sql}"));
    }

    // Migration 013: add columns (ignore if already exist).
    let _ = sqlx::raw_sql(
        "ALTER TABLE connected_accounts ADD COLUMN sync_depth TEXT NOT NULL DEFAULT '30d';\
         ALTER TABLE connected_accounts ADD COLUMN sync_frequency INTEGER NOT NULL DEFAULT 5;",
    )
    .execute(&pool)
    .await;

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();

    pool
}

fn build_test_router(state: TestState) -> Router {
    let api = Router::new()
        .nest(
            "/emails",
            Router::new()
                .route("/", get(list_emails))
                .route("/counts", get(email_counts))
                .route("/{id}", get(get_email).delete(delete_email))
                .route("/{id}/archive", post(archive_email))
                .route("/{id}/star", post(star_email)),
        )
        .nest(
            "/auth",
            Router::new()
                .route("/accounts", get(list_accounts))
                .route("/accounts/{id}", delete(disconnect_account)),
        )
        .nest(
            "/consent",
            Router::new()
                .route("/gdpr", get(list_gdpr_consents).post(record_gdpr_consent))
                .route("/erase", post(erase_user_data)),
        )
        .nest(
            "/ingestion",
            Router::new().route("/start", post(start_ingestion)),
        );

    Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        .nest("/api/v1", api)
        .with_state(state)
}

async fn insert_test_email(pool: &SqlitePool, id: &str, account_id: &str, subject: &str) {
    sqlx::query(
        "INSERT INTO emails (id, account_id, provider, subject, from_addr, to_addrs, \
         received_at, is_read, is_starred, has_attachments, embedding_status, category) \
         VALUES (?1, ?2, 'gmail', ?3, 'sender@example.com', 'recipient@example.com', \
         datetime('now'), 0, 0, 0, 'pending', 'Inbox')",
    )
    .bind(id)
    .bind(account_id)
    .bind(subject)
    .execute(pool)
    .await
    .expect("Failed to insert test email");
}

async fn insert_test_account(pool: &SqlitePool, id: &str, email: &str) {
    sqlx::query(
        "INSERT INTO connected_accounts (id, provider, email_address, status) \
         VALUES (?1, 'gmail', ?2, 'connected')",
    )
    .bind(id)
    .bind(email)
    .execute(pool)
    .await
    .expect("Failed to insert test account");
}

/// Send a request and return (status, body_bytes).
async fn send_request(app: Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    // Read body using axum's built-in limited reader.
    let body_bytes: Bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, body_bytes.to_vec())
}

/// Send a GET request to the given path.
async fn get_request(app: Router, path: &str) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .uri(path)
        .body(Body::empty())
        .unwrap();
    send_request(app, req).await
}

/// Send a POST request with a JSON body.
async fn post_json(app: Router, path: &str, body: &serde_json::Value) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap();
    send_request(app, req).await
}

/// Send a DELETE request to the given path.
async fn delete_request(app: Router, path: &str) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method("DELETE")
        .uri(path)
        .body(Body::empty())
        .unwrap();
    send_request(app, req).await
}

// ---------------------------------------------------------------------------
// Tests: Health / Readiness
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_health_endpoint_returns_200() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, _) = get_request(app, "/health").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_readiness_endpoint_returns_200() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, _) = get_request(app, "/ready").await;
    assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Tests: Emails
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_emails_empty_db_returns_200() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, body) = get_request(app, "/api/v1/emails").await;
    assert_eq!(status, StatusCode::OK);

    let resp: ListEmailsResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.total, 0);
    assert!(resp.emails.is_empty());
}

#[tokio::test]
async fn test_list_emails_returns_inserted_emails() {
    let pool = setup_test_db().await;
    insert_test_email(&pool, "email-1", "acc-001", "Hello World").await;
    insert_test_email(&pool, "email-2", "acc-001", "Second Email").await;

    let app = build_test_router(TestState { pool });
    let (status, body) = get_request(app, "/api/v1/emails").await;
    assert_eq!(status, StatusCode::OK);

    let resp: ListEmailsResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.total, 2);
    assert_eq!(resp.emails.len(), 2);
}

#[tokio::test]
async fn test_list_emails_pagination() {
    let pool = setup_test_db().await;
    for i in 0..5 {
        insert_test_email(&pool, &format!("email-{i}"), "acc-001", &format!("Email {i}")).await;
    }

    let app = build_test_router(TestState { pool });
    let (status, body) = get_request(app, "/api/v1/emails?limit=2&offset=0").await;
    assert_eq!(status, StatusCode::OK);

    let resp: ListEmailsResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.total, 5);
    assert_eq!(resp.emails.len(), 2);
}

#[tokio::test]
async fn test_list_emails_filter_by_category() {
    let pool = setup_test_db().await;
    insert_test_email(&pool, "email-f1", "acc-001", "Important").await;
    insert_test_email(&pool, "email-f2", "acc-001", "Newsletter").await;
    sqlx::query("UPDATE emails SET category = 'Newsletter' WHERE id = 'email-f2'")
        .execute(&pool)
        .await
        .unwrap();

    let app = build_test_router(TestState { pool });
    let (status, body) = get_request(app, "/api/v1/emails?category=Newsletter").await;
    assert_eq!(status, StatusCode::OK);

    let resp: ListEmailsResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.total, 1);
    assert_eq!(resp.emails[0].category, "Newsletter");
}

#[tokio::test]
async fn test_list_emails_filter_by_account_id() {
    let pool = setup_test_db().await;
    insert_test_email(&pool, "email-a1", "acc-001", "Account 1").await;
    insert_test_email(&pool, "email-a2", "acc-002", "Account 2").await;

    let app = build_test_router(TestState { pool });
    let (status, body) = get_request(app, "/api/v1/emails?accountId=acc-001").await;
    assert_eq!(status, StatusCode::OK);

    let resp: ListEmailsResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.total, 1);
    assert_eq!(resp.emails[0].account_id, "acc-001");
}

#[tokio::test]
async fn test_get_email_by_id() {
    let pool = setup_test_db().await;
    insert_test_email(&pool, "email-42", "acc-001", "Test Subject").await;

    let app = build_test_router(TestState { pool });
    let (status, body) = get_request(app, "/api/v1/emails/email-42").await;
    assert_eq!(status, StatusCode::OK);

    let resp: EmailResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.id, "email-42");
    assert_eq!(resp.subject, "Test Subject");
    assert_eq!(resp.from_addr, "sender@example.com");
}

#[tokio::test]
async fn test_get_email_not_found_returns_404() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, _) = get_request(app, "/api/v1/emails/nonexistent").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_email() {
    let pool = setup_test_db().await;
    insert_test_email(&pool, "email-del", "acc-001", "To Delete").await;

    let app = build_test_router(TestState { pool: pool.clone() });
    let (status, _) = delete_request(app, "/api/v1/emails/email-del").await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM emails WHERE id = 'email-del'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
}

#[tokio::test]
async fn test_delete_email_not_found_returns_404() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, _) = delete_request(app, "/api/v1/emails/nonexistent").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_archive_email() {
    let pool = setup_test_db().await;
    insert_test_email(&pool, "email-arc", "acc-001", "To Archive").await;

    let app = build_test_router(TestState { pool: pool.clone() });
    let (status, _) = post_json(app, "/api/v1/emails/email-arc/archive", &serde_json::json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (labels,): (String,) = sqlx::query_as("SELECT labels FROM emails WHERE id = 'email-arc'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(labels, "ARCHIVED");
}

#[tokio::test]
async fn test_archive_email_not_found_returns_404() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, _) = post_json(app, "/api/v1/emails/nonexistent/archive", &serde_json::json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_star_email_toggles() {
    let pool = setup_test_db().await;
    insert_test_email(&pool, "email-star", "acc-001", "Starrable").await;

    let app = build_test_router(TestState { pool: pool.clone() });
    let (status, _) = post_json(app, "/api/v1/emails/email-star/star", &serde_json::json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (starred,): (bool,) =
        sqlx::query_as("SELECT is_starred FROM emails WHERE id = 'email-star'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(starred);
}

#[tokio::test]
async fn test_star_email_not_found_returns_404() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, _) = post_json(app, "/api/v1/emails/nonexistent/star", &serde_json::json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_email_counts_empty_db() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, body) = get_request(app, "/api/v1/emails/counts").await;
    assert_eq!(status, StatusCode::OK);

    let resp: EmailCounts = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.total, 0);
    assert_eq!(resp.unread, 0);
}

#[tokio::test]
async fn test_email_counts_with_data() {
    let pool = setup_test_db().await;
    insert_test_email(&pool, "email-c1", "acc-001", "Email 1").await;
    insert_test_email(&pool, "email-c2", "acc-001", "Email 2").await;
    sqlx::query("UPDATE emails SET is_read = 1 WHERE id = 'email-c1'")
        .execute(&pool)
        .await
        .unwrap();

    let app = build_test_router(TestState { pool });
    let (status, body) = get_request(app, "/api/v1/emails/counts").await;
    assert_eq!(status, StatusCode::OK);

    let resp: EmailCounts = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.total, 2);
    assert_eq!(resp.unread, 1);
}

// ---------------------------------------------------------------------------
// Tests: Accounts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_accounts_empty_returns_200() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, body) = get_request(app, "/api/v1/auth/accounts").await;
    assert_eq!(status, StatusCode::OK);

    let resp: Vec<AccountResponse> = serde_json::from_slice(&body).unwrap();
    assert!(resp.is_empty());
}

#[tokio::test]
async fn test_list_accounts_returns_accounts() {
    let pool = setup_test_db().await;
    insert_test_account(&pool, "acc-uuid-001", "user@example.com").await;

    let app = build_test_router(TestState { pool });
    let (status, body) = get_request(app, "/api/v1/auth/accounts").await;
    assert_eq!(status, StatusCode::OK);

    let resp: Vec<AccountResponse> = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.len(), 1);
    assert_eq!(resp[0].email_address, "user@example.com");
    assert_eq!(resp[0].provider, "gmail");
    assert!(resp[0].is_active);
}

#[tokio::test]
async fn test_disconnect_account_invalid_uuid_returns_400() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, _) = delete_request(app, "/api/v1/auth/accounts/not-a-uuid").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_disconnect_account_not_found_returns_404() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let fake_uuid = uuid::Uuid::new_v4().to_string();
    let (status, _) = delete_request(app, &format!("/api/v1/auth/accounts/{fake_uuid}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_disconnect_account_success() {
    let pool = setup_test_db().await;
    let account_id = uuid::Uuid::new_v4().to_string();
    insert_test_account(&pool, &account_id, "disconnect@example.com").await;

    let app = build_test_router(TestState { pool: pool.clone() });
    let (status, _) =
        delete_request(app, &format!("/api/v1/auth/accounts/{account_id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (db_status,): (String,) =
        sqlx::query_as("SELECT status FROM connected_accounts WHERE id = ?1")
            .bind(&account_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(db_status, "disconnected");
}

// ---------------------------------------------------------------------------
// Tests: Consent (GDPR)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_record_gdpr_consent_returns_200() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, body) = post_json(
        app,
        "/api/v1/consent/gdpr",
        &serde_json::json!({"consent_type": "cloud_ai", "granted": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp: GdprConsentResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.decision.consent_type, "cloud_ai");
    assert!(resp.decision.granted);
}

#[tokio::test]
async fn test_record_gdpr_consent_invalid_type_returns_400() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, _) = post_json(
        app,
        "/api/v1/consent/gdpr",
        &serde_json::json!({"consent_type": "invalid_type", "granted": true}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_list_gdpr_consents_empty() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, body) = get_request(app, "/api/v1/consent/gdpr").await;
    assert_eq!(status, StatusCode::OK);

    let resp: GdprConsentListResponse = serde_json::from_slice(&body).unwrap();
    assert!(resp.decisions.is_empty());
}

#[tokio::test]
async fn test_list_gdpr_consents_after_recording() {
    let pool = setup_test_db().await;

    // Record a consent directly in DB.
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO consent_decisions (id, consent_type, granted, created_at) \
         VALUES (?1, 'analytics', 1, datetime('now'))",
    )
    .bind(&id)
    .execute(&pool)
    .await
    .unwrap();

    let app = build_test_router(TestState { pool });
    let (status, body) = get_request(app, "/api/v1/consent/gdpr").await;
    assert_eq!(status, StatusCode::OK);

    let resp: GdprConsentListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.decisions.len(), 1);
    assert_eq!(resp.decisions[0].consent_type, "analytics");
}

// ---------------------------------------------------------------------------
// Tests: Ingestion
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ingestion_start_with_nonexistent_account_returns_404() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, _) = post_json(
        app,
        "/api/v1/ingestion/start",
        &serde_json::json!({"account_id": "nonexistent-account"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_ingestion_start_default_account_returns_200() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, body) = post_json(
        app,
        "/api/v1/ingestion/start",
        &serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp: JobResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.status, "started");
    assert!(!resp.job_id.is_empty());
}

// ---------------------------------------------------------------------------
// Tests: Erasure (GDPR Art 17)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_erase_requires_confirmation_token() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, _) = post_json(
        app,
        "/api/v1/consent/erase",
        &serde_json::json!({"confirmation_token": "WRONG_TOKEN"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_erase_with_valid_token() {
    let pool = setup_test_db().await;
    insert_test_email(&pool, "email-erase", "acc-001", "Erasable").await;

    let app = build_test_router(TestState { pool: pool.clone() });
    let (status, _) = post_json(
        app,
        "/api/v1/consent/erase",
        &serde_json::json!({"confirmation_token": "CONFIRM_ERASE_ALL_DATA"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM emails")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

// ---------------------------------------------------------------------------
// Tests: Routing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_nonexistent_route_returns_404() {
    let pool = setup_test_db().await;
    let app = build_test_router(TestState { pool });

    let (status, _) = get_request(app, "/api/v1/nonexistent").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
