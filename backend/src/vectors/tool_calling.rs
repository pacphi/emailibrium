//! Tool-calling provider abstraction (ADR-028).
//!
//! Extends the existing `GenerativeModel` trait with tool-calling capabilities.
//! Providers that support native tool calling (Claude, GPT-4o, Ollama) implement
//! [`ToolCallingProvider`]. The [`ChatOrchestrator`](super::chat_orchestrator::ChatOrchestrator)
//! uses this to run the agentic loop.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Mode
// ---------------------------------------------------------------------------

/// How a provider handles tool calling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCallingMode {
    /// Provider API natively supports a `tools` parameter (Claude, OpenAI, Ollama).
    NativeApi,
    /// Local model uses Hermes-style tags with grammar-constrained generation.
    HermesGrammar,
    /// No tool-calling support — use the RAG-only chat path.
    None,
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// Definition of a tool that can be called by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema describing the input parameters.
    pub input_schema: Value,
}

/// A tool call requested by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned ID for correlating results.
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Result of executing a tool call.
/// Wire type for the tool-calling protocol. Constructed by the orchestrator
/// when returning tool execution results to the LLM in a multi-turn loop.
/// Part of the public tool-calling API; direct construction begins in Phase 2
/// when the async orchestrator loop sends results back to the LLM.
#[allow(dead_code)] // Public wire type for Phase 2 orchestrator integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
}

// ---------------------------------------------------------------------------
// Completion
// ---------------------------------------------------------------------------

/// Response from a tool-calling LLM.
#[derive(Debug, Clone)]
pub enum ChatCompletion {
    /// LLM produced a text response (conversation turn complete).
    Text(String),
    /// LLM wants to call one or more tools before responding.
    ToolCalls(Vec<ToolCall>),
}

// ---------------------------------------------------------------------------
// Conversation messages
// ---------------------------------------------------------------------------

/// Message in the tool-calling conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMessage {
    pub role: ToolMessageRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Role of a [`ToolMessage`] participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolMessageRole {
    System,
    User,
    Assistant,
    Tool,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Trait for generative providers that support tool calling.
///
/// Implementations translate the canonical types above into the wire format
/// required by each provider (Anthropic, OpenAI, Ollama, etc.).
#[async_trait]
pub trait ToolCallingProvider: Send + Sync {
    /// Send messages with tool definitions, get back either text or tool calls.
    async fn chat_with_tools(
        &self,
        messages: &[ToolMessage],
        tools: &[ToolDefinition],
        temperature: f32,
        max_tokens: u32,
    ) -> Result<ChatCompletion, Box<dyn std::error::Error + Send + Sync>>;

    /// The tool-calling mode this provider uses.
    fn tool_calling_mode(&self) -> ToolCallingMode;
}
