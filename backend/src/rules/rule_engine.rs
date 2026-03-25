//! Rule engine service -- CRUD operations backed by SQLite (R-03).
//!
//! `RuleEngine` holds rules in memory and persists them to the `rules` table.
//! It delegates evaluation to `rule_processor` and validation to `rule_validator`.

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::{Row, SqlitePool};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::types::{Rule, RuleAction, RuleCondition};

/// In-memory rule engine backed by SQLite persistence.
pub struct RuleEngine {
    rules: Vec<Rule>,
}

impl RuleEngine {
    /// Create an empty engine (rules are loaded separately via `load_rules`).
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Load all rules from the database.
    pub async fn load_rules(pool: &SqlitePool) -> Result<Vec<Rule>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, conditions_json, actions_json,
                   priority, enabled, created_at, updated_at
            FROM rules
            ORDER BY priority DESC
            "#,
        )
        .fetch_all(pool)
        .await
        .context("Failed to load rules from database")?;

        let mut rules = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.get("id");
            let name: String = row.get("name");
            let description: String = row.get("description");
            let conditions_json: String = row.get("conditions_json");
            let actions_json: String = row.get("actions_json");
            let priority: i32 = row.get("priority");
            let enabled: bool = row.get("enabled");
            let created_at: chrono::DateTime<Utc> = row.get("created_at");
            let updated_at: chrono::DateTime<Utc> = row.get("updated_at");

            let conditions: Vec<RuleCondition> = serde_json::from_str(&conditions_json)
                .with_context(|| format!("Failed to deserialise conditions for rule '{id}'"))?;

            let actions: Vec<RuleAction> = serde_json::from_str(&actions_json)
                .with_context(|| format!("Failed to deserialise actions for rule '{id}'"))?;

            rules.push(Rule {
                id,
                name,
                description,
                conditions,
                actions,
                priority,
                enabled,
                created_at,
                updated_at,
            });
        }

        info!(count = rules.len(), "Rules loaded from database");
        Ok(rules)
    }

    /// Save (insert or update) a rule to the database.
    pub async fn save_rule(pool: &SqlitePool, rule: &Rule) -> Result<()> {
        let conditions_json = serde_json::to_string(&rule.conditions)
            .context("Failed to serialise rule conditions")?;
        let actions_json =
            serde_json::to_string(&rule.actions).context("Failed to serialise rule actions")?;

        sqlx::query(
            r#"
            INSERT INTO rules (id, name, description, conditions_json, actions_json, priority, enabled, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                conditions_json = excluded.conditions_json,
                actions_json = excluded.actions_json,
                priority = excluded.priority,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&rule.id)
        .bind(&rule.name)
        .bind(&rule.description)
        .bind(&conditions_json)
        .bind(&actions_json)
        .bind(rule.priority)
        .bind(rule.enabled)
        .bind(rule.created_at)
        .bind(rule.updated_at)
        .execute(pool)
        .await
        .context("Failed to save rule")?;

        debug!(rule_id = %rule.id, "Rule saved to database");
        Ok(())
    }

    /// Delete a rule by ID.
    pub async fn delete_rule(pool: &SqlitePool, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM rules WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .context("Failed to delete rule")?;

        let deleted = result.rows_affected() > 0;
        if deleted {
            info!(rule_id = %id, "Rule deleted");
        } else {
            warn!(rule_id = %id, "Rule not found for deletion");
        }
        Ok(deleted)
    }

    /// Get a single rule by ID.
    pub async fn get_rule(pool: &SqlitePool, id: &str) -> Result<Option<Rule>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, description, conditions_json, actions_json,
                   priority, enabled, created_at, updated_at
            FROM rules
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("Failed to fetch rule")?;

        match row {
            Some(row) => {
                let conditions: Vec<RuleCondition> =
                    serde_json::from_str(row.get("conditions_json"))?;
                let actions: Vec<RuleAction> = serde_json::from_str(row.get("actions_json"))?;

                Ok(Some(Rule {
                    id: row.get("id"),
                    name: row.get("name"),
                    description: row.get("description"),
                    conditions,
                    actions,
                    priority: row.get("priority"),
                    enabled: row.get("enabled"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                }))
            }
            None => Ok(None),
        }
    }

    // -- In-memory helpers (useful for batch evaluation) --

    /// Replace the in-memory rule set.
    pub fn set_rules(&mut self, rules: Vec<Rule>) {
        self.rules = rules;
    }

    /// Get a reference to the in-memory rules.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Generate a new UUID-based rule ID.
    pub fn new_id() -> String {
        Uuid::new_v4().to_string()
    }
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::types::*;

    #[test]
    fn new_engine_is_empty() {
        let engine = RuleEngine::new();
        assert!(engine.rules().is_empty());
    }

    #[test]
    fn set_and_get_rules() {
        let mut engine = RuleEngine::new();
        let rules = vec![Rule {
            id: RuleEngine::new_id(),
            name: "Test".to_string(),
            description: String::new(),
            conditions: vec![RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: "test".to_string(),
            }],
            actions: vec![RuleAction::MarkRead],
            priority: 0,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        engine.set_rules(rules);
        assert_eq!(engine.rules().len(), 1);
    }

    #[test]
    fn new_id_is_valid_uuid() {
        let id = RuleEngine::new_id();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }
}
