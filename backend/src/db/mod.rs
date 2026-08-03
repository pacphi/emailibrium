//! Database layer — SQLite or PostgreSQL via SQLx, selected by the connection URL's scheme.
//!
//! See `docs/ADRs/ADR-033-postgresql-backend-support.md` for why URL-scheme dispatch (not a
//! separate config flag) is the one backend-selection mechanism, applied consistently across
//! every deploy/run mode (native, Docker dev, Docker prod).

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

/// Database connection pool, dispatched to SQLite or PostgreSQL by the connection URL's scheme
/// (`sqlite:...` vs `postgres://...` / `postgresql://...`) — see ADR-033.
///
/// This phase introduces the abstraction and the PostgreSQL connection path; most call sites
/// elsewhere in the crate still hold a raw `SqlitePool` obtained via [`Database::pool`] rather
/// than matching on this enum directly — that bridge is intentional and temporary. A later
/// phase migrates every remaining call site onto this enum so a PostgreSQL-backed deployment
/// works end to end, not just at the connection layer.
#[derive(Debug, Clone)]
pub enum Database {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

impl Database {
    /// Connect to SQLite or PostgreSQL, chosen by `url`'s scheme.
    ///
    /// `postgres://` or `postgresql://` selects PostgreSQL; anything else (including the
    /// default `sqlite:...?mode=rwc`) selects SQLite. See ADR-033 for why the URL scheme alone
    /// is the selector — no separate feature flag exists, or should be added, for this choice.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            let pool = PgPoolOptions::new().max_connections(5).connect(url).await?;
            Ok(Self::Postgres(pool))
        } else {
            let pool = SqlitePoolOptions::new()
                .max_connections(5)
                .connect(url)
                .await?;
            Ok(Self::Sqlite(pool))
        }
    }

    /// Run all pending migrations for the connected backend.
    ///
    /// Each backend has its own migration directory (`migrations/sqlite/`,
    /// `migrations/postgres/`) rather than one shared, dialect-straddling set — see ADR-033
    /// §2.3 for why. The two directories carry the same relational schema under matching
    /// filenames/numbers, with one deliberate exception: `migrations/postgres/` omits the 3
    /// SQLite FTS5 migrations (005/019/020), since PostgreSQL has no FTS5 equivalent — full-text
    /// search there is a separate, later capability (ADR-034), not part of this phase's schema
    /// port. Connecting to PostgreSQL and running migrations works end to end for everything
    /// except full-text search.
    pub async fn run_migrations(&self) -> Result<(), sqlx::Error> {
        match self {
            Self::Sqlite(pool) => sqlx::migrate!("./migrations/sqlite").run(pool).await?,
            Self::Postgres(pool) => sqlx::migrate!("./migrations/postgres").run(pool).await?,
        }
        Ok(())
    }

    /// The SQLite pool, for call sites not yet migrated onto this enum directly (see the module
    /// doc and ADR-033). Panics if connected to PostgreSQL — a later phase migrates these
    /// remaining callers onto the enum so this accessor can be removed.
    pub fn pool(&self) -> &SqlitePool {
        match self {
            Self::Sqlite(pool) => pool,
            Self::Postgres(_) => panic!(
                "Database::pool() called on a PostgreSQL-backed connection — this accessor is a \
                 temporary bridge for call sites not yet migrated onto the Database enum \
                 directly (see ADR-033); a later phase removes it."
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Dynamic SQL safety (sqlx 0.9 `SqlSafeStr`)
// ---------------------------------------------------------------------------

/// Mark a runtime-built SQL string as safe for sqlx 0.9's [`SqlSafeStr`] guard.
///
/// sqlx 0.9 only implements `SqlSafeStr` for `&'static str`. Any query whose
/// text is assembled at runtime — dynamic `IN (?, ?, …)` placeholder lists,
/// optional `WHERE` fragments, retention-window `DELETE`s, etc. — must be
/// explicitly asserted safe via [`sqlx::AssertSqlSafe`].
///
/// This function is the crate's single audited choke point for that assertion,
/// so the invariant is documented and reviewed in one place instead of being
/// repeated at every call site. Every caller composes SQL the same vetted way:
/// the query *structure* is built only from string literals and bind-parameter
/// markers (`?` / `?N`) — never from caller- or user-supplied values, which are
/// always passed through `.bind(...)`. Bare integer literals that appear inline
/// (e.g. `bool as i32`, config-derived retention day counts) are never user
/// input.
///
/// Do **not** route a string through here that interpolates untrusted data;
/// doing so reintroduces the SQL-injection risk the guard exists to prevent.
///
/// [`SqlSafeStr`]: sqlx::SqlSafeStr
#[inline]
pub fn audited_sql(sql: &str) -> sqlx::AssertSqlSafe<String> {
    sqlx::AssertSqlSafe(sql.to_owned())
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
