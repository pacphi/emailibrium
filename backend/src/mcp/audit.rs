//! Audit logging for MCP tool calls (ADR-028 Phase 6).

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tracing::info;

#[derive(Debug, Serialize)]
pub struct ToolCallAuditEntry {
    pub timestamp: DateTime<Utc>,
    pub tool_name: String,
    pub arguments_hash: String, // SHA-256 of arguments (not raw args, for privacy)
    pub result_status: &'static str, // "success", "error", "denied"
    pub latency_ms: u64,
}

/// Log a tool call to both tracing and the database.
pub async fn log_tool_call(pool: &SqlitePool, entry: &ToolCallAuditEntry) {
    info!(
        tool = %entry.tool_name,
        status = %entry.result_status,
        latency_ms = entry.latency_ms,
        "MCP tool call"
    );

    // Best-effort database logging (don't fail the tool call if audit insert fails)
    let _ = sqlx::query(
        "INSERT INTO mcp_tool_audit (timestamp, tool_name, arguments_hash, result_status, latency_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(entry.timestamp.to_rfc3339())
    .bind(&entry.tool_name)
    .bind(&entry.arguments_hash)
    .bind(entry.result_status)
    .bind(entry.latency_ms as i64)
    .execute(pool)
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
}
