//! Hybrid search combining FTS5 + vector similarity with Reciprocal Rank Fusion (ADR-001).
//!
//! `HybridSearch` runs keyword (FTS5/LIKE) and semantic (vector) searches in
//! parallel via `tokio::join!`, then fuses results using RRF scoring.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::db::Database;

use super::config::SearchConfig;
use super::embedding::EmbeddingPipeline;
use super::error::VectorError;
use super::store::VectorStoreBackend;
use super::types::{ScoredResult, SearchParams, VectorCollection};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Mode selector for hybrid search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchMode {
    /// Run both vector and keyword search, fuse with RRF.
    Hybrid,
    /// Pure vector (semantic) search only.
    Semantic,
    /// Pure keyword (FTS5/LIKE) search only.
    Keyword,
}

/// Structured filters applied after fusion scoring.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilters {
    pub date_from: Option<DateTime<Utc>>,
    pub date_to: Option<DateTime<Utc>>,
    pub senders: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
    pub has_attachment: Option<bool>,
    pub is_read: Option<bool>,
}

/// Input query for hybrid search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchQuery {
    pub text: String,
    pub mode: SearchMode,
    pub filters: Option<SearchFilters>,
    pub limit: Option<usize>,
}

/// A single fused result combining vector and keyword rankings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusedResult {
    pub email_id: String,
    pub score: f32,
    pub match_type: String,
    pub vector_rank: Option<usize>,
    pub fts_rank: Option<usize>,
    pub metadata: HashMap<String, String>,
}

/// The output of a hybrid search operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchResult {
    pub results: Vec<FusedResult>,
    pub total: usize,
    pub latency_ms: u64,
    pub mode: SearchMode,
}

// ---------------------------------------------------------------------------
// Reciprocal Rank Fusion
// ---------------------------------------------------------------------------

/// Fuse two ranked lists using Reciprocal Rank Fusion (RRF).
///
/// For each document present in either list, the score is:
///
///   `score(d) = 1/(k + rank_vector(d)) + 1/(k + rank_fts(d))`
///
/// where `rank` is 1-indexed. Documents missing from a list contribute zero
/// from that component.
pub fn reciprocal_rank_fusion(
    vector_results: &[(String, f32)],
    fts_results: &[(String, f32)],
    k: u32,
) -> Vec<(String, f32)> {
    let mut scores: HashMap<String, (f32, Option<usize>, Option<usize>)> = HashMap::new();

    for (rank_0, (id, _score)) in vector_results.iter().enumerate() {
        let rank = (rank_0 + 1) as f32;
        let entry = scores.entry(id.clone()).or_insert((0.0, None, None));
        entry.0 += 1.0 / (k as f32 + rank);
        entry.1 = Some(rank_0 + 1);
    }

    for (rank_0, (id, _score)) in fts_results.iter().enumerate() {
        let rank = (rank_0 + 1) as f32;
        let entry = scores.entry(id.clone()).or_insert((0.0, None, None));
        entry.0 += 1.0 / (k as f32 + rank);
        entry.2 = Some(rank_0 + 1);
    }

    let mut fused: Vec<(String, f32)> = scores
        .into_iter()
        .map(|(id, (score, _vr, _fr))| (id, score))
        .collect();

    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

/// Internal variant that preserves rank information for building `FusedResult`.
fn reciprocal_rank_fusion_detailed(
    vector_results: &[(String, f32)],
    fts_results: &[(String, f32)],
    k: u32,
) -> Vec<(String, f32, Option<usize>, Option<usize>)> {
    let mut scores: HashMap<String, (f32, Option<usize>, Option<usize>)> = HashMap::new();

    for (rank_0, (id, _score)) in vector_results.iter().enumerate() {
        let rank = (rank_0 + 1) as f32;
        let entry = scores.entry(id.clone()).or_insert((0.0, None, None));
        entry.0 += 1.0 / (k as f32 + rank);
        entry.1 = Some(rank_0 + 1);
    }

    for (rank_0, (id, _score)) in fts_results.iter().enumerate() {
        let rank = (rank_0 + 1) as f32;
        let entry = scores.entry(id.clone()).or_insert((0.0, None, None));
        entry.0 += 1.0 / (k as f32 + rank);
        entry.2 = Some(rank_0 + 1);
    }

    let mut fused: Vec<(String, f32, Option<usize>, Option<usize>)> = scores
        .into_iter()
        .map(|(id, (score, vr, fr))| (id, score, vr, fr))
        .collect();

    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

// ---------------------------------------------------------------------------
// HybridSearch
// ---------------------------------------------------------------------------

/// Orchestrates hybrid FTS + vector search with RRF fusion (ADR-001).
pub struct HybridSearch {
    store: Arc<dyn VectorStoreBackend>,
    embedding: Arc<EmbeddingPipeline>,
    db: Arc<Database>,
    config: SearchConfig,
}

impl HybridSearch {
    /// Create a new `HybridSearch` instance.
    pub fn new(
        store: Arc<dyn VectorStoreBackend>,
        embedding: Arc<EmbeddingPipeline>,
        db: Arc<Database>,
        config: SearchConfig,
    ) -> Self {
        Self {
            store,
            embedding,
            db,
            config,
        }
    }

    /// Execute a hybrid search combining vector and keyword results.
    pub async fn search(
        &self,
        query: &HybridSearchQuery,
    ) -> Result<HybridSearchResult, VectorError> {
        let start = Instant::now();
        let limit = query.limit.unwrap_or(self.config.default_limit);
        let fetch_limit = self.config.max_limit.min(100);

        match query.mode {
            SearchMode::Hybrid => self
                .search_hybrid(&query.text, fetch_limit, limit, &query.filters)
                .await
                .map(|mut r| {
                    r.latency_ms = start.elapsed().as_millis() as u64;
                    r
                }),
            SearchMode::Semantic => {
                let results = self
                    .semantic_search(&query.text, limit, VectorCollection::EmailText, None, None)
                    .await?;
                let fused: Vec<FusedResult> = results
                    .iter()
                    .enumerate()
                    .map(|(i, sr)| FusedResult {
                        email_id: sr.document.email_id.clone(),
                        score: sr.score,
                        match_type: "semantic".to_string(),
                        vector_rank: Some(i + 1),
                        fts_rank: None,
                        metadata: sr.document.metadata.clone(),
                    })
                    .collect();
                let total = fused.len();
                Ok(HybridSearchResult {
                    results: fused,
                    total,
                    latency_ms: start.elapsed().as_millis() as u64,
                    mode: SearchMode::Semantic,
                })
            }
            SearchMode::Keyword => {
                let fts = self.fts_search(&query.text, fetch_limit).await?;
                let fused: Vec<FusedResult> = fts
                    .iter()
                    .enumerate()
                    .take(limit)
                    .map(|(i, (id, score))| FusedResult {
                        email_id: id.clone(),
                        score: *score,
                        match_type: "keyword".to_string(),
                        vector_rank: None,
                        fts_rank: Some(i + 1),
                        metadata: HashMap::new(),
                    })
                    .collect();
                let total = fused.len();
                Ok(HybridSearchResult {
                    results: fused,
                    total,
                    latency_ms: start.elapsed().as_millis() as u64,
                    mode: SearchMode::Keyword,
                })
            }
        }
    }

    /// Pure vector (semantic) search — delegates to the store after embedding.
    pub async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        collection: VectorCollection,
        filters: Option<HashMap<String, String>>,
        min_score: Option<f32>,
    ) -> Result<Vec<ScoredResult>, VectorError> {
        let query_vec = self.embedding.embed(query).await?;

        let params = SearchParams {
            vector: query_vec,
            limit,
            collection,
            filters,
            min_score: Some(min_score.unwrap_or(self.config.similarity_threshold)),
        };

        self.store.search(&params).await
    }

    /// Find emails similar to the given email by looking up its vector.
    pub async fn find_similar(
        &self,
        email_id: &str,
        limit: usize,
    ) -> Result<Vec<ScoredResult>, VectorError> {
        let doc =
            self.store.get_by_email_id(email_id).await?.ok_or_else(|| {
                VectorError::NotFound(format!("No vector for email_id: {email_id}"))
            })?;

        let params = SearchParams {
            vector: doc.vector,
            limit: limit + 1, // fetch one extra to exclude self
            collection: doc.collection,
            filters: None,
            min_score: Some(self.config.similarity_threshold),
        };

        let results = self.store.search(&params).await?;

        // Exclude the source email itself.
        Ok(results
            .into_iter()
            .filter(|r| r.document.email_id != email_id)
            .take(limit)
            .collect())
    }

    // -- private helpers -----------------------------------------------------

    /// Run vector and FTS searches in parallel, then fuse with RRF.
    async fn search_hybrid(
        &self,
        text: &str,
        fetch_limit: usize,
        result_limit: usize,
        filters: &Option<SearchFilters>,
    ) -> Result<HybridSearchResult, VectorError> {
        // Embed the query.
        let query_vec = self.embedding.embed(text).await?;

        let vector_params = SearchParams {
            vector: query_vec,
            limit: fetch_limit,
            collection: VectorCollection::EmailText,
            filters: None,
            min_score: None,
        };

        // Run both searches in parallel.
        let (vector_res, fts_res) = tokio::join!(
            self.store.search(&vector_params),
            self.fts_search(text, fetch_limit)
        );

        let vector_results = vector_res.unwrap_or_else(|e| {
            warn!("vector search failed: {e}");
            Vec::new()
        });

        let fts_results = fts_res.unwrap_or_else(|e| {
            warn!("FTS search failed: {e}");
            Vec::new()
        });

        // Build ID -> metadata lookup from vector results.
        let mut meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let vector_pairs: Vec<(String, f32)> = vector_results
            .iter()
            .map(|sr| {
                meta_map.insert(sr.document.email_id.clone(), sr.document.metadata.clone());
                (sr.document.email_id.clone(), sr.score)
            })
            .collect();

        // Fuse with RRF (k = 60 as per standard).
        let fused = reciprocal_rank_fusion_detailed(&vector_pairs, &fts_results, 60);

        let mut results: Vec<FusedResult> = fused
            .into_iter()
            .map(|(id, score, vr, fr)| {
                let match_type = match (vr, fr) {
                    (Some(_), Some(_)) => "hybrid",
                    (Some(_), None) => "semantic",
                    (None, Some(_)) => "keyword",
                    (None, None) => "unknown",
                };
                FusedResult {
                    metadata: meta_map.get(&id).cloned().unwrap_or_default(),
                    email_id: id,
                    score,
                    match_type: match_type.to_string(),
                    vector_rank: vr,
                    fts_rank: fr,
                }
            })
            .collect();

        // Apply structured filters if present.
        if let Some(f) = filters {
            results = self.apply_filters(results, f).await;
        }

        results.truncate(result_limit);
        let total = results.len();

        Ok(HybridSearchResult {
            results,
            total,
            latency_ms: 0, // caller sets this
            mode: SearchMode::Hybrid,
        })
    }

    /// Keyword search using LIKE-based matching (upgrade to FTS5 tracked in ADR-006).
    async fn fts_search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(String, f32)>, VectorError> {
        let pattern = format!("%{query}%");
        let limit_i64 = limit as i64;

        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT id
            FROM emails
            WHERE subject LIKE ?1
               OR from_addr LIKE ?1
               OR body_text LIKE ?1
            ORDER BY received_at DESC
            LIMIT ?2
            "#,
        )
        .bind(&pattern)
        .bind(limit_i64)
        .fetch_all(&self.db.pool)
        .await?;

        // Assign descending rank scores (1.0 for first, decaying).
        let total = rows.len() as f32;
        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(i, (id,))| {
                let score = if total > 0.0 {
                    1.0 - (i as f32 / total)
                } else {
                    0.0
                };
                (id, score)
            })
            .collect())
    }

    /// Apply structured filters to fused results.
    ///
    /// Filters that require DB lookups (date_range, senders, etc.) are
    /// applied by querying the emails table for matching IDs.
    async fn apply_filters(
        &self,
        results: Vec<FusedResult>,
        filters: &SearchFilters,
    ) -> Vec<FusedResult> {
        if results.is_empty() {
            return results;
        }

        // Collect IDs to look up.
        let ids: Vec<String> = results.iter().map(|r| r.email_id.clone()).collect();

        // Build a set of IDs that pass the filter.
        let mut passing_ids: HashMap<String, bool> = HashMap::new();

        // For efficiency, we do a single query with all filter conditions.
        // If the query fails, we return results unfiltered.
        match self.filter_email_ids(&ids, filters).await {
            Ok(filtered) => {
                for id in filtered {
                    passing_ids.insert(id, true);
                }
            }
            Err(e) => {
                debug!("filter query failed, returning unfiltered: {e}");
                return results;
            }
        }

        results
            .into_iter()
            .filter(|r| passing_ids.contains_key(&r.email_id))
            .collect()
    }

    /// Query the database for email IDs matching structured filters.
    async fn filter_email_ids(
        &self,
        ids: &[String],
        filters: &SearchFilters,
    ) -> Result<Vec<String>, VectorError> {
        // Build dynamic SQL. SQLx doesn't support dynamic IN clauses
        // elegantly, so we build the query string manually.
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let in_clause = placeholders.join(", ");

        let mut conditions = vec![format!("id IN ({in_clause})")];
        let mut bind_offset = ids.len();

        if let Some(ref date_from) = filters.date_from {
            conditions.push(format!(
                "received_at >= '{}'",
                date_from.format("%Y-%m-%d %H:%M:%S")
            ));
        }

        if let Some(ref date_to) = filters.date_to {
            conditions.push(format!(
                "received_at <= '{}'",
                date_to.format("%Y-%m-%d %H:%M:%S")
            ));
        }

        if let Some(ref senders) = filters.senders {
            if !senders.is_empty() {
                let sender_placeholders: Vec<String> =
                    senders.iter().map(|_| "?".to_string()).collect();
                conditions.push(format!("from_addr IN ({})", sender_placeholders.join(", ")));
                bind_offset += senders.len();
            }
        }

        if let Some(ref categories) = filters.categories {
            if !categories.is_empty() {
                let cat_placeholders: Vec<String> =
                    categories.iter().map(|_| "?".to_string()).collect();
                conditions.push(format!("category IN ({})", cat_placeholders.join(", ")));
                bind_offset += categories.len();
            }
        }

        if let Some(has_attachment) = filters.has_attachment {
            conditions.push(format!("has_attachments = {}", has_attachment as i32));
        }

        if let Some(is_read) = filters.is_read {
            conditions.push(format!("is_read = {}", is_read as i32));
        }

        let _ = bind_offset; // suppress unused warning

        let sql = format!("SELECT id FROM emails WHERE {}", conditions.join(" AND "));

        // Use raw query with dynamic bindings.
        let mut query = sqlx::query_scalar::<_, String>(&sql);

        // Bind the email IDs.
        for id in ids {
            query = query.bind(id.as_str());
        }

        // Bind sender values.
        if let Some(ref senders) = filters.senders {
            for sender in senders {
                query = query.bind(sender.as_str());
            }
        }

        // Bind category values.
        if let Some(ref categories) = filters.categories {
            for cat in categories {
                query = query.bind(cat.as_str());
            }
        }

        let result = query.fetch_all(&self.db.pool).await?;
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- RRF unit tests (no DB or store needed) -----------------------------

    #[test]
    fn test_rrf_both_lists_have_results() {
        let vector = vec![
            ("email_1".to_string(), 0.95),
            ("email_2".to_string(), 0.80),
            ("email_3".to_string(), 0.70),
        ];
        let fts = vec![
            ("email_4".to_string(), 1.0),
            ("email_5".to_string(), 0.9),
            ("email_3".to_string(), 0.8),
        ];

        let fused = reciprocal_rank_fusion(&vector, &fts, 60);

        // email_3 appears in both lists and should have the highest score.
        assert!(!fused.is_empty());
        let email_3 = fused.iter().find(|(id, _)| id == "email_3").unwrap();
        let email_1 = fused.iter().find(|(id, _)| id == "email_1").unwrap();
        assert!(
            email_3.1 > email_1.1,
            "email_3 (in both lists) should score higher than email_1 (vector only)"
        );

        // Should have 5 unique emails total.
        assert_eq!(fused.len(), 5);
    }

    #[test]
    fn test_rrf_vector_only() {
        let vector = vec![("a".to_string(), 0.9), ("b".to_string(), 0.8)];
        let fts: Vec<(String, f32)> = vec![];

        let fused = reciprocal_rank_fusion(&vector, &fts, 60);
        assert_eq!(fused.len(), 2);

        // "a" at rank 1 -> score = 1/(60+1) = 1/61
        // "b" at rank 2 -> score = 1/(60+2) = 1/62
        assert!(fused[0].0 == "a");
        assert!(fused[1].0 == "b");
        assert!(fused[0].1 > fused[1].1);

        let expected_a = 1.0 / 61.0;
        assert!((fused[0].1 - expected_a).abs() < 1e-6);
    }

    #[test]
    fn test_rrf_fts_only() {
        let vector: Vec<(String, f32)> = vec![];
        let fts = vec![("x".to_string(), 1.0), ("y".to_string(), 0.5)];

        let fused = reciprocal_rank_fusion(&vector, &fts, 60);
        assert_eq!(fused.len(), 2);
        assert!(fused[0].0 == "x");
        assert!(fused[1].0 == "y");

        let expected_x = 1.0 / 61.0;
        assert!((fused[0].1 - expected_x).abs() < 1e-6);
    }

    #[test]
    fn test_rrf_overlapping_results() {
        // Same document in both lists should get a combined score.
        let vector = vec![("shared".to_string(), 0.9)];
        let fts = vec![("shared".to_string(), 1.0)];

        let fused = reciprocal_rank_fusion(&vector, &fts, 60);
        assert_eq!(fused.len(), 1);

        // score = 1/(60+1) + 1/(60+1) = 2/61
        let expected = 2.0 / 61.0;
        assert!(
            (fused[0].1 - expected).abs() < 1e-6,
            "expected {expected}, got {}",
            fused[0].1
        );
    }

    #[test]
    fn test_rrf_k_parameter_effect() {
        let vector = vec![("a".to_string(), 0.9)];
        let fts: Vec<(String, f32)> = vec![];

        // With k=1, score = 1/(1+1) = 0.5
        let fused_k1 = reciprocal_rank_fusion(&vector, &fts, 1);
        let expected_k1 = 1.0 / 2.0;
        assert!(
            (fused_k1[0].1 - expected_k1).abs() < 1e-6,
            "k=1: expected {expected_k1}, got {}",
            fused_k1[0].1
        );

        // With k=60, score = 1/(60+1) ≈ 0.0164
        let fused_k60 = reciprocal_rank_fusion(&vector, &fts, 60);
        let expected_k60 = 1.0 / 61.0;
        assert!(
            (fused_k60[0].1 - expected_k60).abs() < 1e-6,
            "k=60: expected {expected_k60}, got {}",
            fused_k60[0].1
        );

        // Larger k dampens the rank contribution.
        assert!(fused_k1[0].1 > fused_k60[0].1);
    }

    #[test]
    fn test_rrf_empty_inputs() {
        let vector: Vec<(String, f32)> = vec![];
        let fts: Vec<(String, f32)> = vec![];

        let fused = reciprocal_rank_fusion(&vector, &fts, 60);
        assert!(fused.is_empty());
    }

    #[test]
    fn test_rrf_sorted_by_score_descending() {
        let vector = vec![
            ("low".to_string(), 0.5),
            ("mid".to_string(), 0.7),
            ("high".to_string(), 0.9),
        ];
        let fts = vec![("high".to_string(), 1.0), ("low".to_string(), 0.8)];

        let fused = reciprocal_rank_fusion(&vector, &fts, 60);

        // Verify descending order.
        for i in 1..fused.len() {
            assert!(
                fused[i - 1].1 >= fused[i].1,
                "results should be sorted descending: {} >= {}",
                fused[i - 1].1,
                fused[i].1
            );
        }
    }

    #[test]
    fn test_rrf_many_results() {
        let vector: Vec<(String, f32)> = (0..50)
            .map(|i| (format!("v_{i}"), 1.0 - (i as f32 / 50.0)))
            .collect();
        let fts: Vec<(String, f32)> = (0..50)
            .map(|i| (format!("f_{i}"), 1.0 - (i as f32 / 50.0)))
            .collect();

        let fused = reciprocal_rank_fusion(&vector, &fts, 60);
        assert_eq!(fused.len(), 100);

        // First result should be v_0 or f_0 (both rank 1, same score).
        let first_score = fused[0].1;
        let expected = 1.0 / 61.0; // rank 1 with k=60
        assert!(
            (first_score - expected).abs() < 1e-6,
            "first score should be {expected}, got {first_score}"
        );
    }

    // -- Integration-like tests using mock store + in-memory DB -------------

    use crate::vectors::config::EmbeddingConfig;
    use crate::vectors::embedding::EmbeddingPipeline;
    use crate::vectors::store::InMemoryVectorStore;
    use crate::vectors::types::{VectorCollection, VectorDocument, VectorId};

    /// Create a HybridSearch with in-memory store and an in-memory SQLite DB.
    async fn make_hybrid_search() -> (HybridSearch, Arc<dyn VectorStoreBackend>) {
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let config = EmbeddingConfig {
            provider: "mock".to_string(),
            ..EmbeddingConfig::default()
        };
        let embedding = Arc::new(EmbeddingPipeline::new(&config).unwrap());

        let db = Arc::new(
            Database::connect("sqlite::memory:")
                .await
                .expect("in-memory DB"),
        );

        // Create the emails table for FTS queries.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS emails (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL DEFAULT '',
                provider TEXT NOT NULL DEFAULT '',
                subject TEXT NOT NULL DEFAULT '',
                from_addr TEXT NOT NULL DEFAULT '',
                body_text TEXT DEFAULT '',
                received_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                category TEXT DEFAULT 'Uncategorized',
                has_attachments BOOLEAN DEFAULT FALSE,
                is_read BOOLEAN DEFAULT FALSE
            )
            "#,
        )
        .execute(&db.pool)
        .await
        .unwrap();

        let search_config = SearchConfig::default();
        let hs = HybridSearch::new(store.clone(), embedding, db, search_config);
        (hs, store)
    }

    /// Insert a test email into the in-memory DB.
    async fn insert_test_email(
        hs: &HybridSearch,
        id: &str,
        subject: &str,
        from_addr: &str,
        body: &str,
    ) {
        sqlx::query("INSERT INTO emails (id, subject, from_addr, body_text) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(subject)
            .bind(from_addr)
            .bind(body)
            .execute(&hs.db.pool)
            .await
            .unwrap();
    }

    /// Insert a vector document into the store and a matching email row.
    async fn insert_email_with_vector(
        hs: &HybridSearch,
        store: &Arc<dyn VectorStoreBackend>,
        email_id: &str,
        subject: &str,
        from_addr: &str,
        body: &str,
        vector: Vec<f32>,
    ) {
        insert_test_email(hs, email_id, subject, from_addr, body).await;

        let doc = VectorDocument {
            id: VectorId::new(),
            email_id: email_id.to_string(),
            vector,
            metadata: HashMap::new(),
            collection: VectorCollection::EmailText,
            created_at: Utc::now(),
        };
        store.insert(doc).await.unwrap();
    }

    #[tokio::test]
    async fn test_semantic_search() {
        let (hs, store) = make_hybrid_search().await;

        // Insert a document with a known vector.
        let embedding = hs.embedding.embed("quarterly budget review").await.unwrap();
        insert_email_with_vector(
            &hs,
            &store,
            "e1",
            "Budget Review",
            "alice@example.com",
            "Quarterly budget review discussion",
            embedding,
        )
        .await;

        let results = hs
            .semantic_search(
                "quarterly budget review",
                10,
                VectorCollection::EmailText,
                None,
                Some(0.0),
            )
            .await
            .unwrap();

        assert!(!results.is_empty(), "should find at least one result");
        assert_eq!(results[0].document.email_id, "e1");
        assert!(results[0].score > 0.9, "same-text search should score high");
    }

    #[tokio::test]
    async fn test_find_similar() {
        let (hs, store) = make_hybrid_search().await;

        let vec1 = hs
            .embedding
            .embed("quarterly budget review meeting")
            .await
            .unwrap();
        let vec2 = hs
            .embedding
            .embed("quarterly budget planning session")
            .await
            .unwrap();
        let vec3 = hs
            .embedding
            .embed("completely unrelated topic about cats")
            .await
            .unwrap();

        insert_email_with_vector(
            &hs,
            &store,
            "e1",
            "Budget Review",
            "a@x.com",
            "review",
            vec1,
        )
        .await;
        insert_email_with_vector(
            &hs,
            &store,
            "e2",
            "Budget Planning",
            "b@x.com",
            "planning",
            vec2,
        )
        .await;
        insert_email_with_vector(&hs, &store, "e3", "Cat Pictures", "c@x.com", "cats", vec3).await;

        let similar = hs.find_similar("e1", 10).await.unwrap();

        // Should not include e1 itself.
        assert!(
            similar.iter().all(|r| r.document.email_id != "e1"),
            "find_similar should exclude the source email"
        );
    }

    #[tokio::test]
    async fn test_hybrid_search_parallel_execution() {
        let (hs, store) = make_hybrid_search().await;

        // Insert emails into both DB (for FTS) and store (for vector search).
        let vec1 = hs.embedding.embed("important meeting notes").await.unwrap();
        insert_email_with_vector(
            &hs,
            &store,
            "e1",
            "Meeting Notes",
            "boss@work.com",
            "Important meeting notes from today",
            vec1,
        )
        .await;

        // This email is only in the DB (no vector) — should be found by FTS.
        insert_test_email(
            &hs,
            "e2",
            "Meeting Agenda",
            "team@work.com",
            "Agenda for the upcoming meeting",
        )
        .await;

        let query = HybridSearchQuery {
            text: "meeting".to_string(),
            mode: SearchMode::Hybrid,
            filters: None,
            limit: Some(10),
        };

        let result = hs.search(&query).await.unwrap();

        // We should get results from both vector and FTS paths.
        assert!(!result.results.is_empty(), "should have results");
        assert!(result.latency_ms < 5000, "should complete quickly");
        assert_eq!(result.mode, SearchMode::Hybrid);

        // e2 should appear (found by FTS keyword match on "meeting").
        let has_e2 = result.results.iter().any(|r| r.email_id == "e2");
        assert!(has_e2, "e2 should be found via FTS keyword search");
    }

    #[tokio::test]
    async fn test_keyword_only_search() {
        let (hs, _store) = make_hybrid_search().await;

        insert_test_email(
            &hs,
            "k1",
            "Invoice #1234",
            "billing@company.com",
            "Please pay invoice",
        )
        .await;
        insert_test_email(
            &hs,
            "k2",
            "Hello World",
            "friend@example.com",
            "Just saying hi",
        )
        .await;

        let query = HybridSearchQuery {
            text: "invoice".to_string(),
            mode: SearchMode::Keyword,
            filters: None,
            limit: Some(10),
        };

        let result = hs.search(&query).await.unwrap();
        assert_eq!(result.mode, SearchMode::Keyword);
        assert!(
            result.results.iter().any(|r| r.email_id == "k1"),
            "should find k1 by keyword"
        );
        assert!(
            !result.results.iter().any(|r| r.email_id == "k2"),
            "k2 should not match 'invoice'"
        );
    }

    #[tokio::test]
    async fn test_search_mode_semantic_only() {
        let (hs, store) = make_hybrid_search().await;

        let vec1 = hs.embedding.embed("financial report Q3").await.unwrap();
        insert_email_with_vector(
            &hs,
            &store,
            "s1",
            "Q3 Report",
            "cfo@company.com",
            "Q3 financial results",
            vec1,
        )
        .await;

        let query = HybridSearchQuery {
            text: "financial report Q3".to_string(),
            mode: SearchMode::Semantic,
            filters: None,
            limit: Some(5),
        };

        let result = hs.search(&query).await.unwrap();
        assert_eq!(result.mode, SearchMode::Semantic);
        assert!(!result.results.is_empty());
        // All results should be tagged as semantic.
        for r in &result.results {
            assert_eq!(r.match_type, "semantic");
            assert!(r.vector_rank.is_some());
            assert!(r.fts_rank.is_none());
        }
    }
}
