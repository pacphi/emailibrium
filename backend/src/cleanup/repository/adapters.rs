//! SQLx adapters for the cleanup domain port traits.
//!
//! Each adapter reads from the existing tables shared with the rest of the
//! application. No new tables are introduced here — the cleanup domain reuses
//! `emails`, `topic_clusters`, `connected_accounts`, `sync_state`, and `rules`.

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::cleanup::domain::operation::{AccountStateEtag, EmailRef, UnsubscribeMethodKind};
use crate::cleanup::domain::ports::{
    AccountStateProvider, ClusterRepository, EmailRepository, RepoError, RuleEvalError,
    RuleEvaluator, SubscriptionRecord, SubscriptionRepository,
};
use crate::rules::rule_processor::evaluate_rules;
use crate::rules::types::{EvaluationScope, RuleEvaluation, RuleExecutionMode};

// ---------------------------------------------------------------------------
// Email repository
// ---------------------------------------------------------------------------

pub struct SqlxEmailRepository {
    pub pool: SqlitePool,
}

#[async_trait]
impl EmailRepository for SqlxEmailRepository {
    async fn list_by_account(&self, account_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        let rows = sqlx::query("SELECT id, account_id FROM emails WHERE account_id = ?")
            .bind(account_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| EmailRef {
                id: r.get("id"),
                account_id: r.get("account_id"),
            })
            .collect())
    }

    async fn list_by_cluster(&self, cluster_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        // topic_clusters.email_ids is a JSON array of email id strings.
        let rows = sqlx::query(
            r#"SELECT e.id, e.account_id
               FROM emails e
               INNER JOIN (
                   SELECT j.value AS eid
                   FROM topic_clusters tc, json_each(tc.email_ids) AS j
                   WHERE tc.id = ?
               ) AS cm ON cm.eid = e.id"#,
        )
        .bind(cluster_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| EmailRef {
                id: r.get("id"),
                account_id: r.get("account_id"),
            })
            .collect())
    }

    async fn count_by_account(&self, account_id: &str) -> Result<u64, RepoError> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM emails WHERE account_id = ?")
            .bind(account_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0.max(0) as u64)
    }
}

// ---------------------------------------------------------------------------
// Subscription repository
// ---------------------------------------------------------------------------

pub struct SqlxSubscriptionRepository {
    pub pool: SqlitePool,
}

#[async_trait]
impl SubscriptionRepository for SqlxSubscriptionRepository {
    async fn find_by_sender(
        &self,
        account_id: &str,
        sender: &str,
    ) -> Result<Option<SubscriptionRecord>, RepoError> {
        // Most recent email from this sender that carries unsubscribe headers.
        let header_row = sqlx::query(
            r#"SELECT list_unsubscribe, list_unsubscribe_post
               FROM emails
               WHERE account_id = ? AND from_addr = ?
                 AND (list_unsubscribe IS NOT NULL OR list_unsubscribe_post IS NOT NULL)
               ORDER BY received_at DESC
               LIMIT 1"#,
        )
        .bind(account_id)
        .bind(sender)
        .fetch_optional(&self.pool)
        .await?;

        let Some(r) = header_row else {
            return Ok(None);
        };

        let lu: Option<String> = r.get("list_unsubscribe");
        let lup: Option<String> = r.get("list_unsubscribe_post");

        let method = if lup.is_some() {
            UnsubscribeMethodKind::ListUnsubscribePost
        } else if lu.as_deref().is_some_and(|v| v.starts_with("mailto:")) {
            UnsubscribeMethodKind::Mailto
        } else if lu.as_deref().is_some_and(|v| v.starts_with("http")) {
            UnsubscribeMethodKind::WebLink
        } else {
            UnsubscribeMethodKind::None
        };

        Ok(Some(SubscriptionRecord { method }))
    }
}

// ---------------------------------------------------------------------------
// Cluster repository
// ---------------------------------------------------------------------------

pub struct SqlxClusterRepository {
    pub pool: SqlitePool,
}

#[async_trait]
impl ClusterRepository for SqlxClusterRepository {
    async fn emails(&self, cluster_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        let rows = sqlx::query(
            r#"SELECT e.id, e.account_id
               FROM emails e
               INNER JOIN (
                   SELECT j.value AS eid
                   FROM topic_clusters tc, json_each(tc.email_ids) AS j
                   WHERE tc.id = ?
               ) AS cm ON cm.eid = e.id"#,
        )
        .bind(cluster_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| EmailRef {
                id: r.get("id"),
                account_id: r.get("account_id"),
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Account state provider
// ---------------------------------------------------------------------------

pub struct SqlxAccountStateProvider {
    pub pool: SqlitePool,
}

#[async_trait]
impl AccountStateProvider for SqlxAccountStateProvider {
    async fn etag(&self, account_id: &str) -> Result<AccountStateEtag, RepoError> {
        let row = sqlx::query(
            r#"SELECT ca.provider, ss.history_id
               FROM connected_accounts ca
               LEFT JOIN sync_state ss ON ss.account_id = ca.id
               WHERE ca.id = ?"#,
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(r) = row else {
            return Ok(AccountStateEtag::None);
        };

        let provider: String = r.get("provider");
        let history_id: Option<String> = r.get("history_id");

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
    pub pool: SqlitePool,
}

#[async_trait]
impl RuleEvaluator for SqlxRuleEvaluator {
    async fn evaluate_scope(
        &self,
        mode: RuleExecutionMode,
        scope: EvaluationScope,
    ) -> Result<Vec<RuleEvaluation>, RuleEvalError> {
        let rules = crate::rules::rule_engine::RuleEngine::load_rules(&self.pool)
            .await
            .map_err(|e| RuleEvalError::Engine(e.to_string()))?;

        let email_rows = sqlx::query(
            r#"SELECT id, thread_id, from_addr, to_addrs, subject,
                      body_text, body_html, labels, received_at, is_read,
                      list_unsubscribe, list_unsubscribe_post
               FROM emails WHERE account_id = ?"#,
        )
        .bind(&scope.account_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RuleEvalError::Engine(e.to_string()))?;

        let emails: Vec<crate::email::types::EmailMessage> = email_rows
            .iter()
            .map(|r| {
                let date = r
                    .try_get::<chrono::DateTime<chrono::Utc>, _>("received_at")
                    .unwrap_or_else(|_| chrono::Utc::now());

                let to_s: String = r.get("to_addrs");
                let to: Vec<String> = to_s
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                let labels_s: String = r.try_get("labels").unwrap_or_default();
                let labels: Vec<String> = if labels_s.starts_with('[') {
                    serde_json::from_str(&labels_s).unwrap_or_default()
                } else {
                    labels_s
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                let body_text: Option<String> = r.get("body_text");
                let snippet = body_text
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(256)
                    .collect();

                crate::email::types::EmailMessage {
                    id: r.get("id"),
                    thread_id: r.get("thread_id"),
                    from: r.get("from_addr"),
                    to,
                    subject: r.get("subject"),
                    snippet,
                    body: body_text,
                    body_html: r.get("body_html"),
                    labels,
                    date,
                    is_read: r.get("is_read"),
                    list_unsubscribe: r.get("list_unsubscribe"),
                    list_unsubscribe_post: r.get("list_unsubscribe_post"),
                }
            })
            .collect();

        Ok(evaluate_rules(mode, &rules, &emails, &scope))
    }
}
