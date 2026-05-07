//! REST API routes for the Rules Engine (R-03).
//!
//! - GET    /api/v1/rules          -- list all rules
//! - POST   /api/v1/rules          -- create a rule
//! - GET    /api/v1/rules/:id      -- get a single rule
//! - PUT    /api/v1/rules/:id      -- update a rule
//! - DELETE /api/v1/rules/:id      -- delete a rule
//! - POST   /api/v1/rules/validate -- validate a rule without saving
//! - POST   /api/v1/rules/test     -- test a rule against a sample email

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::Row;

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
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/rules -- list all rules.
async fn list_rules(
    State(state): State<AppState>,
) -> Result<Json<Vec<RuleResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let loaded = RuleEngine::load_rules(&state.db.pool)
        .await
        .map_err(internal_error)?;

    let mut engine = RuleEngine::new();
    engine.set_rules(loaded);

    let responses: Vec<RuleResponse> = engine
        .rules()
        .iter()
        .cloned()
        .map(RuleResponse::from)
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

    RuleEngine::save_rule(&state.db.pool, &rule)
        .await
        .map_err(internal_error)?;

    Ok((StatusCode::CREATED, Json(RuleResponse::from(rule))))
}

/// GET /api/v1/rules/:id -- get a single rule.
async fn get_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RuleResponse>, (StatusCode, Json<ErrorResponse>)> {
    let rule = RuleEngine::get_rule(&state.db.pool, &id)
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
    let mut rule = RuleEngine::get_rule(&state.db.pool, &id)
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

    RuleEngine::save_rule(&state.db.pool, &rule)
        .await
        .map_err(internal_error)?;

    Ok(Json(RuleResponse::from(rule)))
}

/// DELETE /api/v1/rules/:id -- delete a rule.
async fn delete_rule_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let deleted = RuleEngine::delete_rule(&state.db.pool, &id)
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
    if let Ok(existing_rules) = RuleEngine::load_rules(&state.db.pool).await {
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
    let rows = sqlx::query(
        "SELECT id, from_addr, to_addrs, subject, body_text, labels, received_at \
         FROM emails \
         WHERE is_trash = 0 AND is_spam = 0 AND deleted_at IS NULL \
         ORDER BY received_at DESC",
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| {
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

    for row in &rows {
        let id: String = row.get("id");
        let from_addr: String = row.get("from_addr");
        let to_addrs: String = row.get("to_addrs");
        let subject: String = row.get("subject");
        let body_text: Option<String> = row.get("body_text");
        let labels: String = row.get("labels");
        let received_at: chrono::DateTime<Utc> = row.get("received_at");

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
/// Evaluates the rule against all active (non-trash, non-spam) emails and
/// applies each action to every matching email via direct DB writes.
/// Returns counts and a sample of affected emails.
async fn run_rule(
    State(state): State<AppState>,
    Path(rule_id): Path<String>,
) -> Result<Json<RunRuleResponse>, (StatusCode, Json<ErrorResponse>)> {
    use crate::rules::executor::apply_rule_action;
    use crate::rules::rule_processor::evaluate_rule;

    let rule = RuleEngine::get_rule(&state.db.pool, &rule_id)
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

    let rows = sqlx::query(
        "SELECT id, from_addr, to_addrs, subject, body_text, labels, received_at \
         FROM emails \
         WHERE is_trash = 0 AND is_spam = 0 AND deleted_at IS NULL \
         ORDER BY received_at DESC",
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| {
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

    for row in &rows {
        let email_id: String = row.get("id");
        let from_addr: String = row.get("from_addr");
        let to_addrs: String = row.get("to_addrs");
        let subject: String = row.get("subject");
        let body_text: Option<String> = row.get("body_text");
        let labels: String = row.get("labels");
        let received_at: chrono::DateTime<Utc> = row.get("received_at");

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

        if evaluate_rule(&rule, &email) {
            match_count += 1;
            for action in &rule.actions {
                if apply_rule_action(&state.db.pool, &email_id, action)
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
    }

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

    let existing = RuleEngine::load_rules(&state.db.pool)
        .await
        .map_err(internal_error)?;

    let covered: Vec<String> = existing
        .iter()
        .flat_map(|r| collect_from_values(&r.conditions))
        .collect();

    // Fetch all qualifying senders (unbounded) so Rust-side pagination is correct
    // even after filtering out already-covered senders.
    let rows = sqlx::query(
        "SELECT from_addr, COUNT(*) as cnt \
         FROM emails \
         WHERE is_trash = 0 AND is_spam = 0 AND deleted_at IS NULL AND from_addr != '' \
         GROUP BY from_addr \
         HAVING COUNT(*) >= ? \
         ORDER BY COUNT(*) DESC",
    )
    .bind(min_count)
    .fetch_all(&state.db.pool)
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
        .iter()
        .filter_map(|row| {
            let from_addr: String = row.get("from_addr");
            let cnt: i64 = row.get("cnt");

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
