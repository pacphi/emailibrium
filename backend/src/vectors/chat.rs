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
    /// Pre-formatted email context from the RAG pipeline (ADR-022).
    pub email_context: Option<String>,
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
            email_context: None,
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
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub message_count: usize,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub context_email_count: usize,
}

/// Response from a chat turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    pub session_id: String,
    /// The assistant's reply text (serialized as "message" to match the frontend contract).
    #[serde(rename = "message")]
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
    /// System prompt loaded from YAML config.
    system_prompt: String,
    /// Max response tokens from `tuning.yaml` (`llm.chat_max_tokens`).
    /// Per-model `tuning.max_tokens` from `models-llm.yaml` overrides this.
    chat_max_tokens: u32,
}

impl ChatService {
    /// Create a new chat service.
    ///
    /// * `session_ttl` -- sessions expire after this duration of inactivity.
    /// * `max_history` -- default sliding-window size (messages per session).
    /// * `generative` -- the generative model used for response generation.
    /// * `chat_max_tokens` -- global default from `tuning.yaml` (`llm.chat_max_tokens`).
    pub fn new(
        session_ttl: Duration,
        max_history: usize,
        generative: Arc<dyn GenerativeModel>,
        chat_max_tokens: u32,
    ) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            session_ttl,
            max_history,
            generative,
            system_prompt: email_assistant_system_prompt(),
            chat_max_tokens,
        }
    }

    /// Create a new chat service with a custom system prompt (from YAML config).
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = prompt;
        self
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
    ///
    /// `email_context` is a pre-formatted block of email snippets from the RAG
    /// pipeline (ADR-022).  When `Some`, it is injected into the prompt so the
    /// LLM can answer with real email data.
    pub async fn chat(
        &self,
        session_id: &str,
        user_message: &str,
        email_context: Option<String>,
    ) -> Result<ChatResponse, VectorError> {
        // Ensure session exists.
        {
            let mut sessions = self.sessions.lock().await;
            let session = sessions
                .entry(session_id.to_string())
                .or_insert_with(|| ChatSession::new(session_id.to_string(), self.max_history));

            // Update RAG context for this turn.
            session.email_context = email_context;

            // Record the user message.
            session.push_message(ChatRole::User, user_message.to_string());
        }

        // Build the prompt outside the lock.
        let prompt = {
            let sessions = self.sessions.lock().await;
            let session = sessions.get(session_id).expect("session was just created");
            self.build_prompt(session, user_message)
        };

        // Dump prompt for debugging RAG.
        if prompt.contains("[Email Context]") {
            tracing::info!(
                prompt_len = prompt.len(),
                "Chat prompt CONTAINS email context"
            );
            // Write full prompt to temp file for inspection.
            let _ = std::fs::write("/tmp/emailibrium_last_prompt.txt", &prompt);
        } else {
            tracing::warn!("Chat prompt has NO email context — RAG may not be injecting");
        }

        // Generate the response and strip <think>...</think> blocks (Qwen 3 CoT).
        let raw_reply = self.generate_response(&prompt).await?;
        let reply = strip_think_blocks(&raw_reply);

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
        parts.push(format!("[System]\n{}", self.system_prompt));

        // Email context from RAG pipeline (ADR-022).
        if let Some(ref ctx) = session.email_context {
            if !ctx.is_empty() {
                parts.push(format!("[Email Context]\n{ctx}"));
            }
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
    ///
    /// Token budget resolution order:
    /// 1. Per-model `tuning.max_tokens` from `models-llm.yaml` (via `configured_max_tokens()`)
    /// 2. Global `llm.chat_max_tokens` from `tuning.yaml`
    async fn generate_response(&self, prompt: &str) -> Result<String, VectorError> {
        let max_tokens = self
            .generative
            .configured_max_tokens()
            .unwrap_or(self.chat_max_tokens);
        self.generative.generate(prompt, max_tokens).await
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
                context_email_count: s.email_context.as_ref().map_or(0, |c| {
                    if c.is_empty() {
                        0
                    } else {
                        1
                    }
                }),
            })
            .collect()
    }
}

/// Build the system prompt for the email assistant persona.
fn email_assistant_system_prompt() -> String {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M %Z");
    format!(
        "You are an email assistant with full access to the user's inbox.\n\
         The current date and time is: {now}\n\
         The [Email Context] section contains REAL emails from the user's inbox that match their query.\n\
         IMPORTANT RULES:\n\
         1. If emails are shown in the context, answer YES and list them with sender, subject, and date.\n\
         2. NEVER say you don't have access. You DO have access — the emails are shown above.\n\
         3. If no emails are in the context, say no matching emails were found.\n\
         4. Be specific — quote subjects and senders from the provided emails.\n\
         5. Do NOT include internal reasoning or thinking in your response. Answer directly."
    )
}

/// Strip `<think>...</think>` blocks from model output (Qwen 3 chain-of-thought).
fn strip_think_blocks(text: &str) -> String {
    if let Some(end) = text.find("</think>") {
        text[end + 8..].trim().to_string()
    } else if text.starts_with("<think>") {
        // Thinking block never closed — return everything after first newline as fallback
        text.lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    } else {
        text.to_string()
    }
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
        ChatService::new(Duration::from_secs(3600), 20, Arc::new(MockChatModel), 512)
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
        let ctx = Some(
            "--- Email ---\nFrom: test@example.com\nSubject: Test\nDate: 2026-01-01".to_string(),
        );
        let resp = svc.chat("s1", "Tell me about these", ctx).await.unwrap();
        assert!(resp.reply.contains("Mock response"));

        let session = svc.get_or_create_session("s1").await;
        assert!(session.email_context.is_some());
    }

    #[tokio::test]
    async fn test_sliding_window() {
        let svc = ChatService::new(
            Duration::from_secs(3600),
            4, // very small window
            Arc::new(MockChatModel),
            512,
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
            512,
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
        assert!(prompt.contains("email assistant"));
        assert!(prompt.contains("Email Context"));
        assert!(prompt.contains("current date and time"));
        // Verify date is injected (contains year)
        assert!(prompt.contains("202"));
    }

    #[test]
    fn test_strip_think_blocks_with_closing_tag() {
        let input = "<think>\nLet me reason about this.\n</think>\nThe answer is 42.";
        assert_eq!(strip_think_blocks(input), "The answer is 42.");
    }

    #[test]
    fn test_strip_think_blocks_unclosed() {
        let input = "<think>\nSome reasoning\nMore reasoning";
        let result = strip_think_blocks(input);
        assert!(result.contains("Some reasoning"));
        assert!(!result.contains("<think>"));
    }

    #[test]
    fn test_strip_think_blocks_no_think() {
        let input = "Just a normal response.";
        assert_eq!(strip_think_blocks(input), "Just a normal response.");
    }
}
