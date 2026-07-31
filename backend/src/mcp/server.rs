//! MCP server handler for emailibrium (ADR-028).
//!
//! Implements the `ServerHandler` trait from `rmcp` so the emailibrium backend
//! can serve MCP tool calls over Streamable HTTP, sharing the same Axum
//! process and services as the REST API.
//!
//! The server holds no tool definitions of its own. Every tool comes from
//! [`ToolRegistry`], which the chat orchestrator and the integration tests
//! drive through the same [`ToolRegistry::dispatch`] path, so the two callers
//! cannot drift apart on which tools exist or what limits apply.

use std::sync::Arc;

use futures::future::BoxFuture;
use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::router::tool::{ToolRoute, ToolRouter};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolResult, Content, GetPromptRequestParams, GetPromptResult, JsonObject,
    ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, PaginatedRequestParams,
    ReadResourceRequestParams, ReadResourceResult, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{prompt_handler, tool_handler, RoleServer, ServerHandler};

use super::resources;
use crate::tools::{CallSource, ToolContext, ToolError, ToolRegistry};

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

/// The MCP server that exposes emailibrium capabilities as tools.
#[derive(Clone)]
pub struct EmailibriumMcpServer {
    tool_router: ToolRouter<Self>,
    /// Prompt templates, built once from the `#[prompt_router]` block in
    /// [`super::resources`]. Read by `#[prompt_handler(router =
    /// self.prompt_router)]` below — the argument matters for the same reason
    /// it does on `tool_handler`.
    prompt_router: PromptRouter<Self>,
    /// Services the tool handlers run against.
    pub ctx: Arc<ToolContext>,
    /// Shared with the chat path, so rate limits and audit apply once and to
    /// both. Constructed by the caller, not here — see [`Self::new`].
    pub registry: Arc<ToolRegistry>,
}

impl EmailibriumMcpServer {
    /// Build a server over an already-constructed context and registry.
    ///
    /// Both arguments are `Arc`s the caller owns: the transport builds a fresh
    /// `EmailibriumMcpServer` per session, so anything constructed *here*
    /// would reset on every reconnect. That is exactly how the rate limiter
    /// used to be defeated.
    pub fn new(ctx: Arc<ToolContext>, registry: Arc<ToolRegistry>) -> Self {
        Self {
            tool_router: build_tool_router(&registry),
            prompt_router: Self::prompt_router(),
            ctx,
            registry,
        }
    }
}

/// Build a router with one dynamic route per enabled tool.
///
/// Routes are built from the registry rather than from `#[tool]` methods, so
/// `config/tools.yaml` decides what is served.
///
/// The `#[tool_handler(router = self.tool_router)]` attribute below must keep
/// its argument. The default form expands to `Self::tool_router()`, which no
/// longer exists here — so dropping the argument today is a compile error, not
/// a silent failure. That backstop is incidental: reintroducing a
/// `#[tool_router]` block would make `Self::tool_router()` resolve again, and
/// the default would then quietly serve `#[tool]` methods instead of the
/// registry. `Self::prompt_router()` does still exist, so the prompt attribute
/// has no equivalent protection.
fn build_tool_router(registry: &Arc<ToolRegistry>) -> ToolRouter<EmailibriumMcpServer> {
    let mut router = ToolRouter::new();

    for decl in registry.enabled() {
        let name = decl.name;
        let tool = Tool::new(name, decl.description, input_schema(&decl.input_schema));

        router.add_route(ToolRoute::new_dyn(tool, move |tcc| {
            // Boxed through a named helper: a closure cannot infer a return
            // type whose lifetime is tied to its argument, which is what
            // `new_dyn`'s higher-ranked bound asks for.
            boxed(dispatch_call(tcc, name))
        }));
    }

    router
}

/// Run one tool call through the shared dispatch path.
async fn dispatch_call(
    tcc: ToolCallContext<'_, EmailibriumMcpServer>,
    name: &'static str,
) -> Result<CallToolResult, rmcp::ErrorData> {
    let server = tcc.service;
    let args = tcc
        .arguments
        .map_or(serde_json::Value::Null, serde_json::Value::Object);

    match server
        .registry
        .dispatch(server.ctx.clone(), name, args, CallSource::Mcp)
        .await
    {
        Ok(value) => Ok(CallToolResult::success(vec![Content::text(
            value.to_string(),
        )])),
        Err(e) => Err(failure(e)),
    }
}

/// Rate-limit rejection.
///
/// JSON-RPC reserves -32000..=-32099 for implementation-defined server errors
/// and MCP defines nothing for this case, so a client that needs to tell "back
/// off and retry" from "your request was wrong" needs a code of our own.
pub(crate) const RATE_LIMITED: rmcp::model::ErrorCode = rmcp::model::ErrorCode(-32001);

/// Map a failed tool call onto a JSON-RPC error.
///
/// Every variant becomes a protocol error rather than a success-shaped result.
/// The earlier version of this function returned
/// `CallToolResult::success(json!({"error": ...}))` for every failure, which
/// meant a client saw `isError: false` on a failed call and the audit trail
/// recorded a served call — including rate-limit rejections, which are exactly
/// the ones a caller most needs to notice.
///
/// The code carries the variant, since `Display` prints only the message.
fn failure(error: ToolError) -> rmcp::ErrorData {
    let data = Some(serde_json::json!({ "kind": error.kind() }));
    let message = error.to_string();

    match error {
        ToolError::Invalid(_) => rmcp::ErrorData::invalid_params(message, data),
        ToolError::NotFound(_) => rmcp::ErrorData::resource_not_found(message, data),
        ToolError::Denied(_) => rmcp::ErrorData::invalid_request(message, data),
        ToolError::RateLimited(_) => rmcp::ErrorData::new(RATE_LIMITED, message, data),
        // The caller-facing message is already generic — `db_error` logs the
        // underlying failure and returns "{operation} failed" — so nothing
        // here leaks backing-store detail.
        ToolError::Database(_) | ToolError::NotConfigured(_) => {
            rmcp::ErrorData::internal_error(message, data)
        }
    }
}

fn boxed<'a>(
    fut: impl std::future::Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + 'a,
) -> BoxFuture<'a, Result<CallToolResult, rmcp::ErrorData>> {
    Box::pin(fut)
}

/// Adapt a declaration's schema to the shape `Tool::new` wants.
///
/// Declarations carry a `Value` because the chat orchestrator needs one; rmcp
/// wants the object itself. A non-object schema is a declaration bug, and an
/// empty object is the closest safe reading of "takes no arguments".
fn input_schema(schema: &serde_json::Value) -> Arc<JsonObject> {
    match schema {
        serde_json::Value::Object(map) => Arc::new(map.clone()),
        _ => Arc::new(JsonObject::new()),
    }
}

// ---------------------------------------------------------------------------
// ServerHandler implementation
// ---------------------------------------------------------------------------

#[tool_handler(router = self.tool_router)]
#[prompt_handler(router = self.prompt_router)]
impl ServerHandler for EmailibriumMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_instructions(
            "Emailibrium MCP server. Provides email search, retrieval, and management \
             tools for AI-assisted email workflows. Read-only views are also exposed as \
             resources: insights://summary for mailbox statistics, and the email://{id} \
             and thread://{key} templates for a single message or a whole conversation. \
             The triage-inbox and weekly-report prompts package the common workflows.",
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, rmcp::ErrorData> {
        Ok(resources::resources())
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, rmcp::ErrorData> {
        Ok(resources::resource_templates())
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, rmcp::ErrorData> {
        resources::read_resource(&self.ctx, &self.registry, &request.uri).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ErrorCode;

    fn code_for(error: ToolError) -> ErrorCode {
        failure(error).code
    }

    #[test]
    fn a_failed_call_never_reports_as_a_success() {
        // The regression this guards: every variant used to come back as
        // `CallToolResult::success` with an error blob in the text, so a
        // client saw `isError: false` on a call that did not happen.
        for error in [
            ToolError::Invalid("bad".into()),
            ToolError::NotFound("gone".into()),
            ToolError::Denied("off".into()),
            ToolError::RateLimited("slow down".into()),
            ToolError::Database("boom".into()),
            ToolError::NotConfigured("absent".into()),
        ] {
            let kind = error.kind();
            let mapped = failure(error);
            assert_ne!(
                mapped.code,
                ErrorCode(0),
                "{kind} must map to a real JSON-RPC error code"
            );
        }
    }

    #[test]
    fn a_throttled_call_is_distinguishable_from_a_refused_one() {
        // A rate limit clears on its own and a denial does not, so a client
        // that cannot tell them apart either retries forever or gives up on a
        // tool it was allowed to call.
        assert_ne!(
            code_for(ToolError::RateLimited("slow down".into())),
            code_for(ToolError::Denied("off".into()))
        );
        assert_eq!(
            code_for(ToolError::RateLimited("slow down".into())),
            RATE_LIMITED
        );
    }

    #[test]
    fn client_errors_and_server_errors_get_different_codes() {
        assert_eq!(
            code_for(ToolError::Invalid("bad".into())),
            ErrorCode::INVALID_PARAMS
        );
        assert_eq!(
            code_for(ToolError::NotFound("gone".into())),
            ErrorCode::RESOURCE_NOT_FOUND
        );
        assert_eq!(
            code_for(ToolError::Database("boom".into())),
            ErrorCode::INTERNAL_ERROR
        );
    }

    #[test]
    fn the_variant_survives_the_mapping() {
        // `Display` prints only the message, so without `kind` in the data the
        // caller cannot tell a missing email from a failed query.
        let mapped = failure(ToolError::NotConfigured("no vectors".into()));
        assert_eq!(
            mapped.data.as_ref().and_then(|d| d.get("kind")),
            Some(&serde_json::json!("not_configured"))
        );
        assert_eq!(mapped.message, "no vectors");
    }
}
