//! Read-only MCP tool handlers (ADR-028 follow-on, task A3).
//!
//! Each public `async fn` in the submodules here is a tool handler: it takes an
//! [`Arc<ToolContext>`](crate::tools::ToolContext) plus its parsed request
//! struct and returns `Result<serde_json::Value, ToolError>`. Cross-cutting
//! concerns — rate limiting, audit logging, and the MCP `CallToolResult`
//! wrapper — belong to the shared dispatch path, not to individual handlers.
//!
//! Failures are returned as [`ToolError`](crate::tools::ToolError) variants
//! rather than an `{"error": ...}` payload: an embedded error would look like a
//! success to both the MCP client and the audit log.
//!
//! Every handler here is strictly read-only. `cleanup_preview` in particular
//! builds a `CleanupPlan` in memory and never persists it (see that module).

pub mod accounts;
pub mod cleanup_preview;
pub mod emails;
pub mod insights;
pub mod params;

/// Upper bound on any free-form identifier argument.
const MAX_ID_LEN: usize = 200;

/// Log a backing-service failure and return a caller-safe [`ToolError`].
///
/// Raw `sqlx::Error` text can carry table names, column names, and the database
/// file path. Operators get the full detail in the log; the caller — which may
/// be a model relaying text to a user — gets only the operation that failed.
pub fn db_error(operation: &str, e: impl std::fmt::Display) -> crate::tools::ToolError {
    tracing::error!(operation, error = %e, "tool backing operation failed");
    crate::tools::ToolError::Database(format!("{operation} failed"))
}

/// Reject empty or over-long identifiers.
///
/// `label` names the argument so the caller gets an actionable message
/// rather than a bare "invalid input".
pub fn validate_id(label: &str, id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if id.len() > MAX_ID_LEN {
        return Err(format!("{label} too long (max {MAX_ID_LEN} characters)"));
    }
    Ok(())
}

/// Reject identifiers that are not well-formed UUIDs.
///
/// Account and plan ids are UUIDs throughout the REST surface; validating
/// here keeps malformed ids from reaching a query as a silent no-match.
pub fn validate_uuid(label: &str, id: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| format!("Invalid {label}: expected a UUID"))
}

/// Reject user ids containing anything outside `[A-Za-z0-9_-]`.
///
/// Mirrors the sanitization the wipe endpoint applies, so a user id can
/// never carry path or query metacharacters.
pub fn validate_user_id(id: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("user_id must not be empty".to_string());
    }
    if id.len() > MAX_ID_LEN {
        return Err(format!("user_id too long (max {MAX_ID_LEN} characters)"));
    }
    if !id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err("user_id may contain only letters, digits, '-' and '_'".to_string());
    }
    Ok(())
}

/// Reject dates that are not `YYYY-MM-DD`.
pub fn validate_date(date: &str) -> Result<(), String> {
    if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return Err(format!("Invalid date format: {date}. Expected YYYY-MM-DD"));
    }
    Ok(())
}

/// Clamp a caller-supplied limit into `1..=max`.
///
/// Bounds are enforced here rather than in the JSON Schema: a schema
/// `maximum` is advisory to the model, whereas clamping is binding.
pub fn validate_limit(limit: u32, max: u32) -> u32 {
    limit.clamp(1, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_id_rejects_empty_and_overlong() {
        assert!(validate_id("email_id", "").is_err());
        assert!(validate_id("email_id", &"x".repeat(MAX_ID_LEN + 1)).is_err());
        assert!(validate_id("email_id", "abc123").is_ok());
    }

    #[test]
    fn validate_uuid_accepts_only_uuids() {
        assert!(validate_uuid("account_id", "not-a-uuid").is_err());
        assert!(validate_uuid("account_id", "0192f3c4-5678-7abc-8def-0123456789ab").is_ok());
    }

    #[test]
    fn validate_user_id_rejects_metacharacters() {
        assert!(validate_user_id("../etc/passwd").is_err());
        assert!(validate_user_id("user?id=1").is_err());
        assert!(validate_user_id("").is_err());
        assert!(validate_user_id("local-user_1").is_ok());
    }

    #[test]
    fn db_error_withholds_backing_error_text() {
        let raw = "no such column: secret_col in /var/data/emailibrium.db";
        let err = db_error("Listing attachments", sqlx::Error::Protocol(raw.into()));

        let surfaced = err.to_string();
        assert!(!surfaced.contains("secret_col"));
        assert!(!surfaced.contains("/var/data"));
        assert!(surfaced.contains("Listing attachments"));
    }

    #[test]
    fn validate_date_requires_iso_day() {
        assert!(validate_date("2026-07-31").is_ok());
        assert!(validate_date("31/07/2026").is_err());
        assert!(validate_date("2026-13-01").is_err());
    }

    #[test]
    fn validate_limit_clamps_into_range() {
        assert_eq!(validate_limit(0, 100), 1);
        assert_eq!(validate_limit(5_000, 100), 100);
        assert_eq!(validate_limit(25, 100), 25);
    }
}
