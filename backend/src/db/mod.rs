//! Database layer — SQLite or PostgreSQL via SQLx, selected by the connection URL's scheme.
//!
//! See `docs/ADRs/ADR-033-postgresql-backend-support.md` for why URL-scheme dispatch (not a
//! separate config flag) is the one backend-selection mechanism, applied consistently across
//! every deploy/run mode (native, Docker dev, Docker prod).

use std::borrow::Cow;

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

pub mod entities;

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

    /// Adapt a SQLite-authored query for the connected backend — see ADR-035. Returns `sql`
    /// unchanged for SQLite (zero-cost); for PostgreSQL, rewrites `?`/`?N` placeholders to
    /// `$N` and `datetime('now')` to `now()` (the one SQLite-specific function call this
    /// codebase's application-code queries use — migrations 004/006/010/011/012 needed the
    /// same substitution, plus a `TIMESTAMPTZ` column-type change these inline application
    /// queries don't need to make, since they only ever pass the value through `now()`/
    /// `datetime('now')`'s return, never declare a column type). Every call site authors ONE
    /// query string, SQLite-style, and calls this before executing it against whichever pool
    /// is live.
    pub fn adapt<'a>(&self, sql: &'a str) -> Cow<'a, str> {
        match self {
            Self::Sqlite(_) => Cow::Borrowed(sql),
            Self::Postgres(_) => {
                let with_now = sql.replace("datetime('now')", "now()");
                Cow::Owned(sqlite_placeholders_to_postgres(&with_now))
            }
        }
    }
}

/// Rewrite SQLite's `?`/`?N` bind placeholders into PostgreSQL's `$N` positional parameters.
///
/// Single-pass scan tracking single-quoted string literals (SQL-standard `''` escaped quote)
/// and double-quoted identifiers — a `?` inside either is data/an identifier character, never
/// rewritten. `?N` (already explicitly numbered) becomes `$N` directly, since SQLite's and
/// PostgreSQL's numbered-parameter semantics are identical, just spelled differently. A bare
/// `?` becomes `$<k>` where `k` counts bare `?` occurrences in order of appearance, matching
/// how SQLite itself treats consecutive bare `?`s as sequential parameters. Mixing bare and
/// numbered placeholders in the same query is not a case this function is designed to make
/// sensible — no call site in this codebase does that (see ADR-035 §2.3).
fn sqlite_placeholders_to_postgres(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 8);
    let mut chars = sql.char_indices().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut bare_counter: u32 = 0;

    while let Some((_, c)) = chars.next() {
        match c {
            '\'' if !in_double_quote => {
                out.push(c);
                // A doubled '' inside a single-quoted literal is an escaped quote, not the
                // string's end — consume both characters and stay inside the literal.
                if in_single_quote && chars.peek().map(|&(_, next)| next) == Some('\'') {
                    let (_, next) = chars.next().unwrap();
                    out.push(next);
                } else {
                    in_single_quote = !in_single_quote;
                }
            }
            '"' if !in_single_quote => {
                out.push(c);
                if in_double_quote && chars.peek().map(|&(_, next)| next) == Some('"') {
                    let (_, next) = chars.next().unwrap();
                    out.push(next);
                } else {
                    in_double_quote = !in_double_quote;
                }
            }
            '?' if !in_single_quote && !in_double_quote => {
                let mut digits = String::new();
                while let Some(&(_, next)) = chars.peek() {
                    if next.is_ascii_digit() {
                        digits.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if digits.is_empty() {
                    bare_counter += 1;
                    out.push('$');
                    out.push_str(&bare_counter.to_string());
                } else {
                    out.push('$');
                    out.push_str(&digits);
                }
            }
            _ => out.push(c),
        }
    }

    out
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

    // -----------------------------------------------------------------------
    // Placeholder translation (ADR-035)
    // -----------------------------------------------------------------------

    #[test]
    fn adapt_bare_placeholders_numbered_sequentially() {
        let sql = sqlite_placeholders_to_postgres("SELECT * FROM t WHERE a = ? AND b = ?");
        assert_eq!(sql, "SELECT * FROM t WHERE a = $1 AND b = $2");
    }

    #[test]
    fn adapt_numbered_placeholders_pass_through_with_dollar_prefix() {
        let sql = sqlite_placeholders_to_postgres(
            "UPDATE emails SET is_trash = ?1, is_spam = ?2, folder = ?3 WHERE id = ?5",
        );
        assert_eq!(
            sql,
            "UPDATE emails SET is_trash = $1, is_spam = $2, folder = $3 WHERE id = $5"
        );
    }

    #[test]
    fn adapt_ten_bare_placeholders_matches_cloud_api_audit_log_insert() {
        // Mirrors vectors/audit.rs's real INSERT — the largest bind count in the codebase.
        let sql = sqlite_placeholders_to_postgres(
            "INSERT INTO cloud_api_audit_log \
             (timestamp, provider, model, input_tokens, output_tokens, latency_ms, \
              user_id, request_type, status, error_message) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        );
        assert_eq!(
            sql,
            "INSERT INTO cloud_api_audit_log \
             (timestamp, provider, model, input_tokens, output_tokens, latency_ms, \
              user_id, request_type, status, error_message) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
        );
    }

    #[test]
    fn adapt_ignores_question_mark_inside_single_quoted_literal() {
        let sql =
            sqlite_placeholders_to_postgres("SELECT * FROM t WHERE a = ? AND b = 'is it ok?'");
        assert_eq!(sql, "SELECT * FROM t WHERE a = $1 AND b = 'is it ok?'");
    }

    #[test]
    fn adapt_honors_escaped_single_quote_inside_literal() {
        // 'it''s a ?' is ONE literal (the '' is an escaped quote, not the string's end) —
        // the ? inside must not be rewritten, and the bare ? after the literal is $1.
        let sql =
            sqlite_placeholders_to_postgres("SELECT * FROM t WHERE a = 'it''s a ?' AND b = ?");
        assert_eq!(sql, "SELECT * FROM t WHERE a = 'it''s a ?' AND b = $1");
    }

    #[test]
    fn adapt_ignores_question_mark_inside_double_quoted_identifier() {
        let sql = sqlite_placeholders_to_postgres(r#"SELECT "weird?col" FROM t WHERE a = ?"#);
        assert_eq!(sql, r#"SELECT "weird?col" FROM t WHERE a = $1"#);
    }

    #[test]
    fn adapt_no_placeholders_is_unchanged() {
        let sql = sqlite_placeholders_to_postgres("SELECT * FROM t");
        assert_eq!(sql, "SELECT * FROM t");
    }

    #[tokio::test]
    async fn database_adapt_is_noop_borrowed_for_sqlite() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let sql = "SELECT * FROM t WHERE a = ?";
        let adapted = db.adapt(sql);
        assert_eq!(adapted, sql);
        assert!(matches!(adapted, Cow::Borrowed(_)));
    }

    #[tokio::test]
    async fn database_adapt_rewrites_datetime_now_and_placeholders_for_postgres() {
        // Doesn't require a live Postgres connection to exercise `adapt()`'s branch — only
        // `Database::connect()` needs a real socket, and this test never calls it for Postgres;
        // it directly constructs the enum variant it needs to check the Postgres arm.
        let db = Database::Postgres(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect_lazy("postgres://user:pass@localhost/db")
                .unwrap(),
        );
        let adapted = db.adapt("UPDATE t SET updated_at = datetime('now') WHERE id = ?");
        assert_eq!(adapted, "UPDATE t SET updated_at = now() WHERE id = $1");
        assert!(matches!(adapted, Cow::Owned(_)));
    }
}
