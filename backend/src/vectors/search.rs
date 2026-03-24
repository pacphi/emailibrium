//! Hybrid search combining FTS5 + vector similarity with Reciprocal Rank Fusion (ADR-001).
//!
//! `HybridSearch` runs keyword (FTS5/LIKE) and semantic (vector) searches in
//! parallel via `tokio::join!`, then fuses results using RRF scoring.
//!
//! Extensions (DDD-002):
//! - **SONA Re-ranking** (item #18): optional post-RRF re-ranking using session
//!   preference vectors from the SONA learning engine.
//! - **Multi-collection search** (item #25): searches across multiple
//!   `VectorCollection` variants and merges results with per-collection RRF.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::db::Database;

use super::categorizer::cosine_similarity;
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
    /// Whether SONA re-ranking was applied.
    #[serde(default)]
    pub sona_applied: bool,
    /// Number of collections that were searched.
    #[serde(default = "default_collections_searched")]
    pub collections_searched: usize,
}

fn default_collections_searched() -> usize {
    1
}

// ---------------------------------------------------------------------------
// SONA Re-ranker (DDD-002, item #18)
// ---------------------------------------------------------------------------

/// Re-ranks search results using SONA session preference vectors.
///
/// Applied after RRF fusion, before returning results. The re-ranker blends
/// the RRF score with a cosine similarity boost derived from the user's
/// session preference vector (learned from click/skip interactions).
///
/// `final_score = (1 - sona_weight) * rrf_score + sona_weight * sona_boost`
pub struct SONAReranker {
    /// Blending weight: 0.0 = pure RRF, 1.0 = pure SONA preference.
    sona_weight: f32,
}

impl SONAReranker {
    /// Create a new re-ranker with the given blending weight.
    pub fn new(sona_weight: f32) -> Self {
        Self {
            sona_weight: sona_weight.clamp(0.0, 1.0),
        }
    }

    /// Re-rank fused results using a session preference vector.
    ///
    /// Each result's score is blended with a SONA boost derived from cosine
    /// similarity between the document embedding and the preference vector.
    /// Results without an associated embedding are left unchanged.
    ///
    /// `doc_embeddings` maps `email_id` to the document's embedding vector.
    pub fn rerank(
        &self,
        mut results: Vec<FusedResult>,
        preference_vector: &[f32],
        doc_embeddings: &HashMap<String, Vec<f32>>,
    ) -> Vec<FusedResult> {
        if preference_vector.is_empty() || results.is_empty() {
            return results;
        }

        for result in &mut results {
            if let Some(embedding) = doc_embeddings.get(&result.email_id) {
                let sona_boost = cosine_similarity(embedding, preference_vector);
                // Normalize sona_boost from [-1, 1] to [0, 1] for blending.
                let normalized_boost = (sona_boost + 1.0) / 2.0;
                result.score =
                    (1.0 - self.sona_weight) * result.score + self.sona_weight * normalized_boost;
            }
        }

        // Re-sort by the blended score.
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }
}

// ---------------------------------------------------------------------------
// Multi-collection search helpers (DDD-002, item #25)
// ---------------------------------------------------------------------------

/// Parse a collection name string into a `VectorCollection` enum variant.
pub fn parse_collection(name: &str) -> Option<VectorCollection> {
    match name {
        "email_text" => Some(VectorCollection::EmailText),
        "image_text" => Some(VectorCollection::ImageText),
        "image_visual" => Some(VectorCollection::ImageVisual),
        "attachment_text" => Some(VectorCollection::AttachmentText),
        _ => None,
    }
}

/// Merge multiple ranked lists from different collections using RRF.
///
/// Each collection's results are weighted by the collection weight before
/// fusion. Returns a deduplicated, score-sorted list of `(email_id, score)`.
fn multi_collection_rrf(
    collection_results: &[(Vec<(String, f32)>, f32)], // (results, weight)
    k: u32,
) -> Vec<(String, f32)> {
    let mut scores: HashMap<String, f32> = HashMap::new();

    for (results, weight) in collection_results {
        for (rank_0, (id, _score)) in results.iter().enumerate() {
            let rank = (rank_0 + 1) as f32;
            let rrf_score = weight / (k as f32 + rank);
            *scores.entry(id.clone()).or_insert(0.0) += rrf_score;
        }
    }

    let mut fused: Vec<(String, f32)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
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
        self.search_with_sona(query, None).await
    }

    /// Execute a hybrid search with optional SONA re-ranking.
    ///
    /// When `preference_vector` is `Some` and `config.sona_reranking_enabled`
    /// is `true`, SONA re-ranking is applied after RRF fusion.
    pub async fn search_with_sona(
        &self,
        query: &HybridSearchQuery,
        preference_vector: Option<&[f32]>,
    ) -> Result<HybridSearchResult, VectorError> {
        let start = Instant::now();
        let limit = query.limit.unwrap_or(self.config.default_limit);
        let fetch_limit = self.config.max_limit.min(100);

        let mut result = match query.mode {
            SearchMode::Hybrid => {
                self.search_hybrid(&query.text, fetch_limit, limit, &query.filters)
                    .await?
            }
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
                HybridSearchResult {
                    results: fused,
                    total,
                    latency_ms: 0,
                    mode: SearchMode::Semantic,
                    sona_applied: false,
                    collections_searched: 1,
                }
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
                HybridSearchResult {
                    results: fused,
                    total,
                    latency_ms: 0,
                    mode: SearchMode::Keyword,
                    sona_applied: false,
                    collections_searched: 1,
                }
            }
        };

        // Apply SONA re-ranking if enabled and a preference vector is available.
        if self.config.sona_reranking_enabled {
            if let Some(pref) = preference_vector {
                if !pref.is_empty() {
                    let doc_embeddings = self.fetch_embeddings_for_results(&result.results).await;
                    let reranker = SONAReranker::new(self.config.sona_weight);
                    result.results = reranker.rerank(result.results, pref, &doc_embeddings);
                    result.total = result.results.len();
                    result.sona_applied = true;
                    debug!(
                        sona_weight = self.config.sona_weight,
                        results = result.total,
                        "SONA re-ranking applied"
                    );
                }
            }
        }

        result.latency_ms = start.elapsed().as_millis() as u64;
        Ok(result)
    }

    /// Fetch embeddings for a list of fused results from the vector store.
    ///
    /// Returns a map of email_id -> embedding vector. Results without a stored
    /// embedding are silently omitted.
    async fn fetch_embeddings_for_results(
        &self,
        results: &[FusedResult],
    ) -> HashMap<String, Vec<f32>> {
        let mut embeddings = HashMap::new();
        for result in results {
            if let Ok(Some(doc)) = self.store.get_by_email_id(&result.email_id).await {
                embeddings.insert(result.email_id.clone(), doc.vector);
            }
        }
        embeddings
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
    ///
    /// When `config.collections` contains more than one entry, runs a
    /// multi-collection vector search and merges results with per-collection
    /// weighted RRF before fusing with FTS results (DDD-002, item #25).
    async fn search_hybrid(
        &self,
        text: &str,
        fetch_limit: usize,
        result_limit: usize,
        filters: &Option<SearchFilters>,
    ) -> Result<HybridSearchResult, VectorError> {
        // Embed the query.
        let query_vec = self.embedding.embed(text).await?;

        // Determine which collections to search.
        let collections: Vec<VectorCollection> = self
            .config
            .collections
            .iter()
            .filter_map(|name| parse_collection(name))
            .collect();

        let collections_searched = collections.len().max(1);

        // Multi-collection vector search: build params first, then create
        // futures so that borrows outlive the loop (E0597 fix).
        let all_params: Vec<SearchParams> = collections
            .iter()
            .map(|collection| SearchParams {
                vector: query_vec.clone(),
                limit: fetch_limit,
                collection: collection.clone(),
                filters: None,
                min_score: None,
            })
            .collect();

        let vector_futures: Vec<_> = all_params
            .iter()
            .map(|params| self.store.search(params))
            .collect();

        // Run all vector searches and FTS in parallel.
        let (multi_vector_results, fts_res) = tokio::join!(
            futures::future::join_all(vector_futures),
            self.fts_search(text, fetch_limit)
        );

        // Build ID -> metadata lookup and merge vector results across collections.
        let mut meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let merged_vector_pairs: Vec<(String, f32)> = if collections_searched <= 1 {
            // Single collection: use results directly.
            let vector_results = multi_vector_results
                .into_iter()
                .next()
                .unwrap_or(Ok(Vec::new()))
                .unwrap_or_else(|e| {
                    warn!("vector search failed: {e}");
                    Vec::new()
                });
            vector_results
                .iter()
                .map(|sr| {
                    meta_map.insert(sr.document.email_id.clone(), sr.document.metadata.clone());
                    (sr.document.email_id.clone(), sr.score)
                })
                .collect()
        } else {
            // Multi-collection: merge with per-collection weighted RRF.
            let mut collection_results: Vec<(Vec<(String, f32)>, f32)> = Vec::new();
            for (i, result) in multi_vector_results.into_iter().enumerate() {
                let collection_name = &self.config.collections[i];
                let weight = self
                    .config
                    .collection_weights
                    .get(collection_name)
                    .copied()
                    .unwrap_or(1.0);

                let results = result.unwrap_or_else(|e| {
                    warn!(collection = collection_name, "vector search failed: {e}");
                    Vec::new()
                });

                let pairs: Vec<(String, f32)> = results
                    .iter()
                    .map(|sr| {
                        meta_map.insert(sr.document.email_id.clone(), sr.document.metadata.clone());
                        (sr.document.email_id.clone(), sr.score)
                    })
                    .collect();

                collection_results.push((pairs, weight));
            }

            info!(
                collections = collections_searched,
                "Multi-collection vector search completed"
            );
            multi_collection_rrf(&collection_results, 60)
        };

        let fts_results = fts_res.unwrap_or_else(|e| {
            warn!("FTS search failed: {e}");
            Vec::new()
        });

        // Fuse vector results (potentially multi-collection) with FTS using RRF (k=60).
        let fused = reciprocal_rank_fusion_detailed(&merged_vector_pairs, &fts_results, 60);

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
            sona_applied: false,
            collections_searched,
        })
    }

    /// Keyword search using FTS5 MATCH with BM25 ranking (ADR-001).
    ///
    /// Falls back to LIKE-based search when the `email_fts` virtual table
    /// does not exist (e.g. migrations have not yet been applied).
    async fn fts_search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(String, f32)>, VectorError> {
        match self.fts5_search(query, limit).await {
            Ok(results) => Ok(results),
            Err(_) => {
                debug!("FTS5 table unavailable, falling back to LIKE search");
                self.like_search(query, limit).await
            }
        }
    }

    /// FTS5 MATCH query returning `(email_id, bm25_score)` pairs.
    ///
    /// The `rank` column is the built-in BM25 score provided by FTS5
    /// (negative values where more-negative = better match). We negate
    /// so that higher is better, consistent with the rest of the pipeline.
    async fn fts5_search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(String, f32)>, VectorError> {
        let limit_i64 = limit as i64;

        // FTS5 rank values are negative (lower = better). We negate them so
        // that a higher score means a better match, matching the convention
        // used by the vector similarity pipeline.
        let rows: Vec<(String, f64)> = sqlx::query_as(
            r#"
            SELECT email_id, -rank AS score
            FROM email_fts
            WHERE email_fts MATCH ?1
            ORDER BY rank
            LIMIT ?2
            "#,
        )
        .bind(query)
        .bind(limit_i64)
        .fetch_all(&self.db.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(id, score)| (id, score as f32))
            .collect())
    }

    /// Legacy LIKE-based keyword search (fallback when FTS5 is unavailable).
    async fn like_search(
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
                labels TEXT DEFAULT '',
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

        // Create FTS5 virtual table and sync triggers (mirrors migration 004).
        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS email_fts USING fts5(
                email_id,
                subject,
                from_addr,
                body_text,
                labels,
                content='emails',
                content_rowid='rowid',
                tokenize='porter unicode61'
            )
            "#,
        )
        .execute(&db.pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS emails_ai AFTER INSERT ON emails BEGIN
                INSERT INTO email_fts(rowid, email_id, subject, from_addr, body_text, labels)
                VALUES (new.rowid, new.id, new.subject, new.from_addr, new.body_text, new.labels);
            END
            "#,
        )
        .execute(&db.pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS emails_ad AFTER DELETE ON emails BEGIN
                INSERT INTO email_fts(email_fts, rowid, email_id, subject, from_addr, body_text, labels)
                VALUES ('delete', old.rowid, old.id, old.subject, old.from_addr, old.body_text, old.labels);
            END
            "#,
        )
        .execute(&db.pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS emails_au AFTER UPDATE ON emails BEGIN
                INSERT INTO email_fts(email_fts, rowid, email_id, subject, from_addr, body_text, labels)
                VALUES ('delete', old.rowid, old.id, old.subject, old.from_addr, old.body_text, old.labels);
                INSERT INTO email_fts(rowid, email_id, subject, from_addr, body_text, labels)
                VALUES (new.rowid, new.id, new.subject, new.from_addr, new.body_text, new.labels);
            END
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

    // -- SONA Re-ranker unit tests (item #18) --------------------------------

    #[test]
    fn test_sona_reranker_boosts_aligned_results() {
        let reranker = SONAReranker::new(0.5);

        // Preference vector points toward [1, 0, 0].
        let preference = vec![1.0, 0.0, 0.0];

        let mut doc_embeddings = HashMap::new();
        doc_embeddings.insert("aligned".to_string(), vec![1.0, 0.0, 0.0]);
        doc_embeddings.insert("orthogonal".to_string(), vec![0.0, 1.0, 0.0]);
        doc_embeddings.insert("opposite".to_string(), vec![-1.0, 0.0, 0.0]);

        let results = vec![
            FusedResult {
                email_id: "opposite".to_string(),
                score: 0.5,
                match_type: "hybrid".to_string(),
                vector_rank: Some(1),
                fts_rank: None,
                metadata: HashMap::new(),
            },
            FusedResult {
                email_id: "orthogonal".to_string(),
                score: 0.5,
                match_type: "hybrid".to_string(),
                vector_rank: Some(2),
                fts_rank: None,
                metadata: HashMap::new(),
            },
            FusedResult {
                email_id: "aligned".to_string(),
                score: 0.5,
                match_type: "hybrid".to_string(),
                vector_rank: Some(3),
                fts_rank: None,
                metadata: HashMap::new(),
            },
        ];

        let reranked = reranker.rerank(results, &preference, &doc_embeddings);

        // Aligned result should be first after SONA re-ranking.
        assert_eq!(reranked[0].email_id, "aligned");
        // Opposite result should be last.
        assert_eq!(reranked[2].email_id, "opposite");
        // Scores should be descending.
        assert!(reranked[0].score >= reranked[1].score);
        assert!(reranked[1].score >= reranked[2].score);
    }

    #[test]
    fn test_sona_reranker_zero_weight_preserves_order() {
        let reranker = SONAReranker::new(0.0);
        let preference = vec![1.0, 0.0, 0.0];

        let mut doc_embeddings = HashMap::new();
        doc_embeddings.insert("a".to_string(), vec![-1.0, 0.0, 0.0]);
        doc_embeddings.insert("b".to_string(), vec![1.0, 0.0, 0.0]);

        let results = vec![
            FusedResult {
                email_id: "a".to_string(),
                score: 0.9,
                match_type: "hybrid".to_string(),
                vector_rank: Some(1),
                fts_rank: None,
                metadata: HashMap::new(),
            },
            FusedResult {
                email_id: "b".to_string(),
                score: 0.8,
                match_type: "hybrid".to_string(),
                vector_rank: Some(2),
                fts_rank: None,
                metadata: HashMap::new(),
            },
        ];

        let reranked = reranker.rerank(results, &preference, &doc_embeddings);

        // With weight=0, original order should be preserved.
        assert_eq!(reranked[0].email_id, "a");
        assert_eq!(reranked[1].email_id, "b");
    }

    #[test]
    fn test_sona_reranker_empty_preference_is_noop() {
        let reranker = SONAReranker::new(0.5);
        let empty_pref: Vec<f32> = vec![];

        let results = vec![FusedResult {
            email_id: "a".to_string(),
            score: 0.9,
            match_type: "hybrid".to_string(),
            vector_rank: Some(1),
            fts_rank: None,
            metadata: HashMap::new(),
        }];

        let reranked = reranker.rerank(results.clone(), &empty_pref, &HashMap::new());
        assert_eq!(reranked[0].score, results[0].score);
    }

    #[test]
    fn test_sona_reranker_missing_embedding_preserves_score() {
        let reranker = SONAReranker::new(0.5);
        let preference = vec![1.0, 0.0, 0.0];

        // No embeddings in the map for "a".
        let doc_embeddings: HashMap<String, Vec<f32>> = HashMap::new();

        let results = vec![FusedResult {
            email_id: "a".to_string(),
            score: 0.9,
            match_type: "hybrid".to_string(),
            vector_rank: Some(1),
            fts_rank: None,
            metadata: HashMap::new(),
        }];

        let reranked = reranker.rerank(results, &preference, &doc_embeddings);
        // Score should be unchanged since no embedding was found.
        assert!((reranked[0].score - 0.9).abs() < 1e-6);
    }

    // -- Multi-collection RRF tests (item #25) --------------------------------

    #[test]
    fn test_multi_collection_rrf_single_collection() {
        let results = vec![(vec![("a".to_string(), 0.9), ("b".to_string(), 0.8)], 1.0)];
        let fused = multi_collection_rrf(&results, 60);
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].0, "a");
        assert_eq!(fused[1].0, "b");
    }

    #[test]
    fn test_multi_collection_rrf_merges_collections() {
        // Two collections with overlapping results.
        let collection1 = vec![("shared".to_string(), 0.9), ("only_c1".to_string(), 0.7)];
        let collection2 = vec![("shared".to_string(), 0.8), ("only_c2".to_string(), 0.6)];

        let results = vec![(collection1, 1.0), (collection2, 1.0)];
        let fused = multi_collection_rrf(&results, 60);

        // "shared" should be first (appears in both).
        assert_eq!(fused[0].0, "shared");
        assert_eq!(fused.len(), 3);

        // "shared" score = 1/(60+1) + 1/(60+1) = 2/61.
        let expected_shared = 2.0 / 61.0;
        assert!(
            (fused[0].1 - expected_shared).abs() < 1e-6,
            "expected {expected_shared}, got {}",
            fused[0].1
        );
    }

    #[test]
    fn test_multi_collection_rrf_respects_weights() {
        // Collection1 has weight 2.0, collection2 has weight 0.5.
        let c1 = vec![("a".to_string(), 0.9)];
        let c2 = vec![("b".to_string(), 0.8)];

        let results = vec![(c1, 2.0), (c2, 0.5)];
        let fused = multi_collection_rrf(&results, 60);

        // "a" from weight=2.0 collection should score higher than "b" from weight=0.5.
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].0, "a");
        assert_eq!(fused[1].0, "b");

        // a score = 2.0 / (60+1) = 2/61
        // b score = 0.5 / (60+1) = 0.5/61
        let expected_a = 2.0 / 61.0;
        let expected_b = 0.5 / 61.0;
        assert!((fused[0].1 - expected_a).abs() < 1e-6);
        assert!((fused[1].1 - expected_b).abs() < 1e-6);
    }

    #[test]
    fn test_parse_collection_known_variants() {
        assert_eq!(
            parse_collection("email_text"),
            Some(VectorCollection::EmailText)
        );
        assert_eq!(
            parse_collection("image_text"),
            Some(VectorCollection::ImageText)
        );
        assert_eq!(
            parse_collection("image_visual"),
            Some(VectorCollection::ImageVisual)
        );
        assert_eq!(
            parse_collection("attachment_text"),
            Some(VectorCollection::AttachmentText)
        );
        assert_eq!(parse_collection("unknown"), None);
    }

    // -- SONA integration test with HybridSearch ------------------------------

    #[tokio::test]
    async fn test_search_with_sona_reranking() {
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

        // Create emails table and FTS.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS emails (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL DEFAULT '',
                provider TEXT NOT NULL DEFAULT '',
                subject TEXT NOT NULL DEFAULT '',
                from_addr TEXT NOT NULL DEFAULT '',
                body_text TEXT DEFAULT '',
                labels TEXT DEFAULT '',
                received_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                category TEXT DEFAULT 'Uncategorized',
                has_attachments BOOLEAN DEFAULT FALSE,
                is_read BOOLEAN DEFAULT FALSE
            )",
        )
        .execute(&db.pool)
        .await
        .unwrap();

        // SONA-enabled search config.
        let mut search_config = SearchConfig::default();
        search_config.sona_reranking_enabled = true;
        search_config.sona_weight = 0.8;

        let hs = HybridSearch::new(store.clone(), embedding.clone(), db.clone(), search_config);

        // Insert two documents: one aligned with preference, one not.
        let vec_aligned = embedding.embed("budget finance money").await.unwrap();
        let vec_other = embedding.embed("cats dogs pets").await.unwrap();

        let doc_aligned = VectorDocument {
            id: VectorId::new(),
            email_id: "aligned".to_string(),
            vector: vec_aligned.clone(),
            metadata: HashMap::new(),
            collection: VectorCollection::EmailText,
            created_at: Utc::now(),
        };
        let doc_other = VectorDocument {
            id: VectorId::new(),
            email_id: "other".to_string(),
            vector: vec_other.clone(),
            metadata: HashMap::new(),
            collection: VectorCollection::EmailText,
            created_at: Utc::now(),
        };
        store.insert(doc_aligned).await.unwrap();
        store.insert(doc_other).await.unwrap();

        sqlx::query("INSERT INTO emails (id, subject) VALUES (?, ?)")
            .bind("aligned")
            .bind("Budget Report")
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO emails (id, subject) VALUES (?, ?)")
            .bind("other")
            .bind("Cat Pictures")
            .execute(&db.pool)
            .await
            .unwrap();

        // The preference vector is the "aligned" embedding (user prefers finance).
        let query = HybridSearchQuery {
            text: "report".to_string(),
            mode: SearchMode::Semantic,
            filters: None,
            limit: Some(10),
        };

        // With SONA enabled, the aligned doc should be boosted.
        let result = hs
            .search_with_sona(&query, Some(&vec_aligned))
            .await
            .unwrap();

        assert!(result.sona_applied, "SONA should have been applied");
    }

    // -- Multi-collection integration test ------------------------------------

    #[tokio::test]
    async fn test_hybrid_search_multi_collection() {
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

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS emails (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL DEFAULT '',
                provider TEXT NOT NULL DEFAULT '',
                subject TEXT NOT NULL DEFAULT '',
                from_addr TEXT NOT NULL DEFAULT '',
                body_text TEXT DEFAULT '',
                labels TEXT DEFAULT '',
                received_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                category TEXT DEFAULT 'Uncategorized',
                has_attachments BOOLEAN DEFAULT FALSE,
                is_read BOOLEAN DEFAULT FALSE
            )",
        )
        .execute(&db.pool)
        .await
        .unwrap();

        // Configure multi-collection search.
        let mut search_config = SearchConfig::default();
        search_config.collections = vec!["email_text".to_string(), "attachment_text".to_string()];
        search_config
            .collection_weights
            .insert("email_text".to_string(), 1.0);
        search_config
            .collection_weights
            .insert("attachment_text".to_string(), 0.8);

        let hs = HybridSearch::new(store.clone(), embedding.clone(), db.clone(), search_config);

        // Insert a doc in EmailText collection.
        let vec1 = embedding.embed("quarterly budget review").await.unwrap();
        let doc1 = VectorDocument {
            id: VectorId::new(),
            email_id: "email_hit".to_string(),
            vector: vec1,
            metadata: HashMap::new(),
            collection: VectorCollection::EmailText,
            created_at: Utc::now(),
        };
        store.insert(doc1).await.unwrap();

        // Insert a doc in AttachmentText collection.
        let vec2 = embedding.embed("quarterly budget review").await.unwrap();
        let doc2 = VectorDocument {
            id: VectorId::new(),
            email_id: "attachment_hit".to_string(),
            vector: vec2,
            metadata: HashMap::new(),
            collection: VectorCollection::AttachmentText,
            created_at: Utc::now(),
        };
        store.insert(doc2).await.unwrap();

        sqlx::query("INSERT INTO emails (id, subject) VALUES (?, ?)")
            .bind("email_hit")
            .bind("Budget Review")
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO emails (id, subject) VALUES (?, ?)")
            .bind("attachment_hit")
            .bind("Attachment Budget")
            .execute(&db.pool)
            .await
            .unwrap();

        let query = HybridSearchQuery {
            text: "quarterly budget review".to_string(),
            mode: SearchMode::Hybrid,
            filters: None,
            limit: Some(10),
        };

        let result = hs.search(&query).await.unwrap();

        // Should search across 2 collections.
        assert_eq!(
            result.collections_searched, 2,
            "should search 2 collections"
        );

        // Both documents should appear in results.
        let email_ids: Vec<&str> = result.results.iter().map(|r| r.email_id.as_str()).collect();
        assert!(
            email_ids.contains(&"email_hit"),
            "should find email_text result"
        );
        assert!(
            email_ids.contains(&"attachment_hit"),
            "should find attachment_text result"
        );
    }

    // -- Config defaults tests ------------------------------------------------

    #[test]
    fn test_search_config_defaults() {
        let config = SearchConfig::default();
        assert!(!config.sona_reranking_enabled);
        assert!((config.sona_weight - 0.3).abs() < 1e-6);
        assert_eq!(config.collections, vec!["email_text"]);
        assert!(config.collection_weights.is_empty());
    }
}
