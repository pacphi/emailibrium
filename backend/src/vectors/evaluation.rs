//! A/B evaluation infrastructure for search and learning quality (ADR-004, item #22).
//!
//! Provides the ability to run controlled experiments comparing different
//! configurations (e.g., SONA-enabled vs baseline, different reranking weights)
//! and track quality metrics (precision, recall, nDCG, MRR) per variant.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info};

use super::error::VectorError;
use crate::db::Database;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Status of an A/B test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestStatus {
    /// Test is actively collecting data.
    Running,
    /// Test has been concluded and results are final.
    Concluded,
    /// Test was cancelled before enough data was collected.
    Cancelled,
}

/// Configuration for one variant of an A/B test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantConfig {
    /// Human-readable label (e.g., "sona_enabled", "baseline").
    pub name: String,
    /// Free-form configuration parameters for this variant.
    pub params: HashMap<String, String>,
}

/// Quality metrics collected for a variant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvaluationMetrics {
    /// Mean Reciprocal Rank (MRR).
    pub mrr: f64,
    /// Precision at K (default K=5).
    pub precision_at_k: f64,
    /// Recall at K.
    pub recall_at_k: f64,
    /// Normalized Discounted Cumulative Gain.
    pub ndcg: f64,
    /// Silhouette score for clustering quality (-1..1).
    pub silhouette_score: Option<f64>,
    /// Number of queries evaluated.
    pub query_count: u64,
    /// Sum of per-query MRR for incremental averaging.
    pub mrr_sum: f64,
    /// Sum of per-query precision for incremental averaging.
    pub precision_sum: f64,
    /// Sum of per-query recall for incremental averaging.
    pub recall_sum: f64,
    /// Sum of per-query nDCG for incremental averaging.
    pub ndcg_sum: f64,
}

impl EvaluationMetrics {
    /// Record a single query observation and update running averages.
    pub fn record_query(&mut self, mrr: f64, precision: f64, recall: f64, ndcg: f64) {
        self.mrr_sum += mrr;
        self.precision_sum += precision;
        self.recall_sum += recall;
        self.ndcg_sum += ndcg;
        self.query_count += 1;

        self.mrr = self.mrr_sum / self.query_count as f64;
        self.precision_at_k = self.precision_sum / self.query_count as f64;
        self.recall_at_k = self.recall_sum / self.query_count as f64;
        self.ndcg = self.ndcg_sum / self.query_count as f64;
    }
}

/// A complete A/B test definition with two variants and results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ABTest {
    /// Unique test identifier.
    pub test_id: String,
    /// Human-readable test name.
    pub name: String,
    /// Variant A (typically the control / baseline).
    pub variant_a: VariantConfig,
    /// Variant B (typically the treatment / new feature).
    pub variant_b: VariantConfig,
    /// Fraction of traffic routed to variant B (0.0 .. 1.0).
    pub traffic_split: f32,
    /// When the test was created.
    pub created_at: DateTime<Utc>,
    /// When the test was concluded (if applicable).
    pub concluded_at: Option<DateTime<Utc>>,
    /// Current status.
    pub status: TestStatus,
    /// Collected metrics for variant A.
    pub metrics_a: EvaluationMetrics,
    /// Collected metrics for variant B.
    pub metrics_b: EvaluationMetrics,
}

/// Summary of an A/B test with a recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ABTestSummary {
    pub test_id: String,
    pub name: String,
    pub status: TestStatus,
    pub variant_a_name: String,
    pub variant_b_name: String,
    pub metrics_a: EvaluationMetrics,
    pub metrics_b: EvaluationMetrics,
    /// Which variant is recommended ("a", "b", or "inconclusive").
    pub recommendation: String,
    /// Brief rationale for the recommendation.
    pub rationale: String,
}

// ---------------------------------------------------------------------------
// Evaluation Engine
// ---------------------------------------------------------------------------

/// Manages A/B tests and evaluation metrics.
pub struct EvaluationEngine {
    /// Active and historical tests, keyed by test_id.
    tests: RwLock<HashMap<String, ABTest>>,
    /// Database for persistent storage of test results.
    db: Arc<Database>,
}

impl EvaluationEngine {
    /// Create a new evaluation engine.
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            tests: RwLock::new(HashMap::new()),
            db,
        }
    }

    /// Ensure the evaluation tables exist in the database.
    pub async fn ensure_tables(&self) -> Result<(), VectorError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS ab_tests (
                test_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                variant_a_config TEXT NOT NULL,
                variant_b_config TEXT NOT NULL,
                traffic_split REAL NOT NULL DEFAULT 0.5,
                status TEXT NOT NULL DEFAULT 'running',
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                concluded_at TIMESTAMP,
                metrics_a TEXT NOT NULL DEFAULT '{}',
                metrics_b TEXT NOT NULL DEFAULT '{}'
            )",
        )
        .execute(&self.db.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS ab_test_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                test_id TEXT NOT NULL REFERENCES ab_tests(test_id),
                variant TEXT NOT NULL CHECK(variant IN ('a', 'b')),
                timestamp TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                mrr REAL,
                precision_at_k REAL,
                recall_at_k REAL,
                ndcg REAL
            )",
        )
        .execute(&self.db.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_ab_results_test ON ab_test_results(test_id)")
            .execute(&self.db.pool)
            .await?;

        Ok(())
    }

    /// Create a new A/B test.
    pub async fn create_test(
        &self,
        name: String,
        variant_a: VariantConfig,
        variant_b: VariantConfig,
        traffic_split: f32,
    ) -> Result<ABTest, VectorError> {
        let test_id = uuid::Uuid::new_v4().to_string();
        let traffic_split = traffic_split.clamp(0.0, 1.0);

        let test = ABTest {
            test_id: test_id.clone(),
            name: name.clone(),
            variant_a: variant_a.clone(),
            variant_b: variant_b.clone(),
            traffic_split,
            created_at: Utc::now(),
            concluded_at: None,
            status: TestStatus::Running,
            metrics_a: EvaluationMetrics::default(),
            metrics_b: EvaluationMetrics::default(),
        };

        // Store in database.
        let va_json = serde_json::to_string(&variant_a)?;
        let vb_json = serde_json::to_string(&variant_b)?;

        sqlx::query(
            "INSERT INTO ab_tests (test_id, name, variant_a_config, variant_b_config, traffic_split, status, created_at)
             VALUES (?, ?, ?, ?, ?, 'running', ?)",
        )
        .bind(&test_id)
        .bind(&name)
        .bind(&va_json)
        .bind(&vb_json)
        .bind(traffic_split)
        .bind(test.created_at)
        .execute(&self.db.pool)
        .await?;

        // Store in memory.
        self.tests
            .write()
            .await
            .insert(test_id.clone(), test.clone());

        info!(test_id = %test_id, name = %name, "Created A/B test");

        Ok(test)
    }

    /// Route a query to a variant based on the traffic split.
    ///
    /// Returns "a" or "b".
    pub async fn route_query(&self, test_id: &str) -> Result<String, VectorError> {
        let tests = self.tests.read().await;
        let test = tests
            .get(test_id)
            .ok_or_else(|| VectorError::ConfigError(format!("A/B test not found: {test_id}")))?;

        if test.status != TestStatus::Running {
            return Err(VectorError::ConfigError(format!(
                "A/B test {test_id} is not running"
            )));
        }

        let r: f32 = rand::random();
        Ok(if r < test.traffic_split { "b" } else { "a" }.to_string())
    }

    /// Record query-level evaluation metrics for a variant.
    pub async fn record_observation(
        &self,
        test_id: &str,
        variant: &str,
        mrr: f64,
        precision: f64,
        recall: f64,
        ndcg: f64,
    ) -> Result<(), VectorError> {
        // Update in-memory metrics.
        {
            let mut tests = self.tests.write().await;
            let test = tests.get_mut(test_id).ok_or_else(|| {
                VectorError::ConfigError(format!("A/B test not found: {test_id}"))
            })?;

            match variant {
                "a" => test.metrics_a.record_query(mrr, precision, recall, ndcg),
                "b" => test.metrics_b.record_query(mrr, precision, recall, ndcg),
                _ => {
                    return Err(VectorError::ConfigError(format!(
                        "Invalid variant: {variant}"
                    )));
                }
            }
        }

        // Persist observation.
        sqlx::query(
            "INSERT INTO ab_test_results (test_id, variant, timestamp, mrr, precision_at_k, recall_at_k, ndcg)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(test_id)
        .bind(variant)
        .bind(Utc::now())
        .bind(mrr)
        .bind(precision)
        .bind(recall)
        .bind(ndcg)
        .execute(&self.db.pool)
        .await?;

        debug!(
            test_id = %test_id,
            variant = %variant,
            mrr = mrr,
            "Recorded A/B observation"
        );

        Ok(())
    }

    /// Get the current state of an A/B test.
    pub async fn get_test(&self, test_id: &str) -> Result<ABTest, VectorError> {
        let tests = self.tests.read().await;
        tests
            .get(test_id)
            .cloned()
            .ok_or_else(|| VectorError::ConfigError(format!("A/B test not found: {test_id}")))
    }

    /// Conclude an A/B test and generate a summary with recommendation.
    pub async fn conclude_test(&self, test_id: &str) -> Result<ABTestSummary, VectorError> {
        let mut tests = self.tests.write().await;
        let test = tests
            .get_mut(test_id)
            .ok_or_else(|| VectorError::ConfigError(format!("A/B test not found: {test_id}")))?;

        test.status = TestStatus::Concluded;
        test.concluded_at = Some(Utc::now());

        // Persist status change.
        sqlx::query("UPDATE ab_tests SET status = 'concluded', concluded_at = ? WHERE test_id = ?")
            .bind(test.concluded_at)
            .bind(test_id)
            .execute(&self.db.pool)
            .await?;

        // Generate recommendation.
        let (recommendation, rationale) =
            Self::compute_recommendation(&test.metrics_a, &test.metrics_b);

        let summary = ABTestSummary {
            test_id: test_id.to_string(),
            name: test.name.clone(),
            status: TestStatus::Concluded,
            variant_a_name: test.variant_a.name.clone(),
            variant_b_name: test.variant_b.name.clone(),
            metrics_a: test.metrics_a.clone(),
            metrics_b: test.metrics_b.clone(),
            recommendation,
            rationale,
        };

        info!(
            test_id = %test_id,
            recommendation = %summary.recommendation,
            "A/B test concluded"
        );

        Ok(summary)
    }

    /// List all tests (optionally filtered by status).
    pub async fn list_tests(&self, status: Option<TestStatus>) -> Vec<ABTest> {
        let tests = self.tests.read().await;
        tests
            .values()
            .filter(|t| status.is_none() || Some(t.status) == status)
            .cloned()
            .collect()
    }

    /// Compute a recommendation based on metrics comparison.
    fn compute_recommendation(a: &EvaluationMetrics, b: &EvaluationMetrics) -> (String, String) {
        // Need a minimum sample size for a meaningful comparison.
        const MIN_QUERIES: u64 = 30;

        if a.query_count < MIN_QUERIES || b.query_count < MIN_QUERIES {
            return (
                "inconclusive".to_string(),
                format!(
                    "Insufficient data: variant A has {} queries, variant B has {} (need at least {MIN_QUERIES} each)",
                    a.query_count, b.query_count
                ),
            );
        }

        // Compare on nDCG as the primary metric (most comprehensive IR metric).
        let ndcg_diff = b.ndcg - a.ndcg;
        let mrr_diff = b.mrr - a.mrr;

        // Require at least 5% relative improvement to recommend a change.
        let relative_threshold = 0.05;
        let a_ndcg_safe = if a.ndcg == 0.0 { 0.001 } else { a.ndcg };
        let relative_ndcg_change = ndcg_diff / a_ndcg_safe;

        if relative_ndcg_change > relative_threshold {
            (
                "b".to_string(),
                format!(
                    "Variant B shows {:.1}% improvement in nDCG ({:.4} vs {:.4}), MRR diff: {:.4}",
                    relative_ndcg_change * 100.0,
                    b.ndcg,
                    a.ndcg,
                    mrr_diff
                ),
            )
        } else if relative_ndcg_change < -relative_threshold {
            (
                "a".to_string(),
                format!(
                    "Variant A performs better: {:.1}% higher nDCG ({:.4} vs {:.4})",
                    (-relative_ndcg_change) * 100.0,
                    a.ndcg,
                    b.ndcg
                ),
            )
        } else {
            (
                "inconclusive".to_string(),
                format!(
                    "No significant difference: nDCG change is {:.1}% (A={:.4}, B={:.4})",
                    relative_ndcg_change * 100.0,
                    a.ndcg,
                    b.ndcg
                ),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Metric computation helpers
// ---------------------------------------------------------------------------

/// Compute Mean Reciprocal Rank for a single query.
///
/// `relevant_positions` contains the 1-based positions of relevant results.
pub fn compute_mrr(relevant_positions: &[usize]) -> f64 {
    relevant_positions
        .iter()
        .map(|&pos| 1.0 / pos as f64)
        .fold(0.0_f64, f64::max)
}

/// Compute Precision@K for a single query.
///
/// `relevant_in_topk` is the number of relevant results in the top K.
pub fn compute_precision_at_k(relevant_in_topk: usize, k: usize) -> f64 {
    if k == 0 {
        return 0.0;
    }
    relevant_in_topk as f64 / k as f64
}

/// Compute Recall@K for a single query.
///
/// `relevant_in_topk` is the number of relevant results in the top K.
/// `total_relevant` is the total number of relevant results.
pub fn compute_recall_at_k(relevant_in_topk: usize, total_relevant: usize) -> f64 {
    if total_relevant == 0 {
        return 0.0;
    }
    relevant_in_topk as f64 / total_relevant as f64
}

/// Compute nDCG (Normalized Discounted Cumulative Gain) for a single query.
///
/// `relevance_scores` are the relevance values at each position (0-based),
/// ordered by the system's ranking.
pub fn compute_ndcg(relevance_scores: &[f64], ideal_scores: &[f64]) -> f64 {
    let dcg = compute_dcg(relevance_scores);
    let idcg = compute_dcg(ideal_scores);

    if idcg == 0.0 {
        return 0.0;
    }
    dcg / idcg
}

/// Compute Discounted Cumulative Gain.
fn compute_dcg(scores: &[f64]) -> f64 {
    scores
        .iter()
        .enumerate()
        .map(|(i, &rel)| {
            let rank = i as f64 + 2.0; // log2(rank+1), rank is 1-based
            rel / rank.log2()
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mrr_computation() {
        // First relevant result at position 1.
        assert!((compute_mrr(&[1]) - 1.0).abs() < 1e-10);
        // First relevant at position 3.
        assert!((compute_mrr(&[3]) - 1.0 / 3.0).abs() < 1e-10);
        // Multiple relevant: best is position 2.
        assert!((compute_mrr(&[2, 5]) - 0.5).abs() < 1e-10);
        // No relevant results.
        assert_eq!(compute_mrr(&[]), 0.0);
    }

    #[test]
    fn test_precision_at_k() {
        assert!((compute_precision_at_k(3, 5) - 0.6).abs() < 1e-10);
        assert!((compute_precision_at_k(0, 5) - 0.0).abs() < 1e-10);
        assert!((compute_precision_at_k(5, 5) - 1.0).abs() < 1e-10);
        assert_eq!(compute_precision_at_k(3, 0), 0.0);
    }

    #[test]
    fn test_recall_at_k() {
        assert!((compute_recall_at_k(3, 10) - 0.3).abs() < 1e-10);
        assert!((compute_recall_at_k(0, 10) - 0.0).abs() < 1e-10);
        assert_eq!(compute_recall_at_k(5, 0), 0.0);
    }

    #[test]
    fn test_ndcg_perfect_ranking() {
        let ideal = vec![3.0, 2.0, 1.0, 0.0];
        // Perfect ranking: same order as ideal.
        let ndcg = compute_ndcg(&ideal, &ideal);
        assert!((ndcg - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_ndcg_suboptimal_ranking() {
        let system = vec![0.0, 3.0, 2.0, 1.0]; // worst at top
        let ideal = vec![3.0, 2.0, 1.0, 0.0];
        let ndcg = compute_ndcg(&system, &ideal);
        assert!(
            ndcg < 1.0,
            "suboptimal ranking should have nDCG < 1.0, got {}",
            ndcg
        );
        assert!(ndcg > 0.0, "should be non-negative");
    }

    #[test]
    fn test_ndcg_empty() {
        assert_eq!(compute_ndcg(&[], &[]), 0.0);
    }

    #[test]
    fn test_evaluation_metrics_incremental() {
        let mut metrics = EvaluationMetrics::default();
        metrics.record_query(1.0, 0.8, 0.5, 0.9);
        assert_eq!(metrics.query_count, 1);
        assert!((metrics.mrr - 1.0).abs() < 1e-10);

        metrics.record_query(0.5, 0.6, 0.4, 0.7);
        assert_eq!(metrics.query_count, 2);
        assert!((metrics.mrr - 0.75).abs() < 1e-10);
        assert!((metrics.precision_at_k - 0.7).abs() < 1e-10);
    }

    #[tokio::test]
    async fn test_create_and_get_test() {
        let db = Arc::new(
            Database::connect("sqlite::memory:")
                .await
                .expect("in-memory db"),
        );
        let engine = EvaluationEngine::new(db);
        engine.ensure_tables().await.unwrap();

        let test = engine
            .create_test(
                "Test SONA reranking".to_string(),
                VariantConfig {
                    name: "baseline".to_string(),
                    params: HashMap::new(),
                },
                VariantConfig {
                    name: "sona_enabled".to_string(),
                    params: HashMap::from([("sona_weight".to_string(), "0.3".to_string())]),
                },
                0.5,
            )
            .await
            .unwrap();

        assert_eq!(test.status, TestStatus::Running);
        assert_eq!(test.variant_a.name, "baseline");

        let retrieved = engine.get_test(&test.test_id).await.unwrap();
        assert_eq!(retrieved.name, "Test SONA reranking");
    }

    #[tokio::test]
    async fn test_record_and_conclude() {
        let db = Arc::new(
            Database::connect("sqlite::memory:")
                .await
                .expect("in-memory db"),
        );
        let engine = EvaluationEngine::new(db);
        engine.ensure_tables().await.unwrap();

        let test = engine
            .create_test(
                "metric test".to_string(),
                VariantConfig {
                    name: "a".to_string(),
                    params: HashMap::new(),
                },
                VariantConfig {
                    name: "b".to_string(),
                    params: HashMap::new(),
                },
                0.5,
            )
            .await
            .unwrap();

        // Record enough observations for a meaningful conclusion.
        for _ in 0..35 {
            engine
                .record_observation(&test.test_id, "a", 0.5, 0.6, 0.4, 0.5)
                .await
                .unwrap();
            engine
                .record_observation(&test.test_id, "b", 0.7, 0.8, 0.6, 0.7)
                .await
                .unwrap();
        }

        let summary = engine.conclude_test(&test.test_id).await.unwrap();
        assert_eq!(summary.status, TestStatus::Concluded);
        assert_eq!(summary.recommendation, "b");
    }

    #[tokio::test]
    async fn test_conclude_insufficient_data() {
        let db = Arc::new(
            Database::connect("sqlite::memory:")
                .await
                .expect("in-memory db"),
        );
        let engine = EvaluationEngine::new(db);
        engine.ensure_tables().await.unwrap();

        let test = engine
            .create_test(
                "small test".to_string(),
                VariantConfig {
                    name: "a".to_string(),
                    params: HashMap::new(),
                },
                VariantConfig {
                    name: "b".to_string(),
                    params: HashMap::new(),
                },
                0.5,
            )
            .await
            .unwrap();

        // Only 5 observations per variant (below MIN_QUERIES=30).
        for _ in 0..5 {
            engine
                .record_observation(&test.test_id, "a", 0.5, 0.6, 0.4, 0.5)
                .await
                .unwrap();
            engine
                .record_observation(&test.test_id, "b", 0.9, 0.9, 0.9, 0.9)
                .await
                .unwrap();
        }

        let summary = engine.conclude_test(&test.test_id).await.unwrap();
        assert_eq!(summary.recommendation, "inconclusive");
    }

    #[tokio::test]
    async fn test_list_tests_filter() {
        let db = Arc::new(
            Database::connect("sqlite::memory:")
                .await
                .expect("in-memory db"),
        );
        let engine = EvaluationEngine::new(db);
        engine.ensure_tables().await.unwrap();

        let vc = || VariantConfig {
            name: "v".to_string(),
            params: HashMap::new(),
        };

        let t1 = engine
            .create_test("t1".to_string(), vc(), vc(), 0.5)
            .await
            .unwrap();
        let _t2 = engine
            .create_test("t2".to_string(), vc(), vc(), 0.5)
            .await
            .unwrap();

        // Conclude t1.
        engine.conclude_test(&t1.test_id).await.unwrap();

        let running = engine.list_tests(Some(TestStatus::Running)).await;
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].name, "t2");

        let all = engine.list_tests(None).await;
        assert_eq!(all.len(), 2);
    }
}
