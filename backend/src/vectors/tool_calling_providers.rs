//! Concrete [`ToolCallingProvider`] implementations for Anthropic, OpenAI, and Ollama.
//!
//! Each provider translates the canonical [`ToolMessage`] / [`ToolDefinition`] types
//! into the wire format required by its API and parses the response back into
//! [`ChatCompletion`].

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::debug;

use super::tool_calling::{
    ChatCompletion, ToolCall, ToolCallingMode, ToolCallingProvider, ToolDefinition, ToolMessage,
    ToolMessageRole,
};

// ---------------------------------------------------------------------------
// Anthropic Claude
// ---------------------------------------------------------------------------

/// Tool-calling provider backed by the Anthropic Messages API.
pub struct AnthropicToolCallingProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl AnthropicToolCallingProvider {
    pub fn new(api_key: String, model: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url,
        }
    }

    /// Convert canonical tool definitions to Anthropic format.
    fn format_tools(tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect()
    }

    /// Convert canonical messages to Anthropic message format.
    ///
    /// Returns `(system_prompt, messages)` because Anthropic requires the system
    /// prompt as a top-level field, not inside the messages array.
    fn format_messages(messages: &[ToolMessage]) -> (Option<String>, Vec<Value>) {
        let mut system: Option<String> = None;
        let mut out: Vec<Value> = Vec::new();

        for msg in messages {
            match msg.role {
                ToolMessageRole::System => {
                    system = Some(msg.content.clone());
                }
                ToolMessageRole::User => {
                    out.push(json!({"role": "user", "content": msg.content}));
                }
                ToolMessageRole::Assistant => {
                    // If the assistant message contains tool calls, represent them
                    // as Anthropic content blocks.
                    if let Some(ref calls) = msg.tool_calls {
                        let mut blocks: Vec<Value> = Vec::new();
                        if !msg.content.is_empty() {
                            blocks.push(json!({"type": "text", "text": msg.content}));
                        }
                        for call in calls {
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": call.id,
                                "name": call.name,
                                "input": call.arguments,
                            }));
                        }
                        out.push(json!({"role": "assistant", "content": blocks}));
                    } else {
                        out.push(json!({"role": "assistant", "content": msg.content}));
                    }
                }
                ToolMessageRole::Tool => {
                    // Anthropic represents tool results as user messages with
                    // a `tool_result` content block.
                    let tool_call_id = msg.tool_call_id.clone().unwrap_or_default();
                    out.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": msg.content,
                        }],
                    }));
                }
            }
        }

        (system, out)
    }

    /// Parse an Anthropic Messages API response into a [`ChatCompletion`].
    fn parse_response(
        body: &Value,
    ) -> Result<ChatCompletion, Box<dyn std::error::Error + Send + Sync>> {
        let stop_reason = body["stop_reason"].as_str().unwrap_or("");

        let content = body["content"]
            .as_array()
            .ok_or("Anthropic response missing content array")?;

        if stop_reason == "tool_use" {
            let calls: Vec<ToolCall> = content
                .iter()
                .filter(|b| b["type"].as_str() == Some("tool_use"))
                .map(|b| ToolCall {
                    id: b["id"].as_str().unwrap_or("").to_string(),
                    name: b["name"].as_str().unwrap_or("").to_string(),
                    arguments: b["input"].clone(),
                })
                .collect();
            Ok(ChatCompletion::ToolCalls(calls))
        } else {
            // Collect all text blocks.
            let text: String = content
                .iter()
                .filter(|b| b["type"].as_str() == Some("text"))
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            Ok(ChatCompletion::Text(text))
        }
    }
}

#[async_trait]
impl ToolCallingProvider for AnthropicToolCallingProvider {
    async fn chat_with_tools(
        &self,
        messages: &[ToolMessage],
        tools: &[ToolDefinition],
        temperature: f32,
        max_tokens: u32,
    ) -> Result<ChatCompletion, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/v1/messages", self.base_url);
        let (system, msgs) = Self::format_messages(messages);
        let formatted_tools = Self::format_tools(tools);

        let mut body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": msgs,
            "tools": formatted_tools,
            "temperature": temperature,
        });

        if let Some(sys) = system {
            body["system"] = json!(sys);
        }

        debug!(model = %self.model, provider = "anthropic", "Tool-calling request");

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                format!("Anthropic request failed: {e}").into()
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Anthropic returned {status}: {text}").into());
        }

        let parsed: Value =
            resp.json()
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    format!("Anthropic parse error: {e}").into()
                })?;

        Self::parse_response(&parsed)
    }

    fn tool_calling_mode(&self) -> ToolCallingMode {
        ToolCallingMode::NativeApi
    }
}

// ---------------------------------------------------------------------------
// OpenAI
// ---------------------------------------------------------------------------

/// Tool-calling provider backed by the OpenAI Chat Completions API.
pub struct OpenAiToolCallingProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAiToolCallingProvider {
    pub fn new(api_key: String, model: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url,
        }
    }

    /// Convert canonical tool definitions to OpenAI function-calling format.
    fn format_tools(tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    },
                })
            })
            .collect()
    }

    /// Convert canonical messages to OpenAI message format.
    fn format_messages(messages: &[ToolMessage]) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();

        for msg in messages {
            match msg.role {
                ToolMessageRole::System => {
                    out.push(json!({"role": "system", "content": msg.content}));
                }
                ToolMessageRole::User => {
                    out.push(json!({"role": "user", "content": msg.content}));
                }
                ToolMessageRole::Assistant => {
                    if let Some(ref calls) = msg.tool_calls {
                        let tc: Vec<Value> = calls
                            .iter()
                            .map(|c| {
                                json!({
                                    "id": c.id,
                                    "type": "function",
                                    "function": {
                                        "name": c.name,
                                        "arguments": c.arguments.to_string(),
                                    },
                                })
                            })
                            .collect();
                        let mut m = json!({"role": "assistant"});
                        if !msg.content.is_empty() {
                            m["content"] = json!(msg.content);
                        }
                        m["tool_calls"] = json!(tc);
                        out.push(m);
                    } else {
                        out.push(json!({"role": "assistant", "content": msg.content}));
                    }
                }
                ToolMessageRole::Tool => {
                    out.push(json!({
                        "role": "tool",
                        "tool_call_id": msg.tool_call_id.clone().unwrap_or_default(),
                        "content": msg.content,
                    }));
                }
            }
        }

        out
    }

    /// Parse an OpenAI Chat Completions response into a [`ChatCompletion`].
    fn parse_response(
        body: &Value,
    ) -> Result<ChatCompletion, Box<dyn std::error::Error + Send + Sync>> {
        let choice = &body["choices"][0]["message"];

        if let Some(tool_calls) = choice["tool_calls"].as_array() {
            if !tool_calls.is_empty() {
                let calls: Vec<ToolCall> = tool_calls
                    .iter()
                    .map(|tc| {
                        let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                        let arguments: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                        ToolCall {
                            id: tc["id"].as_str().unwrap_or("").to_string(),
                            name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                            arguments,
                        }
                    })
                    .collect();
                return Ok(ChatCompletion::ToolCalls(calls));
            }
        }

        let text = choice["content"].as_str().unwrap_or("").to_string();
        Ok(ChatCompletion::Text(text))
    }
}

#[async_trait]
impl ToolCallingProvider for OpenAiToolCallingProvider {
    async fn chat_with_tools(
        &self,
        messages: &[ToolMessage],
        tools: &[ToolDefinition],
        temperature: f32,
        max_tokens: u32,
    ) -> Result<ChatCompletion, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let msgs = Self::format_messages(messages);
        let formatted_tools = Self::format_tools(tools);

        let body = json!({
            "model": self.model,
            "messages": msgs,
            "tools": formatted_tools,
            "temperature": temperature,
            "max_tokens": max_tokens,
        });

        debug!(model = %self.model, provider = "openai", "Tool-calling request");

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                format!("OpenAI request failed: {e}").into()
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("OpenAI returned {status}: {text}").into());
        }

        let parsed: Value =
            resp.json()
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    format!("OpenAI parse error: {e}").into()
                })?;

        Self::parse_response(&parsed)
    }

    fn tool_calling_mode(&self) -> ToolCallingMode {
        ToolCallingMode::NativeApi
    }
}

// ---------------------------------------------------------------------------
// Ollama
// ---------------------------------------------------------------------------

/// Tool-calling provider backed by Ollama's `/api/chat` endpoint.
pub struct OllamaToolCallingProvider {
    client: reqwest::Client,
    model: String,
    base_url: String,
}

impl OllamaToolCallingProvider {
    pub fn new(model: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            model,
            base_url,
        }
    }

    /// Convert canonical tool definitions to Ollama format (OpenAI-compatible).
    fn format_tools(tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    },
                })
            })
            .collect()
    }

    /// Convert canonical messages to Ollama chat format.
    fn format_messages(messages: &[ToolMessage]) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();

        for msg in messages {
            match msg.role {
                ToolMessageRole::System => {
                    out.push(json!({"role": "system", "content": msg.content}));
                }
                ToolMessageRole::User => {
                    out.push(json!({"role": "user", "content": msg.content}));
                }
                ToolMessageRole::Assistant => {
                    if let Some(ref calls) = msg.tool_calls {
                        let tc: Vec<Value> = calls
                            .iter()
                            .map(|c| {
                                json!({
                                    "function": {
                                        "name": c.name,
                                        "arguments": c.arguments,
                                    },
                                })
                            })
                            .collect();
                        let mut m = json!({"role": "assistant"});
                        if !msg.content.is_empty() {
                            m["content"] = json!(msg.content);
                        }
                        m["tool_calls"] = json!(tc);
                        out.push(m);
                    } else {
                        out.push(json!({"role": "assistant", "content": msg.content}));
                    }
                }
                ToolMessageRole::Tool => {
                    out.push(json!({
                        "role": "tool",
                        "content": msg.content,
                    }));
                }
            }
        }

        out
    }

    /// Parse an Ollama chat response into a [`ChatCompletion`].
    fn parse_response(
        body: &Value,
    ) -> Result<ChatCompletion, Box<dyn std::error::Error + Send + Sync>> {
        let message = &body["message"];

        if let Some(tool_calls) = message["tool_calls"].as_array() {
            if !tool_calls.is_empty() {
                let calls: Vec<ToolCall> = tool_calls
                    .iter()
                    .enumerate()
                    .map(|(i, tc)| {
                        let arguments = tc["function"]["arguments"].clone();
                        ToolCall {
                            id: format!("ollama_{i}"),
                            name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                            arguments,
                        }
                    })
                    .collect();
                return Ok(ChatCompletion::ToolCalls(calls));
            }
        }

        let text = message["content"].as_str().unwrap_or("").to_string();
        Ok(ChatCompletion::Text(text))
    }
}

#[async_trait]
impl ToolCallingProvider for OllamaToolCallingProvider {
    async fn chat_with_tools(
        &self,
        messages: &[ToolMessage],
        tools: &[ToolDefinition],
        temperature: f32,
        max_tokens: u32,
    ) -> Result<ChatCompletion, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/chat", self.base_url);
        let msgs = Self::format_messages(messages);
        let formatted_tools = Self::format_tools(tools);

        let body = json!({
            "model": self.model,
            "messages": msgs,
            "tools": formatted_tools,
            "stream": false,
            "options": {
                "temperature": temperature,
                "num_predict": max_tokens,
            },
        });

        debug!(model = %self.model, provider = "ollama", "Tool-calling request");

        let resp = self.client.post(&url).json(&body).send().await.map_err(
            |e| -> Box<dyn std::error::Error + Send + Sync> {
                format!("Ollama request failed: {e}").into()
            },
        )?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Ollama returned {status}: {text}").into());
        }

        let parsed: Value =
            resp.json()
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    format!("Ollama parse error: {e}").into()
                })?;

        Self::parse_response(&parsed)
    }

    fn tool_calling_mode(&self) -> ToolCallingMode {
        ToolCallingMode::NativeApi
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/// Create the appropriate [`ToolCallingProvider`] based on the configured provider.
///
/// - `"cloud"` inspects `config.generative.cloud.provider` to choose Anthropic or OpenAI.
/// - `"ollama"` creates an [`OllamaToolCallingProvider`].
/// - Everything else returns `None`.
pub fn create_tool_calling_provider(
    provider_name: &str,
    config: &crate::vectors::config::VectorConfig,
) -> Option<Arc<dyn ToolCallingProvider>> {
    match provider_name {
        "cloud" => {
            let cloud = &config.generative.cloud;
            let api_key = std::env::var(&cloud.api_key_env).ok()?;

            match cloud.provider.as_str() {
                "anthropic" => Some(Arc::new(AnthropicToolCallingProvider::new(
                    api_key,
                    cloud.model.clone(),
                    cloud.base_url.clone(),
                ))),
                "openai" => Some(Arc::new(OpenAiToolCallingProvider::new(
                    api_key,
                    cloud.model.clone(),
                    cloud.base_url.clone(),
                ))),
                _ => None,
            }
        }
        "ollama" => {
            let ollama = &config.generative.ollama;
            Some(Arc::new(OllamaToolCallingProvider::new(
                ollama.chat_model.clone(),
                ollama.base_url.clone(),
            )))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectors::tool_calling::{ToolDefinition, ToolMessage, ToolMessageRole};

    fn sample_tools() -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "search_emails".into(),
            description: "Search emails by query".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        }]
    }

    fn sample_messages() -> Vec<ToolMessage> {
        vec![
            ToolMessage {
                role: ToolMessageRole::System,
                content: "You are a helpful assistant.".into(),
                tool_calls: None,
                tool_call_id: None,
            },
            ToolMessage {
                role: ToolMessageRole::User,
                content: "Find my invoices".into(),
                tool_calls: None,
                tool_call_id: None,
            },
        ]
    }

    fn assistant_with_tool_calls() -> ToolMessage {
        ToolMessage {
            role: ToolMessageRole::Assistant,
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "call_123".into(),
                name: "search_emails".into(),
                arguments: json!({"query": "invoices"}),
            }]),
            tool_call_id: None,
        }
    }

    fn tool_result_message() -> ToolMessage {
        ToolMessage {
            role: ToolMessageRole::Tool,
            content: "Found 3 invoices".into(),
            tool_calls: None,
            tool_call_id: Some("call_123".into()),
        }
    }

    // -- Anthropic formatting tests --

    #[test]
    fn anthropic_format_tools() {
        let tools = sample_tools();
        let formatted = AnthropicToolCallingProvider::format_tools(&tools);
        assert_eq!(formatted.len(), 1);
        assert_eq!(formatted[0]["name"], "search_emails");
        assert!(formatted[0]["input_schema"].is_object());
    }

    #[test]
    fn anthropic_format_messages_extracts_system() {
        let msgs = sample_messages();
        let (system, formatted) = AnthropicToolCallingProvider::format_messages(&msgs);
        assert_eq!(system.unwrap(), "You are a helpful assistant.");
        assert_eq!(formatted.len(), 1); // only user message
        assert_eq!(formatted[0]["role"], "user");
    }

    #[test]
    fn anthropic_format_messages_with_tool_calls() {
        let msgs = vec![assistant_with_tool_calls(), tool_result_message()];
        let (_, formatted) = AnthropicToolCallingProvider::format_messages(&msgs);
        assert_eq!(formatted.len(), 2);
        // Assistant has tool_use content blocks
        let assistant_content = formatted[0]["content"].as_array().unwrap();
        assert_eq!(assistant_content[0]["type"], "tool_use");
        assert_eq!(assistant_content[0]["name"], "search_emails");
        // Tool result is a user message with tool_result block
        assert_eq!(formatted[1]["role"], "user");
        let result_content = formatted[1]["content"].as_array().unwrap();
        assert_eq!(result_content[0]["type"], "tool_result");
        assert_eq!(result_content[0]["tool_use_id"], "call_123");
    }

    #[test]
    fn anthropic_parse_text_response() {
        let body = json!({
            "content": [{"type": "text", "text": "Here are your invoices."}],
            "stop_reason": "end_turn",
        });
        let result = AnthropicToolCallingProvider::parse_response(&body).unwrap();
        match result {
            ChatCompletion::Text(t) => assert_eq!(t, "Here are your invoices."),
            _ => panic!("Expected Text completion"),
        }
    }

    #[test]
    fn anthropic_parse_tool_use_response() {
        let body = json!({
            "content": [
                {"type": "tool_use", "id": "toolu_abc", "name": "search_emails", "input": {"query": "invoices"}}
            ],
            "stop_reason": "tool_use",
        });
        let result = AnthropicToolCallingProvider::parse_response(&body).unwrap();
        match result {
            ChatCompletion::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id, "toolu_abc");
                assert_eq!(calls[0].name, "search_emails");
                assert_eq!(calls[0].arguments["query"], "invoices");
            }
            _ => panic!("Expected ToolCalls completion"),
        }
    }

    // -- OpenAI formatting tests --

    #[test]
    fn openai_format_tools() {
        let tools = sample_tools();
        let formatted = OpenAiToolCallingProvider::format_tools(&tools);
        assert_eq!(formatted.len(), 1);
        assert_eq!(formatted[0]["type"], "function");
        assert_eq!(formatted[0]["function"]["name"], "search_emails");
    }

    #[test]
    fn openai_format_messages_includes_system() {
        let msgs = sample_messages();
        let formatted = OpenAiToolCallingProvider::format_messages(&msgs);
        assert_eq!(formatted.len(), 2);
        assert_eq!(formatted[0]["role"], "system");
        assert_eq!(formatted[1]["role"], "user");
    }

    #[test]
    fn openai_format_messages_with_tool_calls() {
        let msgs = vec![assistant_with_tool_calls(), tool_result_message()];
        let formatted = OpenAiToolCallingProvider::format_messages(&msgs);
        assert_eq!(formatted.len(), 2);
        let tc = formatted[0]["tool_calls"].as_array().unwrap();
        assert_eq!(tc[0]["function"]["name"], "search_emails");
        assert_eq!(formatted[1]["role"], "tool");
        assert_eq!(formatted[1]["tool_call_id"], "call_123");
    }

    #[test]
    fn openai_parse_text_response() {
        let body = json!({
            "choices": [{"message": {"content": "Here are your invoices."}}],
        });
        let result = OpenAiToolCallingProvider::parse_response(&body).unwrap();
        match result {
            ChatCompletion::Text(t) => assert_eq!(t, "Here are your invoices."),
            _ => panic!("Expected Text completion"),
        }
    }

    #[test]
    fn openai_parse_tool_calls_response() {
        let body = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_xyz",
                        "type": "function",
                        "function": {
                            "name": "search_emails",
                            "arguments": "{\"query\":\"invoices\"}"
                        }
                    }]
                }
            }],
        });
        let result = OpenAiToolCallingProvider::parse_response(&body).unwrap();
        match result {
            ChatCompletion::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id, "call_xyz");
                assert_eq!(calls[0].name, "search_emails");
                assert_eq!(calls[0].arguments["query"], "invoices");
            }
            _ => panic!("Expected ToolCalls completion"),
        }
    }

    // -- Ollama formatting tests --

    #[test]
    fn ollama_format_tools() {
        let tools = sample_tools();
        let formatted = OllamaToolCallingProvider::format_tools(&tools);
        assert_eq!(formatted.len(), 1);
        assert_eq!(formatted[0]["type"], "function");
        assert_eq!(formatted[0]["function"]["name"], "search_emails");
    }

    #[test]
    fn ollama_format_messages() {
        let msgs = sample_messages();
        let formatted = OllamaToolCallingProvider::format_messages(&msgs);
        assert_eq!(formatted.len(), 2);
        assert_eq!(formatted[0]["role"], "system");
        assert_eq!(formatted[1]["role"], "user");
    }

    #[test]
    fn ollama_parse_text_response() {
        let body = json!({
            "message": {"role": "assistant", "content": "Here are your invoices."},
        });
        let result = OllamaToolCallingProvider::parse_response(&body).unwrap();
        match result {
            ChatCompletion::Text(t) => assert_eq!(t, "Here are your invoices."),
            _ => panic!("Expected Text completion"),
        }
    }

    #[test]
    fn ollama_parse_tool_calls_response() {
        let body = json!({
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "function": {
                        "name": "search_emails",
                        "arguments": {"query": "invoices"}
                    }
                }]
            },
        });
        let result = OllamaToolCallingProvider::parse_response(&body).unwrap();
        match result {
            ChatCompletion::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id, "ollama_0");
                assert_eq!(calls[0].name, "search_emails");
                assert_eq!(calls[0].arguments["query"], "invoices");
            }
            _ => panic!("Expected ToolCalls completion"),
        }
    }

    // -- Mode tests --

    #[test]
    fn all_providers_report_native_api_mode() {
        let a = AnthropicToolCallingProvider::new(String::new(), String::new(), String::new());
        let o = OpenAiToolCallingProvider::new(String::new(), String::new(), String::new());
        let l = OllamaToolCallingProvider::new(String::new(), String::new());
        assert_eq!(a.tool_calling_mode(), ToolCallingMode::NativeApi);
        assert_eq!(o.tool_calling_mode(), ToolCallingMode::NativeApi);
        assert_eq!(l.tool_calling_mode(), ToolCallingMode::NativeApi);
    }
}
