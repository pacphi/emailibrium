//! Search interaction tracking for SONA learning input (S3-08).
//!
//! Records user search queries, click-throughs, and relevance feedback
//! to provide training signal for the Self-Optimizing Neural Architecture.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::error::VectorError;
use crate::db::Database;

/// Tracks search interactions for SONA learning.
pub struct InteractionTracker {
    db: Arc<Database>,
}

/// A recorded search interaction with optional click and feedback data.
#[derive(Debug, Clone)]
pub struct SearchInteraction {
    /// Unique interaction ID.
    pub id: String,
    /// The search query text.
    pub query_text: String,
    /// The email ID of the result that was interacted with.
    pub result_email_id: String,
    /// The rank position of the result in the search results.
    pub result_rank: u32,
    /// Whether the user clicked on this result.
    pub clicked: bool,
    /// Optional relevance feedback: "relevant" or "irrelevant".
    pub feedback: Option<String>,
    /// When the interaction was created.
    pub created_at: DateTime<Utc>,
}

impl InteractionTracker {
    /// Create a new tracker backed by the given database.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Record a new search query. Returns the interaction ID.
    pub async fn record_search(&self, query: &str) -> Result<String, VectorError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO search_interactions (id, query_text, created_at) VALUES (?, ?, ?)",
        )
        .bind(&id)
        .bind(query)
        .bind(now)
        .execute(&self.db.pool)
        .await?;

        Ok(id)
    }

    /// Record a click on a search result.
    pub async fn record_click(
        &self,
        query_id: &str,
        email_id: &str,
        rank: u32,
    ) -> Result<(), VectorError> {
        let rows_affected = sqlx::query(
            "UPDATE search_interactions SET result_email_id = ?, result_rank = ?, clicked = TRUE WHERE id = ?",
        )
        .bind(email_id)
        .bind(rank)
        .bind(query_id)
        .execute(&self.db.pool)
        .await?
        .rows_affected();

        if rows_affected == 0 {
            return Err(VectorError::NotFound(format!(
                "search interaction {} not found",
                query_id
            )));
        }

        Ok(())
    }

    /// Record relevance feedback for a search interaction.
    pub async fn record_feedback(
        &self,
        query_id: &str,
        _email_id: &str,
        feedback: &str,
    ) -> Result<(), VectorError> {
        let rows_affected = sqlx::query("UPDATE search_interactions SET feedback = ? WHERE id = ?")
            .bind(feedback)
            .bind(query_id)
            .execute(&self.db.pool)
            .await?
            .rows_affected();

        if rows_affected == 0 {
            return Err(VectorError::NotFound(format!(
                "search interaction {} not found",
                query_id
            )));
        }

        Ok(())
    }

    /// Get recent search interactions, ordered by creation time descending.
    pub async fn get_interactions(
        &self,
        limit: usize,
    ) -> Result<Vec<SearchInteraction>, VectorError> {
        let rows = sqlx::query_as::<_, InteractionRow>(
            "SELECT id, query_text, result_email_id, result_rank, clicked, feedback, created_at \
             FROM search_interactions \
             ORDER BY created_at DESC \
             LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(&self.db.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// Compute the click-through rate: clicks / total interactions with results.
    pub async fn get_click_through_rate(&self) -> Result<f32, VectorError> {
        let row: (i64, i64) = sqlx::query_as(
            "SELECT \
                COALESCE(SUM(CASE WHEN clicked = TRUE THEN 1 ELSE 0 END), 0), \
                COUNT(*) \
             FROM search_interactions",
        )
        .fetch_one(&self.db.pool)
        .await?;

        let (clicks, total) = row;
        if total == 0 {
            return Ok(0.0);
        }

        Ok(clicks as f32 / total as f32)
    }

    /// Get click count distribution by rank position.
    pub async fn get_rank_distribution(&self) -> Result<HashMap<u32, u64>, VectorError> {
        let rows: Vec<(i32, i64)> = sqlx::query_as(
            "SELECT result_rank, COUNT(*) \
             FROM search_interactions \
             WHERE clicked = TRUE AND result_rank IS NOT NULL \
             GROUP BY result_rank \
             ORDER BY result_rank",
        )
        .fetch_all(&self.db.pool)
        .await?;

        let mut dist = HashMap::new();
        for (rank, count) in rows {
            dist.insert(rank as u32, count as u64);
        }

        Ok(dist)
    }
}

/// Internal row type for SQLx deserialization.
#[derive(sqlx::FromRow)]
struct InteractionRow {
    id: String,
    query_text: String,
    result_email_id: Option<String>,
    result_rank: Option<i32>,
    clicked: bool,
    feedback: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<InteractionRow> for SearchInteraction {
    fn from(row: InteractionRow) -> Self {
        Self {
            id: row.id,
            query_text: row.query_text,
            result_email_id: row.result_email_id.unwrap_or_default(),
            result_rank: row.result_rank.unwrap_or(0) as u32,
            clicked: row.clicked,
            feedback: row.feedback,
            created_at: row.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_db() -> Arc<Database> {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        // Create the table directly for testing.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS search_interactions (
                id TEXT PRIMARY KEY,
                query_text TEXT NOT NULL,
                query_vector_id TEXT,
                result_email_id TEXT,
                result_rank INTEGER,
                clicked BOOLEAN DEFAULT FALSE,
                feedback TEXT,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&db.pool)
        .await
        .unwrap();

        Arc::new(db)
    }

    #[tokio::test]
    async fn test_record_search_and_click() {
        let db = setup_db().await;
        let tracker = InteractionTracker::new(db);

        let id = tracker.record_search("test query").await.unwrap();
        assert!(!id.is_empty());

        tracker.record_click(&id, "email-123", 2).await.unwrap();

        let interactions = tracker.get_interactions(10).await.unwrap();
        assert_eq!(interactions.len(), 1);
        assert_eq!(interactions[0].query_text, "test query");
        assert_eq!(interactions[0].result_email_id, "email-123");
        assert_eq!(interactions[0].result_rank, 2);
        assert!(interactions[0].clicked);
    }

    #[tokio::test]
    async fn test_click_through_rate() {
        let db = setup_db().await;
        let tracker = InteractionTracker::new(db);

        // Empty: rate should be 0.
        let ctr = tracker.get_click_through_rate().await.unwrap();
        assert_eq!(ctr, 0.0);

        // Record 4 searches, click on 2.
        let id1 = tracker.record_search("query 1").await.unwrap();
        let _id2 = tracker.record_search("query 2").await.unwrap();
        let id3 = tracker.record_search("query 3").await.unwrap();
        let _id4 = tracker.record_search("query 4").await.unwrap();

        tracker.record_click(&id1, "e1", 1).await.unwrap();
        tracker.record_click(&id3, "e3", 3).await.unwrap();

        let ctr = tracker.get_click_through_rate().await.unwrap();
        // 2 clicks out of 4 total = 0.5
        assert!((ctr - 0.5).abs() < 1e-6, "expected 0.5, got {}", ctr);
    }

    #[tokio::test]
    async fn test_rank_distribution() {
        let db = setup_db().await;
        let tracker = InteractionTracker::new(db);

        // Record clicks at various ranks.
        let id1 = tracker.record_search("q1").await.unwrap();
        let id2 = tracker.record_search("q2").await.unwrap();
        let id3 = tracker.record_search("q3").await.unwrap();
        let id4 = tracker.record_search("q4").await.unwrap();

        tracker.record_click(&id1, "e1", 1).await.unwrap();
        tracker.record_click(&id2, "e2", 1).await.unwrap();
        tracker.record_click(&id3, "e3", 2).await.unwrap();
        tracker.record_click(&id4, "e4", 3).await.unwrap();

        let dist = tracker.get_rank_distribution().await.unwrap();
        assert_eq!(dist.get(&1), Some(&2)); // rank 1: 2 clicks
        assert_eq!(dist.get(&2), Some(&1)); // rank 2: 1 click
        assert_eq!(dist.get(&3), Some(&1)); // rank 3: 1 click
    }

    #[tokio::test]
    async fn test_record_feedback() {
        let db = setup_db().await;
        let tracker = InteractionTracker::new(db);

        let id = tracker.record_search("feedback query").await.unwrap();
        tracker.record_click(&id, "e1", 1).await.unwrap();
        tracker
            .record_feedback(&id, "e1", "relevant")
            .await
            .unwrap();

        let interactions = tracker.get_interactions(10).await.unwrap();
        assert_eq!(interactions.len(), 1);
        assert_eq!(interactions[0].feedback, Some("relevant".to_string()));
    }
}
