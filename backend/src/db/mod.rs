//! Database layer — SQLite or PostgreSQL via SQLx, selected by the connection URL's scheme.
//!
//! See `docs/ADRs/ADR-033-postgresql-backend-support.md` for why URL-scheme dispatch (not a
//! separate config flag) is the one backend-selection mechanism, applied consistently across
//! every deploy/run mode (native, Docker dev, Docker prod).

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

pub mod entities;

/// Database connection pool, dispatched to SQLite or PostgreSQL by the connection URL's scheme
/// (`sqlite:...` vs `postgres://...` / `postgresql://...`) — see ADR-033.
///
/// Application code talks to the database exclusively through [`Database::sea_orm`], the
/// single dialect-dispatching code path (ADR-036). This enum's own surface is just the
/// connection layer: [`Database::connect`], [`Database::run_migrations`], and the
/// `sea_orm()` wrap.
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

    /// A SeaORM handle over the SAME underlying pool this enum already holds (ADR-036).
    ///
    /// `sea_orm::DatabaseConnection` is itself a backend-dispatching wrapper, so this match
    /// is the one place application code still branches on the backend — the composition-root
    /// wrap proven by the ADR-036 spike (§3 check #2): legacy sqlx call sites and SeaORM share the
    /// identical pool during the incremental port, so there is no second pool to configure,
    /// exhaust, or keep consistent. Cloning the returned handle is cheap (it wraps the pool,
    /// itself a cheap-clone handle); repositories hold their own clone.
    pub fn sea_orm(&self) -> sea_orm::DatabaseConnection {
        match self {
            Self::Sqlite(pool) => sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone()),
            Self::Postgres(pool) => {
                sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone())
            }
        }
    }
}

/// Test-only constructor: an in-memory SQLite [`Database`] on a single
/// connection (so `:memory:` state survives across pooled acquires).
///
/// This helper — together with [`Database::connect`] — is deliberately the
/// ONE place in the crate that names the SQLite pool types; every test module
/// builds its database here rather than assembling a pool by hand (ADR-036 —
/// the phase-3 sweep removes all other `SqlitePool` call sites). Not
/// `#[cfg(test)]`-gated because the server binary's own test modules reach it
/// through this library crate, which cargo builds without `cfg(test)` there.
#[doc(hidden)]
pub async fn test_sqlite_database() -> Database {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(":memory:")
        .await
        .expect("in-memory sqlite");
    Database::Sqlite(pool)
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
/// `is_trash`/`is_spam` are INTEGER 0/1 columns (migration 016), so the bool
/// arguments are widened to `i32` here — the entity mirrors the DDL (ADR-036).
///
/// Returns the number of rows affected (0 if the email was not found).
pub async fn update_email_state(
    conn: &sea_orm::DatabaseConnection,
    email_id: &str,
    is_trash: bool,
    is_spam: bool,
    folder: &str,
    deleted_at: Option<&str>,
) -> Result<u64, sea_orm::DbErr> {
    use sea_orm::sea_query::Expr;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    use entities::emails;

    let result = emails::Entity::update_many()
        .col_expr(emails::Column::IsTrash, Expr::value(is_trash as i32))
        .col_expr(emails::Column::IsSpam, Expr::value(is_spam as i32))
        .col_expr(emails::Column::Folder, Expr::value(folder))
        .col_expr(emails::Column::DeletedAt, Expr::value(deleted_at))
        .filter(emails::Column::Id.eq(email_id))
        .exec(conn)
        .await?;

    Ok(result.rows_affected)
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
