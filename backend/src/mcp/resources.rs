//! MCP resources and prompts for emailibrium (ADR-028).
//!
//! Resources expose read-only views of the mailbox under stable URIs; prompts
//! package the multi-step workflows those views support. Both are surfaced
//! through the `ServerHandler` implementation in [`super::server`].
//!
//! `rmcp` 1.7 ships no resource router and no `#[resource]` macro, so resources
//! are plain `ServerHandler` trait methods. The URI parsing and the two listings
//! therefore live here as free functions that `server.rs` delegates to. Prompts
//! do have a router, built by the `#[prompt_router]` block at the bottom.

use std::sync::Arc;
use std::time::Instant;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    AnnotateAble, ListResourceTemplatesResult, ListResourcesResult, PromptMessage,
    PromptMessageRole, RawResource, RawResourceTemplate, ReadResourceResult, ResourceContents,
};
use rmcp::{prompt, prompt_router, schemars, ErrorData};

use super::server::EmailibriumMcpServer;
use crate::tools::readonly::emails::fetch_thread_by_key;
use crate::tools::readonly::{validate_id, validate_limit};
use crate::tools::{audit, CallSource, ToolContext, ToolError, ToolRegistry};

// ---------------------------------------------------------------------------
// Resource URIs
// ---------------------------------------------------------------------------

/// Prefix for a single email: `email://{id}`.
const EMAIL_URI_PREFIX: &str = "email://";
/// Prefix for a conversation thread: `thread://{key}`.
const THREAD_URI_PREFIX: &str = "thread://";

/// URI template advertised for [`EMAIL_URI_PREFIX`].
const EMAIL_URI_TEMPLATE: &str = "email://{id}";
/// URI template advertised for [`THREAD_URI_PREFIX`].
const THREAD_URI_TEMPLATE: &str = "thread://{key}";

/// The one resource readable without first knowing an identifier.
const INSIGHTS_SUMMARY_URI: &str = "insights://summary";

/// Every resource is rendered as a JSON document.
const RESOURCE_MIME_TYPE: &str = "application/json";

/// A resource URI that has been recognized and validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceUri {
    /// A single email, with headers and full body.
    Email { email_id: String },
    /// Every email sharing a thread key, oldest first.
    Thread { thread_key: String },
    /// Category counts, top senders, and 7-day volume.
    InsightsSummary,
}

/// Parse a `resources/read` URI into the resource it names.
///
/// The three failure modes stay distinct so the caller returns the right MCP
/// error: an unrecognized scheme means no such resource exists, while a
/// recognized scheme carrying an unusable identifier is a malformed request.
/// Identifiers are percent-decoded, so a thread key containing `/` or `#`
/// survives the round trip.
pub fn parse_resource_uri(uri: &str) -> Result<ResourceUri, ErrorData> {
    if let Some(raw) = uri.strip_prefix(EMAIL_URI_PREFIX) {
        let email_id = decode_identifier(raw, uri)?;
        validate_id("email_id", &email_id)
            .map_err(|e| ErrorData::invalid_params(e, Some(serde_json::json!({ "uri": uri }))))?;
        return Ok(ResourceUri::Email { email_id });
    }

    if let Some(raw) = uri.strip_prefix(THREAD_URI_PREFIX) {
        let thread_key = decode_identifier(raw, uri)?;
        validate_id("thread_key", &thread_key)
            .map_err(|e| ErrorData::invalid_params(e, Some(serde_json::json!({ "uri": uri }))))?;
        return Ok(ResourceUri::Thread { thread_key });
    }

    if uri == INSIGHTS_SUMMARY_URI {
        return Ok(ResourceUri::InsightsSummary);
    }

    Err(ErrorData::resource_not_found(
        format!("Unknown resource URI: {uri}"),
        Some(serde_json::json!({
            "uri": uri,
            "supported": [EMAIL_URI_TEMPLATE, THREAD_URI_TEMPLATE, INSIGHTS_SUMMARY_URI],
        })),
    ))
}

/// Percent-decode the identifier portion of a resource URI.
fn decode_identifier(raw: &str, uri: &str) -> Result<String, ErrorData> {
    urlencoding::decode(raw)
        .map(|decoded| decoded.into_owned())
        .map_err(|_| {
            ErrorData::invalid_params(
                "Resource URI is not valid UTF-8 once percent-decoded".to_string(),
                Some(serde_json::json!({ "uri": uri })),
            )
        })
}

/// Translate a tool-layer failure into the MCP error a resource read returns.
///
/// What matters to a client is whether the resource is absent (`NotFound`, so
/// stop asking), the server failed while fetching it (`Database`, so a retry
/// may work), or the caller is over budget (`RateLimited`, so a retry works but
/// only after waiting). `NotConfigured` joins the second: a deployment missing
/// a backing service is a server-side fact the caller cannot act on. `Denied`
/// — an unknown or disabled tool — is permanent, so it maps to the generic
/// invalid-request rather than being reported as a missing resource.
///
/// These map onto the same codes the tool surface uses for the same causes, so
/// the two surfaces cannot disagree about what a given failure means.
fn tool_error_to_mcp(error: ToolError, uri: &str) -> ErrorData {
    // `kind` mirrors what the tool surface attaches, so a client sees the same
    // discriminator whichever surface it came through. Display prints only the
    // message, so without this the variant is lost once rendered.
    let data = Some(serde_json::json!({ "uri": uri, "kind": error.kind() }));
    let message = error.to_string();
    match error {
        ToolError::NotFound(_) => ErrorData::resource_not_found(message, data),
        ToolError::Invalid(_) => ErrorData::invalid_params(message, data),
        ToolError::Denied(_) => ErrorData::invalid_request(message, data),
        ToolError::RateLimited(_) => ErrorData::new(super::server::RATE_LIMITED, message, data),
        ToolError::Database(_) | ToolError::NotConfigured(_) => {
            ErrorData::internal_error(message, data)
        }
    }
}

/// Resources readable without first knowing an identifier.
///
/// The parameterized resources are advertised through [`resource_templates`]
/// instead, per the MCP split between `resources/list` and
/// `resources/templates/list`.
pub fn resources() -> ListResourcesResult {
    ListResourcesResult::with_all_items(vec![RawResource::new(
        INSIGHTS_SUMMARY_URI,
        "mailbox-insights",
    )
    .with_title("Mailbox insights")
    .with_description(
        "Aggregate mailbox statistics: email counts per category, the ten most frequent \
         senders, and daily volume over the last 7 days.",
    )
    .with_mime_type(RESOURCE_MIME_TYPE)
    .no_annotation()])
}

/// URI templates for the resources that take an identifier, so a client can
/// discover their shape rather than having to know it in advance.
pub fn resource_templates() -> ListResourceTemplatesResult {
    ListResourceTemplatesResult::with_all_items(vec![
        RawResourceTemplate::new(EMAIL_URI_TEMPLATE, "email")
            .with_title("Email")
            .with_description(
                "A single email by ID: sender, subject, date, category, and full body text. \
                 IDs come from the search_emails and list_recent_emails tools.",
            )
            .with_mime_type(RESOURCE_MIME_TYPE)
            .no_annotation(),
        RawResourceTemplate::new(THREAD_URI_TEMPLATE, "thread")
            .with_title("Conversation thread")
            .with_description(
                "Every email sharing a thread key, oldest first. Thread keys come from the \
                 get_email_thread tool.",
            )
            .with_mime_type(RESOURCE_MIME_TYPE)
            .no_annotation(),
    ])
}

/// Operation name for the thread resource in rate-limit windows and audit rows.
///
/// Distinct from `get_email_thread` so the resource and the tool remain
/// separable in the audit trail rather than sharing one indistinguishable
/// bucket.
const THREAD_RESOURCE_OP: &str = "resource:thread_read";

/// Read one resource and render it as a JSON document.
///
/// `email://` and `insights://` go through [`ToolRegistry::dispatch`], so they
/// inherit `config/tools.yaml` policy, rate limiting and audit from the same
/// path a tool call takes — a resource must not be a way around a tool the
/// operator disabled. `thread://` has no tool that accepts a thread key, so it
/// calls the shared limiter and audit primitives directly instead; see
/// [`read_thread`].
///
/// All three record [`CallSource::Resource`], never `Mcp`. Two of them reuse a
/// tool to do the work and would otherwise write an audit row indistinguishable
/// from a genuine `tools/call` for that same tool.
pub async fn read_resource(
    ctx: &Arc<ToolContext>,
    registry: &ToolRegistry,
    uri: &str,
) -> Result<ReadResourceResult, ErrorData> {
    let body = match parse_resource_uri(uri)? {
        ResourceUri::Email { email_id } => registry
            .dispatch(
                ctx.clone(),
                "get_email",
                serde_json::json!({ "email_id": email_id }),
                CallSource::Resource,
            )
            .await
            .map_err(|e| tool_error_to_mcp(e, uri))?,

        ResourceUri::InsightsSummary => registry
            .dispatch(
                ctx.clone(),
                "get_insights",
                serde_json::json!({}),
                CallSource::Resource,
            )
            .await
            .map_err(|e| tool_error_to_mcp(e, uri))?,

        ResourceUri::Thread { thread_key } => read_thread(ctx, registry, &thread_key, uri).await?,
    };

    Ok(ReadResourceResult::new(vec![ResourceContents::text(
        body.to_string(),
        uri,
    )
    .with_mime_type(RESOURCE_MIME_TYPE)]))
}

/// Read a thread by key under the same rate limit and audit a tool call gets.
///
/// No tool accepts a thread key, so this cannot go through
/// [`ToolRegistry::dispatch`]. It calls the registry's own limiter — the same
/// shared instance, so the window is not a second independent budget — and the
/// same audit primitive, so reading a thread as a resource cannot sidestep the
/// limits and logging that reading it as a tool would face.
async fn read_thread(
    ctx: &Arc<ToolContext>,
    registry: &ToolRegistry,
    thread_key: &str,
    uri: &str,
) -> Result<serde_json::Value, ErrorData> {
    let start = Instant::now();

    if let Err(e) = registry.rate_limiter().check(THREAD_RESOURCE_OP, None) {
        // RateLimited, not Denied: being over budget clears on its own, and a
        // caller that cannot tell "wait" from "stop" will either give up on a
        // transient limit or hammer a permanent refusal.
        return Err(fail_thread_read(ctx, uri, ToolError::RateLimited(e), start).await);
    }

    let emails = match fetch_thread_by_key(ctx, thread_key).await {
        Ok(emails) => emails,
        Err(e) => return Err(fail_thread_read(ctx, uri, e, start).await),
    };

    // The fetch layer yields an empty list for an unrecognized thread key,
    // which is right for the list-returning tool but wrong here: a resource
    // that does not exist must read as absent, not as empty.
    if emails.is_empty() {
        let absent = ToolError::NotFound(format!("No thread with key: {thread_key}"));
        return Err(fail_thread_read(ctx, uri, absent, start).await);
    }

    let body = serde_json::json!({
        "thread_key": thread_key,
        "count": emails.len(),
        "emails": to_json(&emails, uri)?,
    });

    audit_thread_read(ctx, uri, "success", start).await;
    Ok(body)
}

/// Audit a failed thread read and translate it, deriving the audit status from
/// the error itself.
///
/// One call site per failure keeps the recorded status and the returned error
/// describing the same thing. Passing a literal alongside the error is how they
/// drift: the label lives in `ToolError::status`, and a copy here would go stale
/// the first time that mapping changed, silently and with nothing failing.
async fn fail_thread_read(
    ctx: &ToolContext,
    uri: &str,
    error: ToolError,
    start: Instant,
) -> ErrorData {
    audit_thread_read(ctx, uri, error.status(), start).await;
    tool_error_to_mcp(error, uri)
}

/// Record one thread-resource read, mirroring the row the registry writes.
///
/// The URI is hashed rather than stored, matching how the registry treats tool
/// arguments: a thread key names a real conversation.
async fn audit_thread_read(ctx: &ToolContext, uri: &str, status: &'static str, start: Instant) {
    let entry = audit::ToolCallAuditEntry {
        timestamp: chrono::Utc::now(),
        tool_name: THREAD_RESOURCE_OP.to_string(),
        arguments_hash: audit::hash_arguments(&serde_json::json!({ "uri": uri })),
        result_status: status,
        latency_ms: start.elapsed().as_millis() as u64,
        // Same source the dispatched resources record, so all three read as
        // resource traffic rather than as tool calls that never happened.
        source: CallSource::Resource.as_str(),
    };
    audit::log_tool_call(&ctx.conn(), &entry).await;
}

/// Serialize a fetched record, reporting a serialization failure as a server
/// fault rather than letting it surface as a missing resource.
fn to_json<T: serde::Serialize>(value: &T, uri: &str) -> Result<serde_json::Value, ErrorData> {
    serde_json::to_value(value).map_err(|e| {
        ErrorData::internal_error(
            format!("Failed to serialize resource: {e}"),
            Some(serde_json::json!({ "uri": uri })),
        )
    })
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

/// Default number of recent emails a triage pass reviews.
const DEFAULT_TRIAGE_LIMIT: u32 = 20;
/// Cap on a triage pass, matching the `list_recent_emails` tool's own cap.
const MAX_TRIAGE_LIMIT: u32 = 100;

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct TriageInboxArgs {
    #[schemars(description = "How many recent emails to review (default: 20, max: 100)")]
    pub limit: Option<u32>,
}

#[prompt_router(vis = "pub(crate)")]
impl EmailibriumMcpServer {
    /// Triage recent mail into what needs a reply, what is time-sensitive, and
    /// what can be ignored.
    #[prompt(
        name = "triage-inbox",
        description = "Review recent email and report what needs a reply, what is time-sensitive, and what can be ignored."
    )]
    async fn triage_inbox(
        &self,
        Parameters(args): Parameters<TriageInboxArgs>,
    ) -> Vec<PromptMessage> {
        let limit = validate_limit(args.limit.unwrap_or(DEFAULT_TRIAGE_LIMIT), MAX_TRIAGE_LIMIT);

        vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Review my most recent email and tell me what actually needs my attention.\n\
                 \n\
                 Gather before you judge:\n\
                 1. Call `list_recent_emails` with a limit of {limit} to see what has arrived.\n\
                 2. Read the `insights://summary` resource for my category mix, most frequent \
                 senders, and daily volume over the last 7 days. Use it to tell a normal week \
                 from an unusual one.\n\
                 3. For anything that looks like it needs a reply, a decision, or has a \
                 deadline, read `email://{{id}}` for the full body. Do not judge a message by \
                 its subject line.\n\
                 4. Where a message is part of an ongoing exchange, read `thread://{{key}}` so \
                 you respond to the current state of the conversation rather than to one \
                 message out of context.\n\
                 \n\
                 Then report, in this order:\n\
                 - **Needs a reply** — who, what they asked for, and how long it has been \
                 waiting.\n\
                 - **Time-sensitive** — anything carrying a date or deadline, with the date \
                 stated.\n\
                 - **Can wait** — grouped and counted, not enumerated one by one.\n\
                 \n\
                 Name specific senders and subjects. Where a body is genuinely ambiguous about \
                 whether action is needed, say so instead of guessing. If nothing needs my \
                 attention, say that plainly rather than padding the list."
            ),
        )]
    }

    /// Summarize the previous week of email activity from the aggregate
    /// statistics, cross-checked against the mail itself.
    #[prompt(
        name = "weekly-report",
        description = "Summarize the last week of email activity: volume, dominant senders, and anything left unanswered."
    )]
    async fn weekly_report(&self) -> Vec<PromptMessage> {
        vec![PromptMessage::new_text(
            PromptMessageRole::User,
            "Summarize the last week of my email activity.\n\
             \n\
             Gather the numbers first:\n\
             1. Read the `insights://summary` resource for category counts, my top ten senders, \
             and daily volume across the last 7 days.\n\
             2. Cross-check the total with `count_emails`, passing `after` as the date seven \
             days ago. Call it again with `category` set for any category worth breaking out.\n\
             3. Sample the actual mail with `list_recent_emails` before characterizing it, and \
             read `email://{id}` for anything you name specifically.\n\
             \n\
             Then write:\n\
             - **Volume** — the total for the week, which days were heaviest, and how the \
             category mix shifted.\n\
             - **Who I heard from** — the senders that dominated, and whether that traffic was \
             worth my time.\n\
             - **Loose ends** — anything that looks like it needed a response and did not \
             obviously get one. Read `thread://{key}` to confirm before flagging it.\n\
             \n\
             Report only figures you actually retrieved; do not estimate or extrapolate. If a \
             figure is unavailable, name it and say why.",
        )]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ErrorCode;

    #[test]
    fn parses_email_uri() {
        assert_eq!(
            parse_resource_uri("email://abc-123").unwrap(),
            ResourceUri::Email {
                email_id: "abc-123".to_string()
            }
        );
    }

    #[test]
    fn parses_thread_uri() {
        assert_eq!(
            parse_resource_uri("thread://inbox-thread-7").unwrap(),
            ResourceUri::Thread {
                thread_key: "inbox-thread-7".to_string()
            }
        );
    }

    #[test]
    fn parses_insights_uri() {
        assert_eq!(
            parse_resource_uri(INSIGHTS_SUMMARY_URI).unwrap(),
            ResourceUri::InsightsSummary
        );
    }

    #[test]
    fn percent_decodes_identifiers() {
        // Thread keys are derived from message headers and can carry `/` and `<>`.
        assert_eq!(
            parse_resource_uri("thread://%3Cmsg%2F1%40example.com%3E").unwrap(),
            ResourceUri::Thread {
                thread_key: "<msg/1@example.com>".to_string()
            }
        );
    }

    #[test]
    fn unknown_scheme_is_not_found_rather_than_bad_request() {
        let err = parse_resource_uri("calendar://today").unwrap_err();
        assert_eq!(err.code, ErrorCode::RESOURCE_NOT_FOUND);
    }

    #[test]
    fn insights_subpath_is_not_treated_as_the_summary() {
        let err = parse_resource_uri("insights://summary/extra").unwrap_err();
        assert_eq!(err.code, ErrorCode::RESOURCE_NOT_FOUND);
    }

    #[test]
    fn empty_email_id_is_a_bad_request() {
        let err = parse_resource_uri("email://").unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn empty_thread_key_is_a_bad_request() {
        let err = parse_resource_uri("thread://").unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn oversized_thread_key_is_a_bad_request() {
        // One past the shared MAX_ID_LEN bound in crate::tools::readonly.
        let uri = format!("thread://{}", "k".repeat(201));
        let err = parse_resource_uri(&uri).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn advertised_templates_match_the_prefixes_that_parse() {
        // Guards against the templates and the parser drifting apart.
        assert!(EMAIL_URI_TEMPLATE.starts_with(EMAIL_URI_PREFIX));
        assert!(THREAD_URI_TEMPLATE.starts_with(THREAD_URI_PREFIX));
    }

    #[test]
    fn every_advertised_uri_is_readable() {
        for resource in resources().resources {
            assert!(
                parse_resource_uri(&resource.raw.uri).is_ok(),
                "advertised resource {} does not parse",
                resource.raw.uri
            );
        }
    }

    #[test]
    fn templates_are_advertised_for_both_parameterized_resources() {
        let templates = resource_templates().resource_templates;
        let uris: Vec<&str> = templates
            .iter()
            .map(|t| t.raw.uri_template.as_str())
            .collect();
        assert_eq!(uris, vec![EMAIL_URI_TEMPLATE, THREAD_URI_TEMPLATE]);
    }

    #[test]
    fn absent_resource_is_distinguished_from_a_failed_fetch() {
        // The whole point of the tool layer's error split: a client can tell
        // "stop asking" from "try again".
        let absent = tool_error_to_mcp(ToolError::NotFound("no such email".into()), "email://x");
        let failed = tool_error_to_mcp(ToolError::Database("pool timeout".into()), "email://x");

        assert_eq!(absent.code, ErrorCode::RESOURCE_NOT_FOUND);
        assert_eq!(failed.code, ErrorCode::INTERNAL_ERROR);
        assert_ne!(absent.code, failed.code);
    }

    #[test]
    fn unconfigured_backend_reports_as_server_side_not_missing() {
        let err = tool_error_to_mcp(
            ToolError::NotConfigured("vector search unavailable".into()),
            INSIGHTS_SUMMARY_URI,
        );
        assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
    }

    #[test]
    fn denied_does_not_masquerade_as_a_missing_resource() {
        let err = tool_error_to_mcp(ToolError::Denied("tool is disabled".into()), "email://x");
        assert_eq!(err.code, ErrorCode::INVALID_REQUEST);
    }

    #[test]
    fn being_over_budget_is_distinguishable_from_a_permanent_refusal() {
        // A rate limit clears on its own; a denial does not. Collapsing the two
        // leaves a caller unable to tell "wait" from "stop", so it either gives
        // up on a transient limit or hammers a permanent refusal.
        let throttled =
            tool_error_to_mcp(ToolError::RateLimited("over budget".into()), "email://x");
        let refused = tool_error_to_mcp(ToolError::Denied("tool disabled".into()), "email://x");

        assert_eq!(throttled.code, crate::mcp::server::RATE_LIMITED);
        assert_ne!(
            throttled.code, refused.code,
            "a throttled read must not report the same code as a permanent denial"
        );
    }

    #[test]
    fn invalid_from_the_tool_layer_maps_to_bad_params() {
        let err = tool_error_to_mcp(ToolError::Invalid("malformed id".into()), "email://x");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn mapped_errors_carry_the_message_uri_and_kind() {
        let err = tool_error_to_mcp(ToolError::NotFound("no such thread".into()), "thread://t-9");

        assert_eq!(err.message, "no such thread");
        assert_eq!(
            err.data.as_ref().and_then(|d| d.get("uri")),
            Some(&serde_json::json!("thread://t-9")),
            "the failing URI should survive into the error payload"
        );
        // Display prints only the message, so `kind` is the only thing that
        // tells a client which variant it actually hit.
        assert_eq!(
            err.data.as_ref().and_then(|d| d.get("kind")),
            Some(&serde_json::json!("not_found")),
        );
    }

    #[test]
    fn a_throttled_read_is_tagged_rate_limited_in_its_error_payload() {
        let err = tool_error_to_mcp(ToolError::RateLimited("over budget".into()), "thread://t-9");

        // The same discriminator the audit row records, so the wire and the
        // trail agree about why the read failed.
        assert_eq!(
            err.data.as_ref().and_then(|d| d.get("kind")),
            Some(&serde_json::json!("rate_limited")),
        );
    }

    #[test]
    fn resource_reads_are_distinguishable_from_tool_calls_in_the_audit_trail() {
        // email:// and insights:// are served by reusing get_email and
        // get_insights, so the audit row carries that tool's name. If the
        // source matched a genuine tools/call the two would be byte-identical
        // and an operator could not tell a resource read from a tool call.
        assert_ne!(
            CallSource::Resource.as_str(),
            CallSource::Mcp.as_str(),
            "a resource read must not record the same source as a tools/call"
        );
        assert_eq!(CallSource::Resource.as_str(), "resource");
    }

    #[test]
    fn both_prompts_are_registered_under_their_hyphenated_names() {
        let names: Vec<String> = EmailibriumMcpServer::prompt_router()
            .list_all()
            .into_iter()
            .map(|p| p.name.to_string())
            .collect();
        assert!(names.contains(&"triage-inbox".to_string()), "{names:?}");
        assert!(names.contains(&"weekly-report".to_string()), "{names:?}");
    }
}
