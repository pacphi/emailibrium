//! MCP (Model Context Protocol) server for emailibrium (ADR-028).
//!
//! This module is transport only: it adapts the Model Context Protocol onto
//! [`crate::tools`], which owns the tool declarations, handlers and dispatch.
//! Nothing here decides what a tool does or which tools exist.

pub mod resources;
pub mod server;

use std::sync::Arc;

use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::StreamableHttpService;

use crate::tools::{ToolContext, ToolRegistry};

/// Build the Streamable HTTP service mounted at `/api/v1/mcp`.
///
/// The binary and `backend/tests/` both call this, so an integration test
/// drives the transport that ships rather than a re-implementation of it — the
/// failure mode `tests/api_integration.rs` documents about itself.
///
/// `ctx` and `registry` are built by the caller: the factory closure below runs
/// once per session, so anything constructed inside it would be per-connection
/// state, which is how the rate limiter was previously reset by reconnecting.
pub fn mcp_service(
    ctx: Arc<ToolContext>,
    registry: Arc<ToolRegistry>,
) -> StreamableHttpService<server::EmailibriumMcpServer, LocalSessionManager> {
    StreamableHttpService::new(
        move || {
            Ok(server::EmailibriumMcpServer::new(
                ctx.clone(),
                registry.clone(),
            ))
        },
        Arc::new(LocalSessionManager::default()),
        Default::default(),
    )
}
