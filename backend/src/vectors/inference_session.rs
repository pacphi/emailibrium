//! Inference session aggregate (DDD-006: AI Providers, Audit Item #38).
//!
//! `InferenceSession` tracks active inference contexts including model,
//! provider, token usage, and latency. Sessions are created per request
//! or batch and completed when the inference finishes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::model_registry::ProviderType;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Status of an inference session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Completed,
    Failed,
    TimedOut,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::TimedOut => write!(f, "timed_out"),
        }
    }
}

/// An inference session tracking a single inference request or batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceSession {
    pub id: String,
    pub model: String,
    pub provider: ProviderType,
    pub status: SessionStatus,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

impl InferenceSession {
    /// Create a new active session.
    pub fn start(model: impl Into<String>, provider: ProviderType) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            model: model.into(),
            provider,
            status: SessionStatus::Active,
            prompt_tokens: 0,
            completion_tokens: 0,
            started_at: Utc::now(),
            completed_at: None,
            latency_ms: None,
            error: None,
        }
    }

    /// Mark the session as completed with token counts.
    pub fn complete(&mut self, prompt_tokens: u32, completion_tokens: u32) {
        self.status = SessionStatus::Completed;
        self.prompt_tokens = prompt_tokens;
        self.completion_tokens = completion_tokens;
        self.completed_at = Some(Utc::now());
        self.latency_ms = Some((Utc::now() - self.started_at).num_milliseconds().max(0) as u64);
    }

    /// Mark the session as failed.
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = SessionStatus::Failed;
        self.error = Some(error.into());
        self.completed_at = Some(Utc::now());
        self.latency_ms = Some((Utc::now() - self.started_at).num_milliseconds().max(0) as u64);
    }

    /// Total tokens used.
    pub fn total_tokens(&self) -> u32 {
        self.prompt_tokens + self.completion_tokens
    }
}

// ---------------------------------------------------------------------------
// Session Manager
// ---------------------------------------------------------------------------

/// Aggregate usage statistics.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UsageStats {
    pub total_sessions: u64,
    pub active_sessions: u64,
    pub completed_sessions: u64,
    pub failed_sessions: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub avg_latency_ms: f64,
}

/// Manages inference sessions across the system.
pub struct InferenceSessionManager {
    sessions: Arc<RwLock<HashMap<String, InferenceSession>>>,
    /// Maximum number of completed sessions to retain.
    max_history: usize,
}

impl InferenceSessionManager {
    pub fn new(max_history: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            max_history,
        }
    }

    /// Start a new inference session.
    pub async fn start_session(&self, model: impl Into<String>, provider: ProviderType) -> String {
        let session = InferenceSession::start(model, provider);
        let id = session.id.clone();
        let mut sessions = self.sessions.write().await;
        sessions.insert(id.clone(), session);
        id
    }

    /// Complete a session with token counts.
    pub async fn complete_session(
        &self,
        session_id: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
    ) -> Option<InferenceSession> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.complete(prompt_tokens, completion_tokens);
            Some(session.clone())
        } else {
            None
        }
    }

    /// Fail a session with an error message.
    pub async fn fail_session(
        &self,
        session_id: &str,
        error: impl Into<String>,
    ) -> Option<InferenceSession> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.fail(error);
            Some(session.clone())
        } else {
            None
        }
    }

    /// Get a session by ID.
    pub async fn get_session(&self, session_id: &str) -> Option<InferenceSession> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// Get all active sessions.
    pub async fn active_sessions(&self) -> Vec<InferenceSession> {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .filter(|s| s.status == SessionStatus::Active)
            .cloned()
            .collect()
    }

    /// Compute aggregate usage statistics.
    pub async fn usage_stats(&self) -> UsageStats {
        let sessions = self.sessions.read().await;
        let mut stats = UsageStats::default();
        let mut total_latency: u64 = 0;
        let mut completed_count: u64 = 0;

        for session in sessions.values() {
            stats.total_sessions += 1;
            match session.status {
                SessionStatus::Active => stats.active_sessions += 1,
                SessionStatus::Completed => {
                    stats.completed_sessions += 1;
                    stats.total_prompt_tokens += session.prompt_tokens as u64;
                    stats.total_completion_tokens += session.completion_tokens as u64;
                    if let Some(latency) = session.latency_ms {
                        total_latency += latency;
                        completed_count += 1;
                    }
                }
                SessionStatus::Failed | SessionStatus::TimedOut => {
                    stats.failed_sessions += 1;
                }
            }
        }

        if completed_count > 0 {
            stats.avg_latency_ms = total_latency as f64 / completed_count as f64;
        }

        stats
    }

    /// Prune completed/failed sessions exceeding max_history.
    pub async fn prune(&self) {
        let mut sessions = self.sessions.write().await;
        let mut completed: Vec<(String, DateTime<Utc>)> = sessions
            .iter()
            .filter(|(_, s)| s.status != SessionStatus::Active)
            .filter_map(|(id, s)| s.completed_at.map(|t| (id.clone(), t)))
            .collect();

        if completed.len() <= self.max_history {
            return;
        }

        // Sort by completed_at ascending (oldest first)
        completed.sort_by_key(|(_, t)| *t);
        let to_remove = completed.len() - self.max_history;
        for (id, _) in completed.into_iter().take(to_remove) {
            sessions.remove(&id);
        }
    }
}

impl Default for InferenceSessionManager {
    fn default() -> Self {
        Self::new(1000)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_lifecycle() {
        let manager = InferenceSessionManager::new(100);

        let id = manager
            .start_session("test-model", ProviderType::Ollama)
            .await;

        let session = manager.get_session(&id).await.unwrap();
        assert_eq!(session.status, SessionStatus::Active);
        assert_eq!(session.provider, ProviderType::Ollama);

        let completed = manager.complete_session(&id, 100, 50).await.unwrap();
        assert_eq!(completed.status, SessionStatus::Completed);
        assert_eq!(completed.prompt_tokens, 100);
        assert_eq!(completed.completion_tokens, 50);
        assert_eq!(completed.total_tokens(), 150);
        assert!(completed.latency_ms.is_some());
    }

    #[tokio::test]
    async fn test_session_failure() {
        let manager = InferenceSessionManager::new(100);

        let id = manager
            .start_session("test-model", ProviderType::OpenAi)
            .await;

        let failed = manager.fail_session(&id, "rate limited").await.unwrap();
        assert_eq!(failed.status, SessionStatus::Failed);
        assert_eq!(failed.error.as_deref(), Some("rate limited"));
    }

    #[tokio::test]
    async fn test_active_sessions() {
        let manager = InferenceSessionManager::new(100);

        let id1 = manager.start_session("model-a", ProviderType::Ollama).await;
        let _id2 = manager.start_session("model-b", ProviderType::OpenAi).await;

        assert_eq!(manager.active_sessions().await.len(), 2);

        manager.complete_session(&id1, 10, 5).await;
        assert_eq!(manager.active_sessions().await.len(), 1);
    }

    #[tokio::test]
    async fn test_usage_stats() {
        let manager = InferenceSessionManager::new(100);

        let id1 = manager.start_session("model-a", ProviderType::Ollama).await;
        let id2 = manager.start_session("model-b", ProviderType::OpenAi).await;
        let id3 = manager
            .start_session("model-c", ProviderType::Anthropic)
            .await;

        manager.complete_session(&id1, 100, 50).await;
        manager.complete_session(&id2, 200, 100).await;
        manager.fail_session(&id3, "error").await;

        let stats = manager.usage_stats().await;
        assert_eq!(stats.total_sessions, 3);
        assert_eq!(stats.completed_sessions, 2);
        assert_eq!(stats.failed_sessions, 1);
        assert_eq!(stats.active_sessions, 0);
        assert_eq!(stats.total_prompt_tokens, 300);
        assert_eq!(stats.total_completion_tokens, 150);
    }

    #[tokio::test]
    async fn test_prune() {
        let manager = InferenceSessionManager::new(2);

        // Create and complete 5 sessions
        for i in 0..5 {
            let id = manager
                .start_session(format!("model-{i}"), ProviderType::Onnx)
                .await;
            manager.complete_session(&id, 10, 5).await;
        }

        let before = {
            let sessions = manager.sessions.read().await;
            sessions.len()
        };
        assert_eq!(before, 5);

        manager.prune().await;

        let after = {
            let sessions = manager.sessions.read().await;
            sessions.len()
        };
        assert_eq!(after, 2);
    }

    #[tokio::test]
    async fn test_nonexistent_session_returns_none() {
        let manager = InferenceSessionManager::new(100);
        assert!(manager.get_session("nonexistent").await.is_none());
        assert!(manager
            .complete_session("nonexistent", 0, 0)
            .await
            .is_none());
        assert!(manager.fail_session("nonexistent", "err").await.is_none());
    }

    #[test]
    fn test_session_status_display() {
        assert_eq!(SessionStatus::Active.to_string(), "active");
        assert_eq!(SessionStatus::Failed.to_string(), "failed");
        assert_eq!(SessionStatus::TimedOut.to_string(), "timed_out");
    }
}
