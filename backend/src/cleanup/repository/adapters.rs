//! Adapters binding the cleanup domain's read-only ports to the shared tables.
//!
//! Bodies are single-code-path SeaORM (ADR-036): the entities in
//! `crate::db::entities` own per-backend encode/decode, queries are built on the
//! typed builder, and there is no `match Database::Sqlite/Postgres` left — the
//! same bodies run against SQLite and PostgreSQL. See
//! `cleanup/repository/plan_repo.rs` for the exemplar this follows.
//!
//! Each adapter reads from the existing tables shared with the rest of the
//! application. No new tables are introduced here — the cleanup domain reuses
//! `emails`, `topic_clusters`, `connected_accounts`, `sync_state`, and `rules`.
//!
//! The adapters keep a [`Database`] rather than a `DatabaseConnection` because
//! two of them still need the enum: `SeaOrmRuleEvaluator` hands it to
//! `RuleEngine::load_rules`, and every construction site spells them as struct
//! literals with a `db:` field. Each method derives its connection with
//! `self.db.sea_orm()`, which is a cheap clone of the same underlying pool.

use async_trait::async_trait;
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, EntityTrait, FromQueryResult, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};

use crate::cleanup::domain::operation::{AccountStateEtag, EmailRef, UnsubscribeMethodKind};
use crate::cleanup::domain::ports::{
    AccountStateProvider, ClusterRepository, EmailRepository, RepoError, RuleEvalError,
    RuleEvaluator, SubscriptionRecord, SubscriptionRepository,
};
use crate::db::entities::{connected_accounts, emails, sync_state, topic_clusters};
use crate::db::Database;
use crate::rules::rule_processor::evaluate_rules;
use crate::rules::types::{EvaluationScope, RuleEvaluation, RuleExecutionMode};

/// Ids per `IN (…)` batch when resolving a cluster's members. PostgreSQL caps a
/// statement at 65535 bind parameters; 500 stays far inside that on either
/// backend while resolving even a very large cluster in a few round trips.
const ID_CHUNK: usize = 500;

/// `(id, account_id)` projection of an `emails` row — the shape [`EmailRef`] needs.
#[derive(FromQueryResult)]
struct EmailRefRow {
    id: String,
    account_id: String,
}

/// Resolve a cluster's members to the `emails` rows that actually exist.
///
/// `topic_clusters.email_ids` is a JSON array of email ids stored as plain TEXT.
/// Unpacking it SQL-side needed `json_each` on SQLite and
/// `jsonb_array_elements_text` over a `::jsonb` cast on PostgreSQL — genuinely
/// different join shapes, not a placeholder swap, so each backend used to carry
/// its own SQL text (ADR-035 §2.3). Parsing the array in Rust and filtering with
/// `IN` deletes the divergence outright, the same move ADR-036 §2.4 makes for
/// SQL-side JSON mutation.
///
/// Behavior carried over from the old INNER JOIN: an id with no `emails` row is
/// dropped silently, and an id listed twice yields two refs. Results now follow
/// the JSON array's order (the join left it unspecified).
async fn emails_by_cluster(
    conn: &DatabaseConnection,
    cluster_id: &str,
) -> Result<Vec<EmailRef>, RepoError> {
    let Some(cluster) = topic_clusters::Entity::find_by_id(cluster_id)
        .one(conn)
        .await?
    else {
        return Ok(Vec::new());
    };
    // A malformed array is an error here because it was one there: both
    // `json_each` and `jsonb_array_elements_text` raise on non-JSON input rather
    // than returning zero rows.
    let ids: Vec<String> = serde_json::from_str(&cluster.email_ids).map_err(|e| {
        RepoError::Internal(format!("cluster {cluster_id} has malformed email_ids: {e}"))
    })?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut account_of: std::collections::HashMap<String, String> =
        std::collections::HashMap::with_capacity(ids.len());
    for chunk in ids.chunks(ID_CHUNK) {
        let rows = emails::Entity::find()
            .select_only()
            .column(emails::Column::Id)
            .column(emails::Column::AccountId)
            .filter(emails::Column::Id.is_in(chunk.iter().map(String::as_str)))
            .into_model::<EmailRefRow>()
            .all(conn)
            .await?;
        account_of.extend(rows.into_iter().map(|r| (r.id, r.account_id)));
    }

    Ok(ids
        .into_iter()
        .filter_map(|id| {
            account_of.get(&id).map(|account_id| EmailRef {
                account_id: account_id.clone(),
                id,
            })
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Email repository
// ---------------------------------------------------------------------------

pub struct SeaOrmEmailRepository {
    pub db: Database,
}

/// Pre-port name, kept so the construction sites outside this port's scope
/// (`main.rs`) keep compiling. DELETE IN PHASE 3G with the last caller that
/// spells it this way.
pub type SqlxEmailRepository = SeaOrmEmailRepository;

#[async_trait]
impl EmailRepository for SeaOrmEmailRepository {
    async fn list_by_account(&self, account_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        let conn = self.db.sea_orm();
        let rows = emails::Entity::find()
            .select_only()
            .column(emails::Column::Id)
            .column(emails::Column::AccountId)
            .filter(emails::Column::AccountId.eq(account_id))
            .into_model::<EmailRefRow>()
            .all(&conn)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| EmailRef {
                id: r.id,
                account_id: r.account_id,
            })
            .collect())
    }

    async fn list_by_cluster(&self, cluster_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        emails_by_cluster(&self.db.sea_orm(), cluster_id).await
    }

    async fn count_by_account(&self, account_id: &str) -> Result<u64, RepoError> {
        // `PaginatorTrait::count` returns u64 directly, so the old
        // `COUNT(*) -> i64 -> max(0) as u64` width dance is gone; the port's
        // public `u64` return type is unchanged.
        let conn = self.db.sea_orm();
        Ok(emails::Entity::find()
            .filter(emails::Column::AccountId.eq(account_id))
            .count(&conn)
            .await?)
    }
}

// ---------------------------------------------------------------------------
// Subscription repository
// ---------------------------------------------------------------------------

pub struct SeaOrmSubscriptionRepository {
    pub db: Database,
}

/// Pre-port name — see [`SqlxEmailRepository`].
pub type SqlxSubscriptionRepository = SeaOrmSubscriptionRepository;

/// The two `List-Unsubscribe*` header columns of an `emails` row.
#[derive(FromQueryResult)]
struct UnsubscribeHeadersRow {
    list_unsubscribe: Option<String>,
    list_unsubscribe_post: Option<String>,
}

#[async_trait]
impl SubscriptionRepository for SeaOrmSubscriptionRepository {
    async fn find_by_sender(
        &self,
        account_id: &str,
        sender: &str,
    ) -> Result<Option<SubscriptionRecord>, RepoError> {
        let conn = self.db.sea_orm();
        // Most recent email from this sender that carries unsubscribe headers.
        // `.one()` applies the LIMIT 1; the OR-pair becomes a `Condition::any`
        // so it stays one group rather than widening the AND chain.
        let header_row = emails::Entity::find()
            .select_only()
            .column(emails::Column::ListUnsubscribe)
            .column(emails::Column::ListUnsubscribePost)
            .filter(emails::Column::AccountId.eq(account_id))
            .filter(emails::Column::FromAddr.eq(sender))
            .filter(
                Condition::any()
                    .add(emails::Column::ListUnsubscribe.is_not_null())
                    .add(emails::Column::ListUnsubscribePost.is_not_null()),
            )
            .order_by_desc(emails::Column::ReceivedAt)
            .into_model::<UnsubscribeHeadersRow>()
            .one(&conn)
            .await?;

        let Some(UnsubscribeHeadersRow {
            list_unsubscribe: lu,
            list_unsubscribe_post: lup,
        }) = header_row
        else {
            return Ok(None);
        };

        let method = if lup.is_some() {
            UnsubscribeMethodKind::ListUnsubscribePost
        } else if lu.as_deref().is_some_and(|v| v.starts_with("mailto:")) {
            UnsubscribeMethodKind::Mailto
        } else if lu.as_deref().is_some_and(|v| v.starts_with("http")) {
            UnsubscribeMethodKind::WebLink
        } else {
            UnsubscribeMethodKind::None
        };

        Ok(Some(SubscriptionRecord {
            method,
            list_unsubscribe: lu,
            list_unsubscribe_post: lup,
        }))
    }
}

// ---------------------------------------------------------------------------
// Cluster repository
// ---------------------------------------------------------------------------

pub struct SeaOrmClusterRepository {
    pub db: Database,
}

/// Pre-port name — see [`SqlxEmailRepository`].
pub type SqlxClusterRepository = SeaOrmClusterRepository;

#[async_trait]
impl ClusterRepository for SeaOrmClusterRepository {
    async fn emails(&self, cluster_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        emails_by_cluster(&self.db.sea_orm(), cluster_id).await
    }
}

// ---------------------------------------------------------------------------
// Account state provider
// ---------------------------------------------------------------------------

pub struct SeaOrmAccountStateProvider {
    pub db: Database,
}

/// Pre-port name — see [`SqlxEmailRepository`].
pub type SqlxAccountStateProvider = SeaOrmAccountStateProvider;

/// `connected_accounts.provider`, projected alone so the account's encrypted
/// token blobs are never read into memory (the pre-port SELECT did the same).
#[derive(FromQueryResult)]
struct ProviderRow {
    provider: String,
}

/// `sync_state.history_id`, projected alone.
#[derive(FromQueryResult)]
struct HistoryIdRow {
    history_id: Option<String>,
}

#[async_trait]
impl AccountStateProvider for SeaOrmAccountStateProvider {
    async fn etag(&self, account_id: &str) -> Result<AccountStateEtag, RepoError> {
        let conn = self.db.sea_orm();
        // Two primary-key lookups replace the former
        // `connected_accounts LEFT JOIN sync_state`. `sync_state.account_id` IS
        // that table's primary key, so the join could never fan a row out — the
        // pair is exactly equivalent — and the entities carry empty `Relation`
        // enums, so no join is derivable without editing them. The second lookup
        // only runs for providers that have a history id, where the join always
        // read one.
        let Some(account) = connected_accounts::Entity::find_by_id(account_id)
            .select_only()
            .column(connected_accounts::Column::Provider)
            .into_model::<ProviderRow>()
            .one(&conn)
            .await?
        else {
            return Ok(AccountStateEtag::None);
        };

        Ok(match account.provider.as_str() {
            "gmail" => {
                // What the LEFT (not INNER) JOIN bought: an account with no
                // `sync_state` row is not a missing account, just a missing
                // history id.
                let history_id = sync_state::Entity::find_by_id(account_id)
                    .select_only()
                    .column(sync_state::Column::HistoryId)
                    .into_model::<HistoryIdRow>()
                    .one(&conn)
                    .await?
                    .and_then(|row| row.history_id);
                match history_id {
                    Some(h) if !h.is_empty() => AccountStateEtag::GmailHistory { history_id: h },
                    _ => AccountStateEtag::None,
                }
            }
            // Outlook delta tokens and IMAP UIDVALIDITY/MODSEQ are not tracked
            // in sync_state today; extend here when those sync paths are added.
            _ => AccountStateEtag::None,
        })
    }
}

// ---------------------------------------------------------------------------
// Rule evaluator
// ---------------------------------------------------------------------------

pub struct SeaOrmRuleEvaluator {
    pub db: Database,
}

/// Pre-port name — see [`SqlxEmailRepository`].
pub type SqlxRuleEvaluator = SeaOrmRuleEvaluator;

/// Projection of one `emails` row, in the column order `evaluate_scope` selects.
///
/// Field types come straight from `crate::db::entities::emails`, which is the
/// single source of truth for per-backend decode (ADR-036 §2.2). Two of them are
/// worth spelling out:
///
/// - `received_at` is plain `TIMESTAMP` (no zone) in both dialects, not
///   `TIMESTAMPTZ`, hence `NaiveDateTime`; the stored values are already UTC, so
///   `.and_utc()` reinterprets rather than converts (ADR-035 §2.6).
/// - `is_read` is nullable (`BOOLEAN DEFAULT FALSE`, migration 001). The pre-port
///   tuple decoded it as a bare `bool`, so a NULL failed the whole scope
///   evaluation; honouring the entity's nullability, a NULL now reads as `false`
///   — the column's own default — and the other rows still evaluate.
#[derive(FromQueryResult)]
struct EmailQueryRow {
    id: String,
    thread_id: Option<String>,
    from_addr: String,
    to_addrs: String,
    subject: String,
    body_text: Option<String>,
    body_html: Option<String>,
    labels: Option<String>,
    received_at: chrono::NaiveDateTime,
    is_read: Option<bool>,
    list_unsubscribe: Option<String>,
    list_unsubscribe_post: Option<String>,
}

fn row_to_email_message(row: EmailQueryRow) -> crate::email::types::EmailMessage {
    let date = row.received_at.and_utc();

    let to: Vec<String> = row
        .to_addrs
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // `labels` is stored either as a JSON array (ingestion path) or as a legacy
    // comma-separated list; both shapes are in the wild.
    let labels_s = row.labels.unwrap_or_default();
    let labels: Vec<String> = if labels_s.starts_with('[') {
        serde_json::from_str(&labels_s).unwrap_or_default()
    } else {
        labels_s
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    let snippet = row
        .body_text
        .as_deref()
        .unwrap_or("")
        .chars()
        .take(256)
        .collect();

    crate::email::types::EmailMessage {
        id: row.id,
        thread_id: row.thread_id,
        from: row.from_addr,
        to,
        subject: row.subject,
        snippet,
        body: row.body_text,
        body_html: row.body_html,
        labels,
        date,
        is_read: row.is_read.unwrap_or(false),
        list_unsubscribe: row.list_unsubscribe,
        list_unsubscribe_post: row.list_unsubscribe_post,
    }
}

#[async_trait]
impl RuleEvaluator for SeaOrmRuleEvaluator {
    async fn evaluate_scope(
        &self,
        mode: RuleExecutionMode,
        scope: EvaluationScope,
    ) -> Result<Vec<RuleEvaluation>, RuleEvalError> {
        // `load_rules` takes the `Database` handle (ported in wave 1; it derives
        // its own SeaORM connection internally), which is why this adapter still
        // holds the enum rather than a bare connection.
        let rules = crate::rules::rule_engine::RuleEngine::load_rules(&self.db)
            .await
            .map_err(|e| RuleEvalError::Engine(e.to_string()))?;

        let conn = self.db.sea_orm();
        let email_rows = emails::Entity::find()
            .select_only()
            .column(emails::Column::Id)
            .column(emails::Column::ThreadId)
            .column(emails::Column::FromAddr)
            .column(emails::Column::ToAddrs)
            .column(emails::Column::Subject)
            .column(emails::Column::BodyText)
            .column(emails::Column::BodyHtml)
            .column(emails::Column::Labels)
            .column(emails::Column::ReceivedAt)
            .column(emails::Column::IsRead)
            .column(emails::Column::ListUnsubscribe)
            .column(emails::Column::ListUnsubscribePost)
            .filter(emails::Column::AccountId.eq(scope.account_id.as_str()))
            .into_model::<EmailQueryRow>()
            .all(&conn)
            .await
            .map_err(|e| RuleEvalError::Engine(e.to_string()))?;

        let messages: Vec<crate::email::types::EmailMessage> =
            email_rows.into_iter().map(row_to_email_message).collect();

        Ok(evaluate_rules(mode, &rules, &messages, &scope))
    }
}

// ---------------------------------------------------------------------------
// Behavior pins for the SeaORM port (ADR-036). These were written against the
// hand-rolled dual-arm implementation and must stay green unchanged across the
// port — they ARE the equivalence contract. The adapters had no coverage before
// this module.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::entities::{connected_accounts, emails, sync_state, topic_clusters};
    use crate::rules::rule_engine::RuleEngine;
    use crate::rules::types::{EmailField, MatchOperator, Rule, RuleAction, RuleCondition};
    use chrono::{NaiveDate, NaiveDateTime, Utc};
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseConnection};

    /// In-memory SQLite carrying every table these adapters read: `emails`
    /// (001 + the 016/018/021/027 column additions the entity declares),
    /// `topic_clusters` (017), `connected_accounts`/`sync_state` (004, plus
    /// 013/028's added columns), and `rules` (012 + 026).
    async fn fresh_db() -> Database {
        let db = crate::db::test_sqlite_database().await;
        let conn = db.sea_orm();
        for raw in [
            include_str!("../../../migrations/sqlite/001_initial_schema.sql"),
            include_str!("../../../migrations/sqlite/004_accounts.sql"),
            include_str!("../../../migrations/sqlite/012_rules.sql"),
            include_str!("../../../migrations/sqlite/013_account_settings.sql"),
            include_str!("../../../migrations/sqlite/016_soft_delete_trash_spam.sql"),
            include_str!("../../../migrations/sqlite/017_topic_clusters.sql"),
            include_str!("../../../migrations/sqlite/018_unsubscribe_headers.sql"),
            include_str!("../../../migrations/sqlite/021_thread_key.sql"),
            include_str!("../../../migrations/sqlite/026_rules_match_count.sql"),
            include_str!("../../../migrations/sqlite/027_is_archived.sql"),
            include_str!("../../../migrations/sqlite/028_imap_accounts.sql"),
        ] {
            // Strip line comments before splitting on ';'.
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
        }
        db
    }

    fn ts(day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 1, day)
            .expect("date")
            .and_hms_opt(hour, 0, 0)
            .expect("time")
    }

    /// Seed shape for one `emails` row. Defaults cover the columns no pin
    /// asserts on, so each test names only what it is pinning.
    struct Seed<'a> {
        id: &'a str,
        account_id: &'a str,
        from: &'a str,
        received: NaiveDateTime,
        labels: Option<&'a str>,
        list_unsubscribe: Option<&'a str>,
        list_unsubscribe_post: Option<&'a str>,
    }

    impl Default for Seed<'_> {
        fn default() -> Self {
            Self {
                id: "e1",
                account_id: "acct-a",
                from: "news@example.com",
                received: ts(1, 0),
                labels: None,
                list_unsubscribe: None,
                list_unsubscribe_post: None,
            }
        }
    }

    async fn seed(conn: &DatabaseConnection, s: Seed<'_>) {
        emails::ActiveModel {
            id: Set(s.id.to_owned()),
            account_id: Set(s.account_id.to_owned()),
            provider: Set("gmail".to_owned()),
            subject: Set(format!("subject {}", s.id)),
            from_addr: Set(s.from.to_owned()),
            to_addrs: Set("me@example.com, team@example.com".to_owned()),
            received_at: Set(s.received),
            body_text: Set(Some(format!("body {}", s.id))),
            labels: Set(s.labels.map(str::to_owned)),
            is_read: Set(Some(false)),
            is_starred: Set(Some(false)),
            has_attachments: Set(Some(false)),
            is_spam: Set(0),
            is_trash: Set(0),
            folder: Set("INBOX".to_owned()),
            is_archived: Set(false),
            list_unsubscribe: Set(s.list_unsubscribe.map(str::to_owned)),
            list_unsubscribe_post: Set(s.list_unsubscribe_post.map(str::to_owned)),
            ..Default::default()
        }
        .insert(conn)
        .await
        .expect("seed email");
    }

    async fn seed_cluster(conn: &DatabaseConnection, id: &str, email_ids: &str) {
        topic_clusters::ActiveModel {
            id: Set(id.to_owned()),
            name: Set(format!("cluster {id}")),
            description: Set(String::new()),
            centroid: Set(Vec::new()),
            email_ids: Set(email_ids.to_owned()),
            email_count: Set(0),
            top_terms: Set("[]".to_owned()),
            representative_email_ids: Set("[]".to_owned()),
            stability_score: Set(0.0),
            stability_runs: Set(0),
            is_pinned: Set(0),
            created_at: Set("2026-01-01T00:00:00Z".to_owned()),
            updated_at: Set("2026-01-01T00:00:00Z".to_owned()),
        }
        .insert(conn)
        .await
        .expect("seed cluster");
    }

    async fn seed_account(conn: &DatabaseConnection, id: &str, provider: &str) {
        connected_accounts::ActiveModel {
            id: Set(id.to_owned()),
            provider: Set(provider.to_owned()),
            email_address: Set(format!("{id}@example.com")),
            ..Default::default()
        }
        .insert(conn)
        .await
        .expect("seed account");
    }

    async fn seed_sync_state(
        conn: &DatabaseConnection,
        account_id: &str,
        history_id: Option<&str>,
    ) {
        sync_state::ActiveModel {
            account_id: Set(account_id.to_owned()),
            history_id: Set(history_id.map(str::to_owned)),
            ..Default::default()
        }
        .insert(conn)
        .await
        .expect("seed sync_state");
    }

    async fn seed_rule(db: &Database, id: &str, condition: RuleCondition) {
        let rule = Rule {
            id: id.to_owned(),
            name: format!("rule {id}"),
            description: String::new(),
            conditions: vec![condition],
            actions: vec![RuleAction::Archive],
            priority: 0,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        RuleEngine::save_rule(db, &rule).await.expect("seed rule");
    }

    fn labels_contain(value: &str) -> RuleCondition {
        RuleCondition::FieldMatch {
            field: EmailField::Labels,
            operator: MatchOperator::Contains,
            value: value.to_owned(),
        }
    }

    fn sorted_refs(refs: Vec<EmailRef>) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> =
            refs.into_iter().map(|r| (r.id, r.account_id)).collect();
        out.sort();
        out
    }

    // -----------------------------------------------------------------------
    // Cluster membership: `topic_clusters.email_ids` (a JSON array in a TEXT
    // column) resolved against `emails`.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn cluster_emails_resolve_ids_and_skip_ones_absent_from_emails() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed(
            &conn,
            Seed {
                id: "e1",
                account_id: "acct-a",
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "e2",
                account_id: "acct-b",
                ..Default::default()
            },
        )
        .await;
        // Present in `emails` but NOT in the cluster — must not come back.
        seed(
            &conn,
            Seed {
                id: "e3",
                account_id: "acct-a",
                ..Default::default()
            },
        )
        .await;
        // "ghost" is listed by the cluster but has no `emails` row: the old
        // INNER JOIN dropped it silently, and so must the port.
        seed_cluster(&conn, "c1", r#"["e1","ghost","e2"]"#).await;

        let repo = SeaOrmEmailRepository { db: db.clone() };
        assert_eq!(
            sorted_refs(repo.list_by_cluster("c1").await.expect("by cluster")),
            vec![
                ("e1".to_owned(), "acct-a".to_owned()),
                ("e2".to_owned(), "acct-b".to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn cluster_repository_and_email_repository_agree() {
        // The two adapters ran identical SQL; the port collapses them onto one
        // helper, so their results must remain indistinguishable.
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed(&conn, Seed::default()).await;
        seed(
            &conn,
            Seed {
                id: "e2",
                ..Default::default()
            },
        )
        .await;
        seed_cluster(&conn, "c1", r#"["e1","e2"]"#).await;

        let via_emails = SeaOrmEmailRepository { db: db.clone() }
            .list_by_cluster("c1")
            .await
            .expect("emails repo");
        let via_clusters = SeaOrmClusterRepository { db: db.clone() }
            .emails("c1")
            .await
            .expect("cluster repo");
        assert_eq!(sorted_refs(via_emails), sorted_refs(via_clusters));
    }

    #[tokio::test]
    async fn cluster_emails_empty_for_unknown_cluster_and_empty_array() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed(&conn, Seed::default()).await;
        seed_cluster(&conn, "c-empty", "[]").await;
        // A populated bystander cluster: without it, a lookup that ignored the
        // cluster id would still return empty and this test would still pass.
        seed_cluster(&conn, "c-full", r#"["e1"]"#).await;

        let repo = SeaOrmClusterRepository { db: db.clone() };
        assert!(repo.emails("c-missing").await.expect("unknown").is_empty());
        assert!(repo.emails("c-empty").await.expect("empty").is_empty());
        assert_eq!(repo.emails("c-full").await.expect("full").len(), 1);
    }

    // -----------------------------------------------------------------------
    // Account-scoped listing + counting.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn list_and_count_by_account_are_scoped_to_that_account() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed(
            &conn,
            Seed {
                id: "a1",
                account_id: "acct-a",
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "a2",
                account_id: "acct-a",
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "b1",
                account_id: "acct-b",
                ..Default::default()
            },
        )
        .await;

        let repo = SeaOrmEmailRepository { db: db.clone() };
        assert_eq!(
            sorted_refs(repo.list_by_account("acct-a").await.expect("list a")),
            vec![
                ("a1".to_owned(), "acct-a".to_owned()),
                ("a2".to_owned(), "acct-a".to_owned()),
            ]
        );
        assert_eq!(repo.count_by_account("acct-a").await.expect("count a"), 2);
        assert_eq!(repo.count_by_account("acct-b").await.expect("count b"), 1);
        assert_eq!(
            repo.count_by_account("acct-none")
                .await
                .expect("count none"),
            0
        );
    }

    // -----------------------------------------------------------------------
    // Subscription headers.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn find_by_sender_picks_the_newest_row_carrying_headers() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed(
            &conn,
            Seed {
                id: "old",
                received: ts(1, 0),
                list_unsubscribe: Some("mailto:bye@example.com"),
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "newer",
                received: ts(2, 0),
                list_unsubscribe: Some("https://example.com/u?t=1"),
                ..Default::default()
            },
        )
        .await;
        // Newest overall, but carries no headers — excluded by the WHERE, so
        // the https row still wins.
        seed(
            &conn,
            Seed {
                id: "newest-bare",
                received: ts(3, 0),
                ..Default::default()
            },
        )
        .await;

        let repo = SeaOrmSubscriptionRepository { db: db.clone() };
        let found = repo
            .find_by_sender("acct-a", "news@example.com")
            .await
            .expect("find")
            .expect("present");
        assert_eq!(found.method, UnsubscribeMethodKind::WebLink);
        assert_eq!(
            found.list_unsubscribe.as_deref(),
            Some("https://example.com/u?t=1")
        );
        assert!(found.list_unsubscribe_post.is_none());
    }

    #[tokio::test]
    async fn find_by_sender_classifies_method_and_scopes_by_account() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed(
            &conn,
            Seed {
                id: "post",
                from: "post@example.com",
                list_unsubscribe: Some("https://example.com/u"),
                list_unsubscribe_post: Some("List-Unsubscribe=One-Click"),
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "mailto",
                from: "mailto@example.com",
                list_unsubscribe: Some("mailto:bye@example.com"),
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "weird",
                from: "weird@example.com",
                list_unsubscribe: Some("carrier-pigeon"),
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "bare",
                from: "bare@example.com",
                ..Default::default()
            },
        )
        .await;
        // Same sender, different account: must stay invisible to acct-a.
        seed(
            &conn,
            Seed {
                id: "other-acct",
                account_id: "acct-b",
                from: "only-b@example.com",
                list_unsubscribe: Some("mailto:bye@example.com"),
                ..Default::default()
            },
        )
        .await;

        let repo = SeaOrmSubscriptionRepository { db: db.clone() };
        let method = |sender: &'static str| {
            let repo = SeaOrmSubscriptionRepository { db: db.clone() };
            async move {
                repo.find_by_sender("acct-a", sender)
                    .await
                    .expect("find")
                    .map(|r| r.method)
            }
        };
        assert_eq!(
            method("post@example.com").await,
            Some(UnsubscribeMethodKind::ListUnsubscribePost)
        );
        assert_eq!(
            method("mailto@example.com").await,
            Some(UnsubscribeMethodKind::Mailto)
        );
        assert_eq!(
            method("weird@example.com").await,
            Some(UnsubscribeMethodKind::None)
        );
        // No row carries headers for this sender → no record at all.
        assert!(method("bare@example.com").await.is_none());
        assert!(repo
            .find_by_sender("acct-a", "only-b@example.com")
            .await
            .expect("cross-account")
            .is_none());
    }

    // -----------------------------------------------------------------------
    // Account state etag (connected_accounts + sync_state).
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn etag_reads_the_requested_accounts_own_history_id() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        // Two fully-populated accounts: an etag lookup that ignored the account
        // id would return a plausible history id from the wrong mailbox, and a
        // single-account test would never notice.
        seed_account(&conn, "acct-a", "gmail").await;
        seed_sync_state(&conn, "acct-a", Some("99123")).await;
        seed_account(&conn, "acct-b", "gmail").await;
        seed_sync_state(&conn, "acct-b", Some("55777")).await;

        let provider = SeaOrmAccountStateProvider { db: db.clone() };
        assert_eq!(
            provider.etag("acct-a").await.expect("etag a"),
            AccountStateEtag::GmailHistory {
                history_id: "99123".to_owned()
            }
        );
        assert_eq!(
            provider.etag("acct-b").await.expect("etag b"),
            AccountStateEtag::GmailHistory {
                history_id: "55777".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn etag_is_none_without_a_sync_state_row_or_usable_history_id() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        // The LEFT JOIN's whole point: an account with no sync_state row still
        // resolves (to None), it does not disappear.
        seed_account(&conn, "acct-nostate", "gmail").await;
        seed_account(&conn, "acct-null", "gmail").await;
        seed_sync_state(&conn, "acct-null", None).await;
        seed_account(&conn, "acct-empty", "gmail").await;
        seed_sync_state(&conn, "acct-empty", Some("")).await;
        // Non-gmail providers have no history id concept yet.
        seed_account(&conn, "acct-outlook", "outlook").await;
        seed_sync_state(&conn, "acct-outlook", Some("99123")).await;

        let provider = SeaOrmAccountStateProvider { db: db.clone() };
        for account in [
            "acct-nostate",
            "acct-null",
            "acct-empty",
            "acct-outlook",
            "acct-unknown",
        ] {
            assert_eq!(
                provider.etag(account).await.expect("etag"),
                AccountStateEtag::None,
                "{account} must resolve to AccountStateEtag::None"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Rule evaluation over an account's emails.
    // -----------------------------------------------------------------------

    fn scope(account_id: &str) -> EvaluationScope {
        EvaluationScope {
            account_id: account_id.to_owned(),
            rule_ids: Vec::new(),
            sample_size: 20,
        }
    }

    #[tokio::test]
    async fn evaluate_scope_parses_labels_in_both_stored_formats() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        // The `labels` column holds either a JSON array (ingestion path) or a
        // legacy comma-separated list; both must reach the rule engine as a
        // parsed label list.
        seed(
            &conn,
            Seed {
                id: "e-json",
                labels: Some(r#"["Promo","News"]"#),
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "e-csv",
                labels: Some("Promo, News"),
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "e-null",
                labels: None,
                ..Default::default()
            },
        )
        .await;
        seed_rule(&db, "r-labels", labels_contain("promo")).await;

        let evaluator = SeaOrmRuleEvaluator { db: db.clone() };
        let evals = evaluator
            .evaluate_scope(RuleExecutionMode::EvaluateOnly, scope("acct-a"))
            .await
            .expect("evaluate");
        assert_eq!(evals.len(), 1);
        let mut matched = evals[0].matched_email_ids.clone();
        matched.sort();
        assert_eq!(matched, vec!["e-csv".to_owned(), "e-json".to_owned()]);
        assert_eq!(evals[0].projected_count, 2);
    }

    #[tokio::test]
    async fn evaluate_scope_never_sees_another_accounts_emails() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed(
            &conn,
            Seed {
                id: "a1",
                account_id: "acct-a",
                labels: Some("Promo"),
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "b1",
                account_id: "acct-b",
                labels: Some("Promo"),
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "b2",
                account_id: "acct-b",
                labels: Some("Promo"),
                ..Default::default()
            },
        )
        .await;
        seed_rule(&db, "r-labels", labels_contain("promo")).await;

        let evaluator = SeaOrmRuleEvaluator { db: db.clone() };
        let a = evaluator
            .evaluate_scope(RuleExecutionMode::EvaluateOnly, scope("acct-a"))
            .await
            .expect("evaluate a");
        assert_eq!(a[0].matched_email_ids, vec!["a1".to_owned()]);
        assert_eq!(a[0].projected_count, 1);

        let b = evaluator
            .evaluate_scope(RuleExecutionMode::EvaluateOnly, scope("acct-b"))
            .await
            .expect("evaluate b");
        let mut b_ids = b[0].matched_email_ids.clone();
        b_ids.sort();
        assert_eq!(b_ids, vec!["b1".to_owned(), "b2".to_owned()]);
        assert_eq!(b[0].projected_count, 2);
    }

    // -----------------------------------------------------------------------
    // Row → EmailMessage projection. Snippet truncation and the to_addrs split
    // are not observable through `evaluate_scope`'s result, so they are pinned
    // directly on the mapping function.
    // -----------------------------------------------------------------------

    /// One `emails` projection with the fields no assertion touches defaulted.
    fn query_row(labels: Option<&str>, body_text: Option<String>) -> EmailQueryRow {
        EmailQueryRow {
            id: "e1".to_owned(),
            thread_id: None,
            from_addr: "from@example.com".to_owned(),
            to_addrs: String::new(),
            subject: "subj".to_owned(),
            body_text,
            body_html: None,
            labels: labels.map(str::to_owned),
            received_at: ts(1, 0),
            is_read: Some(false),
            list_unsubscribe: None,
            list_unsubscribe_post: None,
        }
    }

    #[test]
    fn row_to_email_message_truncates_snippet_and_parses_json_labels() {
        let body = "x".repeat(300);
        let msg = row_to_email_message(EmailQueryRow {
            thread_id: Some("t1".to_owned()),
            to_addrs: " a@example.com , b@example.com ,".to_owned(),
            body_html: Some("<p>hi</p>".to_owned()),
            received_at: ts(2, 3),
            is_read: Some(true),
            list_unsubscribe: Some("mailto:bye@example.com".to_owned()),
            ..query_row(Some(r#"["A","B"]"#), Some(body.clone()))
        });

        assert_eq!(
            msg.snippet.chars().count(),
            256,
            "snippet caps at 256 chars"
        );
        assert!(msg.snippet.chars().all(|c| c == 'x'));
        assert_eq!(msg.body.as_deref(), Some(body.as_str()), "body is not cut");
        assert_eq!(msg.labels, vec!["A".to_owned(), "B".to_owned()]);
        // Split on ',', trimmed, empties dropped.
        assert_eq!(
            msg.to,
            vec!["a@example.com".to_owned(), "b@example.com".to_owned()]
        );
        // The column is plain TIMESTAMP (no zone); the stored value is already
        // UTC, so `.and_utc()` reinterprets rather than converts.
        assert_eq!(msg.date.to_rfc3339(), "2026-01-02T03:00:00+00:00");
        assert!(msg.is_read);
        assert_eq!(msg.thread_id.as_deref(), Some("t1"));
        assert_eq!(
            msg.list_unsubscribe.as_deref(),
            Some("mailto:bye@example.com")
        );
    }

    #[test]
    fn row_to_email_message_parses_comma_labels_and_tolerates_absent_text() {
        let msg = row_to_email_message(query_row(Some(" A , B ,"), None));
        assert_eq!(msg.labels, vec!["A".to_owned(), "B".to_owned()]);
        assert!(msg.to.is_empty());
        assert_eq!(msg.snippet, "", "a NULL body yields an empty snippet");

        // A NULL labels column and a malformed JSON array both degrade to the
        // empty label list rather than failing the row.
        assert!(row_to_email_message(query_row(None, None))
            .labels
            .is_empty());
        assert!(row_to_email_message(query_row(Some("[not json"), None))
            .labels
            .is_empty());
    }

    #[test]
    fn row_to_email_message_reads_a_null_is_read_as_false() {
        // The column is nullable (`BOOLEAN DEFAULT FALSE`); the pre-port bare
        // `bool` decode failed the whole scope on a NULL. Honouring the entity's
        // nullability, the row now evaluates with the column's own default.
        let msg = row_to_email_message(EmailQueryRow {
            is_read: None,
            ..query_row(None, None)
        });
        assert!(!msg.is_read);
    }
}
