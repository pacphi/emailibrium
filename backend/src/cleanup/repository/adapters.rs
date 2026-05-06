//! Phase A SQLx adapters for the four `cleanup::domain::ports` traits.
//!
//! These are intentionally thin in Phase A — the live emails table schema and
//! cluster/subscription queries are still in flux as cleanup intersects with
//! existing repositories. Phase B/C will expand these to read real data; for
//! now they return empty vectors so the wiring compiles end-to-end and tests
//! can use in-memory fakes (`builder::tests`).

use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::cleanup::domain::operation::{AccountStateEtag, EmailRef};
use crate::cleanup::domain::ports::{
    AccountStateProvider, ClusterRepository, EmailRepository, RepoError, RuleEvalError,
    RuleEvaluator, SubscriptionRecord, SubscriptionRepository,
};
use crate::rules::types::{EvaluationScope, RuleEvaluation, RuleExecutionMode};

pub struct SqlxEmailRepository {
    pub pool: SqlitePool,
}

#[async_trait]
impl EmailRepository for SqlxEmailRepository {
    async fn list_by_account(&self, _account_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        // TODO(phase-c): read from `emails` table once the repository surface
        // for cleanup-scoped queries is finalised.
        Ok(Vec::new())
    }
    async fn list_by_cluster(&self, _cluster_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        Ok(Vec::new())
    }
    async fn count_by_account(&self, _account_id: &str) -> Result<u64, RepoError> {
        Ok(0)
    }
}

pub struct SqlxSubscriptionRepository {
    pub pool: SqlitePool,
}

#[async_trait]
impl SubscriptionRepository for SqlxSubscriptionRepository {
    async fn list_by_account(
        &self,
        _account_id: &str,
    ) -> Result<Vec<SubscriptionRecord>, RepoError> {
        Ok(Vec::new())
    }
    async fn find_by_sender(
        &self,
        _account_id: &str,
        _sender: &str,
    ) -> Result<Option<SubscriptionRecord>, RepoError> {
        Ok(None)
    }
}

pub struct SqlxClusterRepository {
    pub pool: SqlitePool,
}

#[async_trait]
impl ClusterRepository for SqlxClusterRepository {
    async fn emails(&self, _cluster_id: &str) -> Result<Vec<EmailRef>, RepoError> {
        Ok(Vec::new())
    }
}

pub struct SqlxAccountStateProvider {
    pub pool: SqlitePool,
}

#[async_trait]
impl AccountStateProvider for SqlxAccountStateProvider {
    async fn etag(&self, _account_id: &str) -> Result<AccountStateEtag, RepoError> {
        Ok(AccountStateEtag::None)
    }
}

pub struct SqlxRuleEvaluator {
    pub pool: SqlitePool,
}

#[async_trait]
impl RuleEvaluator for SqlxRuleEvaluator {
    async fn evaluate_scope(
        &self,
        _mode: RuleExecutionMode,
        _scope: EvaluationScope,
    ) -> Result<Vec<RuleEvaluation>, RuleEvalError> {
        // TODO(phase-c): wire to `rules::rule_processor::evaluate_rules`.
        Ok(Vec::new())
    }
}
