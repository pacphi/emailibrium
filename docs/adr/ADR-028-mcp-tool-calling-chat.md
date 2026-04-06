# ADR-028: MCP-Powered Tool-Calling Chat

- **Status:** Proposed
- **Date:** 2026-04-04
- **Deciders:** Chris Phillipson
- **Context:** The chat is currently prompt-only with RAG context injection. Users cannot perform actions (send email, create rules, search, manage accounts) via natural language. This ADR proposes integrating the Model Context Protocol (MCP) to expose all REST API and UI capabilities as tool calls, making the chat a universal natural-language interface to every emailibrium feature.

---

## 1. Problem Statement

The current chat implementation (`backend/src/vectors/chat.rs`) is a **read-only RAG assistant**:

- It can answer questions about emails using retrieved context
- It cannot perform actions (send, delete, classify, create rules, manage accounts)
- It uses a plain `generate(prompt, max_tokens)` interface with no tool-calling support
- The `GenerativeModel` trait has no concept of structured tool calls or function calling
- Models without native tool-calling (e.g., Qwen 3 1.7B via llama-cpp) cannot participate in agentic workflows

**Goal:** Transform the chat into a **full-capability agent** that can do anything the REST API or UI can do, via natural language instructions, while maintaining the existing tiered provider architecture and externalized configuration patterns.

---

## 2. Decision

Integrate the **rmcp** crate (modelcontextprotocol/rust-sdk v1.3+) to build an MCP server embedded in the existing Axum backend. Use **rmcp-openapi** to auto-generate tool definitions from our API, then curate them into task-oriented tools. Refactor the chat service to use a tool-calling loop when backed by capable models, with graceful degradation for models that lack tool-calling support.

---

## 3. Architecture Overview

```text
                    Frontend (React)
                         │
                    POST /api/v1/ai/chat/stream (SSE)
                         │
                    ┌────▼────────────────────────────┐
                    │       Chat Orchestrator          │
                    │  (new: tool-calling loop)        │
                    │                                  │
                    │  1. Build messages + tool defs   │
                    │  2. Call LLM (tool-capable API)  │
                    │  3. If tool_call → execute via   │
                    │     MCP client → MCP server      │
                    │  4. Append result, loop to #2    │
                    │  5. If text → stream to user     │
                    └────┬───────────────┬─────────────┘
                         │               │
              ┌──────────▼──┐    ┌───────▼──────────┐
              │  LLM Provider│    │  MCP Server      │
              │  (tool-call  │    │  (embedded Axum) │
              │   capable)   │    │                  │
              │  - Claude    │    │  Tools:          │
              │  - GPT-4o    │    │  - search_emails │
              │  - Ollama*   │    │  - send_email    │
              └──────────────┘    │  - create_rule   │
                                  │  - list_accounts │
                                  │  - classify_email│
                                  │  - ... (curated) │
                                  └──────────────────┘
```

### Transport

Use **Streamable HTTP** transport via `rmcp`'s built-in Axum integration. The MCP server mounts as a nested Axum service at `/api/v1/mcp`, sharing the same `AppState`. No separate process or port.

### Tri-Mode Chat

| Model Capability       | Chat Behavior                                                                        | Examples                                     |
| ---------------------- | ------------------------------------------------------------------------------------ | -------------------------------------------- |
| **Cloud tool-calling** | Full agentic loop via native API `tools` parameter                                   | Claude, GPT-4o                               |
| **Local tool-calling** | Agentic loop via Hermes-style `<tool_call>` tags with grammar-constrained generation | Qwen3 4B+, Llama 3.1 8B (built-in or Ollama) |
| **Text-only**          | Current RAG-only mode preserved. No tool calling. Graceful degradation.              | Qwen3 1.7B (default), Qwen3 0.6B             |

The chat orchestrator checks a `tool_calling_mode` field on the active provider/model to choose the path. The default built-in model (Qwen3 1.7B) operates in text-only mode. When a user upgrades to Qwen3 4B+ or enables a cloud provider, tool calling activates automatically.

---

## 4. Detailed Design

### 4.1 New Crate Dependencies

```toml
# backend/Cargo.toml additions
[dependencies]
rmcp = { version = "1.3", features = ["server", "client", "macros", "transport-streamable-http-server"] }
schemars = "0.8"  # JSON Schema generation for tool parameters
gbnf = "0.1"      # JSON Schema → GBNF grammar conversion for local model tool calling
```

### 4.1.1 Local Model Tool-Calling Capability

Research findings (April 2026) on local models and tool calling:

**ONNX Runtime is NOT viable** for generative tool calling. ONNX RT GenAI has experimental constrained decoding (PR #1381) but the ecosystem is immature compared to llama.cpp. Continue using ONNX for embeddings only.

**llama-cpp-2 is the correct path** for local tool calling. llama.cpp supports tool calling via:

1. Jinja chat templates with `--jinja` flag (handles Hermes-style `<tools>`/`<tool_call>` formatting)
2. GBNF grammar-constrained decoding (ensures valid JSON tool-call output)
3. Native handlers for Qwen, Llama, Hermes, Functionary, and Mistral model families

#### Local Model Benchmark (Tool Calling)

Source: [MikeVeerman/tool-calling-benchmark](https://github.com/MikeVeerman/tool-calling-benchmark)

| Model               | Params   | Q4_K_M Size | RAM       | Tool Score             | Latency    | Viable?                                                     |
| ------------------- | -------- | ----------- | --------- | ---------------------- | ---------- | ----------------------------------------------------------- |
| Qwen3-0.6B          | 0.6B     | ~400MB      | 800MB     | 0.880                  | 3.4s       | Marginal                                                    |
| LFM 2.5 1.2B        | 1.2B     | ~700MB      | 1.2GB     | 0.920                  | 1.6s       | Good (speed-optimized)                                      |
| **Qwen3-1.7B**      | **1.7B** | **~1.1GB**  | **1.5GB** | **0.960**              | **10.7s**  | **Best score, but high variance and thinking-mode failure** |
| Qwen 2.5-1.5B       | 1.5B     | ~1GB        | 1.5GB     | 0.800                  | 2.2s       | Decent                                                      |
| Llama 3.2-3B        | 3B       | ~1.8GB      | 2.5GB     | 0.660                  | 1.7s       | Poor accuracy                                               |
| Phi-4 Mini          | 3.8B     | ~2.4GB      | 4GB       | 0.780                  | 5.2s       | Decent                                                      |
| **Qwen3-4B**        | **4B**   | **~2.5GB**  | **4GB**   | **0.880**              | **varies** | **Recommended minimum for reliable tool calling**           |
| Hermes-2-Pro 8B     | 8B       | ~4.5GB      | 7GB       | ~0.90                  | ~5s        | Excellent (purpose-built)                                   |
| Functionary v3.2 8B | 8B       | ~5GB        | 7GB       | ~0.90                  | ~5s        | Excellent (purpose-built)                                   |
| **Qwen3-8B**        | **8B**   | **~5GB**    | **7GB**   | **expected excellent** | **~8s**    | **Recommended default for 16GB+ machines**                  |

#### Critical: Qwen3 Thinking Mode vs Tool Calling

The project's current `strip_think_blocks()` function addresses Qwen3's `<think>...</think>` output. However, **thinking mode causes ~60% tool-call failure** (QwenLM/Qwen3#1817): the model reasons about tool calls inside `<think>` blocks but then emits text instead of `<tool_call>` tags.

**Solution:** Disable thinking mode for tool-calling requests:

- Add `/no_think` token to the prompt, OR
- Set `enable_thinking=False` in the chat template kwargs
- Keep thinking mode enabled for regular RAG/chat (non-tool) queries
- The orchestrator manages this toggle based on whether tools are active

#### Recommended Default Model Strategy

| Machine                        | Default Model        | Tool Calling?        | Rationale                                                               |
| ------------------------------ | -------------------- | -------------------- | ----------------------------------------------------------------------- |
| 8GB RAM                        | Qwen3-1.7B Q4_K_M    | RAG-only (text mode) | Current default preserved. Tool calling unreliable at this size.        |
| 8GB RAM + tool calling desired | Qwen3-4B Q4_K_M      | Yes (Hermes format)  | Practical minimum. 2.5GB model fits in 8GB with headroom.               |
| 16GB+ RAM                      | Qwen3-8B Q4_K_M      | Yes (Hermes format)  | Reliable tool calling, equivalent to Qwen2.5-14B quality.               |
| Any + Ollama                   | qwen3:4b or qwen3:8b | Yes (Ollama API)     | Ollama handles template/grammar natively via `/api/chat` `tools` param. |
| Any + Cloud                    | Claude / GPT-4o      | Yes (native API)     | Best quality, highest cost.                                             |

#### Hermes Tool-Call Format (Used by Qwen3)

```text
<|im_start|>system
You are a helpful assistant.
<tools>
[{"type": "function", "function": {"name": "search_emails", "description": "Search emails", "parameters": {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}}}]
</tools>
<|im_end|>
<|im_start|>user
Find emails from Alice about the budget
<|im_end|>
<|im_start|>assistant
<tool_call>
{"name": "search_emails", "arguments": {"query": "from:Alice budget"}}
</tool_call>
<|im_end|>
<|im_start|>tool
{"results": [{"subject": "Q2 Budget Review", "from": "alice@example.com", "date": "2026-03-15"}]}
<|im_end|>
<|im_start|>assistant
I found one email from Alice about the budget: "Q2 Budget Review" sent on March 15, 2026.
<|im_end|>
```

#### Models to Add to Catalog

Add to `config/models-llm.yaml`:

```yaml
# Tool-calling specialist (fine-tuned on 60K function calling examples)
- id: 'qwen3-4b-toolcall-q4km'
  name: 'Qwen3 4B Tool Call'
  family: qwen3
  hf_repo: 'Manojb/Qwen3-4b-toolcall-gguf-llamacpp-codex'
  hf_file: 'qwen3-4b-toolcall-q4_k_m.gguf'
  context_size: 4096
  ram_required_gb: 4.0
  tool_calling: true
  tuning:
    temperature: 0.3 # Lower temp for structured output
    top_p: 0.9

# Purpose-built function calling model
- id: 'hermes-2-pro-8b-q4km'
  name: 'Hermes 2 Pro 8B'
  family: hermes
  hf_repo: 'NousResearch/Hermes-2-Pro-Llama-3-8B-GGUF'
  hf_file: 'Hermes-2-Pro-Llama-3-8B-Q4_K_M.gguf'
  context_size: 8192
  ram_required_gb: 7.0
  tool_calling: true
```

### 4.2 MCP Server: Tool Definitions

Rather than exposing every CRUD endpoint 1:1, curate tools into **task-oriented groups** optimized for LLM understanding:

#### Email Tools

| Tool Name               | Description                                        | Maps To                                             |
| ----------------------- | -------------------------------------------------- | --------------------------------------------------- |
| `search_emails`         | Search emails by query, sender, date range, folder | `GET /api/v1/vectors/search` + `GET /api/v1/emails` |
| `get_email`             | Get full email content by ID                       | `GET /api/v1/emails/:id`                            |
| `get_email_thread`      | Get all emails in a conversation thread            | `GET /api/v1/emails/:id/thread`                     |
| `list_recent_emails`    | List most recent emails with optional filters      | `GET /api/v1/emails?sort=date&limit=N`              |
| `get_email_attachments` | List attachments for an email                      | `GET /api/v1/emails/:id/attachments`                |

#### Action Tools

| Tool Name        | Description                                       | Maps To                                 |
| ---------------- | ------------------------------------------------- | --------------------------------------- |
| `send_email`     | Compose and send an email (requires confirmation) | `POST /api/v1/emails/send` (new)        |
| `reply_to_email` | Reply to an existing email                        | `POST /api/v1/emails/:id/reply` (new)   |
| `forward_email`  | Forward an email to recipients                    | `POST /api/v1/emails/:id/forward` (new) |
| `move_email`     | Move email to a folder                            | `POST /api/v1/emails/:id/move`          |
| `delete_email`   | Delete an email (requires confirmation)           | `DELETE /api/v1/emails/:id`             |
| `mark_email`     | Mark email as read/unread/starred                 | `PATCH /api/v1/emails/:id`              |

#### Organization Tools

| Tool Name        | Description                       | Maps To                    |
| ---------------- | --------------------------------- | -------------------------- |
| `classify_email` | Classify an email into a category | `POST /api/v1/ai/classify` |
| `create_rule`    | Create an email processing rule   | `POST /api/v1/rules`       |
| `list_rules`     | List active email rules           | `GET /api/v1/rules`        |
| `update_rule`    | Modify an existing rule           | `PUT /api/v1/rules/:id`    |
| `unsubscribe`    | Unsubscribe from a mailing list   | `POST /api/v1/unsubscribe` |

#### Analytics Tools

| Tool Name      | Description                                           | Maps To                  |
| -------------- | ----------------------------------------------------- | ------------------------ |
| `get_insights` | Get email analytics (volume, top senders, categories) | `GET /api/v1/insights/*` |
| `get_clusters` | Get email topic clusters                              | `GET /api/v1/clustering` |

#### Account Tools

| Tool Name         | Description                       | Maps To                        |
| ----------------- | --------------------------------- | ------------------------------ |
| `list_accounts`   | List connected email accounts     | `GET /api/v1/auth/accounts`    |
| `sync_account`    | Trigger email sync for an account | `POST /api/v1/ingestion/sync`  |
| `get_sync_status` | Check sync progress               | `GET /api/v1/ingestion/status` |

### 4.3 MCP Server Implementation (Rust)

New file: `backend/src/mcp/server.rs`

```rust
use rmcp::{ServerHandler, tool, tool_router, tool_handler};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use crate::AppState;

#[derive(Clone)]
pub struct EmailibriumMcpServer {
    tool_router: ToolRouter<Self>,
    state: Arc<AppState>,
    user_token: Option<String>,  // forwarded from auth middleware
}

#[tool_router]
impl EmailibriumMcpServer {
    pub fn new(state: Arc<AppState>, user_token: Option<String>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            state,
            user_token,
        }
    }

    #[tool(description = "Search the user's emails by query text, sender, date range, or folder. Returns matching emails with sender, subject, date, and relevance score.")]
    async fn search_emails(
        &self,
        #[tool(param, description = "Search query text")] query: String,
        #[tool(param, description = "Filter by sender email address")] from: Option<String>,
        #[tool(param, description = "Start date (YYYY-MM-DD)")] after: Option<String>,
        #[tool(param, description = "End date (YYYY-MM-DD)")] before: Option<String>,
        #[tool(param, description = "Maximum results to return (default: 10)")] limit: Option<u32>,
    ) -> Result<CallToolResult, McpError> {
        // Delegates to existing HybridSearch + email DB lookup
        // Uses self.state to access VectorService and Database
        todo!()
    }

    #[tool(description = "Send a new email. IMPORTANT: Always confirm with the user before sending.")]
    async fn send_email(
        &self,
        #[tool(param, description = "Recipient email address(es), comma-separated")] to: String,
        #[tool(param, description = "Email subject line")] subject: String,
        #[tool(param, description = "Email body text")] body: String,
        #[tool(param, description = "CC recipients (optional)")] cc: Option<String>,
    ) -> Result<CallToolResult, McpError> {
        // Requires user confirmation flow
        todo!()
    }

    // ... additional tools follow the same pattern
}

#[tool_handler]
impl ServerHandler for EmailibriumMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new("emailibrium", env!("CARGO_PKG_VERSION"))
            .with_protocol_version(ProtocolVersion::V_2025_06_18)
            .with_capabilities(
                ServerCapabilities::builder()
                    .enable_tools()
                    .enable_prompts()
                    .build(),
            )
    }
}
```

### 4.4 MCP Server Mounting (Axum Integration)

In `backend/src/main.rs`, mount the MCP server alongside existing routes:

```rust
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};

let mcp_state = app_state.clone();
let mcp_service = StreamableHttpService::new(
    move || Ok(EmailibriumMcpServer::new(mcp_state.clone(), None)),
    LocalSessionManager::default().into(),
    Default::default(),
);

let app = Router::new()
    .nest("/api/v1", api::routes())
    .nest_service("/api/v1/mcp", mcp_service)  // MCP endpoint
    .with_state(app_state);
```

### 4.5 Chat Orchestrator Refactor

Replace the current single-shot `generate()` call with a **tool-calling loop**:

New file: `backend/src/vectors/chat_orchestrator.rs`

```rust
/// Orchestrates a multi-turn tool-calling conversation.
///
/// Flow:
/// 1. Build messages array (system + history + user message)
/// 2. Include tool definitions from MCP server
/// 3. Call LLM with tool-calling API
/// 4. If LLM returns tool_call(s):
///    a. Execute each via MCP client
///    b. Append tool results to messages
///    c. Go to step 3
/// 5. If LLM returns text: stream to user, done.
///
/// Max iterations capped by config to prevent infinite loops.
pub struct ChatOrchestrator {
    mcp_client: McpClient,           // in-process MCP client
    provider: Arc<dyn ToolCallingProvider>,
    config: OrchestratorConfig,
}

pub struct OrchestratorConfig {
    pub max_tool_iterations: u32,     // default: 5
    pub tool_timeout_ms: u64,         // per-tool timeout
    pub require_confirmation: Vec<String>,  // tools needing user OK
}
```

### 4.6 GenerativeModel Trait Extension

Extend the existing trait to support tool calling alongside the current `generate()` interface:

```rust
// New trait for tool-calling capable providers
#[async_trait]
pub trait ToolCallingProvider: Send + Sync {
    /// Send messages with tool definitions, get back either text or tool calls.
    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        params: &GenerationParams,
    ) -> Result<ChatCompletion>;

    /// Whether this provider supports native tool calling.
    fn supports_tool_calling(&self) -> bool;
}

/// Response from a tool-calling LLM.
pub enum ChatCompletion {
    /// LLM produced a text response (done).
    Text(String),
    /// LLM wants to call one or more tools.
    ToolCalls(Vec<ToolCall>),
}

pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}
```

**Provider implementations:**

| Provider                           | `tool_calling_mode`       | Implementation                                                                                                                                                                                              |
| ---------------------------------- | ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `CloudGenerativeModel` (Anthropic) | `NativeApi`               | Use Claude Messages API with `tools` parameter                                                                                                                                                              |
| `CloudGenerativeModel` (OpenAI)    | `NativeApi`               | Use Chat Completions API with `tools` parameter                                                                                                                                                             |
| `OllamaGenerativeModel`            | `NativeApi` (conditional) | Use Ollama `/api/chat` with `tools` param; detect capability via model metadata                                                                                                                             |
| `BuiltinLlmModel` (llama-cpp)      | `HermesGrammar` or `None` | If model catalog says `tool_calling: true` AND model ≥4B: use Hermes-format prompt + GBNF grammar-constrained generation. Disable thinking mode. Parse `<tool_call>` tags from output. Otherwise: RAG-only. |

```rust
/// How a provider handles tool calling.
pub enum ToolCallingMode {
    /// Provider's API natively supports `tools` parameter (Claude, OpenAI, Ollama).
    NativeApi,
    /// Local model uses Hermes-style tags with grammar-constrained generation.
    HermesGrammar,
    /// No tool calling support — use RAG-only chat path.
    None,
}
```

### 4.7 Externalized Configuration

#### New config file: `config/tools.yaml`

```yaml
version: '1.0'

# ── Tool Definitions ────────────────────────────────────────────────────────
# Controls which MCP tools are available and their behavior.
# The MCP server reads this at startup to configure tool registration.

defaults:
  rate_limit_per_minute: 20
  timeout_ms: 10000

tools:
  search_emails:
    enabled: true
    requires_confirmation: false
    rate_limit_per_minute: 30
    description_override: ~ # Use default from code if null

  send_email:
    enabled: true
    requires_confirmation: true # MUST confirm with user before executing
    rate_limit_per_minute: 5

  delete_email:
    enabled: true
    requires_confirmation: true
    rate_limit_per_minute: 10

  create_rule:
    enabled: true
    requires_confirmation: true
    rate_limit_per_minute: 10

  # Disable tools by setting enabled: false
  # Example: disable account management tools for restricted deployments
  # list_accounts:
  #   enabled: false
```

#### Extend `config/prompts.yaml`

```yaml
# ── Tool-Calling Chat Assistant ─────────────────────────────────────────────
# Used when the active model supports tool calling.
# Variables: {{current_date}}, {{user_name}}
chat_assistant_tools: |
  You are an email assistant with full access to the user's inbox and email management tools.
  Today is {{current_date}}.

  You can:
  - Search and read emails
  - Send, reply to, and forward emails
  - Create and manage email rules
  - Classify and organize emails
  - View analytics and insights
  - Manage email accounts and sync

  IMPORTANT RULES:
  1. For any action that sends, deletes, or modifies data, ALWAYS confirm with the user first.
  2. When searching, present results clearly with sender, subject, and date.
  3. Be specific — use actual data from tool results, never fabricate email content.
  4. If a tool call fails, explain the error clearly and suggest alternatives.
  5. Do NOT include internal reasoning in your response. Answer directly.

# ── Tool Confirmation Prompt ────────────────────────────────────────────────
# Inserted when a tool requires user confirmation before execution.
tool_confirmation: |
  I'd like to perform the following action:
  **{{action_description}}**

  Should I proceed? (yes/no)
```

#### Extend `config/tuning.yaml`

```yaml
# ── Tool-Calling Orchestration ──────────────────────────────────────────────
orchestrator:
  max_tool_iterations: 5 # Max tool-call loops before forcing text response
  tool_timeout_ms: 10000 # Per-tool execution timeout
  parallel_tool_calls: true # Allow LLM to call multiple tools at once
  tool_result_max_chars: 4000 # Truncate tool results beyond this

# ── Model Routing for Tool Calling ──────────────────────────────────────────
# When tool calling is needed, prefer local models first (no cost, no latency),
# then fall back to cloud providers. Order: builtin → ollama → cloud.
tool_calling_providers:
  - provider: builtin
    model: qwen3-4b-q4km # Recommended minimum local model for tool calling
    mode: hermes_grammar # Uses Hermes-format + GBNF grammar
    priority: 1
    min_ram_gb: 4 # Only use if machine has ≥4GB available RAM
  - provider: ollama
    model: qwen3:8b # Ollama handles tool calling natively
    mode: native_api
    priority: 2
  - provider: anthropic
    model: claude-sonnet-4-20250514
    mode: native_api
    priority: 3
  - provider: openai
    model: gpt-4o
    mode: native_api
    priority: 4

# ── Local Tool Calling Settings ────────────────────────────────────────────
local_tool_calling:
  disable_thinking: true # CRITICAL: Qwen3 thinking mode causes ~60% tool-call failure
  grammar_constrain: true # Use GBNF grammar to ensure valid JSON tool output
  min_model_params_b: 4.0 # Don't attempt tool calling with models smaller than this
  no_think_token: '/no_think' # Token injected into prompt to disable Qwen3 thinking
```

### 4.8 Frontend Changes

#### SSE Protocol Extension

Extend the streaming protocol to support tool-call events:

```typescript
// New SSE event types
type: 'token'; // existing: streaming text chunk
type: 'done'; // existing: stream complete
type: 'error'; // existing: error occurred
type: 'tool_call'; // NEW: LLM is calling a tool
type: 'tool_result'; // NEW: tool execution result
type: 'confirmation'; // NEW: tool needs user approval
```

#### Chat UI Enhancements

- **Tool call indicators**: Show a brief status when tools are being called ("Searching emails...", "Creating rule...")
- **Confirmation dialogs**: When a tool requires confirmation, pause streaming and show an approve/reject UI
- **Action result cards**: Display structured results (email cards, rule summaries) instead of raw text
- **Tool call history**: Optionally show what tools were called in a collapsible section

#### Updated `useChat` Hook

```typescript
// frontend/apps/web/src/features/chat/hooks/useChat.ts
interface ChatStreamEvent {
  type: 'token' | 'done' | 'error' | 'tool_call' | 'tool_result' | 'confirmation';
  content?: string;
  sessionId?: string;
  toolName?: string;
  toolArgs?: Record<string, unknown>;
  toolResult?: unknown;
  confirmationId?: string;
}

// New: send confirmation response
async function confirmToolCall(confirmationId: string, approved: boolean): Promise<void>;
```

### 4.9 Security Model

1. **Token Forwarding**: The MCP server receives the user's auth token from the chat endpoint's middleware. All tool executions use this token to call internal services, ensuring the user's permission scope is enforced.

2. **Tool Filtering**: At session start, the MCP server checks user permissions and only registers tools the user is authorized to use. A read-only user won't see `send_email` or `delete_email`.

3. **Confirmation Gates**: Destructive tools (`send_email`, `delete_email`, `create_rule`) require explicit user confirmation before execution. The orchestrator pauses the tool-call loop and sends a `confirmation` SSE event.

4. **Rate Limiting**: Per-user, per-tool rate limits from `tools.yaml`. Prevents runaway tool calls from prompt injection or LLM hallucination.

5. **Input Validation**: All tool arguments validated against JSON Schema before execution. The MCP server never trusts LLM-generated inputs blindly.

6. **Audit Logging**: Every tool call logged with user ID, tool name, arguments (sanitized), result status, and latency.

---

## 5. Implementation Phases

### Phase 1: MCP Server Foundation (Sprint 1)

- [ ] Add `rmcp` + `schemars` to `Cargo.toml`
- [ ] Create `backend/src/mcp/` module with `mod.rs`, `server.rs`, `tools/` directory
- [ ] Implement 3 read-only tools: `search_emails`, `get_email`, `list_recent_emails`
- [ ] Mount MCP server at `/api/v1/mcp` with Streamable HTTP transport
- [ ] Add `tools.yaml` config file with enable/disable flags
- [ ] Write integration tests for MCP tool discovery and execution
- [ ] Verify MCP server works with Claude Desktop or MCP Inspector

### Phase 2: Tool-Calling Chat Orchestrator (Sprint 2)

- [ ] Define `ToolCallingProvider` trait in `generative.rs`
- [ ] Implement `ToolCallingProvider` for `CloudGenerativeModel` (Anthropic Claude)
- [ ] Implement `ToolCallingProvider` for `CloudGenerativeModel` (OpenAI)
- [ ] Build `ChatOrchestrator` with tool-calling loop
- [ ] Extend SSE protocol with `tool_call`, `tool_result`, `confirmation` events
- [ ] Add `chat_assistant_tools` prompt to `prompts.yaml`
- [ ] Add `orchestrator` section to `tuning.yaml`
- [ ] Dual-mode: detect provider capability, route to orchestrator or legacy path
- [ ] Unit + integration tests for the orchestrator loop

### Phase 3: Full Tool Coverage (Sprint 3)

- [ ] Implement action tools: `send_email`, `reply_to_email`, `forward_email`, `move_email`, `delete_email`, `mark_email`
- [ ] Implement organization tools: `classify_email`, `create_rule`, `list_rules`, `update_rule`, `unsubscribe`
- [ ] Implement analytics tools: `get_insights`, `get_clusters`
- [ ] Implement account tools: `list_accounts`, `sync_account`, `get_sync_status`
- [ ] Confirmation gate flow for destructive tools
- [ ] Rate limiting per tool per user

### Phase 4: Frontend Integration (Sprint 3-4)

- [ ] Extend `chatApi.ts` to handle new SSE event types
- [ ] Update `useChat` hook with tool-call state management
- [ ] Build confirmation dialog component
- [ ] Build tool-call status indicators ("Searching emails...")
- [ ] Build structured result cards (email cards, rule summaries)
- [ ] Collapsible tool-call history in chat messages

### Phase 5: Local Model Tool-Calling (Sprint 4)

- [ ] Add `tool_calling: bool` field to model catalog (`models-llm.yaml`, `ModelInfo` struct)
- [ ] Add Qwen3-4B and Qwen3-8B models with `tool_calling: true` to catalog
- [ ] Add `hermes-2-pro-8b` and `qwen3-4b-toolcall` specialist models to catalog
- [ ] Implement `HermesGrammar` mode in `BuiltinLlmModel`:
  - Extend `to_chatml()` to inject `<tools>` block when tools are active
  - Add `/no_think` token injection to disable thinking mode for tool calls
  - Use `gbnf` crate to convert tool JSON schemas to GBNF grammars
  - Apply grammar-constrained sampling during generation
  - Parse `<tool_call>` tags from model output
- [ ] Implement `ToolCallingProvider` for `OllamaGenerativeModel` using `/api/chat` `tools` param
- [ ] Detect Ollama model tool-calling capability via `/api/show` metadata
- [ ] Auto-select tool-calling mode based on model catalog + available RAM
- [ ] Integration tests with Qwen3-4B Q4_K_M for local tool calling
- [ ] Benchmark: local tool-call latency, accuracy, thinking-mode-off impact

### Phase 6: Hardening (Sprint 5)

- [ ] Security audit: token forwarding, permission filtering, input validation
- [ ] Audit logging for all tool calls
- [ ] Prompt injection defenses (output sanitization, tool-call validation)
- [ ] Performance benchmarks: tool-call latency, streaming throughput
- [ ] Documentation: API docs for MCP endpoint, tool catalog

---

## 6. Key Technical Decisions

### Why rmcp over TypeScript MCP SDK?

- The backend is Rust (Axum). Using rmcp keeps the MCP server in-process with zero-cost access to `AppState`, database, and vector services. No inter-process serialization overhead.
- `rmcp-actix-web` exists but we use Axum; rmcp's built-in `StreamableHttpService` integrates natively with Axum.

### Why Streamable HTTP over stdio?

- The chat is a web application. Streamable HTTP works over standard HTTP, supports sessions, and is the current MCP spec recommendation. stdio is for CLI tools.

### Why curated tools over raw OpenAPI mapping?

- LLMs perform better with fewer, well-described, task-oriented tools than with dozens of CRUD endpoints. `rmcp-openapi` can be used as a starting point during development, but production tools should be hand-curated for optimal LLM decision-making.

### Why tri-mode (cloud tool-calling + local tool-calling + RAG-only)?

- **Cloud providers** (Claude, GPT-4o) have mature native tool-calling APIs — use them directly.
- **Local models ≥4B** (Qwen3-4B, Qwen3-8B, Hermes-2-Pro) support tool calling via Hermes-format prompt templates + GBNF grammar-constrained generation in llama-cpp. This enables **tool calling without cloud dependency**.
- **Small local models** (Qwen3-1.7B, the current default) are marginal for tool calling — high variance, thinking-mode interference. The existing RAG path continues working for these.
- ONNX Runtime is not viable for generative tool calling; it remains embeddings-only.
- Tool calling is an additive capability, not a replacement. The system auto-detects capability based on the model catalog's `tool_calling` flag.

### Why in-process MCP over external server?

- Latency: in-process tool calls avoid HTTP round-trips. The MCP client and server share the same Tokio runtime.
- Simplicity: no additional deployment artifact. The MCP server is just another Axum service.
- The MCP endpoint is also exposed externally so Claude Desktop or other MCP clients can connect directly.

---

## 7. Configuration Summary

| File                     | New Sections                                | Purpose                                              |
| ------------------------ | ------------------------------------------- | ---------------------------------------------------- |
| `config/tools.yaml`      | (new file)                                  | Tool enable/disable, rate limits, confirmation flags |
| `config/prompts.yaml`    | `chat_assistant_tools`, `tool_confirmation` | Tool-calling system prompt, confirmation template    |
| `config/tuning.yaml`     | `orchestrator`, `tool_calling_providers`    | Max iterations, timeouts, provider routing           |
| `config/models-llm.yaml` | `supports_tool_calling` per model           | Declare which models support tool calling            |

All configuration follows the existing pattern: YAML files loaded at startup, with `EMAILIBRIUM_*` environment variable overrides for secrets and deployment-specific values.

---

## 8. Risks and Mitigations

| Risk                                          | Mitigation                                                                                                                                                                |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| LLM calls wrong tool or fabricates arguments  | JSON Schema validation on all inputs; confirmation gates on destructive actions; GBNF grammar ensures valid JSON                                                          |
| Prompt injection via email content            | Tool-call results sanitized before re-injection; system prompt hardening                                                                                                  |
| Infinite tool-call loops                      | `max_tool_iterations` cap (default: 5); timeout per tool                                                                                                                  |
| Latency increase from tool-call round-trips   | In-process MCP avoids network hops; parallel tool calls where supported                                                                                                   |
| rmcp crate immaturity                         | v1.3 is stable with active development; fallback to direct API calls if needed                                                                                            |
| Qwen3 thinking mode breaks tool calling       | Disable thinking (`/no_think`) when tools are active. Orchestrator manages this toggle automatically. Per QwenLM/Qwen3#1817: thinking mode causes ~60% tool-call failure. |
| Local 1.7B model too small for reliable tools | Default to RAG-only for ≤2B models. Recommend Qwen3-4B+ for tool calling via UI prompt. Auto-detect from model catalog `tool_calling` flag.                               |
| ONNX not viable for generative tool calling   | Confirmed via research: ONNX RT GenAI has only preview support. Stick with llama-cpp-2 for local inference.                                                               |
| Grammar-constrained generation adds latency   | GBNF constraining is marginal overhead (<5%). The bigger factor is model size — Qwen3-4B at Q4_K_M runs tool calls in ~3-5s on Apple Silicon.                             |

---

## 9. File Impact Summary

### New Files

| Path                                                         | Purpose                                                     |
| ------------------------------------------------------------ | ----------------------------------------------------------- |
| `backend/src/mcp/mod.rs`                                     | MCP module root                                             |
| `backend/src/mcp/server.rs`                                  | MCP server handler with tool definitions                    |
| `backend/src/mcp/tools/`                                     | Tool implementation modules (emails, rules, accounts, etc.) |
| `backend/src/vectors/chat_orchestrator.rs`                   | Tool-calling loop orchestrator                              |
| `backend/src/vectors/tool_calling.rs`                        | `ToolCallingProvider` trait + provider impls                |
| `config/tools.yaml`                                          | Tool configuration                                          |
| `frontend/apps/web/src/features/chat/ToolCallIndicator.tsx`  | Tool-call status UI                                         |
| `frontend/apps/web/src/features/chat/ConfirmationDialog.tsx` | Action confirmation UI                                      |

### Modified Files

| Path                                                   | Changes                                                                                                         |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| `backend/Cargo.toml`                                   | Add `rmcp`, `schemars`, `gbnf` dependencies                                                                     |
| `backend/src/main.rs`                                  | Mount MCP service, pass to orchestrator                                                                         |
| `backend/src/api/ai.rs`                                | Wire orchestrator into chat endpoints                                                                           |
| `backend/src/vectors/chat.rs`                          | Integrate orchestrator, extend session with tool state                                                          |
| `backend/src/vectors/generative.rs`                    | Add `ToolCallingProvider` trait                                                                                 |
| `backend/src/vectors/generative_router.rs`             | Route to tool-calling providers                                                                                 |
| `backend/src/vectors/generative_builtin.rs`            | Extend `to_chatml()` with `<tools>` injection, `/no_think` toggle, `<tool_call>` parsing, GBNF grammar sampling |
| `backend/src/vectors/model_catalog.rs`                 | Add `tool_calling: bool` field to `ModelInfo`                                                                   |
| `config/models-llm.yaml`                               | Add `tool_calling` flag per model; add Qwen3-4B, Hermes-2-Pro models                                            |
| `backend/src/vectors/yaml_config.rs`                   | Load `tools.yaml`, new tuning sections                                                                          |
| `config/prompts.yaml`                                  | Add tool-calling prompts                                                                                        |
| `config/tuning.yaml`                                   | Add orchestrator + tool-calling provider config                                                                 |
| `frontend/packages/api/src/chatApi.ts`                 | Handle new SSE event types                                                                                      |
| `frontend/packages/types/src/chat.ts`                  | Extend types for tool calls                                                                                     |
| `frontend/apps/web/src/features/chat/hooks/useChat.ts` | Tool-call state management                                                                                      |
| `frontend/apps/web/src/features/chat/ChatMessage.tsx`  | Render tool-call indicators and results                                                                         |

---

## 10. References

### MCP & rmcp

- [rmcp crate (v1.3)](https://crates.io/crates/rmcp) — Official Rust MCP SDK
- [rmcp-openapi](https://crates.io/crates/rmcp-openapi) — OpenAPI to MCP tool generation
- [rmcp-actix-web](https://crates.io/crates/rmcp-actix-web) — Actix-Web transport (reference only; we use Axum)
- [MCP Specification](https://spec.modelcontextprotocol.io/) — Protocol specification

### Cloud Provider Tool Calling

- [Anthropic Tool Use](https://docs.anthropic.com/en/docs/build-with-claude/tool-use) — Claude tool calling API
- [OpenAI Function Calling](https://platform.openai.com/docs/guides/function-calling) — GPT-4 tool calling API

### Local Model Tool Calling

- [llama.cpp Function Calling](https://github.com/ggml-org/llama.cpp/blob/master/docs/function-calling.md) — llama.cpp tool calling docs
- [llama-cpp-2 Rust crate](https://crates.io/crates/llama-cpp-2) — Rust bindings (used by project)
- [gbnf crate](https://crates.io/crates/gbnf) — JSON Schema → GBNF grammar conversion
- [MikeVeerman/tool-calling-benchmark](https://github.com/MikeVeerman/tool-calling-benchmark) — Local model tool-calling benchmark
- [Qwen3 Function Calling](https://qwen.readthedocs.io/en/latest/framework/function_call.html) — Official Qwen tool-calling docs
- [QwenLM/Qwen3#1817](https://github.com/QwenLM/Qwen3/issues/1817) — Thinking mode tool-call failure issue
- [Ollama Tool Calling](https://docs.ollama.com/capabilities/tool-calling) — Ollama tool support
- [Hermes-2-Pro](https://huggingface.co/NousResearch/Hermes-2-Pro-Llama-3-8B-GGUF) — Purpose-built tool-calling model
- [Qwen3-4B Tool Call Fine-tune](https://huggingface.co/Manojb/Qwen3-4b-toolcall-gguf-llamacpp-codex) — Specialized tool-calling fine-tune

### Project ADRs

- ADR-012: Generative AI Provider Architecture
- ADR-013: AI API Endpoints
- ADR-022: RAG Pipeline
