//! Audit logging for tool calls (ADR-028 Phase 6).
//!
//! Applied by [`super::registry::ToolRegistry::dispatch`], so every call is
//! recorded once regardless of which caller made it — MCP transport, chat
//! orchestrator, or test.

use chrono::{DateTime, Utc};
use sea_orm::ActiveValue::Set;
use sea_orm::{DatabaseConnection, EntityTrait};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tracing::info;

use crate::db::entities::mcp_tool_audit;

#[derive(Debug, Serialize)]
pub struct ToolCallAuditEntry {
    pub timestamp: DateTime<Utc>,
    pub tool_name: String,
    pub arguments_hash: String, // SHA-256 of arguments (not raw args, for privacy)
    pub result_status: &'static str, // "success", "error", "denied", "rate_limited"
    pub latency_ms: u64,
    /// "mcp" or "chat" — see [`super::registry::CallSource`].
    pub source: &'static str,
}

/// Log a tool call to both tracing and the database.
pub async fn log_tool_call(conn: &DatabaseConnection, entry: &ToolCallAuditEntry) {
    info!(
        tool = %entry.tool_name,
        status = %entry.result_status,
        source = %entry.source,
        latency_ms = entry.latency_ms,
        "tool call"
    );

    // Best-effort database logging (don't fail the tool call if audit insert
    // fails). `latency_ms` is a real 4-byte INTEGER on PostgreSQL — the old
    // `as i64` bind failed there, and silently, precisely because this insert
    // is best-effort (ADR-035's width class; the entity type closes it).
    let _ = mcp_tool_audit::Entity::insert(mcp_tool_audit::ActiveModel {
        timestamp: Set(entry.timestamp.to_rfc3339()),
        tool_name: Set(entry.tool_name.clone()),
        arguments_hash: Set(entry.arguments_hash.clone()),
        result_status: Set(entry.result_status.to_owned()),
        latency_ms: Set(entry.latency_ms.min(i32::MAX as u64) as i32),
        source: Set(entry.source.to_owned()),
        ..Default::default()
    })
    .exec_without_returning(conn)
    .await;
}

/// Hash arguments for audit logging (privacy-preserving).
pub fn hash_arguments(args: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(args.to_string().as_bytes());
    let result = hasher.finalize();
    result.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use sea_orm::{ConnectionTrait, QueryOrder};

    #[test]
    fn hash_arguments_deterministic() {
        let args = serde_json::json!({"query": "hello", "limit": 10});
        let h1 = hash_arguments(&args);
        let h2 = hash_arguments(&args);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn hash_arguments_different_for_different_inputs() {
        let a = serde_json::json!({"query": "hello"});
        let b = serde_json::json!({"query": "world"});
        assert_ne!(hash_arguments(&a), hash_arguments(&b));
    }

    async fn fresh_conn() -> DatabaseConnection {
        let conn = crate::db::test_sqlite_database().await.sea_orm();
        let raw = concat!(
            include_str!("../../migrations/sqlite/022_mcp_tool_audit.sql"),
            "\nALTER TABLE mcp_tool_audit ADD COLUMN source TEXT NOT NULL DEFAULT 'mcp';",
        );
        let cleaned: String = raw
            .lines()
            .map(|l| {
                if let Some(idx) = l.find("--") {
                    &l[..idx]
                } else {
                    l
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        for stmt in cleaned.split(';') {
            let s = stmt.trim();
            if !s.is_empty() {
                conn.execute_unprepared(s).await.expect("migrate");
            }
        }
        conn
    }

    fn sample_entry() -> ToolCallAuditEntry {
        ToolCallAuditEntry {
            timestamp: Utc::now(),
            tool_name: "search_emails".into(),
            arguments_hash: "ab".repeat(32),
            result_status: "success",
            latency_ms: 42,
            source: "mcp",
        }
    }

    #[tokio::test]
    async fn log_tool_call_persists_the_row_with_i32_latency() {
        let conn = fresh_conn().await;
        log_tool_call(&conn, &sample_entry()).await;

        let row = mcp_tool_audit::Entity::find()
            .order_by_desc(mcp_tool_audit::Column::Id)
            .one(&conn)
            .await
            .expect("query")
            .expect("row present");
        assert_eq!(row.tool_name, "search_emails");
        assert_eq!(row.result_status, "success");
        assert_eq!(row.latency_ms, 42);
        assert_eq!(row.source, "mcp");
    }

    /// The insert is best-effort by contract: a missing table must not error
    /// the caller (this is exactly how the old i64 bind failed silently on
    /// PostgreSQL — the swallow is load-bearing and deliberate).
    #[tokio::test]
    async fn log_tool_call_swallows_insert_failure() {
        let conn = crate::db::test_sqlite_database().await.sea_orm();
        // No table created — the insert fails internally, the call must not panic
        // or surface an error (it returns ()).
        log_tool_call(&conn, &sample_entry()).await;
    }
}
