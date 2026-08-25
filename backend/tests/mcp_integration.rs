//! End-to-end integration tests for the MCP server (A6, design §8.5).
//!
//! Drives the SAME `mcp_service(ctx, registry)` constructor `main.rs` mounts —
//! not a probe, not a reimplementation — over the streamable-HTTP transport
//! with raw JSON-RPC (rmcp is built without the `client` feature) plus manual
//! SSE decoding. stdio is exercised in-process over `tokio::io::duplex`.
//!
//! Run with: cargo test --test mcp_integration

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use emailibrium::db::Database;
use emailibrium::email::oauth::OAuthManager;
use emailibrium::mcp::mcp_service;
use emailibrium::mcp::server::EmailibriumMcpServer;
use emailibrium::tools::config::ToolsConfig;
use emailibrium::tools::rate_limit::ToolRateLimiter;
use emailibrium::tools::{declarations, ToolContext, ToolRegistry};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use serde_json::{json, Value};
use tower::ServiceExt as _;

type McpService = StreamableHttpService<EmailibriumMcpServer, LocalSessionManager>;

/// The frozen public tool surface, sorted (README.md tool table).
const FIFTEEN_TOOLS: [&str; 15] = [
    "count_emails",
    "find_similar_emails",
    "get_email",
    "get_email_thread",
    "get_insights",
    "get_learning_metrics",
    "get_sync_status",
    "list_accounts",
    "list_attachments",
    "list_clusters",
    "list_recent_emails",
    "list_rules",
    "list_subscriptions",
    "preview_cleanup_plan",
    "search_emails",
];

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// Full migration run against a file-backed database.
///
/// Not `sqlite::memory:`: in-memory SQLite is per-connection and the pool
/// opens several, which is why `api_integration.rs` hand-lists migrations as a
/// workaround. A temp file has no such problem, and `run_migrations()` picks
/// up new migrations without anyone remembering to add them here.
async fn migrated_db() -> (tempfile::TempDir, Arc<Database>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let url = format!("sqlite://{}?mode=rwc", dir.path().join("test.db").display());
    let db = Database::connect(&url).await.expect("connect");
    db.run_migrations().await.expect("migrations");
    (dir, Arc::new(db))
}

const ACCOUNT_ID: &str = "11111111-1111-4111-8111-111111111111";
const THREAD_KEY: &str = "<msg/1@example.com>";
/// `THREAD_KEY`, percent-encoded for use inside a `thread://` URI.
const THREAD_KEY_ENCODED: &str = "%3Cmsg%2F1%40example.com%3E";

async fn seed(conn: &sea_orm::DatabaseConnection) {
    use sea_orm::ConnectionTrait;

    fn stmt<I>(sql: &str, values: I) -> sea_orm::Statement
    where
        I: IntoIterator<Item = sea_orm::Value>,
    {
        sea_orm::Statement::from_sql_and_values(sea_orm::DatabaseBackend::Sqlite, sql, values)
    }

    // `connected_accounts` has CHECK constraints on provider/status, and
    // timestamps are parsed RFC3339-first — a row failing the parse is
    // silently dropped from list_accounts, so be explicit.
    conn.execute_raw(stmt(
        "INSERT INTO connected_accounts (id, provider, email_address, status, created_at, updated_at) \
         VALUES (?1, 'gmail', 'user@example.com', 'connected', \
                 '2026-07-01T00:00:00+00:00', '2026-07-01T00:00:00+00:00')",
        [ACCOUNT_ID.into()],
    ))
    .await
    .expect("seed account");

    for (id, thread, subject, from_addr, from_name, ago) in [
        (
            "email-1",
            THREAD_KEY,
            "Quarterly report",
            "alice@example.com",
            Some("Alice"),
            30,
        ),
        (
            "email-2",
            THREAD_KEY,
            "Re: Quarterly report",
            "bob@example.com",
            None,
            20,
        ),
        (
            "email-3",
            "thread-solo",
            "Lunch?",
            "carol@example.com",
            None,
            10,
        ),
    ] {
        conn.execute_raw(stmt(
            "INSERT INTO emails (id, account_id, provider, subject, from_addr, from_name, to_addrs, \
             received_at, body_text, is_read, is_starred, has_attachments, embedding_status, \
             category, thread_key) \
             VALUES (?1, ?2, 'gmail', ?3, ?4, ?5, 'me@example.com', datetime('now', ?6), \
                     'body of ' || ?3, 0, 0, 0, 'pending', 'Inbox', ?7)",
            [
                id.into(),
                ACCOUNT_ID.into(),
                subject.into(),
                from_addr.into(),
                from_name.into(),
                format!("-{ago} minutes").into(),
                thread.into(),
            ],
        ))
        .await
        .expect("seed email");
    }

    // Sentinel values so the redaction assertion is real, not decorative.
    conn.execute_raw(stmt(
        "INSERT INTO attachments (id, email_id, account_id, filename, content_type, size_bytes, \
         is_inline, storage_path, provider_attachment_id, fetch_status) \
         VALUES ('att-1', 'email-1', ?1, 'report.pdf', 'application/pdf', 2048, FALSE, \
                 '/var/data/SENTINEL-PATH/report.pdf', 'SENTINEL-PROVIDER-ID', 'fetched')",
        [ACCOUNT_ID.into()],
    ))
    .await
    .expect("seed attachment");
}

async fn audit_rows(conn: &sea_orm::DatabaseConnection) -> Vec<(String, String, String)> {
    use sea_orm::ConnectionTrait;

    conn.query_all_raw(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT tool_name, result_status, source FROM mcp_tool_audit ORDER BY id".to_owned(),
    ))
    .await
    .expect("audit query")
    .iter()
    .map(|row| {
        (
            row.try_get_by_index(0).expect("tool_name"),
            row.try_get_by_index(1).expect("result_status"),
            row.try_get_by_index(2).expect("source"),
        )
    })
    .collect()
}

struct Env {
    _dir: tempfile::TempDir,
    db: Arc<Database>,
    registry: Arc<ToolRegistry>,
    /// One instance, cloned per request: a second `mcp_service` would get a
    /// fresh `LocalSessionManager`, so session ids would not carry across.
    service: McpService,
    next_id: AtomicI64,
}

impl Env {
    async fn new(config: ToolsConfig, limiter: Option<Arc<ToolRateLimiter>>) -> Self {
        let (_dir, db) = migrated_db().await;
        seed(&db.sea_orm()).await;
        let ctx = Arc::new(
            ToolContext::new(db.clone())
                .with_oauth(Arc::new(OAuthManager::new((*db).clone(), None))),
        );
        let registry = Arc::new(match limiter {
            Some(l) => ToolRegistry::with_rate_limiter(declarations(), config, l),
            None => ToolRegistry::new(declarations(), config),
        });
        let service = mcp_service(ctx, registry.clone());
        Self {
            _dir,
            db,
            registry,
            service,
            next_id: AtomicI64::new(1),
        }
    }

    async fn default_env() -> Self {
        Self::new(ToolsConfig::default(), None).await
    }
}

// ---------------------------------------------------------------------------
// Wire driver (streamable HTTP + SSE decode)
// ---------------------------------------------------------------------------

const MCP_ACCEPT: &str = "application/json, text/event-stream";

async fn post(
    service: &McpService,
    session: Option<&str>,
    body: &Value,
) -> (StatusCode, Option<String>, Vec<u8>) {
    let mut b = Request::builder()
        .method("POST")
        .uri("/")
        // rmcp validates Host against `allowed_hosts`, which defaults to
        // loopback only. `oneshot` sends no Host of its own.
        .header("host", "localhost")
        // Both MIME types or 406; `application/json` content-type or 415.
        .header("accept", MCP_ACCEPT)
        .header("content-type", "application/json");
    if let Some(id) = session {
        b = b.header("mcp-session-id", id);
    }
    let req = b
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap();

    let resp = tokio::time::timeout(Duration::from_secs(15), service.clone().oneshot(req))
        .await
        .expect("MCP request timed out")
        .expect("Infallible");

    let status = resp.status();
    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    // `Body::new` adapts rmcp's boxed body; http-body-util is not a direct
    // dependency here and would not compile.
    let bytes = tokio::time::timeout(
        Duration::from_secs(15),
        axum::body::to_bytes(Body::new(resp.into_body()), usize::MAX),
    )
    .await
    .expect("SSE body never terminated")
    .expect("body");
    (status, sid, bytes.to_vec())
}

/// Decode the SSE framing rmcp wraps every response in (the mount is stateful
/// with `json_response: false`, and `sse_retry` prepends a priming event with
/// empty data — hence the emptiness filter).
fn sse_frames(body: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(body)
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| serde_json::from_str(s).expect("SSE data frame must be JSON"))
        .collect()
}

fn sole_frame(body: &[u8]) -> Value {
    let mut f = sse_frames(body);
    assert_eq!(f.len(), 1, "expected one JSON-RPC frame, got {f:?}");
    f.pop().unwrap()
}

impl Env {
    async fn open_session(&self) -> String {
        let body = json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {
                "protocolVersion": rmcp::model::ProtocolVersion::LATEST.as_str(),
                "capabilities": {},
                "clientInfo": { "name": "mcp_integration", "version": "0" }
            }
        });
        let (status, sid, raw) = post(&self.service, None, &body).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "initialize: {}",
            String::from_utf8_lossy(&raw)
        );
        let sid = sid.expect("initialize must return Mcp-Session-Id");

        let (status, _, _) = post(
            &self.service,
            Some(&sid),
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        sid
    }

    async fn rpc(&self, session: &str, method: &str, params: Value) -> Value {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (status, _, raw) = post(
            &self.service,
            Some(session),
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{method}: {}",
            String::from_utf8_lossy(&raw)
        );
        sole_frame(&raw)
    }

    async fn call_tool(&self, s: &str, name: &str, args: Value) -> Value {
        self.rpc(s, "tools/call", json!({"name": name, "arguments": args}))
            .await
    }

    async fn read_resource(&self, s: &str, uri: &str) -> Value {
        self.rpc(s, "resources/read", json!({"uri": uri})).await
    }

    async fn wire_tool_names(&self, s: &str) -> Vec<String> {
        let listed = self.rpc(s, "tools/list", json!({})).await;
        let mut names: Vec<String> = ok(&listed)["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        names.sort();
        names
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC envelope helpers
// ---------------------------------------------------------------------------

fn ok(env: &Value) -> &Value {
    assert!(
        env.get("error").is_none(),
        "expected success, got error: {env}"
    );
    &env["result"]
}

fn err(env: &Value) -> &Value {
    assert!(
        env.get("result").is_none(),
        "expected a protocol error, got success: {env}"
    );
    &env["error"]
}

fn err_code(env: &Value) -> i64 {
    err(env)["code"].as_i64().expect("error code")
}

fn err_kind(env: &Value) -> String {
    err(env)["data"]["kind"]
        .as_str()
        .unwrap_or_else(|| panic!("no data.kind in {env}"))
        .to_string()
}

fn tool_payload(env: &Value) -> Value {
    serde_json::from_str(
        ok(env)["content"][0]["text"]
            .as_str()
            .expect("text content"),
    )
    .expect("tool payload is JSON")
}

fn resource_payload(env: &Value) -> Value {
    serde_json::from_str(
        ok(env)["contents"][0]["text"]
            .as_str()
            .expect("text contents"),
    )
    .expect("resource payload is JSON")
}

/// Recursive leak check — a nested field must not hide a redaction failure.
fn contains_anywhere(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(s) => s.contains(needle),
        Value::Array(items) => items.iter().any(|v| contains_anywhere(v, needle)),
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| k.contains(needle) || contains_anywhere(v, needle)),
        _ => false,
    }
}

/// A tools.yaml written to a tempdir, so per-test policy needs no env vars
/// (integration tests share one process and run in parallel).
fn config_from_yaml(yaml: &str) -> (tempfile::TempDir, ToolsConfig) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("tools.yaml"), yaml).expect("write tools.yaml");
    let cfg = ToolsConfig::load(dir.path().to_str().unwrap());
    assert!(
        !cfg.tools.is_empty(),
        "custom tools.yaml was not loaded — check EMAILIBRIUM_CONFIG_DIR is unset"
    );
    (dir, cfg)
}

// ---------------------------------------------------------------------------
// Tests — tools
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tools_list_returns_exactly_the_fifteen_documented_tools() {
    let env = Env::default_env().await;
    let s = env.open_session().await;
    let names = env.wire_tool_names(&s).await;
    assert_eq!(
        names, FIFTEEN_TOOLS,
        "wire tools/list must equal the README table, sorted"
    );
}

#[tokio::test]
async fn the_wire_and_the_chat_orchestrator_expose_identical_tools() {
    // §8.5's highest-value assertion: the strongest guard against the
    // ai.rs duplication silently returning.
    let env = Env::default_env().await;
    let s = env.open_session().await;
    let wire = env.wire_tool_names(&s).await;
    let mut chat: Vec<String> = env
        .registry
        .chat_definitions()
        .iter()
        .map(|d| d.name.clone())
        .collect();
    chat.sort();
    assert_eq!(
        wire, chat,
        "MCP wire tools and chat definitions must never drift"
    );
}

#[tokio::test]
async fn happy_paths_return_success_payloads() {
    let env = Env::default_env().await;
    let s = env.open_session().await;

    let email = tool_payload(
        &env.call_tool(&s, "get_email", json!({"email_id": "email-1"}))
            .await,
    );
    assert_eq!(email["subject"], "Quarterly report");

    let recent = tool_payload(&env.call_tool(&s, "list_recent_emails", json!({})).await);
    let recent_list = recent["emails"].as_array().expect("emails array");
    assert_eq!(recent_list.len(), 3, "three seeded emails: {recent}");

    let thread = tool_payload(
        &env.call_tool(&s, "get_email_thread", json!({"email_id": "email-1"}))
            .await,
    );
    assert_eq!(
        thread["emails"].as_array().expect("thread emails").len(),
        2,
        "two emails share the seeded thread key: {thread}"
    );

    // Empty-but-successful is a valid happy path for list_rules.
    let rules = env.call_tool(&s, "list_rules", json!({})).await;
    ok(&rules);

    // list_accounts requires the oauth service (wired in the fixture).
    let accounts = tool_payload(&env.call_tool(&s, "list_accounts", json!({})).await);
    assert!(
        contains_anywhere(&accounts, "user@example.com"),
        "seeded account visible: {accounts}"
    );
}

#[tokio::test]
async fn invalid_and_missing_inputs_return_protocol_errors() {
    let env = Env::default_env().await;
    let s = env.open_session().await;

    // Unknown id → resource_not_found with the variant in data.kind.
    let missing = env
        .call_tool(&s, "get_email", json!({"email_id": "no-such-id"}))
        .await;
    assert_eq!(err_code(&missing), -32002, "{missing}");
    assert_eq!(err_kind(&missing), "not_found");

    // Structurally invalid arguments → invalid_params.
    let invalid = env.call_tool(&s, "get_email", json!({})).await;
    assert_eq!(err_code(&invalid), -32602, "{invalid}");

    // No error may ever be success-shaped (the :109 regression class).
    for envl in [&missing, &invalid] {
        assert!(
            envl.get("result").is_none(),
            "error presented as success: {envl}"
        );
    }
}

#[tokio::test]
async fn a_tool_needing_an_unwired_service_reports_not_configured() {
    let env = Env::default_env().await; // no VectorService in the fixture
    let s = env.open_session().await;
    let resp = env
        .call_tool(&s, "search_emails", json!({"query": "report"}))
        .await;
    assert_eq!(err_code(&resp), -32603, "{resp}");
    assert_eq!(err_kind(&resp), "not_configured");
}

#[tokio::test]
async fn a_disabled_tool_vanishes_from_the_router_entirely() {
    // Contract finding (tester-2): a tools.yaml-disabled tool is never
    // registered as a route, so rmcp answers -32602 "tool not found" with no
    // data.kind — ToolError::Denied is NOT reachable over tools/call. The
    // observable effects are disappearance from tools/list and the router's
    // own invalid_params on call.
    let yaml = "version: \"1.0\"\ndefaults:\n  rate_limit_per_minute: 20\ntools:\n  get_email:\n    enabled: false\n";
    let (_cfg_dir, cfg) = config_from_yaml(yaml);
    let env = Env::new(cfg, None).await;
    let s = env.open_session().await;

    let names = env.wire_tool_names(&s).await;
    assert!(
        !names.contains(&"get_email".to_string()),
        "disabled tool still listed: {names:?}"
    );
    assert_eq!(names.len(), 14);

    let resp = env
        .call_tool(&s, "get_email", json!({"email_id": "email-1"}))
        .await;
    assert_eq!(
        err_code(&resp),
        -32602,
        "router-level tool-not-found: {resp}"
    );

    // An unknown name takes the identical path — disabled and nonexistent are
    // indistinguishable to a wire caller, which is the intended posture.
    let unknown = env.call_tool(&s, "no_such_tool", json!({})).await;
    assert_eq!(err_code(&unknown), -32602, "{unknown}");
}

// ---------------------------------------------------------------------------
// Tests — enforcement (rate limits, policy, audit)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rate_limits_are_shared_across_surfaces_and_sessions() {
    // Limit 2/min with no per-tool overrides (ToolsConfig::default()).
    let env = Env::new(
        ToolsConfig::default(),
        Some(Arc::new(ToolRateLimiter::new(2))),
    )
    .await;
    let s = env.open_session().await;

    ok(&env
        .call_tool(&s, "get_email", json!({"email_id": "email-1"}))
        .await);
    ok(&env
        .call_tool(&s, "get_email", json!({"email_id": "email-2"}))
        .await);

    // Third tool call → throttled, with the transient/permanent distinction.
    let throttled = env
        .call_tool(&s, "get_email", json!({"email_id": "email-3"}))
        .await;
    assert_eq!(err_code(&throttled), -32001, "{throttled}");
    assert_eq!(err_kind(&throttled), "rate_limited");

    // Cross-surface: email:// dispatches the get_email tool, so it draws on
    // the SAME window — the one-enforcement-plane property, asserted.
    let via_resource = env.read_resource(&s, "email://email-1").await;
    assert_eq!(err_code(&via_resource), -32001, "{via_resource}");
    assert_eq!(err_kind(&via_resource), "rate_limited");

    // Cross-session: reconnecting must NOT reset the window (the limiter is
    // app-wide; the per-session-factory bug is pinned fixed here).
    let s2 = env.open_session().await;
    let after_reconnect = env
        .call_tool(&s2, "get_email", json!({"email_id": "email-1"}))
        .await;
    assert_eq!(
        err_code(&after_reconnect),
        -32001,
        "reconnect reset the limit: {after_reconnect}"
    );

    // The trail tells the same story the clients saw.
    let rows = audit_rows(&env.db.sea_orm()).await;
    let statuses: Vec<&str> = rows.iter().map(|(_, st, _)| st.as_str()).collect();
    assert_eq!(
        statuses,
        vec![
            "success",
            "success",
            "rate_limited",
            "rate_limited",
            "rate_limited"
        ],
        "{rows:?}"
    );
    // Tool calls audit as source=mcp; the resource read audits as resource.
    let sources: Vec<&str> = rows.iter().map(|(_, _, src)| src.as_str()).collect();
    assert_eq!(
        sources,
        vec!["mcp", "mcp", "mcp", "resource", "mcp"],
        "{rows:?}"
    );
}

#[tokio::test]
async fn disabled_tool_policy_reaches_resource_reads() {
    // The Denied path IS observable via resources/read: the resource layer
    // dispatches through the registry, which applies tools.yaml policy.
    let yaml = "version: \"1.0\"\ndefaults:\n  rate_limit_per_minute: 20\ntools:\n  get_email:\n    enabled: false\n";
    let (_cfg_dir, cfg) = config_from_yaml(yaml);
    let env = Env::new(cfg, None).await;
    let s = env.open_session().await;

    let resp = env.read_resource(&s, "email://email-1").await;
    assert_eq!(err_code(&resp), -32600, "{resp}");
    assert_eq!(err_kind(&resp), "denied");
}

#[tokio::test]
async fn throttled_thread_reads_are_tagged_on_all_four_axes() {
    // thread:// keeps its own limiter bucket (resource:thread_read), so it
    // needs its own exhaustion loop — exhausting get_email does not touch it.
    let env = Env::new(
        ToolsConfig::default(),
        Some(Arc::new(ToolRateLimiter::new(1))),
    )
    .await;
    let s = env.open_session().await;

    let uri = format!("thread://{THREAD_KEY_ENCODED}");
    ok(&env.read_resource(&s, &uri).await);

    let throttled = env.read_resource(&s, &uri).await;
    // Axis 1: wire code. Axis 2: variant discriminator.
    assert_eq!(err_code(&throttled), -32001, "{throttled}");
    assert_eq!(err_kind(&throttled), "rate_limited");

    // Axes 3 + 4: the audit row agrees with the client, and stays a resource.
    let rows = audit_rows(&env.db.sea_orm()).await;
    let last = rows.last().expect("audit row for the throttled read");
    assert_eq!(last.0, "resource:thread_read");
    assert_eq!(last.1, "rate_limited");
    assert_eq!(last.2, "resource");
}

// ---------------------------------------------------------------------------
// Tests — resources and prompts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resources_read_end_to_end_with_policy_grade_errors() {
    let env = Env::default_env().await;
    let s = env.open_session().await;

    // Listings advertise the surface.
    let listed = env.rpc(&s, "resources/list", json!({})).await;
    assert!(
        !ok(&listed)["resources"]
            .as_array()
            .expect("resources")
            .is_empty(),
        "{listed}"
    );
    let templates = env.rpc(&s, "resources/templates/list", json!({})).await;
    assert!(
        !ok(&templates)["resourceTemplates"]
            .as_array()
            .expect("resourceTemplates")
            .is_empty(),
        "{templates}"
    );

    // email://{id}
    let email = resource_payload(&env.read_resource(&s, "email://email-1").await);
    assert_eq!(email["subject"], "Quarterly report");

    // thread://{key} with a URL-encoded, Message-ID-shaped key.
    let thread = resource_payload(
        &env.read_resource(&s, &format!("thread://{THREAD_KEY_ENCODED}"))
            .await,
    );
    assert_eq!(
        thread["emails"].as_array().expect("thread emails").len(),
        2,
        "{thread}"
    );

    // insights://summary
    let insights = env.read_resource(&s, "insights://summary").await;
    assert!(resource_payload(&insights).is_object());

    // Absent must read as not-found — never an empty success.
    let garbage = env.read_resource(&s, "thread://no-such-thread").await;
    assert_eq!(err_code(&garbage), -32002, "{garbage}");
    assert_eq!(err_kind(&garbage), "not_found");

    // Resource reads are attributed as resources in the trail.
    let rows = audit_rows(&env.db.sea_orm()).await;
    assert!(
        rows.iter().any(|(_, _, src)| src == "resource"),
        "no resource-sourced audit rows: {rows:?}"
    );
}

#[tokio::test]
async fn prompts_are_listed_and_retrievable() {
    let env = Env::default_env().await;
    let s = env.open_session().await;

    let listed = env.rpc(&s, "prompts/list", json!({})).await;
    let names: Vec<&str> = ok(&listed)["prompts"]
        .as_array()
        .expect("prompts")
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"triage-inbox"), "{names:?}");
    assert!(names.contains(&"weekly-report"), "{names:?}");

    let prompt = env
        .rpc(
            &s,
            "prompts/get",
            json!({"name": "triage-inbox", "arguments": {}}),
        )
        .await;
    assert!(
        !ok(&prompt)["messages"]
            .as_array()
            .expect("messages")
            .is_empty(),
        "{prompt}"
    );
}

// ---------------------------------------------------------------------------
// Tests — read-only and redaction guarantees
// ---------------------------------------------------------------------------

async fn count_cleanup_plans(conn: &sea_orm::DatabaseConnection) -> Result<i64, sea_orm::DbErr> {
    use sea_orm::ConnectionTrait;

    Ok(conn
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM cleanup_plans".to_owned(),
        ))
        .await?
        .map(|row| row.try_get_by_index(0))
        .transpose()?
        .unwrap_or(0))
}

#[tokio::test]
async fn preview_cleanup_plan_is_strictly_read_only() {
    let env = Env::default_env().await;
    let s = env.open_session().await;

    let before: i64 = count_cleanup_plans(&env.db.sea_orm()).await.expect("count");

    let resp = env
        .call_tool(&s, "preview_cleanup_plan", json!({"user_id": "test-user"}))
        .await;
    ok(&resp);

    let after: i64 = count_cleanup_plans(&env.db.sea_orm()).await.expect("count");
    assert_eq!(
        before, after,
        "preview persisted a plan — the dry-run guarantee is broken"
    );
}

#[tokio::test]
async fn attachment_listings_redact_storage_internals() {
    let env = Env::default_env().await;
    let s = env.open_session().await;

    let listed = tool_payload(
        &env.call_tool(&s, "list_attachments", json!({"email_id": "email-1"}))
            .await,
    );
    assert!(
        contains_anywhere(&listed, "report.pdf"),
        "seeded attachment visible: {listed}"
    );
    for sentinel in ["SENTINEL-PATH", "SENTINEL-PROVIDER-ID"] {
        assert!(
            !contains_anywhere(&listed, sentinel),
            "storage internal {sentinel} leaked: {listed}"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests — stdio transport parity
// ---------------------------------------------------------------------------

async fn write_line<W: tokio::io::AsyncWrite + Unpin>(w: &mut W, v: &Value) {
    use tokio::io::AsyncWriteExt;
    w.write_all(serde_json::to_string(v).unwrap().as_bytes())
        .await
        .unwrap();
    w.write_all(b"\n").await.unwrap();
    w.flush().await.unwrap();
}

async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(r: &mut R) -> Value {
    use tokio::io::AsyncBufReadExt;
    let mut line = String::new();
    let n = tokio::time::timeout(Duration::from_secs(15), r.read_line(&mut line))
        .await
        .expect("stdio read timed out")
        .expect("read");
    assert!(n > 0, "stdio stream closed before a reply arrived");
    serde_json::from_str(&line).expect("stdio frame must be newline-delimited JSON-RPC")
}

#[tokio::test]
async fn stdio_serves_the_identical_tool_list_as_http() {
    // The real risk of a second transport is silent drift, so the assertion
    // that matters is equality with the HTTP surface, not mere startup.
    let (_dir, db) = migrated_db().await;
    let ctx = Arc::new(ToolContext::new(db));
    let registry = Arc::new(ToolRegistry::new(declarations(), ToolsConfig::default()));
    let server = EmailibriumMcpServer::new(ctx, registry);

    let (server_side, client_side) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        use rmcp::ServiceExt as _; // no method clash with tower::ServiceExt
        let running = server.serve(server_side).await.expect("stdio serve");
        let _ = running.waiting().await;
    });

    let (r, mut w) = tokio::io::split(client_side);
    let mut r = tokio::io::BufReader::new(r);

    write_line(
        &mut w,
        &json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": rmcp::model::ProtocolVersion::LATEST.as_str(),
                "capabilities": {},
                "clientInfo": { "name": "mcp_integration_stdio", "version": "0" }
            }
        }),
    )
    .await;
    let _init = read_line(&mut r).await;

    write_line(
        &mut w,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await;
    write_line(
        &mut w,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )
    .await;
    let listed = read_line(&mut r).await;

    task.abort();

    let mut stdio_names: Vec<String> = listed["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    stdio_names.sort();

    // The HTTP side of the comparison, over the mounted service.
    let env = Env::default_env().await;
    let s = env.open_session().await;
    let http_names = env.wire_tool_names(&s).await;

    assert_eq!(stdio_names, http_names, "stdio and HTTP surfaces drifted");
    assert_eq!(stdio_names, FIFTEEN_TOOLS);
}
