//! Database layer — SQLite with SQLx.

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

/// Database connection pool wrapper.
#[derive(Debug, Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    /// Connect to SQLite database.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    /// Run all pending migrations.
    pub async fn run_migrations(&self) -> Result<(), sqlx::Error> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Email state update helper (Phase 4: delta sync state mapping)
// ---------------------------------------------------------------------------

/// Update the local email state columns (`is_trash`, `is_spam`, `folder`,
/// `deleted_at`) for a single email identified by its provider message ID.
///
/// This is the single authoritative function for mutating email folder/state
/// columns and is called from both the delta-sync path and the API endpoints.
///
/// Returns the number of rows affected (0 if the email was not found).
pub async fn update_email_state(
    pool: &SqlitePool,
    email_id: &str,
    is_trash: bool,
    is_spam: bool,
    folder: &str,
    deleted_at: Option<&str>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE emails SET is_trash = ?1, is_spam = ?2, folder = ?3, deleted_at = ?4 \
         WHERE id = ?5",
    )
    .bind(is_trash)
    .bind(is_spam)
    .bind(folder)
    .bind(deleted_at)
    .bind(email_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Derive `(is_trash, is_spam, folder)` from a comma-separated label string.
///
/// Rules (applied in priority order):
/// - If labels contain "TRASH" → `(true, false, "TRASH")`
/// - If labels contain "SPAM"  → `(false, true, "SPAM")`
/// - If labels contain "SENT"  → `(false, false, "SENT")`
/// - If labels contain "DRAFT" → `(false, false, "DRAFT")`
/// - Otherwise                 → `(false, false, "INBOX")`
pub fn derive_state_from_labels(labels: &[String]) -> (bool, bool, &'static str) {
    let has = |name: &str| labels.iter().any(|l| l.eq_ignore_ascii_case(name));

    if has("TRASH") {
        (true, false, "TRASH")
    } else if has("SPAM") {
        (false, true, "SPAM")
    } else if has("SENT") {
        (false, false, "SENT")
    } else if has("DRAFT") {
        (false, false, "DRAFT")
    } else {
        (false, false, "INBOX")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_state_inbox() {
        let labels = vec!["INBOX".to_string(), "UNREAD".to_string()];
        let (trash, spam, folder) = derive_state_from_labels(&labels);
        assert!(!trash);
        assert!(!spam);
        assert_eq!(folder, "INBOX");
    }

    #[test]
    fn test_derive_state_trash() {
        let labels = vec!["TRASH".to_string()];
        let (trash, spam, folder) = derive_state_from_labels(&labels);
        assert!(trash);
        assert!(!spam);
        assert_eq!(folder, "TRASH");
    }

    #[test]
    fn test_derive_state_spam() {
        let labels = vec!["SPAM".to_string(), "UNREAD".to_string()];
        let (trash, spam, folder) = derive_state_from_labels(&labels);
        assert!(!trash);
        assert!(spam);
        assert_eq!(folder, "SPAM");
    }

    #[test]
    fn test_derive_state_sent() {
        let labels = vec!["SENT".to_string()];
        let (trash, spam, folder) = derive_state_from_labels(&labels);
        assert!(!trash);
        assert!(!spam);
        assert_eq!(folder, "SENT");
    }

    #[test]
    fn test_derive_state_draft() {
        let labels = vec!["DRAFT".to_string()];
        let (trash, spam, folder) = derive_state_from_labels(&labels);
        assert!(!trash);
        assert!(!spam);
        assert_eq!(folder, "DRAFT");
    }

    #[test]
    fn test_derive_state_empty_labels() {
        let labels: Vec<String> = vec![];
        let (trash, spam, folder) = derive_state_from_labels(&labels);
        assert!(!trash);
        assert!(!spam);
        assert_eq!(folder, "INBOX");
    }

    #[test]
    fn test_derive_state_case_insensitive() {
        let labels = vec!["trash".to_string()];
        let (trash, spam, folder) = derive_state_from_labels(&labels);
        assert!(trash);
        assert!(!spam);
        assert_eq!(folder, "TRASH");
    }
}
