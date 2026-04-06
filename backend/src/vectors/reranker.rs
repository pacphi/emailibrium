//! Cross-encoder re-ranking for search results (ADR-029).
//!
//! Provides a trait-based abstraction for re-ranking candidates after RRF fusion.
//! The default [`PassthroughReranker`] preserves original ordering and can be
//! swapped for a cross-encoder implementation (e.g. `fastembed::TextRerank` with
//! `BAAI/bge-reranker-base`) when a model is available.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the re-ranking stage.
/// Configuration for the re-ranking stage.
///
/// Mirrors [`crate::vectors::yaml_config::RerankingConfig`] with identical
/// fields.  Construct via [`RerankerConfig::from_yaml`] at search-engine
/// initialisation so that `tuning.yaml` drives the behaviour.
/// Currently used in tests; production wiring arrives in Phase 2.
#[allow(dead_code)] // Wired via from_yaml in Phase 2 cross-encoder integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankerConfig {
    /// Whether cross-encoder re-ranking is enabled.
    pub enabled: bool,
    /// Model identifier (e.g. `BAAI/bge-reranker-base`).
    pub model: String,
    /// How many candidates to retrieve from RRF before re-ranking.
    pub candidates: usize,
    /// How many results to return after re-ranking.
    pub top_k: usize,
    /// Maximum time in milliseconds allowed for re-ranking before falling
    /// back to the passthrough strategy.
    pub timeout_ms: u64,
}

#[allow(dead_code)] // Wired in Phase 2 cross-encoder integration
impl RerankerConfig {
    /// Build from the YAML-sourced reranking configuration.
    pub fn from_yaml(cfg: &crate::vectors::yaml_config::RerankingConfig) -> Self {
        Self {
            enabled: cfg.enabled,
            model: cfg.model.clone(),
            candidates: cfg.candidates,
            top_k: cfg.top_k,
            timeout_ms: cfg.timeout_ms,
        }
    }
}

impl Default for RerankerConfig {
    fn default() -> Self {
        Self {
            enabled: false, // disabled by default until model is downloaded
            model: "BAAI/bge-reranker-base".to_string(),
            candidates: 50,
            top_k: 10,
            timeout_ms: 100,
        }
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A scored document pair for re-ranking.
#[derive(Debug, Clone)]
pub struct RerankCandidate {
    /// Unique document identifier.
    pub id: String,
    /// Document text to score against the query.
    pub text: String,
    /// Score assigned by the upstream stage (e.g. RRF).
    pub original_score: f32,
}

/// Result of re-ranking a single candidate.
#[derive(Debug, Clone)]
pub struct RerankResult {
    /// Unique document identifier.
    pub id: String,
    /// Score assigned by the re-ranker (or the original score for passthrough).
    pub score: f32,
    /// Score from the upstream stage preserved for diagnostics.
    pub original_score: f32,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Trait for cross-encoder re-ranking implementations.
///
/// Implementations receive a query and a list of candidates, and return the
/// top-k candidates re-scored by a cross-encoder model.
#[async_trait]
pub trait Reranker: Send + Sync {
    /// Re-rank `candidates` against `query` and return the top `top_k` results
    /// ordered by descending re-ranker score.
    async fn rerank(
        &self,
        query: &str,
        candidates: Vec<RerankCandidate>,
        top_k: usize,
    ) -> Result<Vec<RerankResult>, Box<dyn std::error::Error + Send + Sync>>;
}

// ---------------------------------------------------------------------------
// Passthrough implementation
// ---------------------------------------------------------------------------

/// Passthrough re-ranker that preserves original RRF ordering.
///
/// Used when no cross-encoder model is available or re-ranking is disabled.
/// Simply truncates the candidate list to `top_k` without changing scores.
pub struct PassthroughReranker;

#[async_trait]
impl Reranker for PassthroughReranker {
    async fn rerank(
        &self,
        _query: &str,
        candidates: Vec<RerankCandidate>,
        top_k: usize,
    ) -> Result<Vec<RerankResult>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(candidates
            .into_iter()
            .take(top_k)
            .map(|c| RerankResult {
                id: c.id,
                score: c.original_score,
                original_score: c.original_score,
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_candidates(n: usize) -> Vec<RerankCandidate> {
        (0..n)
            .map(|i| RerankCandidate {
                id: format!("doc_{i}"),
                text: format!("Document number {i}"),
                original_score: 1.0 - (i as f32 / n as f32),
            })
            .collect()
    }

    #[tokio::test]
    async fn passthrough_preserves_order() {
        let reranker = PassthroughReranker;
        let candidates = make_candidates(5);
        let results = reranker
            .rerank("test query", candidates.clone(), 5)
            .await
            .unwrap();

        assert_eq!(results.len(), 5);
        for (i, result) in results.iter().enumerate() {
            assert_eq!(result.id, format!("doc_{i}"));
            assert_eq!(result.score, result.original_score);
        }
    }

    #[tokio::test]
    async fn passthrough_truncates_to_top_k() {
        let reranker = PassthroughReranker;
        let candidates = make_candidates(10);
        let results = reranker.rerank("test query", candidates, 3).await.unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].id, "doc_0");
        assert_eq!(results[1].id, "doc_1");
        assert_eq!(results[2].id, "doc_2");
    }

    #[tokio::test]
    async fn passthrough_handles_empty_candidates() {
        let reranker = PassthroughReranker;
        let results = reranker.rerank("query", vec![], 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn passthrough_top_k_larger_than_candidates() {
        let reranker = PassthroughReranker;
        let candidates = make_candidates(3);
        let results = reranker.rerank("query", candidates, 10).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn passthrough_score_equals_original_score() {
        let reranker = PassthroughReranker;
        let candidates = vec![
            RerankCandidate {
                id: "a".to_string(),
                text: "alpha".to_string(),
                original_score: 0.95,
            },
            RerankCandidate {
                id: "b".to_string(),
                text: "beta".to_string(),
                original_score: 0.42,
            },
        ];
        let results = reranker.rerank("query", candidates, 2).await.unwrap();

        assert!((results[0].score - 0.95).abs() < f32::EPSILON);
        assert!((results[0].original_score - 0.95).abs() < f32::EPSILON);
        assert!((results[1].score - 0.42).abs() < f32::EPSILON);
        assert!((results[1].original_score - 0.42).abs() < f32::EPSILON);
    }

    #[test]
    fn default_config_is_disabled() {
        let config = RerankerConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.model, "BAAI/bge-reranker-base");
        assert_eq!(config.candidates, 50);
        assert_eq!(config.top_k, 10);
        assert_eq!(config.timeout_ms, 100);

        // Verify from_yaml round-trips the YAML defaults identically.
        let yaml_cfg = crate::vectors::yaml_config::RerankingConfig::default();
        let from_yaml = RerankerConfig::from_yaml(&yaml_cfg);
        assert_eq!(from_yaml.enabled, config.enabled);
        assert_eq!(from_yaml.model, config.model);
        assert_eq!(from_yaml.candidates, config.candidates);
        assert_eq!(from_yaml.top_k, config.top_k);
        assert_eq!(from_yaml.timeout_ms, config.timeout_ms);
    }
}
