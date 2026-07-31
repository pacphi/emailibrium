# MCP Maturation Plan

Consolidating and maturing emailibrium's MCP server (follow-on to ADR-028).

> **Document status.** The **Design** section below is complete and authoritative.
> Rollout, Testing results, Operations, and Changelog describe the lived state as of
> 2026-07-31 and are updated as work lands. Implementation is **mid-flight** — see the
> status table under Rollout for what is actually in the tree versus what is designed.

---

## Design

### 0. Problem and scope

ADR-028 shipped an MCP server at `backend/src/mcp/` exposing seven read-only tools over
Streamable HTTP at `/api/v1/mcp`, with per-tool rate limiting (`mcp/rate_limit.rs`) and
SHA-256 audit logging (`mcp/audit.rs` → `mcp_tool_audit`, migration 022).

Three defects followed:

1. **Duplicated declarations.** `backend/src/api/ai.rs` re-declares the same seven tools by
   hand for the chat orchestrator — `build_tool_definitions()` (line 1199) writes the JSON
   schemas literally, and `build_tool_executor()` (line 1275) re-implements every handler
   against the same tables. The two copies have already drifted: `count_emails` and
   `get_email_thread` carry shorter descriptions in `ai.rs`, and its `search_emails` result
   omits `latency_ms`.
2. **Enforcement only covers one path.** Rate limiting and audit logging live on
   `EmailibriumMcpServer`, so they apply to MCP transport calls only. Tool calls the chat
   orchestrator makes in-process bypass both entirely — no rate limit, no audit row.
3. **`config/tools.yaml` is dead.** ADR-028 §4.7 specifies it; the file exists and is loaded
   nowhere.

This design fixes all three with one declaration per tool, adds eight read-only tools (A3),
and specifies resources and prompts (A5). Action tools (A4) are **out of scope for this
run** — their `tools.yaml` entries stay in the file and are treated as known-deferred.

**Non-goal:** changing the observable behaviour of the seven existing tools. Their names,
argument shapes, and result payloads are preserved exactly; §5.4 lists the two deliberate,
minor exceptions.

---

### 1. Verified rmcp 1.7 API constraints

Everything below was read from the vendored crate source at
`~/.cargo/registry/src/index.crates.io-*/rmcp-1.7.0/` and `rmcp-macros-1.7.0/`, not inferred.
These facts drive the design, so they are recorded with citations.

**A runtime, data-driven tool router is fully supported public API.**
`ToolRouter::with_route()` accepts `ToolRoute::new_dyn(attr: impl Into<Tool>, call)` where
`call` is a plain closure
(`rmcp-1.7.0/src/handler/server/router/tool.rs:182`). The handler type is

```rust
pub type DynCallToolHandler<S> =
    dyn for<'s> Fn(ToolCallContext<'s, S>) -> BoxFuture<'s, Result<CallToolResult, ErrorData>>
        + Send + Sync;                      // handler/server/tool.rs:159
```

so a router can be built at startup from a table of tool descriptors. The `#[tool]` and
`#[tool_router]` macros are a convenience over exactly this, not a requirement.

**`ToolCallContext` exposes everything a closure needs, with public fields**
(`handler/server/tool.rs:32`): `service: &'s S`, `arguments: Option<JsonObject>`, `name`,
`request_context`. No extractor machinery is needed inside a `new_dyn` closure.

**Schema generation is a public, memoized helper.**
`rmcp::handler::server::tool::schema_for_type::<T>() -> Arc<JsonObject>` is the same function
the `#[tool]` macro emits (`handler/server/common.rs`; re-exported at
`handler/server/tool.rs:13`). It caches per `TypeId`, aligns to JSON Schema draft 2020-12,
and deliberately omits the OpenAPI-only `nullable` keyword. A registry calling it produces
schemas byte-identical to the macro's.

**`rmcp::model::Tool` carries the whole declaration** — `name`, `description`,
`input_schema: Arc<JsonObject>` (`model/tool.rs:16`). `JsonObject` is
`serde_json::Map<String, Value>`, so converting to the chat orchestrator's
`ToolDefinition.input_schema: serde_json::Value` is a single `Value::Object` wrap. The struct
is `#[non_exhaustive]` but constructible via `Tool::new(name, description, input_schema)`.

**`ToolRouter` has native enable/disable** — `disable_route`, `with_disabled`, `is_disabled`,
`list_all` (filters disabled), `get` (returns `None` when disabled), and `call` (rejects with
`"tool not found"`). Useful, though this design filters at registry-build time instead so the
MCP and chat paths cannot disagree about which tools exist.

#### Gotchas found in the source

**`#[tool_handler]` defaults to a _static_ router call.** Its default `router` expression is
`Self::tool_router()` (`rmcp-macros-1.7.0/src/tool_handler.rs:20-25`), and the macro expands
to `#router.call(tcc)`, `#router.list_all()`, `#router.get(name)`. It therefore **rebuilds the
router on every single request** and never reads the struct field. This explains the
`#[allow(dead_code)]` on `EmailibriumMcpServer::tool_router` at `mcp/server.rs:36` — that
field is genuinely dead today. Any instance-state router (which config-driven registration
requires) **must** be selected explicitly:

```rust
#[tool_handler(router = self.tool_router)]
```

This is the single most important mechanical detail in the migration.

**`#[tool_handler]` generates `get_info()` only when absent** (`tool_handler.rs:91`). Our
`impl` already defines `get_info()`, so the auto-generated capability wiring never runs — when
resources and prompts are added, `.enable_resources()` / `.enable_prompts()` must be added to
our hand-written `get_info()` by hand.

**Prompts have a router; resources do not.** `PromptRouter<S>` / `PromptRoute::new_dyn` mirror
the tool API (`handler/server/router/prompt.rs`). But `handler/server/resource.rs` is an
**empty file** — resources must be implemented directly on the `ServerHandler` trait methods
`list_resources`, `list_resource_templates`, and `read_resource`
(`handler/server.rs:220-244`).

**`ResourceContents::text()` sets a bogus MIME type.** It hardcodes
`mime_type: Some("text".into())` (`model/resource.rs:87`), which is not a valid MIME type. For
JSON payloads, construct the enum variant directly (see §7.1).

**`rmcp` is compiled without the `client` feature.** `backend/Cargo.toml:100` enables only
`server`, `macros`, `transport-streamable-http-server`. There is no in-tree MCP client, which
constrains the integration-test approach (§8).

**Verdict: the registry approach is feasible.** No macro limitation blocks it. The macro-free
`ToolRouter` path is public, documented, and exercised by rmcp's own tests. We keep
`#[tool_handler]` (it correctly implements `call_tool`/`list_tools`/`get_tool` plus MCP
task-support validation) and drop `#[tool_router]`/`#[tool]`.

---

### 1.5 Crate topology — why handlers cannot take `AppState`

This is the constraint that shapes the whole design, and it is a compile-level fact rather
than a testing preference.

`backend/src/lib.rs` exposes nine modules: `cache, config, content, db, email, events,
middleware, rules, vectors`. `backend/src/main.rs` **re-declares those same nine** with `mod`
rather than importing them, and adds four of its own (`api`, `cleanup`, `mcp`, `sync_lock`)
plus `AppState` at line 33. The binary never imports the library at all.

Two consequences:

1. Each of those nine modules compiles into **both** crates as a distinct type identity.
   `emailibrium::db::Database` and the binary's `crate::db::Database` are different types that
   do not unify. A library-crate registry can therefore neither accept `&AppState` nor even
   hold an `Arc<Database>` taken from one.
2. `backend/tests/*.rs` link the library, so nothing MCP is reachable from an integration
   test. This — not oversight — is why the endpoint has zero end-to-end coverage, and why
   `tests/api_integration.rs` reimplements handlers rather than mounting the real router (its
   header comment says so outright).

**Ratified resolution:**

- `main.rs` stops re-declaring the nine shared modules and re-exports them from the library
  instead, collapsing the two type identities into one. Because the two crates' types never
  currently meet, there is no existing conflict to untangle, and `crate::db::X` in binary
  modules keeps resolving through the re-export. It also stops compiling nine modules twice.
- `tools`, `mcp`, and a single shared `mcp_service(ctx)` constructor are exposed via the
  library. `main.rs` (both transport modes) and the integration tests consume the **same**
  constructor, so tests exercise what ships.
- Handlers take `ToolContext` (§2.2), never `AppState`.
- Hoisting all of `AppState` into the library was considered and **rejected** — it would drag
  OAuth, cleanup orchestration, and the poll scheduler into the public surface.

#### 1.5.1 The two modules that must follow

**`cleanup` — mostly already portable.** `preview_cleanup_plan` needs
`cleanup::domain::builder::PlanBuilder`, and `cleanup` is binary-only today. But its bin-only
references number exactly five, and all sit in layers the preview never touches:
`cleanup/api/{mod,apply,plan,telemetry}.rs` (`use crate::AppState`) and
`cleanup/orchestrator/factory.rs:136` (`crate::api::provider_helpers::guard_imap_addr`).

**The split boundary is verified clean in both directions.** Splitting a module in half only
works if the moving halves do not reach back into the staying halves, so that was checked
explicitly rather than inferred: `cleanup/domain/**` and `cleanup/repository/**` contain **zero**
references to `orchestrator` or `cleanup::api`. Their complete set of cross-module dependencies
is `crate::cleanup::domain` (internal), `crate::db::audited_sql`, `crate::email::types`, and
`crate::rules::{rule_engine, rule_processor, types}` — every one of which is already a library
module. Nothing is dragged along and nothing dangles. The hexagonal design paid off.

So: **hoist `cleanup::domain` and `cleanup::repository` into the library; leave `cleanup::api`
and `cleanup::orchestrator` binary-side.** No trait bridge, no extra `ToolContext` field.

This supersedes the earlier plan to make `build_plan_builder` (`cleanup/api/plan.rs:79`)
`pub(crate)` — that function takes `&AppState`. Write a library-side twin taking
`&ToolContext` that runs the same `SELECT id, provider FROM connected_accounts` against
`ctx.pool()` and wires the same adapters (~35 lines).

**`IngestionBroadcast` — RULED: move to `vectors::ingestion`, no trait.**

`IngestionPhase`, `IngestionProgress`, and `IngestionBroadcast` (`api/ingestion.rs:35`, `:59`,
`:76`) move into `vectors/ingestion.rs`. `get_sync_status` needs them and they are binary-only
today.

> **Correction — "move verbatim" does not compile.** Two of the three names are already taken
> at the destination: `vectors/ingestion.rs:72` defines `IngestionPhase` and `:115` defines
> `IngestionProgress`. Moving the api types in under their current names is `E0428`. Only
> `IngestionBroadcast` has no counterpart and is a clean move.
>
> They are genuinely different types serving different layers, not accidental duplicates:
>
> |                     | `api::ingestion`                           | `vectors::ingestion`                              |
> | ------------------- | ------------------------------------------ | ------------------------------------------------- |
> | `IngestionPhase`    | 6 variants, `Serialize + Deserialize`      | 7 variants (adds `Backfilling`), `Serialize` only |
> | `IngestionProgress` | same 9 fields, but `phase: IngestionPhase` | `phase: String`                                   |
>
> `vectors::IngestionProgress` is the pipeline's own already-stringified snapshot;
> `api::IngestionProgress` is the typed SSE broadcast payload.
>
> **Ruling: rename on move, do not unify.** The api types land as `SyncPhase` and
> `SyncProgress` (fields, derives and `Display` unchanged); `IngestionBroadcast` keeps its
> name. `api/ingestion.rs` then re-exports them under the old names:
>
> ```rust
> pub use crate::vectors::ingestion::{
>     IngestionBroadcast, SyncPhase as IngestionPhase, SyncProgress as IngestionProgress,
> };
> ```
>
> This keeps all ~30 references inside `api/ingestion.rs` and its five test call sites
> compiling untouched, with zero behaviour change, and leaves `main.rs:38` / `:440` working
> through the alias.
>
> **Unifying the two would be a silent wire-format regression, and is forbidden here.**
> `vectors::IngestionProgress.phase` is a `String` populated via `job.phase.to_string()`
> (`vectors/ingestion.rs:519`, `:1594`) and the literal `"backfilling"` (`:1411`) — all
> lowercase, because they route through `Display`. But `vectors::IngestionPhase` derives
> `Serialize` with no `#[serde(rename_all)]`, so switching the field to the enum would emit
> `"Syncing"` where the frontend currently receives `"syncing"`. Do not "tidy" these into one
> type as a follow-up without treating it as a breaking API change.
>
> **Supersedes the `events` recommendation in an earlier revision of this section.** That
> recommendation was wrong; the `vectors::ingestion` instruction already circulating in the
> coders' handoff notes is correct and stands. No rework is required — if you were told
> `vectors::ingestion`, build that.

Three findings decide it:

- **`vectors::ingestion` already exists** (`vectors/mod.rs:26`, `pub mod ingestion;`) and
  already owns `IngestionPipeline` (`vectors/mod.rs:84`) — the component that _produces_ these
  progress events. Co-locating the progress types with their producer is the cohesive
  placement; `events` is the generic `EventBus`, and putting an ingestion-specific DTO there
  would mix a domain type into a general-purpose bus.
- **`vectors` is already exposed by `lib.rs`**, so the move needs no new library surface.
- **The types are dependency-free.** `IngestionPhase` is a plain unit enum with a `Display`
  impl; `IngestionProgress` is nine scalar/`Option` fields; `IngestionBroadcast` holds only
  `broadcast::Sender<IngestionProgress>` and `Arc<RwLock<Option<IngestionProgress>>>`. Across
  `api/ingestion.rs:30-140` there is not one reference to `crate::api`, `AppState`, or `super::`
  outside a doc comment. Nothing is dragged along.

**The `SyncProgressSource` trait is withdrawn.** It existed only as a fallback for the case
where the move pulled api-only types with it. It does not, so the trait would be indirection
with no purpose. `ToolContext` holds the moved type directly (§2.2); `IngestionBroadcast` is
`Clone`, so no `Arc` wrapper is needed either.

Call sites to update are minimal: `main.rs:38` (the `AppState` field type) and `main.rs:440`
(construction). Every other reference is inside `api/ingestion.rs`, including its tests, and
follows the type.

### 2. A1 — the unified tool registry

One declaration per tool. Both consumers derive everything from it.

```text
backend/src/tools/
├── mod.rs             ToolSpec, ToolHandler, ToolError, ToolRegistry, dispatch()
├── config.rs          tools.yaml schema, loading, overlay resolution
└── readonly/
    ├── mod.rs         shared validators                      (already scaffolded)
    ├── params.rs      schemars request structs — all tools
    ├── emails.rs      search_emails, get_email, list_recent_emails, count_emails,
    │                  get_email_thread, list_attachments, find_similar_emails
    ├── accounts.rs    list_accounts, get_sync_status
    ├── insights.rs    get_insights, list_rules, list_subscriptions, list_clusters,
    │                  get_learning_metrics
    └── cleanup_preview.rs   preview_cleanup_plan
```

#### 2.1 Core types

```rust
/// A tool, declared exactly once.
pub struct ToolDecl {
    pub name: &'static str,
    pub description: &'static str,
    /// Schema built from the schemars params struct via rmcp's own helper, so the
    /// MCP transport and the chat orchestrator receive identical JSON Schema.
    pub input_schema: serde_json::Value,
    pub handler: ToolHandler,
    /// Every tool in this run is read-only; A4 action tools will set this false.
    pub read_only: bool,
    pub default_rate_limit_per_minute: u32,
    pub default_requires_confirmation: bool,
}

pub type ToolFuture = BoxFuture<'static, Result<serde_json::Value, ToolError>>;
pub type ToolHandler = Arc<dyn Fn(Arc<ToolContext>, serde_json::Value) -> ToolFuture + Send + Sync>;

pub enum ToolError {
    /// The requested entity does not exist.        → MCP resource_not_found
    NotFound(String),
    /// Caller-supplied arguments failed validation.
    Invalid(String),
    /// A query or backing service failed.          → MCP internal_error
    Database(String),
    /// The backing service is absent in this deployment (see §2.2).
    NotConfigured(String),
    /// Disabled, unknown, or over its rate limit.
    Denied(String),
}
```

The error enum is deliberately richer than a single `Internal` variant: the A5 resource layer
must map not-found to `resource_not_found` and failure to `internal_error`, and today
`mcp/server.rs:184-191` returns both as indistinguishable success strings so that mapping is
impossible.

#### 2.1.1 Two layers per tool

Each tool decomposes into a **fetch layer** and a **declaration layer**. This is load-bearing,
not stylistic: A5 resources (`email://{id}`, `thread://{key}`, `insights://summary`) are
resource reads, not tool calls, so they cannot take `Parameters`/`ToolCallContext` or consume a
pre-serialized JSON string. Binding them to the fetch layer is what lets them share the SQL
instead of duplicating it.

```rust
// Fetch layer — typed in, typed out, rich errors, NO rmcp types. A5 binds HERE.
pub async fn get_email(ctx: &ToolContext, id: &str) -> Result<EmailRecord, ToolError>;

// Declaration layer — what the registry holds. Deserializes, calls fetch, serializes.
pub async fn handler(ctx: Arc<ToolContext>, req: GetEmailRequest) -> Result<Value, ToolError>;
```

`get_email_thread` splits accordingly into `resolve_thread_key(ctx, email_id)` and
`fetch_thread_by_key(ctx, key)`. The tool composes both (unchanged behaviour); the
`thread://{key}` resource calls the second directly, avoiding a pointless extra lookup, since
that URI carries the thread key rather than an email id.

The declaration table is one function:

```rust
pub fn all_specs() -> Vec<ToolSpec> { /* 15 entries: 7 existing + 8 new */ }
```

#### 2.2 The registry

```rust
pub struct ToolRegistry {
    entries: BTreeMap<&'static str, RegisteredTool>,   // enabled tools only
    rate_limiter: ToolRateLimiter,
    confirmation_required: Vec<String>,
}

pub struct RegisteredTool {
    pub spec: ToolSpec,
    pub requires_confirmation: bool,
    pub rate_limit_per_minute: u32,
    /// Materialized once at startup; cloned into the ToolRouter and converted
    /// for the chat orchestrator.
    pub tool: rmcp::model::Tool,
}

impl ToolRegistry {
    pub fn build(cfg: &ToolsConfig) -> Self;
    pub fn mcp_tools(&self) -> Vec<rmcp::model::Tool>;
    pub fn chat_definitions(&self) -> Vec<ToolDefinition>;
    pub fn confirmation_required(&self) -> &[String];
    pub async fn dispatch(
        &self, state: &Arc<AppState>, name: &str,
        args: serde_json::Value, source: CallSource,
    ) -> Result<serde_json::Value, ToolError>;
}

#[derive(Clone, Copy)]
pub enum CallSource { Mcp, Chat }
```

Disabled tools are dropped at build time rather than registered-then-hidden. One filter, one
source of truth, and no way for `mcp_tools()` and `chat_definitions()` to disagree.

#### 2.2 `ToolContext` — the handler's view of the world

Handlers do **not** take `AppState`. They take a narrow `ToolContext`, for a reason that is a
compile constraint rather than a preference — see §1.5.

`ToolContext` is the union of what all fifteen tools actually need. Nine distinct `AppState`
paths are used across the tool set, but they collapse into five fields, because
`VectorService` is a single `Arc` carrying five of them:

```rust
#[derive(Clone)]
pub struct ToolContext {
    pub db: Arc<Database>,                                  // required — all 15 tools
    pub vectors: Option<Arc<VectorService>>,                // hybrid_search, store,
                                                            // cluster_engine,
                                                            // ingestion_pipeline,
                                                            // learning_engine
    pub oauth: Option<Arc<OAuthManager>>,                   // list_accounts, get_sync_status
    pub poll_scheduler: Option<PollSchedulerHandle>,        // get_sync_status
    pub sync_progress: Option<IngestionBroadcast>,          // get_sync_status — §1.5.1
}
```

Five fields against `AppState`'s seventeen. Everything except `db` is `Option`: a tool whose
backing service is absent returns `ToolError::NotConfigured` rather than pretending. That
keeps `ToolContext::new(db)` cheap enough for tests — six of the fifteen tools work with no
service construction at all, eleven with a vectors stub — without any tool lying about what it
can do.

The binary builds one via `From<&AppState>`; that impl stays bin-side, so the library never
learns `AppState` exists.

#### 2.3 Placement and ownership

The registry is built **once at startup** in `main.rs` from the loaded `tools.yaml`, and owned
by shared state:

```rust
pub struct AppState {
    // ...
    pub tools: Arc<tools::ToolRegistry>,
}
```

`EmailibriumMcpServer` clones that `Arc`; its private `rate_limiter` field is removed.

**This fixes a real bug, not just a layering wart.** Today `main.rs:707-712` mounts the
transport with a per-session factory closure, so a fresh `EmailibriumMcpServer` is constructed
for every MCP connection, and `mcp/server.rs:48` builds `ToolRateLimiter::new(20)` inline
inside it. Consequences: a client resets its own rate limit simply by reconnecting; the chat
path has no limiter at all; and the two paths could never share a window even in principle,
because the limiter was unreachable from both. Hoisting the `Arc` into `AppState` is what makes
goal 5 true rather than nominal.

The limiter must also be **injectable** — `ToolRateLimiter::new(20)` is hardcoded and all seven
call sites pass `None` as the per-tool override, so `rate_limit.rs`'s override path is dead
from outside and untestable. The registry takes the limiter (or its config) so `tools.yaml`
overrides and tests both work.

---

### 3. Unified enforcement

`ToolRegistry::dispatch()` is the sole entry point for executing a tool, from either path. It
runs, in order:

1. **Lookup** — absent name → `UnknownTool` (a disabled tool is indistinguishable from an
   unknown one, which is the correct posture: never advertise what is turned off).
2. **Rate limit** — the per-tool resolved limit → `RateLimited`.
3. **Handler** — the adapter deserializes and validates, then runs.
4. **Audit** — always, on every outcome, including denials.

Audit rows record `source` so transport calls are distinguishable from in-process chat calls.
That needs one new column:

```sql
-- backend/migrations/029_mcp_tool_audit_source.sql
ALTER TABLE mcp_tool_audit ADD COLUMN source TEXT NOT NULL DEFAULT 'mcp';
```

The default keeps existing rows valid and means an un-migrated read still parses.

Both callers become thin:

- **MCP** — the `ToolRoute::new_dyn` closure reads `ctx.arguments`, calls
  `ctx.service.state.tools.dispatch(..., CallSource::Mcp)`, and wraps the result in
  `CallToolResult`.
- **Chat** — `build_tool_executor()` in `ai.rs` collapses from ~215 lines to roughly five,
  calling the same `dispatch(..., CallSource::Chat)`.

Sharing one rate limiter across both paths is deliberate. The limit protects a backend
resource, not a transport, so a chat-driven tool storm and an MCP-driven one should draw on
the same budget.

#### 3.1 The MCP server after the change

```rust
#[derive(Clone)]
pub struct EmailibriumMcpServer {
    tool_router: ToolRouter<Self>,     // now load-bearing
    prompt_router: PromptRouter<Self>, // A5
    state: Arc<AppState>,
}

impl EmailibriumMcpServer {
    pub fn new(state: Arc<AppState>) -> Self {
        let tool_router = build_tool_router(&state.tools);
        Self { tool_router, prompt_router: Self::prompt_router(), state }
    }
}

#[tool_handler(router = self.tool_router)]      // NOT the default — see §1
#[prompt_handler(router = self.prompt_router)]
impl ServerHandler for EmailibriumMcpServer {
    fn get_info(&self) -> ServerInfo { /* enable_tools + resources + prompts */ }
    // resources implemented by hand — §7.1
}
```

`build_tool_router` iterates the registry and pushes one `ToolRoute::new_dyn` per entry,
capturing the tool's `&'static str` name. All seven `#[tool]` method bodies move out of
`server.rs` into `tools/readonly/`; `#[tool_router]` and `#[tool]` disappear from the file.

---

### 4. `config/tools.yaml`

#### 4.1 Schema

```yaml
version: '1.0'

defaults:
  rate_limit_per_minute: 20 # applies when a tool declares no default of its own
  timeout_ms: 10000 # reserved; not yet enforced (see note)

tools:
  <tool_name>:
    enabled: true # default true
    requires_confirmation: false # default: the tool's own declared default
    rate_limit_per_minute: 30 # default: tool default, else defaults.rate_limit_per_minute
```

`timeout_ms` is parsed and exposed but **not yet enforced** — the orchestrator already applies
its own `tool_timeout_ms`, and adding a second timeout inside `dispatch` is deferred rather
than half-implemented.

#### 4.2 Rust types (`backend/src/tools/config.rs`)

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)] pub version: String,
    #[serde(default)] pub defaults: ToolDefaults,
    #[serde(default)] pub tools: BTreeMap<String, ToolOverride>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolDefaults {          // Default: 20 / 10_000
    #[serde(default = "default_rate_limit")] pub rate_limit_per_minute: u32,
    #[serde(default = "default_timeout_ms")] pub timeout_ms: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolOverride {
    pub enabled: Option<bool>,
    pub requires_confirmation: Option<bool>,
    pub rate_limit_per_minute: Option<u32>,
}
```

Every field is optional so that a partial entry means "override just this".

#### 4.3 Loading

`vectors::yaml_config::load_yaml_config(config_dir)` already loads six files through
`load_file<T: Default + DeserializeOwned>(dir, filename)`, which falls back to `T::default()`
on a missing or unparseable file. Adding `tools.yaml` as the seventh is a three-line change
that inherits the established behaviour: **a missing file means every tool runs at its
declared defaults**, which is the desired failure mode.

#### 4.4 Resolution rules

For each spec in `all_specs()`, look up `cfg.tools.get(spec.name)`:

| Field                   | Resolution                                                                              |
| ----------------------- | --------------------------------------------------------------------------------------- |
| `enabled`               | `override.enabled` ?? `true`                                                            |
| `requires_confirmation` | `override.requires_confirmation` ?? `spec.default_requires_confirmation`                |
| `rate_limit_per_minute` | `override.rate_limit_per_minute` ?? `spec.default_rate_limit_per_minute` ?? `defaults.` |

Keys in `cfg.tools` with no matching spec are handled in two classes:

```rust
/// A4 action tools: declared in tools.yaml, not yet implemented. Known-absent,
/// so they must not produce startup warnings.
const DEFERRED_TOOLS: &[&str] = &["send_email", "delete_email", "create_rule"];
```

- name in `DEFERRED_TOOLS` → `debug!` "deferred to A4"
- otherwise → `warn!(tool = %name, "tools.yaml references unknown tool; ignoring")`

Unknown _fields_ inside an entry are ignored (no `deny_unknown_fields`) — a hard startup
failure on a stray YAML key is too hostile for an operator-edited file.

Startup emits one summary line: `N tools registered, M disabled by config`.

#### 4.5 Confirmation wiring

`OrchestratorConfig::default()` hardcodes
`require_confirmation = ["send_email", "delete_email", "create_rule"]`
(`chat_orchestrator.rs:57`). The `Default` impl stays (tests depend on it), but the production
construction site in `ai.rs` switches to `registry.confirmation_required()`, so the list comes
from config. With A4 out of scope, that list resolves empty today — correct, since no
confirmation-requiring tool is currently registered.

---

### 5. A3 — eight new read-only tools

All backing functions below were verified against the source. Every tool reaches its data
through `AppState` fields that are **already public**, so — with one exception — no visibility
changes are required.

> **Visibility:** the private `api::{clustering, learning, vectors}` modules are deliberately
> _not_ opened up — the tools reach the services through `ToolContext` instead. The only
> module moves are the ones §1.5.1 requires (`cleanup::domain`, `cleanup::repository`, and the
> ingestion-progress types).

#### 5.1 Tool specifications

**`list_accounts`** — list connected email accounts and their sync state.

- Params: none.
- Backing: `state.oauth_manager.list_accounts()` (`email/oauth.rs:550`), then
  `get_sync_state(&id)` (`email/oauth.rs:681`) per account. Mirrors `AccountResponse`
  (`api/accounts.rs:52`); `is_active` is derived as `status == Connected`.
- Output: `{ count, accounts: [{ id, provider, email_address, status, is_active,
last_sync_at, emails_synced, sync_depth, sync_frequency }] }`
- Rate limit **20/min**. Note: `list_accounts()` applies no status filter, so disconnected
  accounts appear — surface `status` prominently so the model does not assume otherwise.

**`get_sync_status`** — current ingestion, poll, and per-account lock state.

- Params: `account_id: Option<String>` — validated as a UUID when present.
- Backing (all cheap, all already factored):
  `state.vector_service.ingestion_pipeline.get_progress()`, falling back to
  `state.ingestion_broadcast.last_progress()` (the two-tier logic inlined at
  `api/ingestion.rs:1317`); `state.poll_scheduler.status()`
  (`email/poll_scheduler.rs:86`); and, when `account_id` is given,
  `state.pipeline_locks.get_activity(id)` (`sync_lock.rs:17`).
- Output: `{ active, phase, job_id, total, processed, failed, eta_seconds, poll: {...},
lock: {...} | null }`
- Rate limit **30/min**.
- **Deliberately excludes** the `embedding_status` handler (`api/ingestion.rs:1222`). Its query
  projects `COALESCE(embedding_status,'pending')` but groups by the raw column, so a `NULL`
  group collides with the real `'pending'` group and the counts overwrite rather than
  accumulate (`:1244-1247`). Do not propagate that bug into a tool — file it as a separate fix.

**`list_subscriptions`** — detected mailing-list/newsletter subscriptions.

- Params: `limit: Option<u32>` (default 50, max 200), `category: Option<String>` (one of
  `newsletter|marketing|notification|receipt|social|unknown`), `min_email_count: Option<u32>`.
- Backing: `InsightEngine::new(state.db.clone(), state.vector_service.store.clone())` then
  `detect_subscriptions()` (`vectors/insights.rs:190`). Returns `SubscriptionInsight`
  (`insights.rs:110`). Filtering and truncation happen in the tool, after the engine returns.
- Rate limit **5/min** — deliberately low. The engine issues three queries per distinct sender
  with ≥3 emails, and one of them (`insights.rs:233`) selects full `body_text` for every such
  email. This is the most expensive tool in the set; say so in its description.

**`preview_cleanup_plan`** — build a cleanup plan **in memory only**. Strictly dry-run.

- Params: `user_id: String` (validated via `validate_user_id`), the `WizardSelections` shape
  (`cleanup/domain/plan.rs:48`), and `sample_limit: Option<u32>` (default 10, max 50).
- Backing: `PlanBuilder::build(&user_id, selections)` (`cleanup/domain/builder.rs:49`) via the
  library-side wiring helper `plan_builder(pool)` at
  `tools/readonly/cleanup_preview.rs:87` (§1.5.1).

  > **Superseded — no visibility change is needed here.** An earlier revision of this section
  > proposed making `cleanup::api::plan::build_plan_builder` (`cleanup/api/plan.rs:79`)
  > `pub(crate)`. That plan was dropped, because that function takes `&AppState` and so cannot
  > be called from a library-crate handler. The library-side twin above replaces it. This note
  > exists only because the superseded plan keeps resurfacing from stale handoff notes — if you
  > arrived here looking for the `pub(crate)` change, this is the answer.

- **Read-only verification.** `build()` is pure. Its module doc states "Pure orchestrator:
  reads only injected ports, never touches a provider," and a search across `domain/builder.rs`,
  `domain/classifier.rs`, and `repository/adapters.rs` finds no `INSERT`/`UPDATE`/`DELETE` — the
  only `insert` calls are `BTreeMap`/`HashSet::insert`. All seven SQLx adapters are `SELECT`,
  and the rule port evaluates with `RuleExecutionMode::EvaluateOnly` (`builder.rs:202`).
  Persistence happens **outside** the builder, at `state.cleanup_plan_repo.save(&plan)`
  (`cleanup/api/plan.rs:168`). **This tool must never call `save()`,** nor any of `cancel`,
  `expire_due`, `purge_older_than`, `replace_account_rows`, `update_predicate_status`, or
  `ApplyOrchestrator::begin_apply`.
- Output: the `CleanupPlanSummary` shape (`plan.rs:180`) plus at most `sample_limit`
  operations — never the full `operations` vector, which can be enormous. The payload
  **must** include `"dry_run": true` and `"persisted": false` so a model cannot mistake the
  preview for a committed plan. Note in the description that the returned plan id is
  ephemeral and will not resolve via `GET /plan/:id`.
- Rate limit **5/min**.

**`find_similar_emails`** — nearest neighbours of an email by embedding.

- Params: `email_id: String` (validated), `limit: Option<u32>` (default 10, max 50),
  `min_score: Option<f32>` (default 0.5, clamped to `0.0..=1.0`).
- Backing: `state.vector_service.store.get_by_email_id(id)` then `.search(&SearchParams{..})`
  (`vectors/store.rs:33,39`), filtering the source id out of the results — the logic inlined at
  `api/vectors.rs:353-395`.
- Rate limit **30/min**.
- This is the correct similarity primitive precisely because, unlike `semantic_search` and
  `hybrid_search`, `find_similar` does **not** call `interaction_tracker.record_search(...)`
  — so it is genuinely side-effect free. (Also avoid `quantization_status`,
  `api/vectors.rs:492`, which despite reading like a GET writes and deletes a probe document.)
  The hardcoded `min_score: 0.5` at `api/vectors.rs:380` becomes a parameter here.

**`list_clusters`** — topic clusters with representative emails.

- Params: `limit: Option<u32>` (default 20, max 100), `include_representatives: Option<bool>`
  (default true).
- Backing: `state.vector_service.cluster_engine.get_clusters()`
  (`vectors/clustering.rs:1478`) — in-memory, no SQL. Representative-email metadata comes from
  the single `SELECT id, subject, from_addr, from_name FROM emails WHERE id IN (...)` at
  `api/clustering.rs:114`, run through `crate::db::audited_sql`.
- Output mirrors `ClusterSummary` (`api/clustering.rs:59`), omitting the centroid vector.
- Rate limit **20/min**.

**`get_learning_metrics`** — relevance-learning counters.

- Params: none.
- Backing: `state.vector_service.learning_engine.get_metrics()` (`vectors/learning.rs:857`).
  Pure in-memory; the learning module issues zero SQL.
- Output mirrors `MetricsResponse` (`api/learning.rs:63`): `total_feedback`, `rank1_clicks`,
  `total_clicks`, `centroid_drift`, `ab_control_queries`, `ab_sona_queries`.
- Rate limit **30/min**. The description must state that these counters are process-local and
  reset on restart, so a model does not present them as historical totals.

**`list_attachments`** — attachment metadata for one email.

- Params: `email_id: String` (validated), `include_inline: Option<bool>` (default false).
- Backing: the two `SELECT`s at `api/attachments.rs:233-239`, differing only by the
  `AND is_inline = FALSE` clause. Output mirrors `AttachmentResponse`
  (`api/attachments.rs:47`).
- **Must not** touch `lazy_fetch_attachment` (`api/attachments.rs:107`), which writes files to
  `data/attachments/...` and runs `UPDATE attachments SET fetch_status='fetched'`. Metadata
  only — no content, no download.
- Rate limit **30/min**.

#### 5.2 Validation rules

`tools/readonly/mod.rs` (already scaffolded) holds the shared validators: `validate_id`
(non-empty, ≤200 chars), `validate_uuid`, `validate_user_id` (`[A-Za-z0-9_-]` only),
`validate_limit` (clamp into `1..=max`). Every free-form string argument goes through one of
them before reaching a query. `validate_date` moves over from `mcp/tools/email.rs`.

Bounds are enforced in the handler, not the schema — a schema `maximum` is advisory to the
model, whereas clamping is binding.

#### 5.3 Rate-limit summary

| Tool                                                                                                | Limit/min | Rationale                           |
| --------------------------------------------------------------------------------------------------- | --------- | ----------------------------------- |
| `search_emails`                                                                                     | 30        | preserved from `tools.yaml`         |
| `get_email`, `list_recent_emails`, `count_emails`, `get_email_thread`, `get_insights`, `list_rules` | 20        | preserved (global default)          |
| `find_similar_emails`, `list_attachments`, `get_sync_status`, `get_learning_metrics`                | 30        | single indexed read or in-memory    |
| `list_accounts`, `list_clusters`                                                                    | 20        | small fan-out                       |
| `list_subscriptions`, `preview_cleanup_plan`                                                        | 5         | multi-query fan-out over the corpus |

#### 5.4 Preserving the seven existing tools

Names are frozen: `search_emails`, `get_email`, `list_recent_emails`, `count_emails`,
`get_insights`, `list_rules`, `get_email_thread`.

**Descriptions** come from `mcp/server.rs`, not `ai.rs`. The MCP strings are authoritative
(they already ship to real MCP clients); `ai.rs` had shortened `count_emails` and
`get_email_thread`. Chat gains the fuller text.

**Schemas** come from the schemars structs via `schema_for_type`, matching what MCP clients
receive today. The hand-written schemas in `ai.rs` are discarded. They differ in small ways
from the generated ones, so §8 mandates a test pinning the `required` set and property names
of all seven — making the change deliberate and visible rather than incidental.

**Result payloads** come from `mcp/server.rs`, which are the richer versions — chat's
`search_emails` gains the `latency_ms` field it was dropping.

Two deliberate behaviour changes, both minor:

1. **Error results are flagged.** Today a failing MCP tool returns `{"error": "..."}` inside a
   _successful_ `CallToolResult`. The dispatch path will keep that same JSON body but also set
   `is_error: true`, which is what the MCP spec intends. Clients that ignore the flag see
   identical text.
2. **Chat calls are now rate-limited and audited.** That is the point of the change, but it is
   observable: a chat session making many tool calls can now be throttled, and
   `mcp_tool_audit` will grow rows with `source = 'chat'`.

---

### 6. What gets deleted

| Location                                              | Change                                                                                                                |
| ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `api/ai.rs:1199` `build_tool_definitions`             | Deleted — replaced by `registry.chat_definitions()`                                                                   |
| `api/ai.rs:1275` `build_tool_executor`                | Reduced to a ~5-line closure over `registry.dispatch()`                                                               |
| `mcp/server.rs` `#[tool_router]` + 7 `#[tool]` bodies | Moved to `tools/readonly/`                                                                                            |
| `mcp/server.rs` `rate_limiter` field                  | Moved to the registry                                                                                                 |
| `mcp/server.rs` `audit()` method                      | Moved into `ToolRegistry::dispatch`                                                                                   |
| `mcp/tools/email.rs`                                  | Params structs move to `tools/readonly/params.rs`; row structs to `tools/readonly/emails.rs`; `mcp/tools/` is removed |

Net: roughly 215 duplicated lines in `ai.rs` and ~430 in `mcp/server.rs` collapse into one
declaration table plus handler modules.

---

### 7. A5 — resources and prompts

#### 7.1 Resources (hand-implemented — no router exists)

`handler/server/resource.rs` is empty in rmcp 1.7, so these three `ServerHandler` methods are
implemented directly on `EmailibriumMcpServer`:

```rust
fn list_resources(&self, _: Option<PaginatedRequestParams>, _: RequestContext<RoleServer>)
    -> impl Future<Output = Result<ListResourcesResult, McpError>>;
fn list_resource_templates(&self, ..) -> .. ListResourceTemplatesResult ..;
fn read_resource(&self, params: ReadResourceRequestParams, ..) -> .. ReadResourceResult ..;
```

| URI                  | Kind     | Backing                                                               |
| -------------------- | -------- | --------------------------------------------------------------------- |
| `insights://summary` | concrete | reuses the `get_insights` handler                                     |
| `email://{id}`       | template | reuses the `get_email` handler                                        |
| `thread://{key}`     | template | `SELECT ... WHERE thread_key = ?` — takes the thread key **directly** |

`list_resources` returns only `insights://summary`; the two parameterized entries belong in
`list_resource_templates` as `RawResourceTemplate::new(uri_template, name)`.

Note the `thread://` asymmetry: the existing `get_email_thread` **tool** takes an _email id_
and resolves its `thread_key` internally, whereas the `thread://{key}` **resource** is keyed
by the thread key itself. Both are correct for their surface; the difference must be
documented so the two are not confused.

`read_resource` dispatches on the URI scheme, validates the extracted identifier with the
same validators, and routes through `registry.dispatch()` so resources inherit rate limiting
and audit for free. Audit rows use `tool_name = "resource:email"` and similar, keeping them
distinguishable from tool calls.

Build contents explicitly rather than via the convenience constructor, to get a real MIME
type (see §1):

```rust
ResourceContents::TextResourceContents {
    uri, mime_type: Some("application/json".into()), text: payload, meta: None,
}
```

Capabilities: add `.enable_resources()` to the hand-written `get_info()`. Subscriptions and
list-changed notifications are **not** enabled — nothing pushes updates yet.

#### 7.2 Prompts (macro path)

Prompts have a proper router, are not config-driven, and return near-static text, so the macro
path is the smallest correct implementation: `#[prompt_router]` on an inherent impl,
`#[prompt]` per prompt, and `#[prompt_handler(router = self.prompt_router)]` on the
`ServerHandler` impl.

| Prompt          | Arguments                      | Content                                                                                              |
| --------------- | ------------------------------ | ---------------------------------------------------------------------------------------------------- |
| `triage-inbox`  | `limit` (optional, default 20) | Instructs the model to call `list_recent_emails`, then categorize by urgency and propose actions.    |
| `weekly-report` | none                           | Instructs the model to call `get_insights` and `list_subscriptions`, then produce a written summary. |

Both return a `GetPromptResult` holding a single user-role `PromptMessage::new_text`.
Because our `get_info()` is hand-written, `.enable_prompts()` must be added to it manually —
the macro's auto-generated capability merge does not run (§1).

Prompt text is defined inline in `backend/src/mcp/prompts.rs` for this run. Moving it into
`config/prompts.yaml`, alongside the `chat_assistant_tools` entry ADR-028 §4.7 anticipates, is
a follow-up.

---

### 8. Integration-test approach

#### 8.1 What the crate topology change buys

Before §1.5, none of this was possible: the MCP server was binary-only and unreachable from
`backend/tests/`. With `tools`, `mcp`, and `mcp_service(ctx)` exposed via the library and
handlers taking `ToolContext`, integration tests can drive **the same constructor production
uses** — which is the whole point. `tests/api_integration.rs` validates a hand-copied
reimplementation of the handlers; the MCP tests must not repeat that mistake.

Tests are tiered by what they need to construct, cheapest first.

#### 8.2 Tier 1 — registry metadata (no context at all)

In `backend/src/tools/mod.rs`. This tier is where the single-source-of-truth property is
actually **proved**, and it is the most valuable tier:

- every spec name is unique; the full name set equals the frozen list of fifteen.
- the seven pre-existing tools carry their exact pre-change names and descriptions — asserted
  against string literals copied verbatim from today's `mcp/server.rs`.
- **the key test:** `registry.mcp_tools()` and `registry.chat_definitions()` have equal length,
  identical name sets, and for every name the two input schemas are equal after `Value`
  conversion. This makes the `ai.rs`/`server.rs` drift structurally impossible to reintroduce.
- schema pinning: for each of the seven, assert the `required` array and property-name set, so
  a schemars upgrade cannot silently change a published wire contract.

#### 8.3 Tier 2 — config resolution (pure)

In `backend/src/tools/config.rs`, over inline YAML strings:

- absent file → every tool at its declared defaults;
- `enabled: false` removes the tool from **both** `mcp_tools()` and `chat_definitions()`;
- a per-tool `rate_limit_per_minute` beats `defaults.rate_limit_per_minute`;
- an unknown tool name warns without failing startup;
- the three `DEFERRED_TOOLS` names produce no warning;
- `requires_confirmation` flows through to `confirmation_required()`.

#### 8.4 Tier 3 — enforcement (needs only a `SqlitePool`)

`audit::log_tool_call` takes `&SqlitePool`, not `AppState` (`mcp/audit.rs:19`), so this tier
needs no application state. Reuse the established harness from `api_integration.rs:491-510` —
`SqlitePoolOptions` plus `include_str!("../migrations/NNN.sql")` executed in order — adding
`022_mcp_tool_audit.sql` and the new `029`. `tempfile` is already a dev-dependency if a
file-backed database is preferred over in-memory.

To make this testable without `AppState`, split the pre-flight out of `dispatch`:

```rust
/// Lookup + rate-limit check. No AppState, no handler execution — directly testable.
pub fn preflight(&self, name: &str) -> Result<&RegisteredTool, ToolError>;
```

`dispatch` then becomes `preflight` → handler → audit. Tests assert:

- a rate-limited call returns `RateLimited` and writes an audit row with
  `result_status = 'denied'`, without invoking the handler;
- a disabled tool is indistinguishable from an unknown one;
- a successful call writes `'success'` with a 64-character SHA-256 hash;
- the `source` column correctly distinguishes `'mcp'` from `'chat'`.

#### 8.5 Tier 4 — HTTP-level MCP integration test (`backend/tests/mcp_integration.rs`)

Now genuinely buildable thanks to §1.5, and worth building: it is the only tier that proves the
mounted transport works end to end.

- Construct a `ToolContext` over a migrated pool, build the registry, and mount through the
  shared `mcp_service(ctx)` constructor — **the same one `main.rs` calls**. Keep the production
  mount at its default stateful + SSE configuration so the test exercises what ships.
- `StreamableHttpService` implements `tower_service::Service<Request<B>>`
  (`transport/streamable_http_server/tower.rs:520`), so `tower::ServiceExt::oneshot` drives it
  exactly as `api_integration.rs` drives its router today.
- **There is no in-tree MCP client** — `rmcp` is built without the `client` feature
  (`Cargo.toml:100`). Raw JSON-RPC over `oneshot` is the path, and needs no new dependency.
- Wire sequence: `POST /api/v1/mcp` with `Accept: application/json, text/event-stream` and an
  `initialize` body → capture the `Mcp-Session-Id` response header → `POST`
  `notifications/initialized` → then `tools/list` and `tools/call`, echoing the session header
  on every request. Responses are SSE-framed and need decoding before JSON parsing.

The highest-value assertion here is cross-path: **`tools/list` over the wire must equal the
chat orchestrator's `chat_definitions()`**. That single test locks the two paths together
permanently and is the strongest available guard against the `ai.rs:1199` duplication silently
returning.

#### 8.6 Handler correctness

Individual handler behaviour (SQL shape, payload fields) is covered by Tier 3-style tests
against a migrated pool for the handlers that only need `state.db.pool`, and left to manual
verification for those requiring `VectorService` or `ClusterEngine`. `preview_cleanup_plan`
deserves one dedicated assertion: after invoking it, `SELECT COUNT(*) FROM cleanup_plans` must
be unchanged — the read-only guarantee, enforced by test rather than by comment.

---

### 9. Ownership of the single `impl ServerHandler` block

`#[tool_handler]` and `#[prompt_handler]` must sit on the **same** `impl ServerHandler` block —
they stack correctly and both skip `get_info()` when it is hand-written
(`rmcp-macros-1.7.0/src/prompt_handler.rs:100-103`) — and Rust permits only one such block per
type. That block (`mcp/server.rs:510-518`) is therefore shared territory between the registry
work and A5.

**Arbitrated split:** A5 owns `get_info()` (so the capability chain
`.enable_tools().enable_resources().enable_prompts()` lives in one place) plus the appended
resource and prompt methods; the registry work owns everything else in the file. Only one agent
edits the block; the other supplies the attribute line and delegate signatures.

---

### 10. Transport modes: HTTP and stdio

The backend serves the **same** `EmailibriumMcpServer` over two transports. This is a genuine
capability addition: spawn-based MCP clients (Claude Desktop and similar) speak stdio, not
HTTP, and cannot edit config files — they pass argv and environment.

#### 10.1 Mode as a first-class config value

Mode is an enum resolved through the existing figment chain, **not** an ad-hoc argv check:

| Precedence | Source                 | Status                                                                                                                                                             |
| ---------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1 highest  | CLI flag               | **Shipped this cycle** — e.g. `--mcp-stdio`                                                                                                                        |
| 2          | `EMAILIBRIUM_MCP_MODE` | **Shipped this cycle**                                                                                                                                             |
| 3          | config file            | ⚠️ **Designed, NOT implemented this cycle** — `config.yaml` does not exist; env + flag are the only shipped sources. Wiring a mode key here silently does nothing. |
| 4 lowest   | default                | **Shipped this cycle** — `http`, zero change for existing users                                                                                                    |

The table above is the full intended design; only rows 1, 2, and 4 exist in code. Row 3 carries
its caveat inline deliberately, because the failure shape is the worst kind: a reader who wires
a config key from the table alone gets silent nothing, with no error to diagnose.

**Why row 3 is deferred.** `config.yaml` does not exist, so implementing the tier would mean
creating a file solely to host one key — and spawn-based clients, the entire reason stdio
exists, pass argv and environment rather than editing files. Adding it later is purely
additive: it slots between env and default without disturbing either, so this is a scope
decision rather than a design one. User-facing documentation should describe exactly two
sources.

Three config facts must be encoded rather than assumed, and together they are why the file
tier is not worth its cost yet:

- **The file tier has no file.** The figment chain (`vectors/config.rs:99-104`) reads
  `config.yaml` then `config.local.yaml` at the repo root; **neither exists**, and figment
  treats a missing `Yaml::file` as a no-op. Today the mode resolves from env or default only.
- **`config.yaml` ≠ `config/app.yaml`.** The latter exists and is committed, but is consumed by
  a separate `apply_yaml_path_defaults` pass that figment never reads. A mode key placed there
  is silently ignored — an easy and near-undebuggable mistake.
- **Keep the key single-word.** Figment's `.split("_")` maps `EMAILIBRIUM_MCP_MODE` to
  `mcp.mode` correctly, but any multi-word field under `mcp` becomes unreachable from the
  environment (`rate_limit_per_minute` → `mcp.rate.limit.per.minute`, a path that does not
  exist), and figment fails silently rather than erroring.

#### 10.2 Logging isolation — a correctness requirement, not hygiene

In stdio mode **stdout is the JSON-RPC channel**. The console tracing layer
(`main.rs:82`, inside the `tracing_subscriber::registry()` block at `main.rs:80-89`) currently
defaults to stdout with ANSI enabled, which would interleave log lines and escape codes into
the protocol stream and surface as intermittent, hard-to-diagnose parse errors.

The logging profile must therefore be derived from the **resolved mode, before tracing
initialisation**:

| Mode    | Console layer | ANSI | HTTP listener     |
| ------- | ------------- | ---- | ----------------- |
| `http`  | stdout        | on   | binds (unchanged) |
| `stdio` | **stderr**    | off  | **does not bind** |

**`main()` must be reordered — the current structure makes this impossible as written.**
Today tracing is initialised at `main.rs:89` (the console layer is
`fmt::layer().with_ansi(true)` at `:82`, defaulting to stdout), and argv is not read until
`main.rs:92`. Mode resolution therefore happens _after_ the logging profile is already fixed.
Implementing stdio requires hoisting mode resolution above the `tracing_subscriber::registry()`
block at `main.rs:80-89`, ahead of the existing `--download-model` / `--download-models` /
`--verify-models` CLI branches. This is a small change but not an optional one: leave the order
as it is and stdio mode emits ANSI-coloured log lines onto the JSON-RPC channel before anything
has a chance to redirect them.

In stdio mode the process is a pure MCP server over stdin/stdout. The repository's 40
executable stdout print sites all live on CLI subcommand paths that return before serving, and
must stay off the stdio serving path; anything writing to stdout there is a defect. (A naive
grep reports 44 and a textual match reports 41 — see the note under Rollout for why those
numbers differ and which one to cite.)

#### 10.3 The rmcp stdio API (verified in crate source)

```rust
let running = server.serve(rmcp::transport::io::stdio()).await?;  // ServiceExt::serve, service.rs:167
running.waiting().await?;                                          // service.rs:545
```

`stdio()` returns `(tokio::io::Stdin, tokio::io::Stdout)` (`transport/io.rs`), and `(R, W)`
implements `IntoTransport` where `R: AsyncRead + Send + Unpin` and `W: AsyncWrite + Send +
Unpin` (`transport/async_rw.rs:24`), so the tuple feeds `serve` directly.

**Cargo gotcha:** `stdio()` is feature-gated — `transport.rs:91-94` is
`#[cfg(feature = "transport-io")]`, and `backend/Cargo.toml:100` currently enables only
`server`, `macros`, `transport-streamable-http-server`. Add `transport-io`, and check whether
it pulls `transport-async-rw` transitively (rmcp `Cargo.toml:129`). Without this the stdio path
fails to compile with an error that reads as a missing module rather than a missing feature.

---

### 11. Authentication stance

**The MCP endpoint remains unauthenticated and is intended for localhost only.** This is a
deliberate, recorded position for this run, not an oversight.

It is defensible only while the bind address is loopback. **Bearer-token authentication is a
hard prerequisite for any non-local bind** — exposing `/api/v1/mcp` on a routable interface
without it would grant unauthenticated read access to the user's entire mailbox, including full
message bodies via `get_email`. The stdio mode does not change this: it inherits the trust model
of the process that spawned it.

This should be carried into the ADR notes alongside ADR-028 §4.9, whose token-forwarding and
permission-filtering design remains unimplemented.

---

### 12. Implementation order

Implementation began before this section was finalised, so the ordering below is a
**reconciliation** sequence rather than a greenfield one. Steps 0.x describe the half-applied
state that must be squared away first; nothing after them compiles until they are.

**Step 0 — reconcile the split contract. ✅ COMPLETE.** Verified against the tree:

| Step | Work                                                                                         | State                                                                                                                           |
| ---- | -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| 0.1  | `main.rs` re-exports the shared modules from the library instead of re-declaring them (§1.5) | ✅ `main.rs:21` — `pub use emailibrium::{…}`; only `mod api;` and `mod cleanup;` remain binary-side, which is correct           |
| 0.2  | Expose `tools`, `mcp`, and `mcp_service(ctx)` via `lib.rs`                                   | ✅ `lib.rs` now exports `mcp`, `tools`, `sync_lock` and the split `cleanup`                                                     |
| 0.3  | Widen `ToolContext` to the fields in §2.2                                                    | ✅ all five present, including `sync_progress: Option<IngestionBroadcast>`                                                      |
| 0.4  | Convert the A3 handlers to `ToolContext` + `Result<_, ToolError>`                            | ✅ done                                                                                                                         |
| 0.5  | Hoist `cleanup::domain` + `repository`; library-side `PlanBuilder` wiring helper             | ✅ both — `lib.rs` carries `pub mod cleanup { pub mod domain; pub mod repository; }`, `api` and `orchestrator` stay binary-side |

The two type identities have collapsed, so `emailibrium::db::Database` and the binary's
`crate::db::Database` are now one type, and `backend/tests/` can reach the shipping code. The
`IngestionBroadcast` move landed as ruled in §1.5.1 (renamed `SyncPhase`/`SyncProgress` at the
destination, re-exported from `api/ingestion.rs` under the old names, zero wire-format change).

**Then the original sequence** — status verified against the tree:

| Step | Work                                                                                            | State                                                                                                                                                                                                            |
| ---- | ----------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1    | `tools/{mod.rs, config.rs}` — registry, dispatch, config types                                  | ✅ **done**                                                                                                                                                                                                      |
| 2    | Move the seven existing handlers into `tools/readonly/`                                         | ✅ **done** — all fifteen declared in `tools/mod.rs::declarations()`; `mcp/server.rs` has zero `#[tool(` macros left                                                                                             |
| 3    | Rewrite `mcp/server.rs` onto `build_tool_router` + `#[tool_handler(router = self.tool_router)]` | ✅ **done** — `build_tool_router` at `mcp/server.rs:60`, `ToolRoute::new_dyn` at `:67`, explicit router attribute at `:126`                                                                                      |
| 4    | Collapse `ai.rs` onto `registry.chat_definitions()` / `registry.dispatch()`                     | ✅ **done** — both builders deleted, `ai.rs` 1619 → 1355 lines                                                                                                                                                   |
| 5    | Migration 029; `source` on the audit path; Tier 3 tests                                         | ✅ **done** — `029_mcp_tool_audit_source.sql` exists; audit moved to `tools/audit.rs` and binds `source` (`:21`, `:37`, `:45`)                                                                                   |
| 6    | Wire the eight A3 tools into the declaration table                                              | ✅ **done** — all eight present in `declarations()`                                                                                                                                                              |
| 7    | A5 resources and prompts                                                                        | ✅ **done** — `mcp/resources.rs` routes reads through registry enforcement; `get_info` (`mcp/server.rs:182-184`) declares `.enable_tools().enable_prompts().enable_resources()`                                  |
| 8    | stdio mode (§10), including the `transport-io` Cargo feature                                    | ✅ **done** — feature added (`Cargo.toml:103`); `resolve_mcp_mode()` runs at `main.rs:164`, ahead of `registry()` at `:188` and `.init()` at `:197`; stdio selects a stderr, ANSI-off console layer (`:180-184`) |

`config/tools.yaml` **is now loaded** — `main.rs:544` calls `tools::config::ToolsConfig::load("../config")`; the `TODO(A1)` is gone.

> ### ✅ Resolved — and the fix eliminated the bug _class_, not just the instance.
>
> `sync_progress` and `pipeline_locks` briefly shipped as permanently `None`: the builders
> existed but `From<&AppState>` never called them. No compile error (the fields are `Option`),
> and no test failure, because unit tests build `ToolContext` via the builders and so exercise
> the populated path the binary never took. **A test that constructs its context differently
> from production cannot detect production's context being wrong.**
>
> The obvious fix was to add the two missing builder calls. What landed instead is better:
> `ToolContext::wired(db, vectors, oauth, ingestion_broadcast, pipeline_locks, poll_scheduler)`
> takes every always-available service as a **required positional argument**, so omitting one
> is now a compile error rather than a silent `None`. `From<&AppState>` (`main.rs:76-92`) calls
> it, and no builder callers remain outside `tools/context.rs`.
>
> Adding the two calls would have fixed these two fields and left the next field added to
> `ToolContext` free to ship as `None` in exactly the same way. Making the omission
> unrepresentable closes the whole class. Worth keeping as the template: when a silent-`None`
> bug appears, ask what makes the omission impossible rather than what makes this instance
> correct.
>
> #### The lint that cannot exist
>
> The natural follow-up — "if this recurs, add a lint" — was investigated and **the answer is
> no.** A sweep with `dead_code` un-suppressed reports zero instances of the
> defined-but-never-constructed class, but that clean result is misleading: **the lint
> structurally cannot see this class for `pub` items in a library crate**, because public API
> is never "dead" to it. Something outside the crate might call it, so the compiler must assume
> something does.
>
> This very file is the proof. `ToolContext::wired` (`tools/context.rs:61`) has exactly one
> caller, `main.rs:83`. But `ToolContext::new` (`:81`) and all five `with_*` builders (`:92`,
> `:97`, `:102`, `:107`, `:112`) have **zero callers anywhere in `src/` or `tests/`** — and
> clippy is silent on all six. The `wired()` refactor that fixed the bug is what orphaned them.
>
> So this class needs periodic grep, not tooling. That is a weaker guarantee than a lint and
> should be treated as one: the two instances found here were both caught by reading the tree,
> and nothing automated would have caught either.
>
> **Deferred decision on the six orphans:** leave them until A6 lands. The test matrix will
> most likely adopt `new()` + the builders as its context-construction API, which is exactly
> what they are now good for — production gets the compile-time safety of `wired()`, tests get
> the ergonomics of incremental construction. If A6 does not use them, they get `#[cfg(test)]`
> gating or deletion at that point. Deciding now would either delete something the tester is
> about to need or entrench something nobody wants.

#### The dominant defect pattern: one concept, two representations, one drifts

Every significant defect this branch produced is the same shape, and naming it is more useful
than the individual fixes:

| #   | Instance                         | The two representations                               |
| --- | -------------------------------- | ----------------------------------------------------- |
| 1   | The original problem             | `ai.rs` hand-written schemas vs the `#[tool]` macros  |
| 2   | Flattened `Denied`/`RateLimited` | one audit status covering two opposite caller actions |
| 3   | `CallSource` on `thread://`      | the enum's documented intent vs the value passed      |
| 4   | `thread://` limiter rejection    | mapped to `Denied` where dispatch maps `RateLimited`  |

**Every one sits at a seam where a special case bypasses the shared path**, and **two of the
four are `thread://` specifically** — because it is the only resource that cannot go through
`ToolRegistry::dispatch` (no tool accepts a thread key), so it hand-rolls the limiter and audit
calls. Each time `dispatch` gains a property, the hand-rolled copy must be updated by hand to
match, and nothing enforces it.

**The exception is not a one-time cost. It is a standing liability that re-accrues every time
`dispatch` gains a property.**

A fifth instance appeared and was **cured rather than patched**, which is the part worth
studying.

`read_thread` originally hand-wrote the audit status `"rate_limited"` to match
`ToolError::status()`, which was **private to the registry** — so `resources.rs` could not call
it and had to duplicate the literal across a privacy boundary. The strings agreed; nothing made
them agree tomorrow. No compile error, no test, and a comment acknowledging the coupling was
the only guard. Textbook instance six-in-waiting.

Three parties spotted it independently — the reviewer, the implementer, and the comment the
implementer had written admitting the coupling.

The fix was not to add a test pinning the two strings together. It was to **make the shared
representation reachable**: `status()` became `pub(crate)` (`tools/registry.rs:89`), and the
audit call site now derives from it — `audit_thread_read(ctx, uri, error.status(), …)`
(`mcp/resources.rs:289`), routed through a `fail_thread_read` helper that funnels every failure
path through the same derivation. The duplicate representation ceased to exist rather than
being kept in sync. Pinned by
`a_throttled_read_is_tagged_rate_limited_in_its_error_payload` (`:591`).

**That is the class-level cure, and it generalises**: when one concept has two
representations, the fix is to delete one — usually by making the canonical one reachable —
not to add a guard that detects them diverging. A test that asserts two literals match is
itself a third representation of the same concept.

Note what made the cure cheap: the duplication existed only because of a visibility choice.
`pub(crate)` was always the right scope; the literal was a workaround for a boundary that
should not have been there. Worth checking first whenever this pattern appears — the shared
form may already exist, one keyword away.

> ### ✅ Step 4 is DONE — the consolidation's goal is met.
>
> `build_tool_definitions()` and `build_tool_executor()` are **deleted**. `api/ai.rs` dropped
> from 1619 to 1355 lines and now builds the orchestrator from the registry:
> `.with_tools(state.tools.chat_definitions())`, `.with_executor(registry_executor(&state))`,
> and `require_confirmation: state.tools.confirmation_required()`. `registry_executor`
> (`api/ai.rs:1207`) dispatches with `CallSource::Chat` (`:1218`), so chat tool calls are now
> rate-limited and audited on the same path as the transport.
>
> There is one source of truth. The §8.2 parity assertion is live and meaningful.

A second gap in the same category: **`config/tools.yaml` is still not loaded.** `tools/config.rs`
implements `ToolsConfig` and its policy resolution, but `main.rs:731` carries
`TODO(A1): load config/tools.yaml here` and passes `ToolsConfig::default()` at `:735`. The
parsing exists; nothing feeds it the file, so §4 is designed and built but not wired.

Steps 1–4 are the consolidation and are independently shippable: a reviewer can verify
`tools/list` output is byte-identical before any new surface area appears.

**Two things easy to leave half-done, worth checking explicitly in review:**

- The rate limiter must be constructed **once** on the registry, not inside
  `EmailibriumMcpServer::new`. Landing the registry while leaving `ToolRateLimiter::new(20)`
  where it is keeps the reconnect-to-reset hole open (§2.3) and would make goal 5 nominal
  rather than real.
- `tools.yaml` must be **reconciled**, not appended to — it currently invents three tools that
  do not exist and omits four that do (§4).

### 13. Open items

- `timeout_ms` in `tools.yaml` is parsed but not enforced in `dispatch` (§4.1).
- The `embedding_status` `GROUP BY` bug (`api/ingestion.rs:1226`) is documented but not fixed
  here (§5.1).
- Prompt text lives in Rust rather than `config/prompts.yaml` (§7.2).
- A4 action tools, their confirmation flow, and the `requires_confirmation` round trip through
  the orchestrator remain unimplemented; the `tools.yaml` entries are retained and
  intentionally silent (§4.4).
- **⚠️ READ THIS BEFORE ADDING A FOURTH RESOURCE, OR BEFORE GIVING `dispatch` A NEW PROPERTY.**
  **Designed next step: retire `thread://`'s hand-rolled path.**

  `thread://` is the only resource that bypasses `ToolRegistry::dispatch`, because no tool
  accepts a thread key. It therefore hand-rolls the limiter and audit calls, and it has
  produced two of the four defects catalogued in §12 — it re-drifts on every axis `dispatch`
  gains.

  **The design:** give `thread://` a dispatchable form — an internal tool that accepts a thread
  key, deliberately **not** listed in `declarations()`, so it is reachable by `dispatch` but
  never advertised in `tools/list`. Every resource then travels one path, the hand-rolled copy
  disappears, and the whole defect class dies rather than being patched instance by instance.

  **Deferred to the follow-up cycle, not this branch.** It is real design work arriving at the
  finish line, the instance-level fixes have landed, and the liability only re-accrues on two
  specific triggers: a new `dispatch` property, or a fourth resource. Those triggers are
  precisely when the retirement should land — which is why this note sits at the top of §13
  rather than in a backlog someone has to remember to read.

  If you are here because you hit one of those triggers: do the retirement instead of copying
  the pattern a third time.

- **✅ Resolved — resource reads are audited as `CallSource::Resource`.** A5 routes resource
  reads through registry enforcement: `email://` and `insights://` via `ToolRegistry::dispatch`
  (`mcp/resources.rs:203`, `:213`), and `thread://` via the registry's own limiter and audit
  primitives under the synthetic operation `resource:thread_read`, so no resource read can
  sidestep `tools.yaml` policy.

  All three now record `CallSource::Resource` (`:203`, `:213`, `:290`), which `as_str()`
  renders as `"resource"` (`tools/registry.rs:114`). Previously all three passed `Mcp`, which
  made an `email://` read produce an audit row indistinguishable from a direct `tools/call` of
  `get_email`.

  The variant already existed with a doc comment arguing its own case — "a resource read is not
  something the caller asked a tool for" — so leaving the call sites on `Mcp` would have
  recorded a permanent contradiction between the enum's documented intent and its use. Locked
  in by `resource_reads_are_distinguishable_from_tool_calls_in_the_audit_trail`
  (`mcp/resources.rs:553`), and the reasoning now lives in the code's own doc comment (`:189`)
  rather than only here.

- **Pre-existing debt, surfaced but not fixed here: two parallel progress types.**
  `api::ingestion` and `vectors::ingestion` each define an `IngestionPhase` and an
  `IngestionProgress` with the same names, different variants, and different `phase` field
  types (§1.5.1). This predates the consolidation. The alias re-export preserves the status quo
  exactly rather than making it worse, but the duplication remains, and collapsing it is a
  breaking wire-format change that needs its own task — see the warning in §1.5.1 before
  attempting it.

---

## Rollout

Delivery follows the ordering in §12. The consolidation (steps 1–4) is independently
shippable and is the regression gate: `tools/list` output must be byte-identical before any
new surface lands. New tools (step 6) and resources/prompts (step 7) are additive and safe to
land after.

### Status as of 2026-07-31

**Per-step status lives in the §12 table and only there.** This section previously carried a
second status table, which drifted out of sync with §12 and briefly had the document
contradicting itself about whether the registry declared anything. One table, one owner: §12 is
verified against the tree and is authoritative. What follows is the delivery-facing reading of
it, not a restatement.

**What a client connecting today gets: all fifteen tools, no resources, no prompts.** The MCP
half of the consolidation is delivered — the fifteen tools are declared once in
`tools/mod.rs::declarations()`, `mcp/server.rs` carries no `#[tool]` macros, and its router is
built at runtime from `registry.enabled()`. The original seven kept their names, arguments, and
result payloads, so §2's non-goal held. Resources and prompts exist in `mcp/resources.rs` but
are not advertised: `get_info` declares only `.enable_tools()`, which is the §1 gotcha about
`#[tool_handler]` not generating `get_info()` when one is already present.

**The chat half is untouched, and it is the point of the work.** `build_tool_definitions` and
`build_tool_executor` are still in `api/ai.rs`, so a hand-maintained second copy of every tool
still serves the orchestrator. Until that collapses, "one declaration per tool" is true of MCP
and false of the system.

Two further gaps worth naming because neither is visible from the step table alone.
`config/tools.yaml` has a schema, a loader, and overlay resolution, but **nothing hands the
loader a path** — runtime policy is built-in defaults, not the file an operator would edit. And
crate topology has not started: `lib.rs` exports neither `tools` nor `mcp`, `main.rs` still
re-declares the shared modules, `cleanup` is not hoisted, and `ToolContext` lacks the
`sync_progress` field `get_sync_status` needs. Handler signatures are done; the structure they
depend on is not.

The fifteen names in the `README.md` and `docs/setup-guide.md` tool tables now match
`declarations()` exactly. That match was verified by reading the declaration list, not by
calling the service — **the merge gate still requires a live `tools/list` returning all fifteen
names over the mounted endpoint.** Reading the source is not evidence that the wired service
answers.

**stdio transport is not implemented.** The mode is specified below under Operations and was
ratified as `--mcp-stdio` / `EMAILIBRIUM_MCP_MODE=stdio`, but no flag, env read, or transport
selection exists in `main.rs` yet. The stdio subsection of the setup guide documents the
ratified contract, not shipped behaviour.

### Blocking issue carried into implementation

The stdio transport cannot work until logging is moved off stdout. `main.rs:82` builds the
console layer as `tracing_subscriber::fmt::layer().with_ansi(true)` with no explicit writer,
which defaults to **stdout**; MCP over stdio requires stdout to carry only JSON-RPC frames, so
startup logs and ANSI escapes would corrupt the stream on first connect. At the default
`emailibrium=info` filter this fails every time, not intermittently.

The fix has an ordering constraint. The writer is chosen when the subscriber is constructed,
so the mode must be read _before_ `.init()` at `main.rs:89` — earlier than the existing CLI
flags, which are all parsed from `std::env::args()` at `main.rs:92` and after. §10.2 records
the same constraint from the design side. This also constrains any future config-file layer for
the mode: the `figment` chain that would read it runs later in startup, so a file layer needs
either a narrow standalone read above tracing init or a restructure that makes config-parse
errors unloggable.

Worth recording because it is counterintuitive: **no other stdout writes are a problem.** There
are 40 executable stdout print sites under `backend/src/` — 35 in `vectors/model_download.rs`,
reachable only from the two CLI paths that `return` before serving, and 5 in the
`--verify-models` block of `main.rs`, which also returns. Nothing under `cleanup/` or `rules/`
writes to stdout at runtime: the site in `cleanup/domain/builder.rs` is `eprintln!` inside a
`#[cfg(test)]` benchmark, and the one in `rules/parser.rs:17` sits inside a `//!` rustdoc
example and never executes.

Take care counting these. A naive `grep 'println!\|print!'` reports 44, overcounting twice
over: the pattern matches `eprintln!` as a substring, picking up 3 stderr sites that are
harmless here, and it counts the rustdoc line as if it were code. The accurate framing is that
the stdio risk is the tracing console layer — fixed by making the writer mode-conditional —
plus CLI print paths that all return before serving. It was never a codebase-wide print
problem, and fixing the tracing layer is sufficient rather than partial.

## Testing results

The integration harness in `backend/tests/mcp_integration.rs` is scaffolded and green: it
completes a real Streamable HTTP handshake, parses SSE frames, and exercises `tools/list` and
`tools/call`. It currently runs against a **probe router** — a minimal in-test server with a
single `probe_echo` tool — which validates the harness itself rather than emailibrium's tools.

**A green run here is not coverage, and the file proves it: it contains zero `emailibrium::`
imports.** Its only dependencies are `axum`, `tower`, `rmcp`, and `tokio`, so it cannot reach
production wiring even in principle — it is exercising a server it built itself. This is
precisely the `api_integration.rs` failure mode §8.1 warns about, and it stays that way until
the topology work lands and `lib.rs` exports `tools`. Read the green tick as "the harness
works", never as "the tools work".

Pointing it at the real router is gated on step 1, since the registry is what the assertions
in §8 target. The underlying reason e2e coverage is zero rather than merely thin is the crate
topology in §1.5: `main.rs` re-declares modules that `lib.rs` also exports, so
`emailibrium::db::Database` and `crate::db::Database` are distinct types that don't unify, and
tests linking the library cannot reach binary-side handlers. That is a compile constraint, not
a testing preference — which is why `ToolContext` exists. Until the registry lands:

| Tier | Coverage                                                  | State                                                                                                                 |
| ---- | --------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| 1    | Per-handler unit tests                                    | Present for the new handlers and the shared validators in `tools/readonly/mod.rs`.                                    |
| 2    | Registry dispatch, rate limiting, audit rows              | Blocked on step 1.                                                                                                    |
| 3    | Parity — chat definitions equal the `tools/list` response | Blocked on step 4. This is the assertion that prevents the `ai.rs` drift from recurring, so it should not be dropped. |

The regression gate from §12 has not run: it requires the seven existing tools to pass through
the registry unchanged, and the registry does not exist yet.

## Operations

### Endpoint and transport

MCP is served from the same Axum process as the REST API — no second port, no separate daemon.

| Mode             | Selector                                      | Behaviour                                                   |
| ---------------- | --------------------------------------------- | ----------------------------------------------------------- |
| `http` (default) | —                                             | Streamable HTTP at `http://localhost:8080/api/v1/mcp`.      |
| `stdio`          | `--mcp-stdio` or `EMAILIBRIUM_MCP_MODE=stdio` | JSON-RPC over stdin/stdout; the HTTP server does not start. |

The CLI flag wins when both are set. The two spellings are deliberately asymmetric — the flag
is a boolean shorthand, the environment variable takes the mode name — so there is no
`--mcp-mode` flag and no `EMAILIBRIUM_MCP_STDIO` variable. In stdio mode all logging goes to
stderr and `data/logs/emailibrium.log`. §10.2 has the full logging profile and §10.3 the rmcp
API details, including the `transport-io` Cargo feature that must be enabled.

Mode selection is a **startup-sequence** constraint, not a configuration one. The logging
profile is fixed when the tracing subscriber is built (`main.rs:89`), which happens before
`std::env::args()` is read at all (`main.rs:92`), so mode resolution has to be hoisted above
the `tracing_subscriber::registry()` block at `main.rs:80-89` — ahead of the existing
`--download-model` / `--download-models` / `--verify-models` branches rather than alongside
them. Left in the usual place, stdio mode emits ANSI-coloured log lines onto the JSON-RPC
channel before anything can redirect them, and the resulting parse errors look like an rmcp
bug rather than an ordering bug.

> **Scope decision — two sources, not four.** §10.1 presents a four-tier precedence ending in a
> config file. **Only the flag and the environment variable are in scope for this cycle**; the
> config-file tier was considered and deliberately dropped, because `config.yaml` does not
> exist (so the tier would mean creating a file to host one key), and spawn-based MCP clients —
> the whole reason stdio exists — pass argv and environment rather than editing files. §10.1's
> own caveats explain why that tier is inert today. Adding it later is a small additive change,
> not a redesign. User-facing docs therefore document exactly two sources.

Client setup for both modes is in
[the setup guide](../setup-guide.md#connecting-an-mcp-client).

### Security posture

The stance is specified in §11 and summarized here for operators; §11 governs if the two
diverge.

**The MCP surface is unauthenticated, and this is deliberate.** Emailibrium is a local-first
single-user application; `/api/v1` as a whole carries no auth, and MCP inherits that. The
security boundary is the network interface, not a credential: anyone who can reach port 8080
can read the user's mail — including full message bodies via `get_email`.

Two operational consequences follow. Keep the port bound to localhost — do not expose it
through a reverse proxy, container port mapping, or SSH forward without adding auth first. And
treat **bearer auth as a blocking prerequisite for any non-localhost bind**, not as optional
hardening to schedule later. stdio mode does not change this: it inherits the trust model of
whatever process spawned it.

Per-tool rate limits come from `config/tools.yaml`; tools without an explicit entry inherit
`defaults.rate_limit_per_minute`. Note that `timeout_ms` is parsed but not enforced (§13).
Tool invocations are audit-logged with SHA-256 argument hashes to `mcp_tool_audit`
(migration 022).

## Changelog

**2026-07-31** — Design section completed and ratified. **The registry landed and the MCP path
is on it**: all fifteen tools declared once in `tools/mod.rs::declarations()`, `mcp/server.rs`
rewritten onto `build_tool_router` over `registry.enabled()` with every `#[tool]` macro gone,
and handlers taking `Arc<ToolContext>` returning `Result<Value, ToolError>`. Eight A3 read-only
tools added (`list_accounts`, `get_sync_status`, `find_similar_emails`, `list_attachments`,
`list_subscriptions`, `list_clusters`, `get_learning_metrics`, `preview_cleanup_plan`). The
superseded `mcp/tools/` module — a surviving second copy of the request structs and validators —
was deleted. A5 resources and prompts written (`mcp/resources.rs`): three resources —
`email://{id}`, `thread://{key}`, `insights://summary` — and two prompts, `triage-inbox` and
`weekly-report`; not yet advertised, since `get_info` still declares only `.enable_tools()`.
Integration-test harness scaffolded against a probe router. Documentation landed across
`README.md`, `docs/setup-guide.md`, `docs/maintainer-guide.md`, and ADR-028. stdio mode ratified
as `--mcp-stdio` / `EMAILIBRIUM_MCP_MODE=stdio`; not yet implemented.

Outstanding: the `api/ai.rs` collapse (the chat orchestrator still runs a hand-maintained second
copy — the central goal of this work), reading `config/tools.yaml` from disk, crate topology,
migration 029 and the audit `source` column, and end-to-end tests against the real service.

**2026-04-04** — ADR-028 accepted; Phase 1 shipped seven read-only tools over Streamable HTTP
with per-tool rate limiting and SHA-256 audit logging.
