//! Unified tool registry and shared dispatch path.
//!
//! One declaration per tool describes its name, description, JSON Schema and
//! handler. Both callers — the MCP server and the in-process chat
//! orchestrator — go through [`ToolRegistry::dispatch`], so rate limiting and
//! audit logging apply to each exactly once and cannot drift apart.
//!
//! Adding a tool means appending one [`ToolDecl`] in [`declarations`]; no
//! caller needs a per-tool registration line.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use futures::future::BoxFuture;
use serde_json::Value;

use super::audit;
use super::config::ToolsConfig;
use super::context::ToolContext;
use super::rate_limit::ToolRateLimiter;
use crate::vectors::tool_calling::ToolDefinition;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Failure modes a handler can report.
///
/// Distinguishing "not found" from "database error" matters to callers that
/// map onto protocol-level errors — the MCP resource layer returns
/// `resource_not_found` for the former and `internal_error` for the latter.
#[derive(Debug)]
pub enum ToolError {
    /// The requested entity does not exist.
    NotFound(String),
    /// Caller-supplied arguments failed validation.
    Invalid(String),
    /// A query or backing service failed.
    Database(String),
    /// The service backing this tool is not configured in this deployment.
    NotConfigured(String),
    /// The tool is unknown or disabled by policy.
    Denied(String),
    /// The tool exists and is allowed, but is over its per-minute budget.
    ///
    /// Separate from [`Denied`](Self::Denied) because the two call for opposite
    /// client behaviour: a denial is permanent and should not be retried, a
    /// rate limit clears on its own. Folded together, a throttled caller cannot
    /// tell "wait" from "stop".
    RateLimited(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(m) => write!(f, "{m}"),
            Self::Invalid(m) => write!(f, "{m}"),
            Self::Database(m) => write!(f, "{m}"),
            Self::NotConfigured(m) => write!(f, "{m}"),
            Self::Denied(m) => write!(f, "{m}"),
            Self::RateLimited(m) => write!(f, "{m}"),
        }
    }
}

impl ToolError {
    /// Stable machine-readable label for this failure class.
    ///
    /// `Display` prints only the message, so without this the variant is lost
    /// the moment an error is rendered for a caller.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::Invalid(_) => "invalid",
            Self::Database(_) => "database",
            Self::NotConfigured(_) => "not_configured",
            Self::Denied(_) => "denied",
            Self::RateLimited(_) => "rate_limited",
        }
    }

    /// Audit status label for this failure class.
    ///
    /// Reachable outside the registry so the resource layer, which writes its
    /// own audit rows for reads that cannot go through `dispatch`, derives the
    /// label instead of repeating it. A matching literal there would diverge
    /// silently the first time this mapping changed.
    pub(crate) fn status(&self) -> &'static str {
        match self {
            Self::Denied(_) => "denied",
            Self::RateLimited(_) => "rate_limited",
            _ => "error",
        }
    }
}

/// Which caller ran a tool.
///
/// Both callers share one rate-limit budget and one audit trail, so without
/// this the trail could no longer tell an MCP client's traffic from the chat
/// assistant's — the one piece of information consolidating the two paths
/// would otherwise destroy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSource {
    /// A `tools/call` over the MCP transport.
    Mcp,
    /// In-process, from the chat orchestrator.
    Chat,
    /// A `resources/read` served by reusing a tool.
    ///
    /// Distinct from [`Mcp`](Self::Mcp) even though both arrive over the same
    /// transport: a resource read is not something the caller asked a tool for,
    /// and design §7.1 wants the two separable in the audit trail.
    Resource,
}

impl CallSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Chat => "chat",
            Self::Resource => "resource",
        }
    }
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

/// Boxed future returned by a tool handler.
pub type ToolFuture = BoxFuture<'static, Result<Value, ToolError>>;

/// A tool's async implementation, invoked with the shared context and the
/// caller's raw JSON arguments.
pub type ToolHandler = Arc<dyn Fn(Arc<ToolContext>, Value) -> ToolFuture + Send + Sync>;

/// Everything the registry knows about one tool.
pub struct ToolDecl {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub handler: ToolHandler,
}

/// Build a [`ToolDecl`] from an async function taking `(Arc<ToolContext>, P)`.
///
/// `P` is deserialized from the caller's arguments, so handlers work with a
/// typed request struct rather than raw JSON.
pub fn declare<P, F, Fut>(
    name: &'static str,
    description: &'static str,
    input_schema: Value,
    f: F,
) -> ToolDecl
where
    P: serde::de::DeserializeOwned + Send + 'static,
    F: Fn(Arc<ToolContext>, P) -> Fut + Send + Sync + Copy + 'static,
    Fut: std::future::Future<Output = Result<Value, ToolError>> + Send + 'static,
{
    ToolDecl {
        name,
        description,
        input_schema,
        handler: Arc::new(move |ctx, args| {
            Box::pin(async move {
                // Absent arguments are an empty object so parameterless tools
                // and tools with all-optional fields both deserialize.
                let args = if args.is_null() {
                    Value::Object(Default::default())
                } else {
                    args
                };
                let parsed: P = serde_json::from_value(args)
                    .map_err(|e| ToolError::Invalid(format!("Invalid arguments: {e}")))?;
                f(ctx, parsed).await
            })
        }),
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// The set of tools this build exposes, plus the shared cross-cutting state.
pub struct ToolRegistry {
    decls: Vec<ToolDecl>,
    by_name: HashMap<&'static str, usize>,
    config: ToolsConfig,
    rate_limiter: Arc<ToolRateLimiter>,
}

impl ToolRegistry {
    /// Build a registry from the declaration set, applying `tools.yaml`.
    ///
    /// Tools absent from the config keep their defaults; config entries with
    /// no matching tool are reported by [`ToolsConfig::unknown_tools`].
    pub fn new(decls: Vec<ToolDecl>, config: ToolsConfig) -> Self {
        let rate_limiter = Arc::new(ToolRateLimiter::new(config.defaults.rate_limit_per_minute));
        Self::with_rate_limiter(decls, config, rate_limiter)
    }

    /// Same as [`new`](Self::new) but with a caller-supplied limiter, so tests
    /// can drive rate-limit behaviour without issuing a full window of calls.
    pub fn with_rate_limiter(
        decls: Vec<ToolDecl>,
        config: ToolsConfig,
        rate_limiter: Arc<ToolRateLimiter>,
    ) -> Self {
        for name in config.unknown_tools(decls.iter().map(|d| d.name)) {
            tracing::warn!("config/tools.yaml lists unknown tool '{name}' — ignoring");
        }
        let by_name = decls.iter().enumerate().map(|(i, d)| (d.name, i)).collect();
        Self {
            decls,
            by_name,
            config,
            rate_limiter,
        }
    }

    /// The registry used in production, wired from `config/tools.yaml`.
    pub fn from_config(config: ToolsConfig) -> Self {
        Self::new(super::declarations(), config)
    }

    pub fn rate_limiter(&self) -> &Arc<ToolRateLimiter> {
        &self.rate_limiter
    }

    /// Declarations for tools enabled in this build, in declaration order.
    pub fn enabled(&self) -> impl Iterator<Item = &ToolDecl> {
        self.decls
            .iter()
            .filter(|d| self.config.policy(d.name).enabled)
    }

    /// Names of enabled tools. The single source both transports list from.
    pub fn names(&self) -> Vec<&'static str> {
        self.enabled().map(|d| d.name).collect()
    }

    /// Whether a tool requires user confirmation before execution.
    pub fn requires_confirmation(&self, name: &str) -> bool {
        self.config.policy(name).requires_confirmation
    }

    /// Names of enabled tools that require confirmation — replaces the
    /// hardcoded list the chat orchestrator used to carry.
    pub fn confirmation_required(&self) -> Vec<String> {
        self.enabled()
            .filter(|d| self.requires_confirmation(d.name))
            .map(|d| d.name.to_string())
            .collect()
    }

    /// Tool definitions for the chat orchestrator, in declaration order.
    ///
    /// Built from the same declarations the MCP transport publishes, so the
    /// assistant and an MCP client cannot be offered different tools or
    /// different schemas for the same tool.
    pub fn chat_definitions(&self) -> Vec<ToolDefinition> {
        self.enabled()
            .map(|d| ToolDefinition {
                name: d.name.to_string(),
                description: d.description.to_string(),
                input_schema: d.input_schema.clone(),
            })
            .collect()
    }

    /// Execute a tool: policy check, rate limit, handler, audit.
    ///
    /// Every caller goes through here, so a tool cannot be reachable from one
    /// transport with different limits or audit coverage than another.
    pub async fn dispatch(
        &self,
        ctx: Arc<ToolContext>,
        name: &str,
        args: Value,
        source: CallSource,
    ) -> Result<Value, ToolError> {
        let start = Instant::now();
        let conn = ctx.conn();

        let result = self.invoke(ctx, name, &args).await;

        let entry = audit::ToolCallAuditEntry {
            timestamp: chrono::Utc::now(),
            tool_name: name.to_string(),
            arguments_hash: audit::hash_arguments(&args),
            result_status: match &result {
                Ok(_) => "success",
                Err(e) => e.status(),
            },
            latency_ms: start.elapsed().as_millis() as u64,
            source: source.as_str(),
        };
        audit::log_tool_call(&conn, &entry).await;

        result
    }

    /// Policy + rate limit + handler, without the audit wrapper.
    async fn invoke(
        &self,
        ctx: Arc<ToolContext>,
        name: &str,
        args: &Value,
    ) -> Result<Value, ToolError> {
        let idx = *self
            .by_name
            .get(name)
            .ok_or_else(|| ToolError::Denied(format!("Unknown tool: {name}")))?;
        let decl = &self.decls[idx];

        let policy = self.config.policy(decl.name);
        if !policy.enabled {
            return Err(ToolError::Denied(format!("Tool '{name}' is disabled")));
        }

        self.rate_limiter
            .check(decl.name, policy.rate_limit_per_minute)
            .map_err(ToolError::RateLimited)?;

        (decl.handler)(ctx, args.clone()).await
    }
}
