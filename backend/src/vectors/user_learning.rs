//! Per-user learning models (DDD-004, item #27).
//!
//! Extends the shared SONA learning engine with per-user model isolation.
//! Each user gets their own set of centroid adjustments and session state,
//! with fallback to the shared model for cold-start users.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::debug;

use super::error::VectorError;
use super::learning::{FeedbackAction, LearningConfig};

/// Row tuple for user learning model queries.
type UserModelRow = (String, String, i64, DateTime<Utc>, DateTime<Utc>);
use super::types::EmailCategory;
use crate::db::Database;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-user centroid adjustment: an offset applied on top of the shared centroid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCentroidOffset {
    /// Category this offset applies to.
    pub category: EmailCategory,
    /// Per-dimension offset vector added to the shared centroid.
    pub offset: Vec<f32>,
    /// Number of feedback events that contributed to this offset.
    pub feedback_count: u32,
    /// Last time this offset was updated.
    pub updated_at: DateTime<Utc>,
}

/// A per-user learning model containing centroid offsets and interaction state.
#[derive(Debug, Clone)]
pub struct UserLearningModel {
    /// User identifier.
    pub user_id: String,
    /// Per-category centroid offsets relative to the shared model.
    pub offsets: HashMap<EmailCategory, UserCentroidOffset>,
    /// Total feedback events for this user.
    pub total_feedback: u32,
    /// When this model was created.
    pub created_at: DateTime<Utc>,
    /// When this model was last updated.
    pub updated_at: DateTime<Utc>,
}

impl UserLearningModel {
    /// Create a new empty model for a user.
    pub fn new(user_id: String) -> Self {
        let now = Utc::now();
        Self {
            user_id,
            offsets: HashMap::new(),
            total_feedback: 0,
            created_at: now,
            updated_at: now,
        }
    }

    /// Apply a feedback-driven offset update for a category.
    ///
    /// The offset is an EMA update: `offset_new = (1 - alpha) * offset_old + alpha * delta`
    /// where `delta = embedding - shared_centroid`.
    pub fn apply_feedback(&mut self, category: EmailCategory, delta: &[f32], alpha: f32) {
        let alpha = alpha.clamp(0.0, 1.0);
        let now = Utc::now();

        let entry = self
            .offsets
            .entry(category)
            .or_insert_with(|| UserCentroidOffset {
                category,
                offset: vec![0.0; delta.len()],
                feedback_count: 0,
                updated_at: now,
            });

        if entry.offset.len() != delta.len() {
            entry.offset = vec![0.0; delta.len()];
        }

        for (o, &d) in entry.offset.iter_mut().zip(delta.iter()) {
            *o = (1.0 - alpha) * *o + alpha * d;
        }

        entry.feedback_count += 1;
        entry.updated_at = now;
        self.total_feedback += 1;
        self.updated_at = now;
    }

    /// Get the effective centroid for a category: shared_centroid + user_offset.
    pub fn effective_centroid(&self, category: EmailCategory, shared_centroid: &[f32]) -> Vec<f32> {
        match self.offsets.get(&category) {
            Some(offset) if offset.offset.len() == shared_centroid.len() => shared_centroid
                .iter()
                .zip(offset.offset.iter())
                .map(|(&s, &o)| s + o)
                .collect(),
            _ => shared_centroid.to_vec(),
        }
    }

    /// Whether this user has enough data to use their personal model.
    pub fn is_warm(&self, min_feedback: u32) -> bool {
        self.total_feedback >= min_feedback
    }
}

// ---------------------------------------------------------------------------
// UserLearningStore
// ---------------------------------------------------------------------------

/// Manages per-user learning models with database persistence.
pub struct UserLearningStore {
    /// In-memory cache of user models.
    models: RwLock<HashMap<String, UserLearningModel>>,
    /// Database for persistence.
    db: Arc<Database>,
    /// Shared learning configuration.
    config: LearningConfig,
}

impl UserLearningStore {
    /// Create a new per-user learning store.
    pub fn new(db: Arc<Database>, config: LearningConfig) -> Self {
        Self {
            models: RwLock::new(HashMap::new()),
            db,
            config,
        }
    }

    /// Ensure the per-user learning table exists.
    pub async fn ensure_table(&self) -> Result<(), VectorError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS user_learning_models (
                user_id TEXT NOT NULL,
                category TEXT NOT NULL,
                offset_json TEXT NOT NULL,
                feedback_count INTEGER NOT NULL DEFAULT 0,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (user_id, category)
            )",
        )
        .execute(&self.db.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_user_learning_user ON user_learning_models(user_id)",
        )
        .execute(&self.db.pool)
        .await?;

        Ok(())
    }

    /// Get or create a user's learning model.
    pub async fn get_or_create(&self, user_id: &str) -> UserLearningModel {
        // Check in-memory cache first.
        {
            let models = self.models.read().await;
            if let Some(model) = models.get(user_id) {
                return model.clone();
            }
        }

        // Try loading from database.
        if let Ok(model) = self.load_from_db(user_id).await {
            let mut models = self.models.write().await;
            models.insert(user_id.to_string(), model.clone());
            return model;
        }

        // Create a new empty model.
        let model = UserLearningModel::new(user_id.to_string());
        let mut models = self.models.write().await;
        models.insert(user_id.to_string(), model.clone());
        model
    }

    /// Process user feedback and update the per-user model.
    pub async fn on_feedback(
        &self,
        user_id: &str,
        category: EmailCategory,
        embedding: &[f32],
        shared_centroid: &[f32],
        action: &FeedbackAction,
    ) -> Result<(), VectorError> {
        let quality = super::learning::LearningEngine::quality_for_action(action);
        let alpha = quality * self.config.positive_learning_rate;

        // Compute delta: difference between the email embedding and the shared centroid.
        let delta: Vec<f32> = embedding
            .iter()
            .zip(shared_centroid.iter())
            .map(|(&e, &c)| e - c)
            .collect();

        // Update in-memory model.
        {
            let mut models = self.models.write().await;
            let model = models
                .entry(user_id.to_string())
                .or_insert_with(|| UserLearningModel::new(user_id.to_string()));

            model.apply_feedback(category, &delta, alpha);
        }

        // Persist to database.
        self.persist_offset(user_id, category).await?;

        debug!(
            user_id = %user_id,
            category = %category,
            "Updated per-user learning model"
        );

        Ok(())
    }

    /// Check if a user's model is warm enough to use.
    pub async fn is_user_warm(&self, user_id: &str) -> bool {
        let models = self.models.read().await;
        models
            .get(user_id)
            .map(|m| m.is_warm(self.config.min_feedback_events))
            .unwrap_or(false)
    }

    /// Get the effective centroid for a user+category, falling back to shared.
    pub async fn effective_centroid(
        &self,
        user_id: &str,
        category: EmailCategory,
        shared_centroid: &[f32],
    ) -> Vec<f32> {
        let models = self.models.read().await;
        match models.get(user_id) {
            Some(model) if model.is_warm(self.config.min_feedback_events) => {
                model.effective_centroid(category, shared_centroid)
            }
            _ => shared_centroid.to_vec(),
        }
    }

    /// Load a user model from the database.
    async fn load_from_db(&self, user_id: &str) -> Result<UserLearningModel, VectorError> {
        let rows: Vec<UserModelRow> = sqlx::query_as(
            "SELECT category, offset_json, feedback_count, created_at, updated_at
             FROM user_learning_models WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_all(&self.db.pool)
        .await?;

        if rows.is_empty() {
            return Err(VectorError::NotFound(format!(
                "No learning model for user: {user_id}"
            )));
        }

        let mut model = UserLearningModel::new(user_id.to_string());

        for (cat_str, offset_json, feedback_count, created_at, updated_at) in rows {
            let category: EmailCategory = serde_json::from_str(&format!("\"{cat_str}\""))
                .unwrap_or(EmailCategory::Uncategorized);
            let offset: Vec<f32> = serde_json::from_str(&offset_json).unwrap_or_default();

            model.offsets.insert(
                category,
                UserCentroidOffset {
                    category,
                    offset,
                    feedback_count: feedback_count as u32,
                    updated_at,
                },
            );
            model.total_feedback += feedback_count as u32;
            if created_at < model.created_at {
                model.created_at = created_at;
            }
            if updated_at > model.updated_at {
                model.updated_at = updated_at;
            }
        }

        Ok(model)
    }

    /// Persist a single category offset to the database.
    async fn persist_offset(
        &self,
        user_id: &str,
        category: EmailCategory,
    ) -> Result<(), VectorError> {
        let models = self.models.read().await;
        let model = match models.get(user_id) {
            Some(m) => m,
            None => return Ok(()),
        };

        let offset = match model.offsets.get(&category) {
            Some(o) => o,
            None => return Ok(()),
        };

        let offset_json = serde_json::to_string(&offset.offset)?;
        let cat_str = serde_json::to_string(&category)?;
        // Remove surrounding quotes from the category string.
        let cat_str = cat_str.trim_matches('"');

        sqlx::query(
            "INSERT INTO user_learning_models (user_id, category, offset_json, feedback_count, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(user_id, category) DO UPDATE SET
                offset_json = excluded.offset_json,
                feedback_count = excluded.feedback_count,
                updated_at = excluded.updated_at",
        )
        .bind(user_id)
        .bind(cat_str)
        .bind(&offset_json)
        .bind(offset.feedback_count as i64)
        .bind(offset.updated_at)
        .execute(&self.db.pool)
        .await?;

        Ok(())
    }

    /// List all user IDs with learning models.
    pub async fn list_users(&self) -> Result<Vec<String>, VectorError> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT DISTINCT user_id FROM user_learning_models ORDER BY user_id")
                .fetch_all(&self.db.pool)
                .await?;

        Ok(rows.into_iter().map(|(uid,)| uid).collect())
    }

    /// Return the number of cached user models.
    pub async fn cached_model_count(&self) -> usize {
        self.models.read().await.len()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_model_new() {
        let model = UserLearningModel::new("user-1".to_string());
        assert_eq!(model.user_id, "user-1");
        assert!(model.offsets.is_empty());
        assert_eq!(model.total_feedback, 0);
    }

    #[test]
    fn test_user_model_apply_feedback() {
        let mut model = UserLearningModel::new("user-1".to_string());
        let delta = vec![0.1, -0.2, 0.3];

        model.apply_feedback(EmailCategory::Work, &delta, 0.5);

        assert_eq!(model.total_feedback, 1);
        let offset = &model.offsets[&EmailCategory::Work];
        assert_eq!(offset.feedback_count, 1);
        // offset = (1-0.5)*[0,0,0] + 0.5*[0.1,-0.2,0.3] = [0.05, -0.1, 0.15]
        assert!((offset.offset[0] - 0.05).abs() < 1e-5);
        assert!((offset.offset[1] - (-0.1)).abs() < 1e-5);
        assert!((offset.offset[2] - 0.15).abs() < 1e-5);
    }

    #[test]
    fn test_user_model_effective_centroid() {
        let mut model = UserLearningModel::new("user-1".to_string());

        // No offset yet: effective = shared.
        let shared = vec![1.0, 0.0, 0.0];
        assert_eq!(
            model.effective_centroid(EmailCategory::Work, &shared),
            shared
        );

        // Apply offset.
        model.apply_feedback(EmailCategory::Work, &[0.2, 0.1, -0.1], 1.0);

        let effective = model.effective_centroid(EmailCategory::Work, &shared);
        assert!((effective[0] - 1.2).abs() < 1e-5);
        assert!((effective[1] - 0.1).abs() < 1e-5);
        assert!((effective[2] - (-0.1)).abs() < 1e-5);
    }

    #[test]
    fn test_user_model_is_warm() {
        let mut model = UserLearningModel::new("user-1".to_string());
        assert!(!model.is_warm(5));

        for _ in 0..5 {
            model.apply_feedback(EmailCategory::Work, &[0.1, 0.0, 0.0], 0.1);
        }
        assert!(model.is_warm(5));
        assert!(!model.is_warm(6));
    }

    #[tokio::test]
    async fn test_store_get_or_create() {
        let db = Arc::new(
            Database::connect("sqlite::memory:")
                .await
                .expect("in-memory DB"),
        );
        let config = LearningConfig::default();
        let store = UserLearningStore::new(db, config);
        store.ensure_table().await.unwrap();

        let model = store.get_or_create("user-1").await;
        assert_eq!(model.user_id, "user-1");
        assert_eq!(model.total_feedback, 0);

        // Second call should return cached model.
        let model2 = store.get_or_create("user-1").await;
        assert_eq!(model2.user_id, "user-1");
    }

    #[tokio::test]
    async fn test_store_feedback_and_persist() {
        let db = Arc::new(
            Database::connect("sqlite::memory:")
                .await
                .expect("in-memory DB"),
        );
        let config = LearningConfig {
            min_feedback_events: 0,
            ..LearningConfig::default()
        };
        let store = UserLearningStore::new(db, config);
        store.ensure_table().await.unwrap();

        let _ = store.get_or_create("user-1").await;

        store
            .on_feedback(
                "user-1",
                EmailCategory::Work,
                &[0.5, 0.5, 0.0],
                &[1.0, 0.0, 0.0],
                &FeedbackAction::Star,
            )
            .await
            .unwrap();

        let effective = store
            .effective_centroid("user-1", EmailCategory::Work, &[1.0, 0.0, 0.0])
            .await;
        // Should differ from shared centroid.
        // alpha = 0.4 * 0.05 = 0.02 (very small)
        // delta = [0.5-1.0, 0.5-0.0, 0.0-0.0] = [-0.5, 0.5, 0.0]
        // But min_feedback_events=0 so is_warm=true.
        // effective = [1.0, 0.0, 0.0] + offset
        assert!(effective[0] != 1.0 || effective[1] != 0.0);
    }

    #[tokio::test]
    async fn test_store_cold_user_fallback() {
        let db = Arc::new(
            Database::connect("sqlite::memory:")
                .await
                .expect("in-memory DB"),
        );
        let config = LearningConfig {
            min_feedback_events: 100, // high threshold
            ..LearningConfig::default()
        };
        let store = UserLearningStore::new(db, config);
        store.ensure_table().await.unwrap();

        let shared = vec![1.0, 0.0, 0.0];
        let effective = store
            .effective_centroid("cold-user", EmailCategory::Work, &shared)
            .await;
        // Cold user: should get shared centroid.
        assert_eq!(effective, shared);
    }

    #[tokio::test]
    async fn test_store_is_user_warm() {
        let db = Arc::new(
            Database::connect("sqlite::memory:")
                .await
                .expect("in-memory DB"),
        );
        let config = LearningConfig {
            min_feedback_events: 2,
            ..LearningConfig::default()
        };
        let store = UserLearningStore::new(db, config);
        store.ensure_table().await.unwrap();

        assert!(!store.is_user_warm("user-1").await);

        let _ = store.get_or_create("user-1").await;
        assert!(!store.is_user_warm("user-1").await);

        // Add feedback.
        store
            .on_feedback(
                "user-1",
                EmailCategory::Work,
                &[0.5, 0.5, 0.0],
                &[1.0, 0.0, 0.0],
                &FeedbackAction::Star,
            )
            .await
            .unwrap();
        assert!(!store.is_user_warm("user-1").await);

        store
            .on_feedback(
                "user-1",
                EmailCategory::Work,
                &[0.5, 0.5, 0.0],
                &[1.0, 0.0, 0.0],
                &FeedbackAction::Star,
            )
            .await
            .unwrap();
        assert!(store.is_user_warm("user-1").await);
    }
}
