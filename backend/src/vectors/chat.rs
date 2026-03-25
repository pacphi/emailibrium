//! Chat service with session management for AI-assisted email conversations (R-07).
//!
//! Manages multi-turn chat sessions with sliding-window history, email context
//! injection, and generation via the existing `GenerativeModel` trait. Sessions
//! expire after a configurable TTL of inactivity.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::debug;

use super::error::VectorError;
use super::generative::GenerativeModel;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// Role of the message sender.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
    System,
}

impl std::fmt::Display for ChatRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatRole::User => write!(f, "user"),
            ChatRole::Assistant => write!(f, "assistant"),
            ChatRole::System => write!(f, "system"),
        }
    }
}

/// A chat session holding message history and email context.
#[derive(Debug, Clone)]
pub struct ChatSession {
    pub id: String,
    pub messages: Vec<ChatMessage>,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    /// Email IDs whose content should be included as context.
    pub context_emails: Vec<String>,
    /// Maximum number of messages kept in the sliding window.
    pub max_history: usize,
}

impl ChatSession {
    fn new(id: String, max_history: usize) -> Self {
        let now = Utc::now();
        Self {
            id,
            messages: Vec::new(),
            created_at: now,
            last_active: now,
            context_emails: Vec::new(),
            max_history,
        }
    }

    /// Append a message, trimming the oldest when the window overflows.
    fn push_message(&mut self, role: ChatRole, content: String) {
        self.messages.push(ChatMessage {
            role,
            content,
            timestamp: Utc::now(),
        });
        self.last_active = Utc::now();

        // Sliding window: keep only the most recent `max_history` messages.
        if self.messages.len() > self.max_history {
            let excess = self.messages.len() - self.max_history;
            self.messages.drain(..excess);
        }
    }
}

/// Lightweight summary returned when listing sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub message_count: usize,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub context_email_count: usize,
}

/// Response from a chat turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub session_id: String,
    pub reply: String,
    pub message_count: usize,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Chat service managing sessions and LLM interaction.
pub struct ChatService {
    sessions: Arc<Mutex<HashMap<String, ChatSession>>>,
    session_ttl: Duration,
    max_history: usize,
    generative: Arc<dyn GenerativeModel>,
}

impl ChatService {
    /// Create a new chat service.
    ///
    /// * `session_ttl` -- sessions expire after this duration of inactivity.
    /// * `max_history` -- default sliding-window size (messages per session).
    /// * `generative` -- the generative model used for response generation.
    pub fn new(
        session_ttl: Duration,
        max_history: usize,
        generative: Arc<dyn GenerativeModel>,
    ) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            session_ttl,
            max_history,
            generative,
        }
    }

    /// Get an existing session or create a new one.
    pub async fn get_or_create_session(&self, session_id: &str) -> ChatSession {
        let mut sessions = self.sessions.lock().await;
        sessions
            .entry(session_id.to_string())
            .or_insert_with(|| ChatSession::new(session_id.to_string(), self.max_history))
            .clone()
    }

    /// Add a user message and generate an assistant response.
    pub async fn chat(
        &self,
        session_id: &str,
        user_message: &str,
        email_context: Option<Vec<String>>,
    ) -> Result<ChatResponse, VectorError> {
        // Ensure session exists.
        {
            let mut sessions = self.sessions.lock().await;
            let session = sessions
                .entry(session_id.to_string())
                .or_insert_with(|| ChatSession::new(session_id.to_string(), self.max_history));

            // Update context emails if provided.
            if let Some(ctx) = email_context {
                session.context_emails = ctx;
            }

            // Record the user message.
            session.push_message(ChatRole::User, user_message.to_string());
        }

        // Build the prompt outside the lock.
        let prompt = {
            let sessions = self.sessions.lock().await;
            let session = sessions.get(session_id).expect("session was just created");
            self.build_prompt(session, user_message)
        };

        // Generate the response.
        let reply = self.generate_response(&prompt).await?;

        // Record the assistant reply.
        let message_count = {
            let mut sessions = self.sessions.lock().await;
            let session = sessions.get_mut(session_id).expect("session exists");
            session.push_message(ChatRole::Assistant, reply.clone());
            session.messages.len()
        };

        debug!(
            session_id = session_id,
            message_count = message_count,
            "Chat turn completed"
        );

        Ok(ChatResponse {
            session_id: session_id.to_string(),
            reply,
            message_count,
        })
    }

    /// Build the full prompt with system instructions, email context, and history.
    fn build_prompt(&self, session: &ChatSession, _user_message: &str) -> String {
        let mut parts: Vec<String> = Vec::new();

        // System prompt.
        parts.push(format!("[System]\n{}", email_assistant_system_prompt()));

        // Email context (IDs only -- actual content would be fetched from DB in production).
        if !session.context_emails.is_empty() {
            let ids = session.context_emails.join(", ");
            parts.push(format!(
                "[Email Context]\nThe user is referencing these emails: {ids}"
            ));
        }

        // Conversation history.
        for msg in &session.messages {
            let role_label = match msg.role {
                ChatRole::User => "User",
                ChatRole::Assistant => "Assistant",
                ChatRole::System => "System",
            };
            parts.push(format!("[{role_label}]\n{}", msg.content));
        }

        parts.join("\n\n")
    }

    /// Generate a response using the configured generative model.
    async fn generate_response(&self, prompt: &str) -> Result<String, VectorError> {
        // 1024 tokens is a reasonable default for chat responses.
        self.generative.generate(prompt, 1024).await
    }

    /// Remove sessions that have been inactive longer than `session_ttl`.
    /// Returns the number of sessions cleaned up.
    pub async fn cleanup_expired(&self) -> usize {
        let mut sessions = self.sessions.lock().await;
        let now = Utc::now();
        let ttl_chrono = chrono::Duration::from_std(self.session_ttl)
            .unwrap_or_else(|_| chrono::Duration::hours(1));

        let expired: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| now - s.last_active > ttl_chrono)
            .map(|(id, _)| id.clone())
            .collect();

        let count = expired.len();
        for id in expired {
            sessions.remove(&id);
        }

        if count > 0 {
            debug!(count = count, "Cleaned up expired chat sessions");
        }

        count
    }

    /// Delete a specific session by ID.
    pub async fn delete_session(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(session_id).is_some()
    }

    /// List all active sessions as lightweight summaries.
    pub async fn list_sessions(&self) -> Vec<SessionSummary> {
        let sessions = self.sessions.lock().await;
        sessions
            .values()
            .map(|s| SessionSummary {
                id: s.id.clone(),
                message_count: s.messages.len(),
                created_at: s.created_at,
                last_active: s.last_active,
                context_email_count: s.context_emails.len(),
            })
            .collect()
    }
}

/// Build the system prompt for the email assistant persona.
fn email_assistant_system_prompt() -> String {
    "You are Emailibrium's AI email assistant. You help users understand, organize, \
     and manage their emails. You can answer questions about email content, suggest \
     actions (archive, label, delete), help draft replies, and provide insights about \
     email patterns. Be concise and helpful. Reference specific emails when relevant."
        .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    /// Mock generative model that echoes the prompt length.
    struct MockChatModel;

    #[async_trait]
    impl GenerativeModel for MockChatModel {
        async fn generate(&self, prompt: &str, _max_tokens: u32) -> Result<String, VectorError> {
            Ok(format!("Mock response (prompt length: {})", prompt.len()))
        }

        async fn classify(&self, _text: &str, categories: &[&str]) -> Result<String, VectorError> {
            Ok(categories.first().unwrap_or(&"Unknown").to_string())
        }

        fn model_name(&self) -> &str {
            "mock-chat"
        }

        async fn is_available(&self) -> bool {
            true
        }
    }

    fn make_service() -> ChatService {
        ChatService::new(Duration::from_secs(3600), 20, Arc::new(MockChatModel))
    }

    #[tokio::test]
    async fn test_get_or_create_session() {
        let svc = make_service();
        let s1 = svc.get_or_create_session("s1").await;
        assert_eq!(s1.id, "s1");
        assert!(s1.messages.is_empty());

        // Second call returns the same session.
        let s1b = svc.get_or_create_session("s1").await;
        assert_eq!(s1b.id, "s1");
    }

    #[tokio::test]
    async fn test_chat_basic() {
        let svc = make_service();
        let resp = svc.chat("s1", "Hello", None).await.unwrap();
        assert_eq!(resp.session_id, "s1");
        assert!(resp.reply.contains("Mock response"));
        assert_eq!(resp.message_count, 2); // user + assistant
    }

    #[tokio::test]
    async fn test_chat_with_email_context() {
        let svc = make_service();
        let ctx = Some(vec!["email-001".to_string(), "email-002".to_string()]);
        let resp = svc.chat("s1", "Tell me about these", ctx).await.unwrap();
        assert!(resp.reply.contains("Mock response"));

        let session = svc.get_or_create_session("s1").await;
        assert_eq!(session.context_emails.len(), 2);
    }

    #[tokio::test]
    async fn test_sliding_window() {
        let svc = ChatService::new(
            Duration::from_secs(3600),
            4, // very small window
            Arc::new(MockChatModel),
        );

        // Send 5 messages -> 10 total (user + assistant each), window = 4
        for i in 0..5 {
            svc.chat("s1", &format!("msg-{i}"), None).await.unwrap();
        }

        let session = svc.get_or_create_session("s1").await;
        assert_eq!(session.messages.len(), 4);
    }

    #[tokio::test]
    async fn test_delete_session() {
        let svc = make_service();
        svc.chat("s1", "Hi", None).await.unwrap();

        assert!(svc.delete_session("s1").await);
        assert!(!svc.delete_session("s1").await); // already gone
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let svc = make_service();
        svc.chat("s1", "Hi", None).await.unwrap();
        svc.chat("s2", "Hey", None).await.unwrap();

        let list = svc.list_sessions().await;
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_cleanup_expired() {
        let svc = ChatService::new(
            Duration::from_millis(1), // very short TTL
            20,
            Arc::new(MockChatModel),
        );
        svc.chat("s1", "Hi", None).await.unwrap();

        // Wait for TTL to elapse.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let cleaned = svc.cleanup_expired().await;
        assert_eq!(cleaned, 1);

        let list = svc.list_sessions().await;
        assert!(list.is_empty());
    }

    #[test]
    fn test_chat_role_display() {
        assert_eq!(ChatRole::User.to_string(), "user");
        assert_eq!(ChatRole::Assistant.to_string(), "assistant");
        assert_eq!(ChatRole::System.to_string(), "system");
    }

    #[test]
    fn test_chat_role_serde_roundtrip() {
        let json = serde_json::to_string(&ChatRole::User).unwrap();
        assert_eq!(json, "\"user\"");
        let parsed: ChatRole = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ChatRole::User);
    }

    #[test]
    fn test_chat_message_serde() {
        let msg = ChatMessage {
            role: ChatRole::Assistant,
            content: "Hello!".to_string(),
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, ChatRole::Assistant);
        assert_eq!(parsed.content, "Hello!");
    }

    #[test]
    fn test_session_summary_serde() {
        let summary = SessionSummary {
            id: "s1".to_string(),
            message_count: 5,
            created_at: Utc::now(),
            last_active: Utc::now(),
            context_email_count: 2,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"id\":\"s1\""));
    }

    #[test]
    fn test_email_assistant_system_prompt() {
        let prompt = email_assistant_system_prompt();
        assert!(prompt.contains("Emailibrium"));
        assert!(prompt.contains("email assistant"));
    }
}
