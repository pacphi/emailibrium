//! Hexagonal ports for the Cleanup Planning domain (Phase A).
//!
//! These traits are the *only* dependencies `PlanBuilder` has on the outside
//! world. The SQLx-backed adapters live in `cleanup::repository::adapters`;
//! tests use in-memory fakes.

use async_trait::async_trait;
use thiserror::Error;

use crate::rules::types::{EvaluationScope, RuleEvaluation, RuleExecutionMode};

use super::operation::{AccountStateEtag, EmailRef, UnsubscribeMethodKind};

#[derive(Debug, Error)]
pub enum RepoError {
    #[error("not found")]
    NotFound,
    #[error("sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Debug, Error)]
pub enum RuleEvalError {
    #[error("rule engine: {0}")]
    Engine(String),
}

/// Domain-side subscription record. Distinct from
/// `email::unsubscribe::Subscription` to avoid name collision with
/// `PlanSource::Subscription`.
#[derive(Debug, Clone)]
pub struct SubscriptionRecord {
    pub method: UnsubscribeMethodKind,
}

#[async_trait]
pub trait EmailRepository: Send + Sync {
    async fn list_by_account(&self, account_id: &str) -> Result<Vec<EmailRef>, RepoError>;
    async fn list_by_cluster(&self, cluster_id: &str) -> Result<Vec<EmailRef>, RepoError>;
    async fn count_by_account(&self, account_id: &str) -> Result<u64, RepoError>;
}

#[async_trait]
pub trait SubscriptionRepository: Send + Sync {
    async fn find_by_sender(
        &self,
        account_id: &str,
        sender: &str,
    ) -> Result<Option<SubscriptionRecord>, RepoError>;
}

#[async_trait]
pub trait ClusterRepository: Send + Sync {
    async fn emails(&self, cluster_id: &str) -> Result<Vec<EmailRef>, RepoError>;
}

#[async_trait]
pub trait AccountStateProvider: Send + Sync {
    async fn etag(&self, account_id: &str) -> Result<AccountStateEtag, RepoError>;
}

#[async_trait]
pub trait RuleEvaluator: Send + Sync {
    async fn evaluate_scope(
        &self,
        mode: RuleExecutionMode,
        scope: EvaluationScope,
    ) -> Result<Vec<RuleEvaluation>, RuleEvalError>;
}
