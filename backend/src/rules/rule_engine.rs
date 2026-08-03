//! Rule engine service -- CRUD operations backed by SQLite (R-03).
//!
//! `RuleEngine` holds rules in memory and persists them to the `rules` table.
//! It delegates evaluation to `rule_processor` and validation to `rule_validator`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::types::{Rule, RuleAction, RuleCondition};
use crate::db::{audited_sql, Database};

/// Portable decode target for one `rules` row across both backends — `enabled` is
/// decoded as its raw INTEGER (0/1) rather than `bool` because the column is
/// INTEGER (not BOOLEAN) in both dialects, and Postgres won't implicitly coerce
/// an integer literal into a `bool`-typed bind on the way back in either
/// (see `bind_enabled` below). See ADR-035.
type RuleRow = (
    String,
    String,
    String,
    String,
    String,
    i32,
    i32,
    DateTime<Utc>,
    DateTime<Utc>,
);

fn row_to_rule(row: RuleRow) -> Result<Rule> {
    let (
        id,
        name,
        description,
        conditions_json,
        actions_json,
        priority,
        enabled,
        created_at,
        updated_at,
    ) = row;
    let conditions: Vec<RuleCondition> = serde_json::from_str(&conditions_json)
        .with_context(|| format!("Failed to deserialise conditions for rule '{id}'"))?;
    let actions: Vec<RuleAction> = serde_json::from_str(&actions_json)
        .with_context(|| format!("Failed to deserialise actions for rule '{id}'"))?;
    Ok(Rule {
        id,
        name,
        description,
        conditions,
        actions,
        priority,
        enabled: enabled != 0,
        created_at,
        updated_at,
    })
}

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
    pub async fn load_rules(db: &Database) -> Result<Vec<Rule>> {
        let sql = "SELECT id, name, description, conditions_json, actions_json, \
                   priority, enabled, created_at, updated_at \
                   FROM rules ORDER BY priority DESC";
        let rows: Vec<RuleRow> = match db {
            Database::Sqlite(pool) => sqlx::query_as(sql).fetch_all(pool).await,
            Database::Postgres(pool) => sqlx::query_as(sql).fetch_all(pool).await,
        }
        .context("Failed to load rules from database")?;

        let rules = rows
            .into_iter()
            .map(row_to_rule)
            .collect::<Result<Vec<_>>>()?;

        info!(count = rules.len(), "Rules loaded from database");
        Ok(rules)
    }

    /// Save (insert or update) a rule to the database.
    pub async fn save_rule(db: &Database, rule: &Rule) -> Result<()> {
        let conditions_json = serde_json::to_string(&rule.conditions)
            .context("Failed to serialise rule conditions")?;
        let actions_json =
            serde_json::to_string(&rule.actions).context("Failed to serialise rule actions")?;

        // `INSERT ... ON CONFLICT(id) DO UPDATE SET ... = excluded....` is valid upsert
        // syntax in both SQLite (3.24+) and PostgreSQL, so this goes through the ordinary
        // placeholder-adapting path — only `enabled`'s bind type differs per backend,
        // since the column is INTEGER (not BOOLEAN) in both dialects and Postgres refuses
        // to implicitly coerce a `bool`-typed bind into an integer column (see ADR-035).
        let sql = db.adapt(
            r#"INSERT INTO rules (id, name, description, conditions_json, actions_json, priority, enabled, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name,
                   description = excluded.description,
                   conditions_json = excluded.conditions_json,
                   actions_json = excluded.actions_json,
                   priority = excluded.priority,
                   enabled = excluded.enabled,
                   updated_at = excluded.updated_at"#,
        );
        match db {
            Database::Sqlite(pool) => sqlx::query(audited_sql(&sql))
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
                .map(|_| ()),
            Database::Postgres(pool) => sqlx::query(audited_sql(&sql))
                .bind(&rule.id)
                .bind(&rule.name)
                .bind(&rule.description)
                .bind(&conditions_json)
                .bind(&actions_json)
                .bind(rule.priority)
                .bind(rule.enabled as i32)
                .bind(rule.created_at)
                .bind(rule.updated_at)
                .execute(pool)
                .await
                .map(|_| ()),
        }
        .context("Failed to save rule")?;

        debug!(rule_id = %rule.id, "Rule saved to database");
        Ok(())
    }

    /// Delete a rule by ID.
    pub async fn delete_rule(db: &Database, id: &str) -> Result<bool> {
        let sql = db.adapt("DELETE FROM rules WHERE id = ?");
        let affected = match db {
            Database::Sqlite(pool) => sqlx::query(audited_sql(&sql))
                .bind(id)
                .execute(pool)
                .await
                .context("Failed to delete rule")?
                .rows_affected(),
            Database::Postgres(pool) => sqlx::query(audited_sql(&sql))
                .bind(id)
                .execute(pool)
                .await
                .context("Failed to delete rule")?
                .rows_affected(),
        };

        let deleted = affected > 0;
        if deleted {
            info!(rule_id = %id, "Rule deleted");
        } else {
            warn!(rule_id = %id, "Rule not found for deletion");
        }
        Ok(deleted)
    }

    /// Get a single rule by ID.
    pub async fn get_rule(db: &Database, id: &str) -> Result<Option<Rule>> {
        let sql = db.adapt(
            r#"SELECT id, name, description, conditions_json, actions_json,
                      priority, enabled, created_at, updated_at
               FROM rules WHERE id = ?"#,
        );
        let row: Option<RuleRow> = match db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(id)
                    .fetch_optional(pool)
                    .await
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(id)
                    .fetch_optional(pool)
                    .await
            }
        }
        .context("Failed to fetch rule")?;

        row.map(row_to_rule).transpose()
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
