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
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::rules::rule_engine::RuleEngine;
use crate::rules::rule_validator::{self, Severity};
use crate::rules::types::{Rule, RuleAction, RuleCondition};

use crate::AppState;

/// Build the rules API router.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_rules).post(create_rule))
        .route("/{id}", get(get_rule).put(update_rule).delete(delete_rule_handler))
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
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
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
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    pub email: TestEmail,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestEmail {
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub labels: Vec<String>,
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
    pub matched: bool,
    pub pending_actions: Vec<PendingActionResponse>,
    pub validation: ValidationResponse,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingActionResponse {
    pub action_type: String,
    pub details: serde_json::Value,
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
    let rules = RuleEngine::load_rules(&state.db.pool)
        .await
        .map_err(internal_error)?;

    let responses: Vec<RuleResponse> = rules.into_iter().map(RuleResponse::from).collect();
    Ok(Json(responses))
}

/// POST /api/v1/rules -- create a new rule.
async fn create_rule(
    State(state): State<AppState>,
    Json(req): Json<CreateRuleRequest>,
) -> Result<(StatusCode, Json<RuleResponse>), (StatusCode, Json<ErrorResponse>)> {
    let now = Utc::now();
    let rule = Rule {
        id: RuleEngine::new_id(),
        name: req.name,
        description: req.description,
        conditions: req.conditions,
        actions: req.actions,
        priority: req.priority,
        enabled: req.enabled,
        created_at: now,
        updated_at: now,
    };

    // Validate before saving.
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
async fn validate_rule(
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

    let findings = rule_validator::validate_rule(&rule);
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

/// POST /api/v1/rules/test -- test a rule against a sample email.
async fn test_rule(
    Json(req): Json<TestRuleRequest>,
) -> Json<TestResponse> {
    use crate::rules::rule_processor;
    use crate::rules::types::PendingAction;

    let now = Utc::now();
    let rule = Rule {
        id: "test-rule".to_string(),
        name: "Test Rule".to_string(),
        description: String::new(),
        conditions: req.conditions.clone(),
        actions: req.actions.clone(),
        priority: 0,
        enabled: true,
        created_at: now,
        updated_at: now,
    };

    // Validate first.
    let findings = rule_validator::validate_rule(&rule);
    let errors: Vec<String> = findings
        .iter()
        .filter(|w| w.severity == Severity::Error)
        .map(|w| w.message.clone())
        .collect();
    let warnings_list: Vec<String> = findings
        .iter()
        .filter(|w| w.severity == Severity::Warning)
        .map(|w| w.message.clone())
        .collect();

    // Build a minimal EmailMessage from the test input.
    let email = crate::email::EmailMessage {
        id: "test-msg".to_string(),
        thread_id: None,
        subject: req.email.subject,
        from: req.email.from,
        to: req.email.to,
        snippet: String::new(),
        body: if req.email.body.is_empty() {
            None
        } else {
            Some(req.email.body)
        },
        labels: req.email.labels,
        date: now,
        is_read: false,
    };

    let matched = rule_processor::evaluate_rule(&rule, &email);
    let pending: Vec<PendingAction> = if matched {
        rule_processor::apply_actions(&rule.actions, &email.id, &rule.id, &rule.name)
    } else {
        vec![]
    };

    let action_responses: Vec<PendingActionResponse> = pending
        .iter()
        .map(|p| PendingActionResponse {
            action_type: p.action.action_type().to_string(),
            details: serde_json::to_value(&p.action).unwrap_or_default(),
        })
        .collect();

    Json(TestResponse {
        matched,
        pending_actions: action_responses,
        validation: ValidationResponse {
            valid: errors.is_empty(),
            errors,
            warnings: warnings_list,
        },
    })
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
