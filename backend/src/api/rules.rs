//! REST API routes for the Rules Engine (R-03).
//!
//! - GET    /api/v1/rules          -- list all rules
//! - POST   /api/v1/rules          -- create a rule
//! - GET    /api/v1/rules/:id      -- get a single rule
//! - PUT    /api/v1/rules/:id      -- update a rule
//! - DELETE /api/v1/rules/:id      -- delete a rule
//! - POST   /api/v1/rules/validate -- validate a rule without saving
//! - POST   /api/v1/rules/test     -- test a rule against a sample email
//!
//! Persistence is single-code-path SeaORM (ADR-036): the `rules` and `emails`
//! entities own per-backend encode/decode, so every query below runs unchanged
//! against SQLite and PostgreSQL. Rule CRUD itself lives in
//! `rules::rule_engine`; what this module adds on top are the four queries in
//! the "Queries" section — match-count reporting, the rule-evaluation corpus,
//! the manual-run counter write, and the sender histogram behind suggestions.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use sea_orm::sea_query::{Asterisk, Expr, ExprTrait, Func};
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, FromQueryResult, QueryFilter, QueryOrder,
    QuerySelect,
};
use serde::{Deserialize, Serialize};

use crate::db::entities::{emails, rules};
use crate::rules::json_parser;
use crate::rules::rule_engine::RuleEngine;
use crate::rules::rule_validator::{self, Severity};
use crate::rules::types::{EmailField, MatchOperator, Rule, RuleAction, RuleCondition};

use crate::AppState;

/// Build the rules API router.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_rules).post(create_rule))
        .route("/suggestions", get(get_suggestions))
        .route(
            "/{id}",
            get(get_rule).put(update_rule).delete(delete_rule_handler),
        )
        .route("/{id}/run", post(run_rule))
        .route("/validate", post(validate_rule))
        .route("/test", post(test_rule))
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRuleRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Raw conditions. When `natural_language` is provided this field may be
    /// empty -- the parsed NL condition will be used instead.
    #[serde(default)]
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Optional natural-language condition string. When present, it is parsed
    /// via `json_parser::parse_natural_language` and prepended to `conditions`.
    #[serde(default)]
    pub natural_language: Option<String>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRuleRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub conditions: Option<Vec<RuleCondition>>,
    pub actions: Option<Vec<RuleAction>>,
    pub priority: Option<i32>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateRequest {
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRuleRequest {
    /// Raw condition objects — parsed via json_parser so flat frontend payloads
    /// (without a `type` discriminant) are accepted alongside the canonical tagged form.
    pub conditions: Vec<serde_json::Value>,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    pub priority: i32,
    pub enabled: bool,
    pub match_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Rule> for RuleResponse {
    fn from(r: Rule) -> Self {
        Self {
            id: r.id,
            name: r.name,
            description: r.description,
            conditions: r.conditions,
            actions: r.actions,
            priority: r.priority,
            enabled: r.enabled,
            match_count: 0,
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResponse {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResponse {
    pub match_count: i64,
    pub sample_matches: Vec<SampleEmailMatch>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleEmailMatch {
    pub email_id: String,
    pub subject: String,
    pub from: String,
    pub received_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSuggestion {
    pub rule: RuleResponse,
    pub reason: String,
    pub estimated_matches: i64,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// ---------------------------------------------------------------------------
// Queries
//
// Each takes a `&DatabaseConnection` rather than `AppState` so the tests at the
// bottom of this file can drive it directly; the handlers pass `&state.orm`.
// ---------------------------------------------------------------------------

/// `match_count` per rule id.
///
/// The pre-port query read `COALESCE(match_count, 0)` and decoded it as `i64`.
/// The column is `INTEGER NOT NULL DEFAULT 0` (migration 026), so the COALESCE
/// never fired, and the `i64` decode was ADR-035's width class waiting to fail
/// on PostgreSQL, where INTEGER is INT4. The entity's `i32` settles the width;
/// widening to the response type's `i64` now happens here, in Rust.
async fn fetch_match_counts(conn: &DatabaseConnection) -> Result<HashMap<String, i64>, DbErr> {
    let rows: Vec<(String, i32)> = rules::Entity::find()
        .select_only()
        .column(rules::Column::Id)
        .column(rules::Column::MatchCount)
        .into_tuple()
        .all(conn)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, count)| (id, i64::from(count)))
        .collect())
}

/// One email row as the rule evaluator needs it.
#[derive(FromQueryResult)]
struct RuleEmailRow {
    id: String,
    /// Only the run path reads this — it resolves the owning account's provider.
    account_id: String,
    from_addr: String,
    to_addrs: String,
    subject: String,
    body_text: Option<String>,
    /// Nullable column, so a NULL reads as "no labels" rather than as an error.
    /// The pre-port `row.get::<String, _>("labels")` panicked on such a row;
    /// this matches the reading `rules::executor` already settled on.
    labels: Option<String>,
    /// Plain `TIMESTAMP` (no zone) in both dialects, hence `NaiveDateTime` —
    /// callers re-attach UTC.
    received_at: chrono::NaiveDateTime,
}

/// Every active (non-trash, non-spam, non-deleted) email, newest first.
///
/// Deliberately unbounded, as before the port: the rule-test count and the
/// suggestion histogram are both defined over the whole corpus, so a LIMIT here
/// would silently disagree with what the suggestions panel reports. The rule-test
/// path ignores `account_id`; sharing one projection with the run path keeps the
/// two corpora provably identical.
async fn fetch_active_emails(conn: &DatabaseConnection) -> Result<Vec<RuleEmailRow>, DbErr> {
    emails::Entity::find()
        .select_only()
        .column(emails::Column::Id)
        .column(emails::Column::AccountId)
        .column(emails::Column::FromAddr)
        .column(emails::Column::ToAddrs)
        .column(emails::Column::Subject)
        .column(emails::Column::BodyText)
        .column(emails::Column::Labels)
        .column(emails::Column::ReceivedAt)
        .filter(emails::Column::IsTrash.eq(0_i32))
        .filter(emails::Column::IsSpam.eq(0_i32))
        .filter(emails::Column::DeletedAt.is_null())
        .order_by_desc(emails::Column::ReceivedAt)
        .into_model::<RuleEmailRow>()
        .all(conn)
        .await
}

/// Add `matched` to a rule's `match_count` and stamp `last_run_at`.
///
/// Both columns belong to the manual-run path alone (`RuleEngine::save_rule`
/// leaves them untouched), and the increment stays one atomic UPDATE rather
/// than becoming a read-modify-write. `last_run_at` used to be bound as a
/// `to_rfc3339()` String — accepted by SQLite, a bind error against
/// PostgreSQL's TIMESTAMPTZ; binding the `DateTime<Utc>` the entity declares
/// fixes that and still encodes as the same RFC3339 text on SQLite (ADR-035's
/// timestamp class, §2.5/§2.6).
async fn record_rule_run(
    conn: &DatabaseConnection,
    rule_id: &str,
    matched: i64,
) -> Result<(), DbErr> {
    // The column is INTEGER (INT4 on PostgreSQL) while the run counter is i64
    // for the response shape, so the delta narrows at the bind.
    let delta = i32::try_from(matched).unwrap_or(i32::MAX);
    rules::Entity::update_many()
        .col_expr(
            rules::Column::MatchCount,
            Expr::col(rules::Column::MatchCount).add(delta),
        )
        .col_expr(rules::Column::LastRunAt, Expr::value(Utc::now()))
        .filter(rules::Column::Id.eq(rule_id))
        .exec(conn)
        .await?;
    Ok(())
}

/// A sender and how many active emails it has sent.
#[derive(FromQueryResult)]
struct SenderCount {
    from_addr: String,
    /// `COUNT(*)` is 8-byte on both backends.
    cnt: i64,
}

/// `COUNT(*)`, spelled out at each use site rather than aliased once.
///
/// PostgreSQL rejects select-alias references in HAVING, so the expression is
/// repeated in the SELECT list, the HAVING and the ORDER BY — which is what the
/// pre-port SQL did too.
fn count_star() -> Expr {
    Expr::from(Func::count(Expr::col(Asterisk)))
}

/// Senders with at least `min_count` active emails, most frequent first.
async fn fetch_sender_counts(
    conn: &DatabaseConnection,
    min_count: i64,
) -> Result<Vec<SenderCount>, DbErr> {
    sender_counts_query(min_count)
        .into_model::<SenderCount>()
        .all(conn)
        .await
}

/// The query behind [`fetch_sender_counts`], split out so a test can assert the
/// SQL text it produces for PostgreSQL without a live server.
fn sender_counts_query(min_count: i64) -> sea_orm::Select<emails::Entity> {
    emails::Entity::find()
        .select_only()
        .column(emails::Column::FromAddr)
        .column_as(count_star(), "cnt")
        .filter(emails::Column::IsTrash.eq(0_i32))
        .filter(emails::Column::IsSpam.eq(0_i32))
        .filter(emails::Column::DeletedAt.is_null())
        .filter(emails::Column::FromAddr.ne(""))
        .group_by(emails::Column::FromAddr)
        .having(count_star().gte(min_count))
        .order_by_desc(count_star())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/rules -- list all rules.
async fn list_rules(
    State(state): State<AppState>,
) -> Result<Json<Vec<RuleResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let loaded = RuleEngine::load_rules(&state.db)
        .await
        .map_err(internal_error)?;

    // Fetch match_count separately (column added in migration 026). A failure
    // reading it degrades to zeros rather than failing the whole list, as before.
    let counts = fetch_match_counts(&state.orm).await.unwrap_or_default();

    let mut engine = RuleEngine::new();
    engine.set_rules(loaded);

    let responses: Vec<RuleResponse> = engine
        .rules()
        .iter()
        .cloned()
        .map(|r| {
            let mc = *counts.get(&r.id).unwrap_or(&0);
            let mut resp = RuleResponse::from(r);
            resp.match_count = mc;
            resp
        })
        .collect();
    Ok(Json(responses))
}

/// POST /api/v1/rules -- create a new rule.
async fn create_rule(
    State(state): State<AppState>,
    Json(req): Json<CreateRuleRequest>,
) -> Result<(StatusCode, Json<RuleResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Parse conditions through the json_parser for normalisation.
    let mut conditions = Vec::new();

    // If a natural-language string is provided, parse it first.
    if let Some(ref nl_text) = req.natural_language {
        let nl_condition = json_parser::parse_natural_language(nl_text).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Failed to parse natural language condition: {e}"),
                }),
            )
        })?;
        conditions.push(nl_condition);
    }

    // Parse each JSON-supplied condition through `parse_condition` for
    // shorthand normalisation (e.g. loose frontend payloads).
    for raw_cond in &req.conditions {
        let json_val = serde_json::to_value(raw_cond).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Failed to serialise condition: {e}"),
                }),
            )
        })?;
        let parsed = json_parser::parse_condition(&json_val).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Failed to parse condition: {e}"),
                }),
            )
        })?;
        conditions.push(parsed);
    }

    let now = Utc::now();
    let rule = Rule {
        id: RuleEngine::new_id(),
        name: req.name,
        description: req.description,
        conditions,
        actions: req.actions,
        priority: req.priority,
        enabled: req.enabled,
        created_at: now,
        updated_at: now,
    };

    // Validate the single rule first.
    let warnings = rule_validator::validate_rule(&rule);
    if rule_validator::has_errors(&warnings) {
        let errors: Vec<String> = warnings
            .iter()
            .filter(|w| w.severity == Severity::Error)
            .map(|w| w.message.clone())
            .collect();
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: errors.join("; "),
            }),
        ));
    }

    RuleEngine::save_rule(&state.db, &rule)
        .await
        .map_err(internal_error)?;

    Ok((StatusCode::CREATED, Json(RuleResponse::from(rule))))
}

/// GET /api/v1/rules/:id -- get a single rule.
async fn get_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RuleResponse>, (StatusCode, Json<ErrorResponse>)> {
    let rule = RuleEngine::get_rule(&state.db, &id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("Rule '{id}' not found"),
                }),
            )
        })?;

    Ok(Json(RuleResponse::from(rule)))
}

/// PUT /api/v1/rules/:id -- update a rule.
async fn update_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateRuleRequest>,
) -> Result<Json<RuleResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut rule = RuleEngine::get_rule(&state.db, &id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("Rule '{id}' not found"),
                }),
            )
        })?;

    // Apply partial updates.
    if let Some(name) = req.name {
        rule.name = name;
    }
    if let Some(description) = req.description {
        rule.description = description;
    }
    if let Some(conditions) = req.conditions {
        rule.conditions = conditions;
    }
    if let Some(actions) = req.actions {
        rule.actions = actions;
    }
    if let Some(priority) = req.priority {
        rule.priority = priority;
    }
    if let Some(enabled) = req.enabled {
        rule.enabled = enabled;
    }
    rule.updated_at = Utc::now();

    // Validate.
    let warnings = rule_validator::validate_rule(&rule);
    if rule_validator::has_errors(&warnings) {
        let errors: Vec<String> = warnings
            .iter()
            .filter(|w| w.severity == Severity::Error)
            .map(|w| w.message.clone())
            .collect();
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: errors.join("; "),
            }),
        ));
    }

    RuleEngine::save_rule(&state.db, &rule)
        .await
        .map_err(internal_error)?;

    Ok(Json(RuleResponse::from(rule)))
}

/// DELETE /api/v1/rules/:id -- delete a rule.
async fn delete_rule_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let deleted = RuleEngine::delete_rule(&state.db, &id)
        .await
        .map_err(internal_error)?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Rule '{id}' not found"),
            }),
        ))
    }
}

/// POST /api/v1/rules/validate -- validate without saving.
///
/// Runs single-rule validation via `validate_rule()` and, when a database
/// connection is available, also performs cross-rule checks (including loop
/// detection) via `validate_rules()`.
async fn validate_rule(
    State(state): State<AppState>,
    Json(req): Json<ValidateRequest>,
) -> Json<ValidationResponse> {
    let rule = Rule {
        id: "validation-check".to_string(),
        name: if req.name.is_empty() {
            "Validation Check".to_string()
        } else {
            req.name
        },
        description: String::new(),
        conditions: req.conditions,
        actions: req.actions,
        priority: 0,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Single-rule validation.
    let mut findings = rule_validator::validate_rule(&rule);

    // Cross-rule validation (loop detection, etc.) against all persisted rules.
    if let Ok(existing_rules) = RuleEngine::load_rules(&state.db).await {
        let mut all_rules = existing_rules;
        all_rules.push(rule);
        let cross_findings = rule_validator::validate_rules(&all_rules);
        // Append only findings that reference our validation-check rule or are
        // cross-rule warnings (loop detection) to avoid duplicating per-rule
        // findings for existing rules.
        for f in cross_findings {
            if f.rule_id == "validation-check" || f.message.contains("loop") {
                findings.push(f);
            }
        }
    }

    let errors: Vec<String> = findings
        .iter()
        .filter(|w| w.severity == Severity::Error)
        .map(|w| w.message.clone())
        .collect();
    let warnings: Vec<String> = findings
        .iter()
        .filter(|w| w.severity == Severity::Warning)
        .map(|w| w.message.clone())
        .collect();

    Json(ValidationResponse {
        valid: errors.is_empty(),
        errors,
        warnings,
    })
}

/// POST /api/v1/rules/test -- test a rule against the local email corpus.
///
/// Accepts flat frontend condition payloads (no `type` discriminant required)
/// via json_parser, scans the inbox, and returns how many emails would match.
async fn test_rule(
    State(state): State<AppState>,
    Json(req): Json<TestRuleRequest>,
) -> Result<Json<TestResponse>, (StatusCode, Json<ErrorResponse>)> {
    use crate::rules::rule_processor::evaluate_rule;

    // Parse raw condition values — skip any that cannot be parsed.
    let conditions: Vec<RuleCondition> = req
        .conditions
        .iter()
        .filter_map(|v| {
            json_parser::parse_condition(v)
                .map_err(|e| tracing::warn!("Skipping unparseable condition: {e}"))
                .ok()
        })
        .collect();

    let now = Utc::now();
    let test_rule = Rule {
        id: "test-rule".to_string(),
        name: if req.name.is_empty() {
            "Test Rule".to_string()
        } else {
            req.name
        },
        description: String::new(),
        conditions,
        actions: vec![],
        priority: 0,
        enabled: true,
        created_at: now,
        updated_at: now,
    };

    // Query all active inbox emails for evaluation so the count matches
    // what the suggestions panel reports (both use the same unbounded corpus).
    let rows = fetch_active_emails(&state.orm).await.map_err(|e| {
        tracing::error!("Failed to fetch emails for rule test: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to evaluate rule against inbox".to_string(),
            }),
        )
    })?;

    let mut match_count: i64 = 0;
    let mut sample_matches: Vec<SampleEmailMatch> = Vec::new();

    for row in rows {
        let RuleEmailRow {
            id,
            from_addr,
            to_addrs,
            subject,
            body_text,
            labels,
            received_at,
            ..
        } = row;
        let labels = labels.unwrap_or_default();
        // Re-attach UTC to the entity's naive timestamp: same instant the
        // pre-port `DateTime<Utc>` decode produced, so the evaluator's date
        // comparisons and the RFC3339 strings below are unchanged.
        let received_at = received_at.and_utc();

        let email = crate::email::EmailMessage {
            id: id.clone(),
            thread_id: None,
            from: from_addr.clone(),
            to: to_addrs
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            subject: subject.clone(),
            snippet: String::new(),
            body: body_text,
            body_html: None,
            labels: labels
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            date: received_at,
            is_read: false,
            list_unsubscribe: None,
            list_unsubscribe_post: None,
        };

        if evaluate_rule(&test_rule, &email) {
            match_count += 1;
            if sample_matches.len() < 5 {
                sample_matches.push(SampleEmailMatch {
                    email_id: id,
                    subject,
                    from: from_addr,
                    received_at: received_at.to_rfc3339(),
                });
            }
        }
    }

    Ok(Json(TestResponse {
        match_count,
        sample_matches,
    }))
}

// ---------------------------------------------------------------------------
// Run rule
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRuleResponse {
    pub match_count: i64,
    pub executed_count: i64,
    pub sample_matches: Vec<SampleEmailMatch>,
}

/// POST /api/v1/rules/:id/run -- apply a single rule to the current inbox.
///
/// Evaluates the rule against all active emails, calls the email provider to
/// apply each action (archive, mark-read, etc.) on matched emails, then
/// persists the updated match_count to the rule row.
async fn run_rule(
    State(state): State<AppState>,
    Path(rule_id): Path<String>,
) -> Result<Json<RunRuleResponse>, (StatusCode, Json<ErrorResponse>)> {
    use crate::api::provider_helpers::resolve_provider_and_token;
    use crate::rules::executor::apply_rule_action;
    use crate::rules::rule_processor::evaluate_rule;

    let rule = RuleEngine::get_rule(&state.db, &rule_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("Rule '{rule_id}' not found"),
                }),
            )
        })?;

    let rows = fetch_active_emails(&state.orm).await.map_err(|e| {
        tracing::error!("Failed to fetch emails for rule run: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to run rule against inbox".to_string(),
            }),
        )
    })?;

    let mut match_count: i64 = 0;
    let mut executed_count: i64 = 0;
    let mut sample_matches: Vec<SampleEmailMatch> = Vec::new();

    for row in rows {
        let RuleEmailRow {
            id: email_id,
            account_id,
            from_addr,
            to_addrs,
            subject,
            body_text,
            labels,
            received_at,
        } = row;
        let labels = labels.unwrap_or_default();
        // Re-attach UTC to the entity's naive timestamp — see `test_rule`.
        let received_at = received_at.and_utc();

        let email = crate::email::EmailMessage {
            id: email_id.clone(),
            thread_id: None,
            from: from_addr.clone(),
            to: to_addrs
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            subject: subject.clone(),
            snippet: String::new(),
            body: body_text,
            body_html: None,
            labels: labels
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            date: received_at,
            is_read: false,
            list_unsubscribe: None,
            list_unsubscribe_post: None,
        };

        if !evaluate_rule(&rule, &email) {
            continue;
        }

        match_count += 1;

        // Apply each action: provider call (best-effort) then local DB update.
        for action in &rule.actions {
            // Provider-level action — errors are logged but don't abort the run.
            if let Ok((provider, token, _)) = resolve_provider_and_token(&state, &account_id).await
            {
                let provider_result = match action {
                    RuleAction::Archive => provider.archive_message(&token, &email_id).await,
                    RuleAction::MarkRead => provider.mark_read(&token, &email_id, true).await,
                    RuleAction::MarkImportant => {
                        provider.star_message(&token, &email_id, true).await
                    }
                    RuleAction::Delete { permanent: true } => {
                        provider.delete_message(&token, &email_id).await
                    }
                    RuleAction::Delete { .. } => provider
                        .move_message(
                            &token,
                            &email_id,
                            "TRASH",
                            crate::email::provider::MoveKind::Folder,
                        )
                        .await
                        .map(|_| ()),
                    // AddLabel / RemoveLabel / Forward: local-only or not yet wired to provider.
                    _ => Ok(()),
                };
                if let Err(e) = provider_result {
                    tracing::warn!(
                        email_id = %email_id,
                        "Provider action failed (applying locally): {e}"
                    );
                }
            }

            // Local DB update (idempotent).
            if apply_rule_action(&state.db, &email_id, action)
                .await
                .is_ok()
            {
                executed_count += 1;
            }
        }

        if sample_matches.len() < 5 {
            sample_matches.push(SampleEmailMatch {
                email_id,
                subject,
                from: from_addr,
                received_at: received_at.to_rfc3339(),
            });
        }
    }

    // Persist updated match count back to the rule. Best-effort, as before: a
    // failed counter write must not fail a run that already touched the mailbox.
    let _ = record_rule_run(&state.orm, &rule_id, match_count).await;

    Ok(Json(RunRuleResponse {
        match_count,
        executed_count,
        sample_matches,
    }))
}

/// Query parameters for the suggestions endpoint.
#[derive(Debug, serde::Deserialize, Default)]
pub struct SuggestionsParams {
    /// Number of already-seen suggestions to skip (for batch loading).
    #[serde(default)]
    pub offset: usize,
    /// Max suggestions to return. Defaults to `rules.suggestions_page_size` from config.
    pub limit: Option<usize>,
}

/// GET /api/v1/rules/suggestions -- email-pattern-driven rule suggestions.
///
/// Returns the next batch of `limit` suggestions starting after `offset`
/// already-seen ones. Callers accumulate batches to build the full list.
async fn get_suggestions(
    State(state): State<AppState>,
    Query(params): Query<SuggestionsParams>,
) -> Result<Json<Vec<RuleSuggestion>>, (StatusCode, Json<ErrorResponse>)> {
    let cfg = &state.yaml_config.app.rules;
    let page_size = params.limit.unwrap_or(cfg.suggestions_page_size as usize);
    let min_count = cfg.suggestions_min_email_count as i64;

    let existing = RuleEngine::load_rules(&state.db)
        .await
        .map_err(internal_error)?;

    let covered: Vec<String> = existing
        .iter()
        .flat_map(|r| collect_from_values(&r.conditions))
        .collect();

    // Fetch all qualifying senders (unbounded) so Rust-side pagination is correct
    // even after filtering out already-covered senders.
    let rows = fetch_sender_counts(&state.orm, min_count)
        .await
        .map_err(|e| {
            tracing::error!("Failed to query email patterns for suggestions: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to generate suggestions".to_string(),
                }),
            )
        })?;

    let now = Utc::now();
    let suggestions: Vec<RuleSuggestion> = rows
        .into_iter()
        .filter_map(|row| {
            let SenderCount { from_addr, cnt } = row;

            if covered
                .iter()
                .any(|c| from_addr.to_lowercase().contains(c.as_str()))
            {
                return None;
            }

            let (action, reason) = if cnt >= 20 {
                (
                    RuleAction::Archive,
                    format!(
                        "You have {cnt} emails from this sender. \
                         Archiving them automatically keeps your inbox clean."
                    ),
                )
            } else {
                (
                    RuleAction::MarkRead,
                    format!(
                        "You have {cnt} emails from this sender. \
                         Marking them read reduces notification noise."
                    ),
                )
            };

            Some(RuleSuggestion {
                rule: RuleResponse {
                    id: RuleEngine::new_id(),
                    name: format!("Manage emails from {from_addr}"),
                    description: format!("Auto-suggested: {cnt} emails from this sender"),
                    conditions: vec![RuleCondition::FieldMatch {
                        field: EmailField::From,
                        operator: MatchOperator::Contains,
                        value: from_addr,
                    }],
                    actions: vec![action],
                    priority: 0,
                    enabled: true,
                    match_count: 0,
                    created_at: now.to_rfc3339(),
                    updated_at: now.to_rfc3339(),
                },
                reason,
                estimated_matches: cnt,
            })
        })
        .skip(params.offset)
        .take(page_size)
        .collect();

    Ok(Json(suggestions))
}

fn collect_from_values(conditions: &[RuleCondition]) -> Vec<String> {
    let mut out = Vec::new();
    for c in conditions {
        collect_condition_from_values(c, &mut out);
    }
    out
}

fn collect_condition_from_values(c: &RuleCondition, out: &mut Vec<String>) {
    match c {
        RuleCondition::FieldMatch {
            field: EmailField::From,
            value,
            ..
        } => out.push(value.to_lowercase()),
        RuleCondition::And { conditions } | RuleCondition::Or { conditions } => {
            for sub in conditions {
                collect_condition_from_values(sub, out);
            }
        }
        RuleCondition::Not { condition } => collect_condition_from_values(condition, out),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn internal_error(e: anyhow::Error) -> (StatusCode, Json<ErrorResponse>) {
    tracing::error!("Internal error: {e:#}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "Internal server error".to_string(),
        }),
    )
}

// ---------------------------------------------------------------------------
// Tests
//
// This module had no coverage at all before the SeaORM port (ADR-036), so these
// pin the four queries the handlers wrap — the handlers themselves need a full
// `AppState`, which is why the queries take a bare connection. What they assert
// is derived from the pre-port SQL text: `SELECT id, COALESCE(match_count, 0)`,
// the `is_trash = 0 AND is_spam = 0 AND deleted_at IS NULL / ORDER BY
// received_at DESC` corpus, `match_count = match_count + ?` with `last_run_at`,
// and the `GROUP BY from_addr HAVING COUNT(*) >= ?` histogram.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use chrono::{DateTime, Duration, NaiveDateTime};
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DbBackend, QueryTrait};
    use sqlx::sqlite::SqlitePoolOptions;

    /// In-memory SQLite carrying every migration the two entities span: 001
    /// creates `emails` and 016/018/021/027 add the columns the entity declares;
    /// 012 creates `rules` and 026 adds `match_count`/`last_run_at`.
    async fn fresh_db() -> Database {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("connect");
        let db = Database::Sqlite(pool);
        let conn = db.sea_orm();
        for raw in [
            include_str!("../../migrations/sqlite/001_initial_schema.sql"),
            include_str!("../../migrations/sqlite/012_rules.sql"),
            include_str!("../../migrations/sqlite/016_soft_delete_trash_spam.sql"),
            include_str!("../../migrations/sqlite/018_unsubscribe_headers.sql"),
            include_str!("../../migrations/sqlite/021_thread_key.sql"),
            include_str!("../../migrations/sqlite/026_rules_match_count.sql"),
            include_str!("../../migrations/sqlite/027_is_archived.sql"),
        ] {
            // Strip line comments before splitting on ';'.
            let cleaned: String = raw
                .lines()
                .map(|l| {
                    if let Some(idx) = l.find("--") {
                        &l[..idx]
                    } else {
                        l
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            for stmt in cleaned.split(';') {
                let s = stmt.trim();
                if !s.is_empty() {
                    conn.execute_unprepared(s).await.expect("migrate");
                }
            }
        }
        db
    }

    async fn seed_rule(conn: &DatabaseConnection, id: &str, match_count: i32) {
        rules::ActiveModel {
            id: Set(id.to_owned()),
            name: Set(format!("rule {id}")),
            description: Set(Some(String::new())),
            conditions_json: Set("[]".to_owned()),
            actions_json: Set("[]".to_owned()),
            priority: Set(Some(0)),
            enabled: Set(Some(1)),
            created_at: Set(Some(Utc::now())),
            updated_at: Set(Some(Utc::now())),
            match_count: Set(match_count),
            last_run_at: Set(None),
        }
        .insert(conn)
        .await
        .expect("seed rule");
    }

    /// Seed one active INBOX email. `labels` is `None` to store SQL NULL.
    async fn seed_email(
        conn: &DatabaseConnection,
        id: &str,
        from_addr: &str,
        received_at: DateTime<Utc>,
        labels: Option<&str>,
    ) {
        emails::ActiveModel {
            id: Set(id.to_owned()),
            account_id: Set("acct-1".to_owned()),
            provider: Set("gmail".to_owned()),
            subject: Set(format!("subject {id}")),
            from_addr: Set(from_addr.to_owned()),
            to_addrs: Set("me@example.com".to_owned()),
            body_text: Set(Some(format!("body {id}"))),
            received_at: Set(received_at.naive_utc()),
            labels: Set(labels.map(str::to_owned)),
            is_read: Set(Some(false)),
            is_starred: Set(Some(false)),
            is_spam: Set(0),
            is_trash: Set(0),
            folder: Set("INBOX".to_owned()),
            is_archived: Set(false),
            ..Default::default()
        }
        .insert(conn)
        .await
        .expect("seed email");
    }

    /// Take a seeded email out of the active corpus the way the app does:
    /// `is_trash`/`is_spam` are INTEGER flags, `deleted_at` is an RFC3339 TEXT.
    async fn hide_email(
        conn: &DatabaseConnection,
        id: &str,
        is_trash: i32,
        is_spam: i32,
        deleted_at: Option<&str>,
    ) {
        emails::Entity::update_many()
            .col_expr(emails::Column::IsTrash, Expr::value(is_trash))
            .col_expr(emails::Column::IsSpam, Expr::value(is_spam))
            .col_expr(
                emails::Column::DeletedAt,
                Expr::value(deleted_at.map(str::to_owned)),
            )
            .filter(emails::Column::Id.eq(id))
            .exec(conn)
            .await
            .expect("hide email");
    }

    fn at(minute: u32) -> DateTime<Utc> {
        NaiveDateTime::parse_from_str(
            &format!("2026-01-02 03:{minute:02}:00"),
            "%Y-%m-%d %H:%M:%S",
        )
        .expect("timestamp")
        .and_utc()
    }

    #[tokio::test]
    async fn match_counts_are_keyed_by_rule_id_and_widened_to_the_response_type() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_rule(&conn, "r-1", 7).await;
        seed_rule(&conn, "r-2", 0).await;

        let counts = fetch_match_counts(&conn).await.expect("counts");

        assert_eq!(counts.len(), 2);
        // i64 is the response type's width; the column is INTEGER (INT4 on
        // PostgreSQL), which is what the old `r.get::<i64, _>("mc")` got wrong.
        assert_eq!(counts.get("r-1"), Some(&7_i64));
        assert_eq!(counts.get("r-2"), Some(&0_i64));
        assert_eq!(counts.get("absent"), None);
    }

    #[tokio::test]
    async fn record_rule_run_increments_match_count_and_stamps_last_run_at() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_rule(&conn, "r-1", 3).await;
        seed_rule(&conn, "r-2", 5).await;

        let before = Utc::now() - Duration::seconds(1);
        record_rule_run(&conn, "r-1", 4).await.expect("record run");
        let after = Utc::now() + Duration::seconds(1);

        let updated = rules::Entity::find_by_id("r-1")
            .one(&conn)
            .await
            .expect("query")
            .expect("row present");
        assert_eq!(updated.match_count, 7, "the increment must accumulate");
        // The pre-port bind was a `to_rfc3339()` String into a column that is
        // TIMESTAMPTZ on PostgreSQL; binding the entity's DateTime<Utc> both
        // fixes that and still round-trips through SQLite's TEXT storage.
        let last_run = updated.last_run_at.expect("last_run_at stamped");
        assert!(
            last_run >= before && last_run <= after,
            "last_run_at {last_run} outside [{before}, {after}]"
        );

        let bystander = rules::Entity::find_by_id("r-2")
            .one(&conn)
            .await
            .expect("query")
            .expect("row present");
        assert_eq!(bystander.match_count, 5, "only the target rule advances");
        assert!(bystander.last_run_at.is_none());
    }

    #[tokio::test]
    async fn active_email_corpus_excludes_trash_spam_and_soft_deleted_newest_first() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e-old", "a@example.com", at(1), Some("")).await;
        seed_email(&conn, "e-new", "a@example.com", at(3), Some("")).await;
        seed_email(&conn, "e-mid", "a@example.com", at(2), Some("")).await;
        seed_email(&conn, "e-trash", "a@example.com", at(4), Some("")).await;
        seed_email(&conn, "e-spam", "a@example.com", at(5), Some("")).await;
        seed_email(&conn, "e-deleted", "a@example.com", at(6), Some("")).await;
        hide_email(&conn, "e-trash", 1, 0, None).await;
        hide_email(&conn, "e-spam", 0, 1, None).await;
        hide_email(&conn, "e-deleted", 0, 0, Some("2026-01-02T04:00:00+00:00")).await;

        let rows = fetch_active_emails(&conn).await.expect("corpus");

        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["e-new", "e-mid", "e-old"]
        );
    }

    #[tokio::test]
    async fn active_email_rows_carry_utc_instants_and_read_null_labels_as_empty() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e-1", "sender@example.com", at(7), None).await;

        let rows = fetch_active_emails(&conn).await.expect("corpus");
        let row = rows.first().expect("one row");

        assert_eq!(row.account_id, "acct-1");
        assert_eq!(row.from_addr, "sender@example.com");
        assert_eq!(row.to_addrs, "me@example.com");
        assert_eq!(row.subject, "subject e-1");
        assert_eq!(row.body_text.as_deref(), Some("body e-1"));
        // A NULL `labels` is "no labels", not a decode error — the handlers call
        // `unwrap_or_default()` and split the empty string into no labels.
        assert_eq!(row.labels, None);
        // The naive column re-attached to UTC is the instant that was written,
        // and the RFC3339 string the API returns is unchanged by the port.
        assert_eq!(row.received_at.and_utc(), at(7));
        assert_eq!(
            row.received_at.and_utc().to_rfc3339(),
            "2026-01-02T03:07:00+00:00"
        );
    }

    #[tokio::test]
    async fn sender_counts_group_by_sender_honour_the_minimum_and_sort_by_frequency() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        for (i, sender) in [
            "loud@example.com",
            "loud@example.com",
            "loud@example.com",
            "quiet@example.com",
            "quiet@example.com",
            "rare@example.com",
        ]
        .iter()
        .enumerate()
        {
            seed_email(&conn, &format!("e-{i}"), sender, at(i as u32), Some("")).await;
        }
        // An empty sender is excluded by `from_addr != ''`, and trashed mail is
        // not counted even when the sender otherwise qualifies.
        seed_email(&conn, "e-blank", "", at(10), Some("")).await;
        seed_email(&conn, "e-blank-2", "", at(11), Some("")).await;
        seed_email(&conn, "e-hidden", "loud@example.com", at(12), Some("")).await;
        hide_email(&conn, "e-hidden", 1, 0, None).await;

        let rows = fetch_sender_counts(&conn, 2).await.expect("histogram");

        assert_eq!(
            rows.iter()
                .map(|r| (r.from_addr.as_str(), r.cnt))
                .collect::<Vec<_>>(),
            [("loud@example.com", 3_i64), ("quiet@example.com", 2_i64)],
            "below-minimum and blank senders drop out; most frequent first"
        );
    }

    #[test]
    fn sender_counts_query_repeats_count_star_rather_than_naming_the_select_alias() {
        // PostgreSQL rejects `HAVING cnt >= n` — a select alias is not in scope
        // there — so the aggregate has to be spelled out again. SQLite accepts
        // both, which is exactly why this can only be caught by reading the SQL.
        let sql = sender_counts_query(5)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("COUNT(*) AS \"cnt\""), "{sql}");
        assert!(sql.contains("HAVING COUNT(*) >= 5"), "{sql}");
        assert!(sql.contains("ORDER BY COUNT(*) DESC"), "{sql}");
        assert!(!sql.contains("HAVING \"cnt\""), "{sql}");
    }
}
