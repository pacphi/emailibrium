//! Chat orchestrator with tool-calling loop (ADR-028).
//!
//! Manages multi-turn tool-calling conversations. When the LLM requests
//! tool calls, the orchestrator executes them and feeds results back
//! until the LLM produces a text response or the iteration limit is hit.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::tool_calling::*;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    /// Maximum tool-call iterations before forcing a text response.
    #[serde(default = "default_max_iterations")]
    pub max_tool_iterations: u32,
    /// Per-tool execution timeout in milliseconds.
    #[serde(default = "default_tool_timeout")]
    pub tool_timeout_ms: u64,
    /// Whether to allow parallel tool calls.
    #[serde(default = "default_parallel")]
    pub parallel_tool_calls: bool,
    /// Maximum characters in tool results before truncation.
    #[serde(default = "default_result_max")]
    pub tool_result_max_chars: usize,
    /// Tools that require user confirmation before execution.
    #[serde(default)]
    pub require_confirmation: Vec<String>,
}

fn default_max_iterations() -> u32 {
    5
}
fn default_tool_timeout() -> u64 {
    10_000
}
fn default_parallel() -> bool {
    true
}
fn default_result_max() -> usize {
    4_000
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_tool_iterations: default_max_iterations(),
            tool_timeout_ms: default_tool_timeout(),
            parallel_tool_calls: default_parallel(),
            tool_result_max_chars: default_result_max(),
            require_confirmation: vec![
                "send_email".into(),
                "delete_email".into(),
                "create_rule".into(),
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Result
// ---------------------------------------------------------------------------

/// Outcome of an orchestration run.
#[derive(Debug)]
pub enum OrchestrationResult {
    /// LLM produced a final text response.
    Response(String),
    /// A tool call requires user confirmation before proceeding.
    ConfirmationRequired {
        confirmation_id: String,
        tool_name: String,
        tool_args: serde_json::Value,
        description: String,
    },
    /// Max iterations reached without a text response.
    MaxIterationsReached(String),
}

// ---------------------------------------------------------------------------
// Tool executor
// ---------------------------------------------------------------------------

/// Callback type for executing a tool by name with JSON arguments.
pub type ToolExecutor = Arc<
    dyn Fn(&str, serde_json::Value) -> futures::future::BoxFuture<'static, Result<String, String>>
        + Send
        + Sync,
>;

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// The chat orchestrator manages the tool-calling loop.
///
/// Given a set of tool definitions and an executor, it drives the
/// conversation with a [`ToolCallingProvider`] until the model emits a
/// text response, a confirmation-required tool is encountered, or the
/// iteration budget is exhausted.
pub struct ChatOrchestrator {
    config: OrchestratorConfig,
    tool_definitions: Vec<ToolDefinition>,
    tool_executor: Option<ToolExecutor>,
}

impl ChatOrchestrator {
    pub fn new(config: OrchestratorConfig) -> Self {
        Self {
            config,
            tool_definitions: Vec::new(),
            tool_executor: None,
        }
    }

    /// Attach tool definitions the LLM may invoke.
    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tool_definitions = tools;
        self
    }

    /// Attach the executor that runs tool calls.
    pub fn with_executor(mut self, executor: ToolExecutor) -> Self {
        self.tool_executor = Some(executor);
        self
    }

    /// Run the orchestration loop.
    ///
    /// Takes the conversation messages and a tool-calling provider,
    /// executes the loop until the LLM produces text or the iteration
    /// budget is spent.
    pub async fn orchestrate(
        &self,
        messages: Vec<ToolMessage>,
        provider: &dyn ToolCallingProvider,
        temperature: f32,
        max_tokens: u32,
    ) -> Result<OrchestrationResult, Box<dyn std::error::Error + Send + Sync>> {
        let mut conversation = messages;

        for _iteration in 0..self.config.max_tool_iterations {
            let completion = provider
                .chat_with_tools(
                    &conversation,
                    &self.tool_definitions,
                    temperature,
                    max_tokens,
                )
                .await?;

            match completion {
                ChatCompletion::Text(text) => {
                    return Ok(OrchestrationResult::Response(text));
                }
                ChatCompletion::ToolCalls(tool_calls) => {
                    // Check for confirmation-required tools first.
                    for tc in &tool_calls {
                        if self.config.require_confirmation.contains(&tc.name) {
                            return Ok(OrchestrationResult::ConfirmationRequired {
                                confirmation_id: tc.id.clone(),
                                tool_name: tc.name.clone(),
                                tool_args: tc.arguments.clone(),
                                description: format!(
                                    "Execute {} with args: {}",
                                    tc.name, tc.arguments
                                ),
                            });
                        }
                    }

                    // Record the assistant turn that requested tool calls.
                    conversation.push(ToolMessage {
                        role: ToolMessageRole::Assistant,
                        content: String::new(),
                        tool_calls: Some(tool_calls.clone()),
                        tool_call_id: None,
                    });

                    // Execute each tool call and append results.
                    for tc in tool_calls {
                        let result = if let Some(ref executor) = self.tool_executor {
                            match executor(&tc.name, tc.arguments.clone()).await {
                                Ok(output) => self.truncate_result(&output),
                                Err(e) => format!("Error: {e}"),
                            }
                        } else {
                            "Tool execution not configured".to_string()
                        };

                        conversation.push(ToolMessage {
                            role: ToolMessageRole::Tool,
                            content: result,
                            tool_calls: None,
                            tool_call_id: Some(tc.id),
                        });
                    }
                }
            }
        }

        Ok(OrchestrationResult::MaxIterationsReached(
            "Maximum tool call iterations reached. Please try a simpler request.".to_string(),
        ))
    }

    /// Truncate a tool result string to the configured maximum length.
    fn truncate_result(&self, result: &str) -> String {
        if result.len() > self.config.tool_result_max_chars {
            format!(
                "{}...[truncated]",
                &result[..self.config.tool_result_max_chars]
            )
        } else {
            result.to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let cfg = OrchestratorConfig::default();
        assert_eq!(cfg.max_tool_iterations, 5);
        assert_eq!(cfg.tool_timeout_ms, 10_000);
        assert!(cfg.parallel_tool_calls);
        assert_eq!(cfg.tool_result_max_chars, 4_000);
        assert_eq!(
            cfg.require_confirmation,
            vec!["send_email", "delete_email", "create_rule"]
        );
    }

    #[test]
    fn truncate_result_short_string() {
        let orchestrator = ChatOrchestrator::new(OrchestratorConfig {
            tool_result_max_chars: 100,
            ..Default::default()
        });
        let input = "hello world";
        assert_eq!(orchestrator.truncate_result(input), "hello world");
    }

    #[test]
    fn truncate_result_long_string() {
        let orchestrator = ChatOrchestrator::new(OrchestratorConfig {
            tool_result_max_chars: 10,
            ..Default::default()
        });
        let input = "abcdefghijklmnopqrstuvwxyz";
        let result = orchestrator.truncate_result(input);
        assert_eq!(result, "abcdefghij...[truncated]");
        assert!(result.starts_with("abcdefghij"));
        assert!(result.ends_with("...[truncated]"));
    }

    #[test]
    fn truncate_result_exact_boundary() {
        let orchestrator = ChatOrchestrator::new(OrchestratorConfig {
            tool_result_max_chars: 5,
            ..Default::default()
        });
        // Exactly at limit — no truncation.
        assert_eq!(orchestrator.truncate_result("abcde"), "abcde");
        // One over — truncated.
        assert_eq!(
            orchestrator.truncate_result("abcdef"),
            "abcde...[truncated]"
        );
    }

    #[test]
    fn confirmation_required_tools_detected() {
        let cfg = OrchestratorConfig::default();
        assert!(cfg.require_confirmation.contains(&"send_email".to_string()));
        assert!(cfg
            .require_confirmation
            .contains(&"delete_email".to_string()));
        assert!(cfg
            .require_confirmation
            .contains(&"create_rule".to_string()));
        // Arbitrary tool should not require confirmation.
        assert!(!cfg
            .require_confirmation
            .contains(&"search_emails".to_string()));
    }

    #[test]
    fn config_serde_round_trip() {
        let cfg = OrchestratorConfig::default();
        let json = serde_json::to_string(&cfg).expect("serialize");
        let deserialized: OrchestratorConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.max_tool_iterations, cfg.max_tool_iterations);
        assert_eq!(deserialized.tool_timeout_ms, cfg.tool_timeout_ms);
        assert_eq!(deserialized.parallel_tool_calls, cfg.parallel_tool_calls);
        assert_eq!(
            deserialized.tool_result_max_chars,
            cfg.tool_result_max_chars
        );
        assert_eq!(deserialized.require_confirmation, cfg.require_confirmation);
    }
}
