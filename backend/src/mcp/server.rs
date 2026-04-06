//! MCP server handler for emailibrium (ADR-028).
//!
//! Implements the `ServerHandler` trait from `rmcp` so the emailibrium
//! backend can serve MCP tool calls over Streamable HTTP, sharing the
//! same Axum process and `AppState`.

use std::sync::Arc;
use std::time::Instant;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};

use crate::AppState;

use super::audit;
use super::rate_limit;
use super::tools::email::{
    validate_date, validate_email_id, validate_limit, validate_query, CountEmailsRequest, EmailRow,
    GetEmailRequest, GetEmailThreadRequest, InsightsRow, ListRecentEmailsRequest, RuleRow,
    SearchEmailsRequest, SenderRow, ThreadEmailRow,
};
use crate::vectors::search::{HybridSearchQuery, SearchMode};

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

/// The MCP server that exposes emailibrium capabilities as tools.
///
/// Holds an `Arc<AppState>` so tool methods can access the same services
/// (database, vector search, etc.) used by the REST API.
#[derive(Clone)]
pub struct EmailibriumMcpServer {
    tool_router: ToolRouter<Self>,
    state: Arc<AppState>,
    rate_limiter: Arc<rate_limit::ToolRateLimiter>,
}

impl EmailibriumMcpServer {
    pub fn new(state: Arc<AppState>) -> Self {
        let tool_router = Self::tool_router();
        Self {
            tool_router,
            state,
            rate_limiter: Arc::new(rate_limit::ToolRateLimiter::new(20)),
        }
    }

    /// Log a tool invocation to the audit trail (best-effort, never fails the call).
    async fn audit(
        &self,
        tool: &str,
        args: &impl serde::Serialize,
        status: &'static str,
        start: Instant,
    ) {
        let entry = audit::ToolCallAuditEntry {
            timestamp: chrono::Utc::now(),
            tool_name: tool.to_string(),
            arguments_hash: audit::hash_arguments(&serde_json::to_value(args).unwrap_or_default()),
            result_status: status,
            latency_ms: start.elapsed().as_millis() as u64,
        };
        audit::log_tool_call(&self.state.db.pool, &entry).await;
    }
}

// ---------------------------------------------------------------------------
// Tool definitions (ADR-028 Phase 5: async implementations)
// ---------------------------------------------------------------------------

#[tool_router]
impl EmailibriumMcpServer {
    /// Search emails using hybrid vector + full-text search.
    #[tool(
        description = "Search the user's emails by query text. Returns matching emails with sender, subject, date, and relevance score."
    )]
    async fn search_emails(&self, Parameters(req): Parameters<SearchEmailsRequest>) -> String {
        let start = Instant::now();

        // Rate limiting (ADR-028 Phase 6)
        if let Err(e) = self.rate_limiter.check("search_emails", None) {
            self.audit("search_emails", &req, "denied", start).await;
            return serde_json::json!({"error": e}).to_string();
        }

        // Input validation (ADR-028 Phase 6)
        if let Err(e) = validate_query(&req.query) {
            self.audit("search_emails", &req, "error", start).await;
            return serde_json::json!({ "error": e }).to_string();
        }
        let limit = validate_limit(req.limit, 100);

        let query = HybridSearchQuery {
            text: req.query.clone(),
            mode: SearchMode::Hybrid,
            filters: None,
            limit: Some(limit as usize),
            vector_weight: 1.0,
            fts_weight: 1.0,
        };

        match self.state.vector_service.hybrid_search.search(&query).await {
            Ok(result) => {
                let items: Vec<serde_json::Value> = result
                    .results
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "email_id": r.email_id,
                            "score": r.score,
                            "match_type": r.match_type,
                            "subject": r.metadata.get("subject").unwrap_or(&String::new()),
                            "from": r.metadata.get("from_addr").unwrap_or(&String::new()),
                            "date": r.metadata.get("received_at").unwrap_or(&String::new()),
                        })
                    })
                    .collect();

                self.audit("search_emails", &req, "success", start).await;
                serde_json::json!({
                    "total": result.total,
                    "results": items,
                    "latency_ms": result.latency_ms,
                })
                .to_string()
            }
            Err(e) => {
                self.audit("search_emails", &req, "error", start).await;
                serde_json::json!({
                    "error": format!("Search failed: {e}"),
                })
                .to_string()
            }
        }
    }

    /// Retrieve a single email by its unique identifier, including full body and metadata.
    #[tool(
        description = "Get full email content including headers, body, and metadata by email ID."
    )]
    async fn get_email(&self, Parameters(req): Parameters<GetEmailRequest>) -> String {
        let start = Instant::now();

        if let Err(e) = self.rate_limiter.check("get_email", None) {
            self.audit("get_email", &req, "denied", start).await;
            return serde_json::json!({"error": e}).to_string();
        }

        // Input validation (ADR-028 Phase 6)
        if let Err(e) = validate_email_id(&req.email_id) {
            self.audit("get_email", &req, "error", start).await;
            return serde_json::json!({ "error": e }).to_string();
        }

        let result = sqlx::query_as::<_, EmailRow>(
            "SELECT id, subject, from_name, from_addr, received_at, body_text, category \
             FROM emails WHERE id = ?1",
        )
        .bind(&req.email_id)
        .fetch_optional(&self.state.db.pool)
        .await;

        match result {
            Ok(Some(row)) => {
                let sender = match &row.from_name {
                    Some(name) if !name.is_empty() => format!("{name} <{}>", row.from_addr),
                    _ => row.from_addr.clone(),
                };
                self.audit("get_email", &req, "success", start).await;
                serde_json::json!({
                    "id": row.id,
                    "subject": row.subject,
                    "from": sender,
                    "date": row.received_at,
                    "category": row.category,
                    "body": row.body_text.unwrap_or_default(),
                })
                .to_string()
            }
            Ok(None) => {
                self.audit("get_email", &req, "error", start).await;
                serde_json::json!({ "error": "Email not found" }).to_string()
            }
            Err(e) => {
                self.audit("get_email", &req, "error", start).await;
                serde_json::json!({ "error": format!("Database error: {e}") }).to_string()
            }
        }
    }

    /// List the most recent emails across all connected accounts.
    #[tool(description = "List the most recent emails across all connected accounts.")]
    async fn list_recent_emails(
        &self,
        Parameters(req): Parameters<ListRecentEmailsRequest>,
    ) -> String {
        let start = Instant::now();

        if let Err(e) = self.rate_limiter.check("list_recent_emails", None) {
            self.audit("list_recent_emails", &req, "denied", start)
                .await;
            return serde_json::json!({"error": e}).to_string();
        }

        let limit = req.limit.unwrap_or(20).min(100) as i64;

        let result = sqlx::query_as::<_, EmailRow>(
            "SELECT id, subject, from_name, from_addr, received_at, body_text, category \
             FROM emails ORDER BY received_at DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.state.db.pool)
        .await;

        match result {
            Ok(rows) => {
                let items: Vec<serde_json::Value> = rows
                    .iter()
                    .map(|row| {
                        let sender = match &row.from_name {
                            Some(name) if !name.is_empty() => {
                                format!("{name} <{}>", row.from_addr)
                            }
                            _ => row.from_addr.clone(),
                        };
                        serde_json::json!({
                            "id": row.id,
                            "subject": row.subject,
                            "from": sender,
                            "date": row.received_at,
                            "category": row.category,
                        })
                    })
                    .collect();

                self.audit("list_recent_emails", &req, "success", start)
                    .await;
                serde_json::json!({ "count": items.len(), "emails": items }).to_string()
            }
            Err(e) => {
                self.audit("list_recent_emails", &req, "error", start).await;
                serde_json::json!({ "error": format!("Database error: {e}") }).to_string()
            }
        }
    }

    /// Count emails matching optional filters (sender, category, date range).
    #[tool(
        description = "Count emails matching optional filters. Supports filtering by sender, category, and date range (ISO 8601)."
    )]
    async fn count_emails(&self, Parameters(req): Parameters<CountEmailsRequest>) -> String {
        let start = Instant::now();

        if let Err(e) = self.rate_limiter.check("count_emails", None) {
            self.audit("count_emails", &req, "denied", start).await;
            return serde_json::json!({"error": e}).to_string();
        }

        // Input validation (ADR-028 Phase 6)
        if let Some(ref after) = req.after {
            if let Err(e) = validate_date(after) {
                self.audit("count_emails", &req, "error", start).await;
                return serde_json::json!({ "error": e }).to_string();
            }
        }
        if let Some(ref before) = req.before {
            if let Err(e) = validate_date(before) {
                self.audit("count_emails", &req, "error", start).await;
                return serde_json::json!({ "error": e }).to_string();
            }
        }

        let mut sql = String::from("SELECT COUNT(*) as cnt FROM emails WHERE 1=1");
        let mut binds: Vec<String> = Vec::new();

        if let Some(ref from) = req.from_filter {
            sql.push_str(&format!(" AND from_addr LIKE ?{}", binds.len() + 1));
            binds.push(format!("%{from}%"));
        }
        if let Some(ref category) = req.category {
            sql.push_str(&format!(" AND category = ?{}", binds.len() + 1));
            binds.push(category.clone());
        }
        if let Some(ref after) = req.after {
            sql.push_str(&format!(" AND received_at >= ?{}", binds.len() + 1));
            binds.push(after.clone());
        }
        if let Some(ref before) = req.before {
            sql.push_str(&format!(" AND received_at <= ?{}", binds.len() + 1));
            binds.push(before.clone());
        }

        let mut query = sqlx::query_scalar::<_, i64>(&sql);
        for b in &binds {
            query = query.bind(b);
        }

        match query.fetch_one(&self.state.db.pool).await {
            Ok(count) => {
                self.audit("count_emails", &req, "success", start).await;
                serde_json::json!({ "count": count }).to_string()
            }
            Err(e) => {
                self.audit("count_emails", &req, "error", start).await;
                serde_json::json!({ "error": format!("Database error: {e}") }).to_string()
            }
        }
    }

    /// Get email analytics: counts by category, top senders, and daily volume for the last 7 days.
    #[tool(
        description = "Get email analytics: counts by category, top senders, and daily volume for the last 7 days."
    )]
    async fn get_insights(&self) -> String {
        let start = Instant::now();
        let empty = serde_json::json!({});

        if let Err(e) = self.rate_limiter.check("get_insights", None) {
            self.audit("get_insights", &empty, "denied", start).await;
            return serde_json::json!({"error": e}).to_string();
        }

        let pool = &self.state.db.pool;

        // Counts by category.
        let categories = sqlx::query_as::<_, InsightsRow>(
            "SELECT category as label, COUNT(*) as count FROM emails GROUP BY category ORDER BY count DESC",
        )
        .fetch_all(pool)
        .await;

        // Top 10 senders.
        let senders = sqlx::query_as::<_, SenderRow>(
            "SELECT COALESCE(from_name, from_addr) as sender, COUNT(*) as count \
             FROM emails GROUP BY sender ORDER BY count DESC LIMIT 10",
        )
        .fetch_all(pool)
        .await;

        // Daily volume for last 7 days.
        let daily = sqlx::query_as::<_, InsightsRow>(
            "SELECT DATE(received_at) as label, COUNT(*) as count FROM emails \
             WHERE received_at >= datetime('now', '-7 days') \
             GROUP BY DATE(received_at) ORDER BY label DESC",
        )
        .fetch_all(pool)
        .await;

        let cat_json: Vec<serde_json::Value> = categories
            .unwrap_or_default()
            .iter()
            .map(|r| serde_json::json!({ "category": r.label, "count": r.count }))
            .collect();

        let sender_json: Vec<serde_json::Value> = senders
            .unwrap_or_default()
            .iter()
            .map(|r| serde_json::json!({ "sender": r.sender, "count": r.count }))
            .collect();

        let daily_json: Vec<serde_json::Value> = daily
            .unwrap_or_default()
            .iter()
            .map(|r| serde_json::json!({ "date": r.label, "count": r.count }))
            .collect();

        self.audit("get_insights", &empty, "success", start).await;
        serde_json::json!({
            "categories": cat_json,
            "top_senders": sender_json,
            "daily_volume": daily_json,
        })
        .to_string()
    }

    /// List all email rules (filters/automation) configured by the user.
    #[tool(description = "List all email rules including their conditions, actions, and status.")]
    async fn list_rules(&self) -> String {
        let start = Instant::now();
        let empty = serde_json::json!({});

        if let Err(e) = self.rate_limiter.check("list_rules", None) {
            self.audit("list_rules", &empty, "denied", start).await;
            return serde_json::json!({"error": e}).to_string();
        }

        let result = sqlx::query_as::<_, RuleRow>(
            "SELECT id, name, conditions_json, actions_json, enabled FROM rules ORDER BY name",
        )
        .fetch_all(&self.state.db.pool)
        .await;

        match result {
            Ok(rows) => {
                let items: Vec<serde_json::Value> = rows
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "id": r.id,
                            "name": r.name,
                            "conditions": r.conditions_json,
                            "actions": r.actions_json,
                            "is_active": r.enabled != 0,
                        })
                    })
                    .collect();
                self.audit("list_rules", &empty, "success", start).await;
                serde_json::json!({ "count": items.len(), "rules": items }).to_string()
            }
            Err(e) => {
                self.audit("list_rules", &empty, "error", start).await;
                serde_json::json!({ "error": format!("Database error: {e}") }).to_string()
            }
        }
    }

    /// Get all emails in the same thread as a given email, ordered by date.
    #[tool(
        description = "Get all emails in the same conversation thread as the specified email, ordered by date."
    )]
    async fn get_email_thread(&self, Parameters(req): Parameters<GetEmailThreadRequest>) -> String {
        let start = Instant::now();

        if let Err(e) = self.rate_limiter.check("get_email_thread", None) {
            self.audit("get_email_thread", &req, "denied", start).await;
            return serde_json::json!({"error": e}).to_string();
        }

        // Input validation (ADR-028 Phase 6)
        if let Err(e) = validate_email_id(&req.email_id) {
            self.audit("get_email_thread", &req, "error", start).await;
            return serde_json::json!({ "error": e }).to_string();
        }

        let pool = &self.state.db.pool;

        // First, find the thread_key for the given email.
        let thread_key_result =
            sqlx::query_scalar::<_, String>("SELECT thread_key FROM emails WHERE id = ?1")
                .bind(&req.email_id)
                .fetch_optional(pool)
                .await;

        let thread_key = match thread_key_result {
            Ok(Some(key)) => key,
            Ok(None) => {
                self.audit("get_email_thread", &req, "error", start).await;
                return serde_json::json!({ "error": "Email not found" }).to_string();
            }
            Err(e) => {
                self.audit("get_email_thread", &req, "error", start).await;
                return serde_json::json!({ "error": format!("Database error: {e}") }).to_string();
            }
        };

        // Fetch all emails in that thread.
        let result = sqlx::query_as::<_, ThreadEmailRow>(
            "SELECT id, subject, from_name, from_addr, received_at, category \
             FROM emails WHERE thread_key = ?1 ORDER BY received_at ASC",
        )
        .bind(&thread_key)
        .fetch_all(pool)
        .await;

        match result {
            Ok(rows) => {
                let items: Vec<serde_json::Value> = rows
                    .iter()
                    .map(|r| {
                        let sender = match &r.from_name {
                            Some(name) if !name.is_empty() => {
                                format!("{name} <{}>", r.from_addr)
                            }
                            _ => r.from_addr.clone(),
                        };
                        serde_json::json!({
                            "id": r.id,
                            "subject": r.subject,
                            "from": sender,
                            "date": r.received_at,
                            "category": r.category,
                        })
                    })
                    .collect();

                self.audit("get_email_thread", &req, "success", start).await;
                serde_json::json!({
                    "thread_key": thread_key,
                    "count": items.len(),
                    "emails": items,
                })
                .to_string()
            }
            Err(e) => {
                self.audit("get_email_thread", &req, "error", start).await;
                serde_json::json!({ "error": format!("Database error: {e}") }).to_string()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ServerHandler implementation
// ---------------------------------------------------------------------------

#[tool_handler]
impl ServerHandler for EmailibriumMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Emailibrium MCP server. Provides email search, retrieval, \
                 and management tools for AI-assisted email workflows.",
        )
    }
}
