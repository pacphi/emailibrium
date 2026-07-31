//! Unified tool registry (ADR-028 follow-on, task A1).
//!
//! One declaration per tool, consumed by every caller: the MCP server, the
//! in-process chat orchestrator, and integration tests. Handlers take a
//! [`ToolContext`] rather than the binary's `AppState`, so this module stays
//! reachable from `backend/tests/`, which links the library crate.
//!
//! To add a tool: write the handler in a submodule, then append one
//! [`registry::declare`] call in [`declarations`]. No caller needs a per-tool
//! registration line.

pub mod audit;
pub mod config;
pub mod context;
pub mod rate_limit;
pub mod readonly;
pub mod registry;

pub use context::ToolContext;
pub use registry::{declare, CallSource, ToolDecl, ToolError, ToolRegistry};

/// JSON Schema for a request struct, generated from its `schemars` derive.
///
/// Generated rather than hand-written so the published MCP input schema cannot
/// drift from the struct callers are actually deserialized into. Uses the same
/// draft 2020-12 settings rmcp applies, so the schema is byte-identical to what
/// the transport would produce itself.
fn schema_for<T: rmcp::schemars::JsonSchema>() -> serde_json::Value {
    let generator = rmcp::schemars::generate::SchemaSettings::draft2020_12().into_generator();
    serde_json::to_value(generator.into_root_schema_for::<T>())
        .unwrap_or_else(|_| serde_json::json!({ "type": "object", "properties": {} }))
}

/// Schema published for a tool that takes no arguments.
///
/// Matches `rmcp::handler::server::common::schema_for_empty_input`, which is
/// what `#[tool]` emitted for a method with no `Parameters`. Deriving one from
/// an empty request struct instead would publish a bare `{"type":"object"}`
/// with no `properties` key, which some strict clients reject.
fn empty_schema() -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": {} })
}

/// Every tool this build knows about.
///
/// Exposure, confirmation and rate-limit policy are layered on top of this by
/// [`ToolRegistry`] from `config/tools.yaml`; this is the set of tools that
/// exist, not the set that is enabled.
pub fn declarations() -> Vec<ToolDecl> {
    use readonly::params as p;

    vec![
        // --- the original seven, moved off `#[tool]` in mcp/server.rs -----
        // Names, descriptions and schemas are carried over verbatim: this move
        // must not change a single byte of `tools/list`.
        declare(
            "search_emails",
            "Search the user's emails by query text. Returns matching emails with sender, subject, date, and relevance score.",
            schema_for::<p::SearchEmailsRequest>(),
            readonly::emails::search_emails,
        ),
        declare(
            "get_email",
            "Get full email content including headers, body, and metadata by email ID.",
            schema_for::<p::GetEmailRequest>(),
            readonly::emails::get_email,
        ),
        declare(
            "list_recent_emails",
            "List the most recent emails across all connected accounts.",
            schema_for::<p::ListRecentEmailsRequest>(),
            readonly::emails::list_recent_emails,
        ),
        declare(
            "count_emails",
            "Count emails matching optional filters. Supports filtering by sender, category, and date range (ISO 8601).",
            schema_for::<p::CountEmailsRequest>(),
            readonly::emails::count_emails,
        ),
        declare(
            "get_email_thread",
            "Get all emails in the same conversation thread as the specified email, ordered by date.",
            schema_for::<p::GetEmailThreadRequest>(),
            readonly::emails::get_email_thread,
        ),
        declare(
            "get_insights",
            "Get email analytics: counts by category, top senders, and daily volume for the last 7 days.",
            empty_schema(),
            readonly::insights::get_insights,
        ),
        declare(
            "list_rules",
            "List all email rules including their conditions, actions, and status.",
            empty_schema(),
            readonly::insights::list_rules,
        ),
        // --- A3: eight new read-only tools --------------------------------
        declare(
            "list_accounts",
            "List connected email accounts with provider, status, and sync counters. \
             Includes disconnected and errored accounts — check the status field.",
            empty_schema(),
            readonly::accounts::list_accounts,
        ),
        declare(
            "get_sync_status",
            "Report ingestion pipeline progress, background poll state, and per-account \
             sync state. The pipeline and per-account sync are independent.",
            schema_for::<p::GetSyncStatusRequest>(),
            readonly::accounts::get_sync_status,
        ),
        declare(
            "list_subscriptions",
            "List detected newsletter and mailing-list senders with frequency, read rate, \
             and a suggested action. Expensive: scans message bodies per sender.",
            schema_for::<p::ListSubscriptionsRequest>(),
            readonly::insights::list_subscriptions,
        ),
        declare(
            "preview_cleanup_plan",
            "Build a cleanup plan in memory and summarize it. Strictly dry-run: nothing is \
             saved and no mailbox is modified. The returned plan is ephemeral and cannot be applied.",
            schema_for::<p::PreviewCleanupPlanRequest>(),
            readonly::cleanup_preview::preview_cleanup_plan,
        ),
        declare(
            "find_similar_emails",
            "Find emails semantically nearest to a given email by embedding similarity.",
            schema_for::<p::FindSimilarEmailsRequest>(),
            readonly::emails::find_similar_emails,
        ),
        declare(
            "list_clusters",
            "List discovered topic clusters with top terms and representative emails.",
            schema_for::<p::ListClustersRequest>(),
            readonly::insights::list_clusters,
        ),
        declare(
            "get_learning_metrics",
            "Report relevance-learning counters: feedback volume, click ranks, and centroid \
             drift. Counters are process-local and reset on restart.",
            empty_schema(),
            readonly::insights::get_learning_metrics,
        ),
        declare(
            "list_attachments",
            "List attachment metadata for one email — filenames, types, and sizes. \
             Never returns file contents.",
            schema_for::<p::ListAttachmentsRequest>(),
            readonly::emails::list_attachments,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<&'static str> {
        declarations().iter().map(|d| d.name).collect()
    }

    #[test]
    fn every_tool_is_declared_exactly_once() {
        let names = names();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            names.len(),
            "a tool name appears twice in declarations(); the registry would \
             silently serve only one of them"
        );
    }

    #[test]
    fn the_seven_tools_that_predate_the_registry_are_still_declared() {
        let names = names();
        for name in [
            "search_emails",
            "get_email",
            "list_recent_emails",
            "count_emails",
            "get_email_thread",
            "get_insights",
            "list_rules",
        ] {
            assert!(names.contains(&name), "{name} is no longer declared");
        }
    }

    #[test]
    fn parameterless_tools_publish_an_explicit_empty_object() {
        let expected = serde_json::json!({ "type": "object", "properties": {} });
        for decl in declarations() {
            if matches!(
                decl.name,
                "get_insights" | "list_rules" | "list_accounts" | "get_learning_metrics"
            ) {
                assert_eq!(
                    decl.input_schema, expected,
                    "{} must keep the schema `#[tool]` published for a \
                     no-argument tool",
                    decl.name
                );
            }
        }
    }

    #[test]
    fn the_shipped_config_names_only_tools_that_exist() {
        // The registry ignores an entry with no matching tool, so a name that
        // rots after a rename fails silently and forever. `deferred` entries
        // are exempt: they record A4's intended policy on purpose.
        let config = config::ToolsConfig::load("../config");
        assert!(
            !config.tools.is_empty(),
            "config/tools.yaml did not load — this test is only meaningful \
             when run from the backend package root"
        );

        let unknown = config.unknown_tools(names().into_iter());
        assert!(
            unknown.is_empty(),
            "config/tools.yaml names tools that do not exist: {unknown:?}"
        );
    }

    #[test]
    fn body_scanning_tools_keep_their_reduced_rate_limit() {
        // Both run subscription detection, which pulls full body_text for
        // every sender. Raised to match the other read-only tools, either one
        // becomes a cheap way to stream the whole mailbox.
        let config = config::ToolsConfig::load("../config");
        for name in ["list_subscriptions", "preview_cleanup_plan"] {
            assert_eq!(
                config.policy(name).rate_limit_per_minute,
                Some(5),
                "{name} must stay rate-limited until its query stops reading \
                 full message bodies"
            );
        }
    }

    #[test]
    fn every_schema_is_a_json_object() {
        for decl in declarations() {
            assert!(
                decl.input_schema.is_object(),
                "{}'s schema is not an object, so the MCP transport would \
                 publish it as taking no arguments",
                decl.name
            );
        }
    }
}
