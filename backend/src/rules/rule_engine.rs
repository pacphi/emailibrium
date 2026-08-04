//! Rule engine service -- CRUD operations backed by the `rules` table (R-03).
//!
//! `RuleEngine` holds rules in memory and persists them to the `rules` table.
//! It delegates evaluation to `rule_processor` and validation to `rule_validator`.
//!
//! Persistence is single-code-path SeaORM (ADR-036): the `rules` entity owns
//! per-backend encode/decode and the upsert goes through `OnConflict`, so the
//! same bodies run against SQLite and PostgreSQL. The one thing that used to
//! differ between the backends here was `enabled`'s bind type — the column is
//! INTEGER, not BOOLEAN, and PostgreSQL refuses to coerce a `bool` bind into it
//! (ADR-035) — which the entity's `i32` column now settles once.

use anyhow::{Context, Result};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait, QueryOrder};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::types::{Rule, RuleAction, RuleCondition};
use crate::db::entities::rules;
use crate::db::Database;

/// Decode one `rules` row into a [`Rule`].
///
/// `enabled` stays an INTEGER 0/1 compared with `!= 0`, exactly as before the
/// port. The nullable columns all carry DDL defaults that every write through
/// [`RuleEngine::save_rule`] fills in, so the fallbacks below only apply to rows
/// written outside this module; a NULL `enabled` reads as disabled, the safe
/// direction.
fn model_to_rule(model: rules::Model) -> Result<Rule> {
    let conditions: Vec<RuleCondition> = serde_json::from_str(&model.conditions_json)
        .with_context(|| format!("Failed to deserialise conditions for rule '{}'", model.id))?;
    let actions: Vec<RuleAction> = serde_json::from_str(&model.actions_json)
        .with_context(|| format!("Failed to deserialise actions for rule '{}'", model.id))?;
    Ok(Rule {
        id: model.id,
        name: model.name,
        description: model.description.unwrap_or_default(),
        conditions,
        actions,
        priority: model.priority.unwrap_or(0),
        enabled: model.enabled.unwrap_or(0) != 0,
        created_at: model.created_at.unwrap_or_else(Utc::now),
        updated_at: model.updated_at.unwrap_or_else(Utc::now),
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
        let conn = db.sea_orm();
        let models = rules::Entity::find()
            .order_by_desc(rules::Column::Priority)
            .all(&conn)
            .await
            .context("Failed to load rules from database")?;

        let loaded = models
            .into_iter()
            .map(model_to_rule)
            .collect::<Result<Vec<_>>>()?;

        info!(count = loaded.len(), "Rules loaded from database");
        Ok(loaded)
    }

    /// Save (insert or update) a rule to the database.
    pub async fn save_rule(db: &Database, rule: &Rule) -> Result<()> {
        let conditions_json = serde_json::to_string(&rule.conditions)
            .context("Failed to serialise rule conditions")?;
        let actions_json =
            serde_json::to_string(&rule.actions).context("Failed to serialise rule actions")?;

        // One `OnConflict` upsert replaces the hand-written
        // `INSERT ... ON CONFLICT(id) DO UPDATE SET ... = excluded....` and its two
        // bind arms. `match_count`/`last_run_at` are owned by the manual-run path
        // (`api/rules.rs`), so they stay `NotSet` — absent from both the INSERT
        // column list and the conflict UPDATE, exactly as the old SQL left them.
        // `created_at` is likewise not in the update set: a re-save must not
        // rewrite it.
        let conn = db.sea_orm();
        let row = rules::ActiveModel {
            id: Set(rule.id.clone()),
            name: Set(rule.name.clone()),
            description: Set(Some(rule.description.clone())),
            conditions_json: Set(conditions_json),
            actions_json: Set(actions_json),
            priority: Set(Some(rule.priority)),
            enabled: Set(Some(i32::from(rule.enabled))),
            created_at: Set(Some(rule.created_at)),
            updated_at: Set(Some(rule.updated_at)),
            ..Default::default()
        };
        rules::Entity::insert(row)
            .on_conflict(
                OnConflict::column(rules::Column::Id)
                    .update_columns([
                        rules::Column::Name,
                        rules::Column::Description,
                        rules::Column::ConditionsJson,
                        rules::Column::ActionsJson,
                        rules::Column::Priority,
                        rules::Column::Enabled,
                        rules::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(&conn)
            .await
            .context("Failed to save rule")?;

        debug!(rule_id = %rule.id, "Rule saved to database");
        Ok(())
    }

    /// Delete a rule by ID.
    pub async fn delete_rule(db: &Database, id: &str) -> Result<bool> {
        let conn = db.sea_orm();
        let affected = rules::Entity::delete_by_id(id)
            .exec(&conn)
            .await
            .context("Failed to delete rule")?
            .rows_affected;

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
        let conn = db.sea_orm();
        let model = rules::Entity::find_by_id(id)
            .one(&conn)
            .await
            .context("Failed to fetch rule")?;

        model.map(model_to_rule).transpose()
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

    // -----------------------------------------------------------------------
    // Persistence round-trips. Added with the SeaORM port (ADR-036): the three
    // tests above never touch a database, so `save_rule`'s upsert — the file's
    // riskiest statement — had no coverage at all. Unlike `executor.rs`'s pins
    // these were written after the port, so what they assert about equivalence
    // is derived from the pre-port SQL *text* rather than from executing it:
    // that INSERT's `ON CONFLICT (id) DO UPDATE SET` listed name, description,
    // conditions_json, actions_json, priority, enabled and updated_at, and
    // deliberately omitted created_at, match_count and last_run_at.
    // -----------------------------------------------------------------------

    use crate::db::Database;
    use sea_orm::sea_query::Expr;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use sqlx::sqlite::SqlitePoolOptions;

    /// In-memory SQLite with the `rules` table: 012 creates it, 026 adds
    /// `match_count`/`last_run_at` (both of which the entity declares).
    async fn fresh_db() -> Database {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("connect");
        let db = Database::Sqlite(pool);
        let conn = db.sea_orm();
        for raw in [
            include_str!("../../migrations/sqlite/012_rules.sql"),
            include_str!("../../migrations/sqlite/026_rules_match_count.sql"),
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
                    use sea_orm::ConnectionTrait;
                    conn.execute_unprepared(s).await.expect("migrate");
                }
            }
        }
        db
    }

    fn sample_rule(id: &str, name: &str, priority: i32) -> Rule {
        Rule {
            id: id.to_string(),
            name: name.to_string(),
            description: "a description".to_string(),
            conditions: vec![RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: "invoice".to_string(),
            }],
            actions: vec![RuleAction::AddLabel {
                label: "billing".to_string(),
            }],
            priority,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn save_load_get_and_delete_round_trip() {
        let db = fresh_db().await;
        let rule = sample_rule("r-1", "Invoices", 5);
        RuleEngine::save_rule(&db, &rule).await.expect("save");

        let fetched = RuleEngine::get_rule(&db, "r-1")
            .await
            .expect("get")
            .expect("present");
        assert_eq!(fetched.name, "Invoices");
        assert_eq!(fetched.description, "a description");
        assert_eq!(fetched.priority, 5);
        assert!(fetched.enabled);
        assert_eq!(fetched.conditions.len(), 1);
        assert_eq!(fetched.actions.len(), 1);
        // TIMESTAMPTZ-class columns must survive the round trip in the right
        // fields — a created_at/updated_at swap would be invisible otherwise.
        assert_eq!(
            fetched.created_at.timestamp_millis(),
            rule.created_at.timestamp_millis()
        );
        assert_eq!(
            fetched.updated_at.timestamp_millis(),
            rule.updated_at.timestamp_millis()
        );

        assert_eq!(RuleEngine::load_rules(&db).await.expect("load").len(), 1);
        assert!(RuleEngine::get_rule(&db, "absent")
            .await
            .expect("get absent")
            .is_none());

        assert!(RuleEngine::delete_rule(&db, "r-1").await.expect("delete"));
        // Deleting a row that is not there reports false rather than erroring.
        assert!(!RuleEngine::delete_rule(&db, "r-1")
            .await
            .expect("delete again"));
        assert!(RuleEngine::load_rules(&db).await.expect("load").is_empty());
    }

    #[tokio::test]
    async fn save_rule_upserts_in_place_and_leaves_run_counters_alone() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        let mut rule = sample_rule("r-1", "Original", 5);
        let bystander = sample_rule("r-2", "Bystander", 1);
        RuleEngine::save_rule(&db, &rule).await.expect("save");
        RuleEngine::save_rule(&db, &bystander)
            .await
            .expect("save bystander");

        // `match_count` is owned by the manual-run path, not by save_rule.
        rules::Entity::update_many()
            .col_expr(rules::Column::MatchCount, Expr::value(7))
            .filter(rules::Column::Id.eq("r-1"))
            .exec(&conn)
            .await
            .expect("seed match_count");

        let original_created_at = rule.created_at;
        rule.name = "Renamed".to_string();
        rule.enabled = false;
        rule.priority = 9;
        rule.updated_at = Utc::now();
        // A re-save must not rewrite created_at, even when the caller passes a
        // different one — the conflict UPDATE never listed that column.
        rule.created_at = Utc::now() + chrono::Duration::hours(1);
        RuleEngine::save_rule(&db, &rule).await.expect("upsert");

        let all = RuleEngine::load_rules(&db).await.expect("load");
        assert_eq!(all.len(), 2, "upsert must not duplicate the row");

        let updated = RuleEngine::get_rule(&db, "r-1")
            .await
            .expect("get")
            .expect("present");
        assert_eq!(updated.name, "Renamed");
        assert!(!updated.enabled, "enabled round-trips through INTEGER 0/1");
        assert_eq!(updated.priority, 9);
        assert_eq!(
            updated.created_at.timestamp_millis(),
            original_created_at.timestamp_millis()
        );

        let row = rules::Entity::find_by_id("r-1")
            .one(&conn)
            .await
            .expect("query")
            .expect("row present");
        assert_eq!(row.match_count, 7, "match_count must survive a re-save");

        let untouched = RuleEngine::get_rule(&db, "r-2")
            .await
            .expect("get")
            .expect("present");
        assert_eq!(untouched.name, "Bystander");
        assert!(untouched.enabled);
    }

    #[tokio::test]
    async fn load_rules_orders_by_priority_descending() {
        let db = fresh_db().await;
        for (id, name, priority) in [
            ("r-lo", "Low", 1),
            ("r-hi", "High", 10),
            ("r-mid", "Mid", 5),
        ] {
            RuleEngine::save_rule(&db, &sample_rule(id, name, priority))
                .await
                .expect("save");
        }

        let loaded = RuleEngine::load_rules(&db).await.expect("load");
        assert_eq!(
            loaded.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["High", "Mid", "Low"]
        );
    }
}
