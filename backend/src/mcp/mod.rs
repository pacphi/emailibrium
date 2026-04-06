//! MCP (Model Context Protocol) server for emailibrium (ADR-028).
//!
//! Exposes email operations as MCP tools, enabling tool-calling LLMs
//! to perform any action available via the REST API or UI.

pub mod audit;
pub mod rate_limit;
pub mod server;
pub mod tools;
