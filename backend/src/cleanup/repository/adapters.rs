//! SQLx adapters for the cleanup domain port traits.
//!
//! Each adapter reads from the existing tables shared with the rest of the
//! application. No new tables are introduced here — the cleanup domain reuses
//! `emails`, `topic_clusters`, `connected_accounts`, `sync_state`, and `rules`.

use async_trait::async_trait;

use crate::cleanup::domain::operation::{AccountStateEtag, EmailRef, UnsubscribeMethodKind};
use crate::cleanup::domain::ports::{
    AccountStateProvider, ClusterRepository, EmailRepository, RepoError, RuleEvalError,
    RuleEvaluator, SubscriptionRecord, SubscriptionRepository,
};
use crate::db::{audited_sql, Database};
use crate::rules::rule_processor::evaluate_rules;
use crate::rules::types::{EvaluationScope, RuleEvaluation, RuleExecutionMode};

/// `topic_clusters.email_ids` is a JSON array of email id strings stored as plain TEXT in
/// both dialects. SQLite unpacks it with the `json_each` table-valued function; Postgres has
/// no equivalent table function for a plain-TEXT column and instead needs
/// `jsonb_array_elements_text` over a `::jsonb` cast — a genuinely different join shape, not
/// a placeholder/function-name substitution `Database::adapt` could cover, so each backend
/// gets its own SQL text here (ADR-035 §2.3).
fn emails_by_cluster_sql(db: &Database) -> &'static str {
    match db {
        Database::Sqlite(_) => {
            r#"SELECT e.id, e.account_id
               FROM emails e
               INNER JOIN (
                   SELECT j.value AS eid
                   FROM topic_clusters tc, json_each(tc.email_ids) AS j
                   WHERE tc.id = ?
               ) AS cm ON cm.eid = e.id"#
        }
        Database::Postgres(_) => {
            r#"SELECT e.id, e.account_id
               FROM emails e
               INNER JOIN (
                   SELECT j.value AS eid
                   FROM topic_clusters tc, jsonb_array_elements_text(tc.email_ids::jsonb) AS j
                   WHERE tc.id = $1
               ) AS cm ON cm.eid = e.id"#
        }
    }
}

// ---------------------------------------------------------------------------
// Email repository
// ---------------------------------------------------------------------------

pub struct SqlxEmailRepository {
    pub db: Database,
}

#[async_trait]
impl EmailRepository for SqlxEmailRepository {
    async fn list_by_account(&self, account_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        let sql = self
            .db
            .adapt("SELECT id, account_id FROM emails WHERE account_id = ?");
        let rows: Vec<(String, String)> = match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .fetch_all(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .fetch_all(pool)
                    .await?
            }
        };
        Ok(rows
            .into_iter()
            .map(|(id, account_id)| EmailRef { id, account_id })
            .collect())
    }

    async fn list_by_cluster(&self, cluster_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        let sql = emails_by_cluster_sql(&self.db);
        let rows: Vec<(String, String)> = match &self.db {
            Database::Sqlite(pool) => sqlx::query_as(sql).bind(cluster_id).fetch_all(pool).await?,
            Database::Postgres(pool) => {
                sqlx::query_as(sql).bind(cluster_id).fetch_all(pool).await?
            }
        };
        Ok(rows
            .into_iter()
            .map(|(id, account_id)| EmailRef { id, account_id })
            .collect())
    }

    async fn count_by_account(&self, account_id: &str) -> Result<u64, RepoError> {
        let sql = self
            .db
            .adapt("SELECT COUNT(*) FROM emails WHERE account_id = ?");
        let row: (i64,) = match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .fetch_one(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .fetch_one(pool)
                    .await?
            }
        };
        Ok(row.0.max(0) as u64)
    }
}

// ---------------------------------------------------------------------------
// Subscription repository
// ---------------------------------------------------------------------------

pub struct SqlxSubscriptionRepository {
    pub db: Database,
}

#[async_trait]
impl SubscriptionRepository for SqlxSubscriptionRepository {
    async fn find_by_sender(
        &self,
        account_id: &str,
        sender: &str,
    ) -> Result<Option<SubscriptionRecord>, RepoError> {
        // Most recent email from this sender that carries unsubscribe headers.
        let sql = self.db.adapt(
            r#"SELECT list_unsubscribe, list_unsubscribe_post
               FROM emails
               WHERE account_id = ? AND from_addr = ?
                 AND (list_unsubscribe IS NOT NULL OR list_unsubscribe_post IS NOT NULL)
               ORDER BY received_at DESC
               LIMIT 1"#,
        );
        let header_row: Option<(Option<String>, Option<String>)> = match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .bind(sender)
                    .fetch_optional(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .bind(sender)
                    .fetch_optional(pool)
                    .await?
            }
        };

        let Some((lu, lup)) = header_row else {
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

pub struct SqlxClusterRepository {
    pub db: Database,
}

#[async_trait]
impl ClusterRepository for SqlxClusterRepository {
    async fn emails(&self, cluster_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        let sql = emails_by_cluster_sql(&self.db);
        let rows: Vec<(String, String)> = match &self.db {
            Database::Sqlite(pool) => sqlx::query_as(sql).bind(cluster_id).fetch_all(pool).await?,
            Database::Postgres(pool) => {
                sqlx::query_as(sql).bind(cluster_id).fetch_all(pool).await?
            }
        };
        Ok(rows
            .into_iter()
            .map(|(id, account_id)| EmailRef { id, account_id })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Account state provider
// ---------------------------------------------------------------------------

pub struct SqlxAccountStateProvider {
    pub db: Database,
}

#[async_trait]
impl AccountStateProvider for SqlxAccountStateProvider {
    async fn etag(&self, account_id: &str) -> Result<AccountStateEtag, RepoError> {
        let sql = self.db.adapt(
            r#"SELECT ca.provider, ss.history_id
               FROM connected_accounts ca
               LEFT JOIN sync_state ss ON ss.account_id = ca.id
               WHERE ca.id = ?"#,
        );
        let row: Option<(String, Option<String>)> = match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .fetch_optional(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .fetch_optional(pool)
                    .await?
            }
        };

        let Some((provider, history_id)) = row else {
            return Ok(AccountStateEtag::None);
        };

        Ok(match provider.as_str() {
            "gmail" => match history_id {
                Some(h) if !h.is_empty() => AccountStateEtag::GmailHistory { history_id: h },
                _ => AccountStateEtag::None,
            },
            // Outlook delta tokens and IMAP UIDVALIDITY/MODSEQ are not tracked
            // in sync_state today; extend here when those sync paths are added.
            _ => AccountStateEtag::None,
        })
    }
}

// ---------------------------------------------------------------------------
// Rule evaluator
// ---------------------------------------------------------------------------

pub struct SqlxRuleEvaluator {
    pub db: Database,
}

/// Portable decode target for one `emails` row, in the exact column order selected by
/// `evaluate_scope` below. `received_at` decodes as `NaiveDateTime` (not `DateTime<Utc>`)
/// because the column is `TIMESTAMP` (no zone) in both dialects, not `TIMESTAMPTZ` — sqlx
/// only accepts an exact type match on decode (see ADR-035); the stored values are already
/// effectively UTC, so `.and_utc()` recovers the same `DateTime<Utc>` the old `Row::get`
/// path produced.
type EmailQueryRow = (
    String,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    chrono::NaiveDateTime,
    bool,
    Option<String>,
    Option<String>,
);

fn row_to_email_message(row: EmailQueryRow) -> crate::email::types::EmailMessage {
    let (
        id,
        thread_id,
        from_addr,
        to_addrs,
        subject,
        body_text,
        body_html,
        labels_s,
        received_at,
        is_read,
        list_unsubscribe,
        list_unsubscribe_post,
    ) = row;

    let date = received_at.and_utc();

    let to: Vec<String> = to_addrs
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let labels_s = labels_s.unwrap_or_default();
    let labels: Vec<String> = if labels_s.starts_with('[') {
        serde_json::from_str(&labels_s).unwrap_or_default()
    } else {
        labels_s
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    let snippet = body_text
        .as_deref()
        .unwrap_or("")
        .chars()
        .take(256)
        .collect();

    crate::email::types::EmailMessage {
        id,
        thread_id,
        from: from_addr,
        to,
        subject,
        snippet,
        body: body_text,
        body_html,
        labels,
        date,
        is_read,
        list_unsubscribe,
        list_unsubscribe_post,
    }
}

#[async_trait]
impl RuleEvaluator for SqlxRuleEvaluator {
    async fn evaluate_scope(
        &self,
        mode: RuleExecutionMode,
        scope: EvaluationScope,
    ) -> Result<Vec<RuleEvaluation>, RuleEvalError> {
        let rules = crate::rules::rule_engine::RuleEngine::load_rules(&self.db)
            .await
            .map_err(|e| RuleEvalError::Engine(e.to_string()))?;

        let sql = self.db.adapt(
            r#"SELECT id, thread_id, from_addr, to_addrs, subject,
                      body_text, body_html, labels, received_at, is_read,
                      list_unsubscribe, list_unsubscribe_post
               FROM emails WHERE account_id = ?"#,
        );
        let email_rows: Vec<EmailQueryRow> = match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(&scope.account_id)
                    .fetch_all(pool)
                    .await
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(&scope.account_id)
                    .fetch_all(pool)
                    .await
            }
        }
        .map_err(|e| RuleEvalError::Engine(e.to_string()))?;

        let emails: Vec<crate::email::types::EmailMessage> =
            email_rows.into_iter().map(row_to_email_message).collect();

        Ok(evaluate_rules(mode, &rules, &emails, &scope))
    }
}
