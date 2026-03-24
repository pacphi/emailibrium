//! Vector-based email classification (S1-06: ADR-004).
//!
//! `VectorCategorizer` classifies emails by comparing their embedding
//! against learned category centroids using cosine similarity.
//! Centroids are updated via exponential moving average when user
//! feedback is received.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio::sync::RwLock;

use tracing::{debug, warn};

use super::embedding::EmbeddingPipeline;
use super::error::VectorError;
use super::generative::{GenerativeModel, RuleBasedClassifier};
use super::store::VectorStoreBackend;
use super::types::*;

/// Compute the cosine similarity between two vectors.
///
/// Returns a value in `[-1.0, 1.0]`. Identical unit vectors yield `1.0`,
/// orthogonal vectors yield `0.0`, and opposite vectors yield `-1.0`.
///
/// Returns `0.0` if either vector has zero magnitude or the lengths differ.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;

    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let magnitude = norm_a.sqrt() * norm_b.sqrt();
    if magnitude == 0.0 {
        return 0.0;
    }

    (dot / magnitude) as f32
}

/// Classifies emails using centroid-based vector comparison (ADR-004).
///
/// Each `EmailCategory` has an associated centroid vector that represents
/// the "average" direction of that category in embedding space. New emails
/// are classified by finding the centroid with the highest cosine similarity
/// to the email's embedding.
pub struct VectorCategorizer {
    #[allow(dead_code)]
    store: Arc<dyn VectorStoreBackend>,
    embedding: Arc<EmbeddingPipeline>,
    centroids: RwLock<HashMap<EmailCategory, CategoryCentroid>>,
    confidence_threshold: f32,
    max_centroid_shift: f32,
    min_feedback_events: u32,
    feedback_count: AtomicU32,
}

impl VectorCategorizer {
    /// Create a new categorizer with the given confidence threshold.
    ///
    /// Uses default values for `max_centroid_shift` (0.1) and
    /// `min_feedback_events` (10).
    pub fn new(
        store: Arc<dyn VectorStoreBackend>,
        embedding: Arc<EmbeddingPipeline>,
        confidence_threshold: f32,
    ) -> Self {
        Self {
            store,
            embedding,
            centroids: RwLock::new(HashMap::new()),
            confidence_threshold,
            max_centroid_shift: 0.1,
            min_feedback_events: 10,
            feedback_count: AtomicU32::new(0),
        }
    }

    /// Create a new categorizer with full configuration options.
    pub fn with_config(
        store: Arc<dyn VectorStoreBackend>,
        embedding: Arc<EmbeddingPipeline>,
        confidence_threshold: f32,
        max_centroid_shift: f32,
        min_feedback_events: u32,
    ) -> Self {
        Self {
            store,
            embedding,
            centroids: RwLock::new(HashMap::new()),
            confidence_threshold,
            max_centroid_shift,
            min_feedback_events,
            feedback_count: AtomicU32::new(0),
        }
    }

    /// Classify an email by embedding its text and comparing against centroids.
    ///
    /// Returns `CategoryResult` with:
    /// - `method: "vector_centroid"` when confidence >= threshold
    /// - `method: "below_threshold"` when confidence < threshold or no centroids
    pub async fn categorize(&self, email_text: &str) -> Result<CategoryResult, VectorError> {
        let query_vector = self
            .embedding
            .embed(email_text)
            .await
            .map_err(|e| VectorError::CategorizationFailed(format!("Embedding failed: {e}")))?;

        let centroids = self.centroids.read().await;

        if centroids.is_empty() {
            return Ok(CategoryResult {
                category: EmailCategory::Uncategorized,
                confidence: 0.0,
                method: "below_threshold".to_string(),
            });
        }

        let mut best_category = EmailCategory::Uncategorized;
        let mut best_score: f32 = f32::NEG_INFINITY;

        for (category, centroid) in centroids.iter() {
            let score = cosine_similarity(&query_vector, &centroid.vector);
            if score > best_score {
                best_score = score;
                best_category = *category;
            }
        }

        if best_score >= self.confidence_threshold {
            Ok(CategoryResult {
                category: best_category,
                confidence: best_score,
                method: "vector_centroid".to_string(),
            })
        } else {
            Ok(CategoryResult {
                category: EmailCategory::Uncategorized,
                confidence: best_score,
                method: "below_threshold".to_string(),
            })
        }
    }

    /// Classify an email with tiered fallback (ADR-012).
    ///
    /// 1. Try vector centroid classification (existing `categorize`)
    /// 2. If below threshold and a generative model is available, use LLM
    /// 3. If no generative model, try rule-based heuristics
    /// 4. If all fail, return Uncategorized
    pub async fn categorize_with_fallback(
        &self,
        email_text: &str,
        from_addr: &str,
        generative: Option<&dyn GenerativeModel>,
    ) -> Result<CategoryResult, VectorError> {
        // Step 1: Try vector centroid classification.
        let result = self.categorize(email_text).await?;
        if result.method != "below_threshold" {
            return Ok(result);
        }

        debug!(
            confidence = result.confidence,
            "Below threshold, trying fallback classification"
        );

        // Step 2: If generative model available, try LLM classification.
        if let Some(gen) = generative {
            let categories = &[
                "Work",
                "Personal",
                "Finance",
                "Shopping",
                "Social",
                "Newsletter",
                "Marketing",
                "Notification",
                "Alerts",
                "Promotions",
            ];
            match gen.classify(email_text, categories).await {
                Ok(cat_name) => {
                    if let Some(category) = parse_email_category(&cat_name) {
                        debug!(category = %cat_name, "LLM fallback classified email");
                        return Ok(CategoryResult {
                            category,
                            confidence: result.confidence,
                            method: "llm_fallback".to_string(),
                        });
                    }
                    warn!(category = %cat_name, "LLM returned unparseable category");
                }
                Err(e) => {
                    warn!(error = %e, "LLM classification failed, trying rules");
                }
            }
        }

        // Step 3: Rule-based fallback.
        if let Some(cat_name) = RuleBasedClassifier::classify_by_rules(email_text, from_addr) {
            if let Some(category) = parse_email_category(&cat_name) {
                debug!(category = %cat_name, "Rule-based fallback classified email");
                return Ok(CategoryResult {
                    category,
                    confidence: result.confidence,
                    method: "rule_based".to_string(),
                });
            }
        }

        // Step 4: Nothing worked.
        Ok(CategoryResult {
            category: EmailCategory::Uncategorized,
            confidence: result.confidence,
            method: "uncategorized".to_string(),
        })
    }

    /// Update a category centroid using exponential moving average.
    ///
    /// - Positive `weight`: standard EMA with alpha = weight.abs() * 0.05
    /// - Negative `weight`: weaker update with beta = weight.abs() * 0.02
    /// - The shift is bounded by `max_centroid_shift`
    /// - No-op if `feedback_count` < `min_feedback_events` (cold start protection)
    pub async fn update_centroid(&self, category: EmailCategory, embedding: &[f32], weight: f32) {
        // Increment feedback counter regardless of whether the update is applied.
        self.feedback_count.fetch_add(1, Ordering::Relaxed);

        // Cold start protection: do not update centroids until we have
        // received enough feedback events.
        if self.feedback_count.load(Ordering::Relaxed) < self.min_feedback_events {
            return;
        }

        let mut centroids = self.centroids.write().await;

        let centroid = match centroids.get_mut(&category) {
            Some(c) => c,
            None => return, // No centroid seeded for this category yet.
        };

        if centroid.vector.len() != embedding.len() {
            return; // Dimension mismatch -- skip silently.
        }

        // Compute learning rate.
        let alpha = if weight >= 0.0 {
            weight.abs() * 0.05
        } else {
            weight.abs() * 0.02
        };
        let alpha = alpha.clamp(0.0, 1.0);

        // Compute the candidate new centroid:
        // mu_new = (1 - alpha) * mu_old + alpha * embedding
        let new_vector: Vec<f32> = centroid
            .vector
            .iter()
            .zip(embedding.iter())
            .map(|(&old, &new)| (1.0 - alpha) * old + alpha * new)
            .collect();

        // Compute the shift magnitude.
        let shift_sq: f32 = centroid
            .vector
            .iter()
            .zip(new_vector.iter())
            .map(|(&old, &new)| {
                let d = new - old;
                d * d
            })
            .sum();
        let shift_magnitude = shift_sq.sqrt();

        // Bound the shift: if it exceeds max_centroid_shift, scale down.
        if shift_magnitude > self.max_centroid_shift && shift_magnitude > 0.0 {
            let scale = self.max_centroid_shift / shift_magnitude;
            for (old, new) in centroid.vector.iter_mut().zip(new_vector.iter()) {
                *old = *old + (*new - *old) * scale;
            }
        } else {
            centroid.vector = new_vector;
        }

        centroid.email_count += 1;
        centroid.last_updated = chrono::Utc::now();
    }

    /// Seed a category centroid with an initial vector.
    ///
    /// Used during system setup or when bootstrapping from labeled data.
    pub async fn seed_centroid(&self, category: EmailCategory, vector: Vec<f32>) {
        let mut centroids = self.centroids.write().await;
        centroids.insert(
            category,
            CategoryCentroid {
                category,
                vector,
                email_count: 0,
                last_updated: chrono::Utc::now(),
            },
        );
    }

    /// Return a snapshot of all current centroids.
    pub async fn get_centroids(&self) -> HashMap<EmailCategory, CategoryCentroid> {
        self.centroids.read().await.clone()
    }

    /// Return the current feedback event count.
    pub fn feedback_count(&self) -> u32 {
        self.feedback_count.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a category name string into an `EmailCategory`.
fn parse_email_category(name: &str) -> Option<EmailCategory> {
    match name.to_lowercase().as_str() {
        "work" => Some(EmailCategory::Work),
        "personal" => Some(EmailCategory::Personal),
        "finance" => Some(EmailCategory::Finance),
        "shopping" => Some(EmailCategory::Shopping),
        "social" => Some(EmailCategory::Social),
        "newsletter" => Some(EmailCategory::Newsletter),
        "marketing" => Some(EmailCategory::Marketing),
        "notification" => Some(EmailCategory::Notification),
        "alerts" => Some(EmailCategory::Alerts),
        "promotions" => Some(EmailCategory::Promotions),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectors::config::EmbeddingConfig;
    use crate::vectors::embedding::EmbeddingPipeline;
    use crate::vectors::store::InMemoryVectorStore;

    /// Build a categorizer backed by a mock embedding pipeline and in-memory store.
    fn make_categorizer(
        confidence_threshold: f32,
        max_centroid_shift: f32,
        min_feedback_events: u32,
    ) -> VectorCategorizer {
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let config = EmbeddingConfig {
            provider: "mock".to_string(),
            ..EmbeddingConfig::default()
        };
        let embedding = Arc::new(EmbeddingPipeline::new(&config).unwrap());
        VectorCategorizer::with_config(
            store,
            embedding,
            confidence_threshold,
            max_centroid_shift,
            min_feedback_events,
        )
    }

    // -- cosine_similarity tests -------------------------------------------

    #[test]
    fn test_cosine_similarity_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!(
            (sim - 1.0).abs() < 1e-6,
            "Identical vectors should have similarity 1.0, got {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_orthogonal_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(
            sim.abs() < 1e-6,
            "Orthogonal vectors should have similarity 0.0, got {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_opposite_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(
            (sim - (-1.0)).abs() < 1e-6,
            "Opposite vectors should have similarity -1.0, got {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_empty_vectors() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn test_cosine_similarity_mismatched_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    // -- categorize tests --------------------------------------------------

    #[tokio::test]
    async fn test_categorize_with_matching_centroid() {
        let cat = make_categorizer(0.0, 0.1, 0);

        // Embed a known text and use it as the centroid for Work.
        let work_text = "quarterly budget review meeting with the team regarding project deadlines";
        let embedding = cat.embedding.embed(work_text).await.unwrap();
        cat.seed_centroid(EmailCategory::Work, embedding).await;

        // Categorize the same text -- should match Work with high confidence.
        let result = cat.categorize(work_text).await.unwrap();
        assert_eq!(result.category, EmailCategory::Work);
        assert!(
            result.confidence > 0.9,
            "Expected high confidence, got {}",
            result.confidence
        );
        assert_eq!(result.method, "vector_centroid");
    }

    #[tokio::test]
    async fn test_categorize_below_threshold() {
        let cat = make_categorizer(0.99, 0.1, 0);

        // Seed a centroid with a fixed vector.
        cat.seed_centroid(EmailCategory::Finance, vec![1.0, 0.0, 0.0])
            .await;

        // The mock embedding for arbitrary text is unlikely to have
        // cosine similarity >= 0.99 with [1, 0, 0], so it should
        // fall below threshold.
        let result = cat
            .categorize(
                "random text about completely unrelated topics that should not match finance",
            )
            .await
            .unwrap();

        assert_eq!(result.category, EmailCategory::Uncategorized);
        assert_eq!(result.method, "below_threshold");
    }

    #[tokio::test]
    async fn test_categorize_no_centroids() {
        let cat = make_categorizer(0.7, 0.1, 0);
        let result = cat.categorize("anything at all").await.unwrap();
        assert_eq!(result.category, EmailCategory::Uncategorized);
        assert_eq!(result.confidence, 0.0);
        assert_eq!(result.method, "below_threshold");
    }

    // -- seed_centroid tests -----------------------------------------------

    #[tokio::test]
    async fn test_seed_centroid() {
        let cat = make_categorizer(0.7, 0.1, 0);

        cat.seed_centroid(EmailCategory::Shopping, vec![0.5, 0.5, 0.0])
            .await;
        cat.seed_centroid(EmailCategory::Social, vec![0.0, 0.5, 0.5])
            .await;

        let centroids = cat.get_centroids().await;
        assert_eq!(centroids.len(), 2);
        assert!(centroids.contains_key(&EmailCategory::Shopping));
        assert!(centroids.contains_key(&EmailCategory::Social));
        assert_eq!(
            centroids[&EmailCategory::Shopping].vector,
            vec![0.5, 0.5, 0.0]
        );
        assert_eq!(centroids[&EmailCategory::Shopping].email_count, 0);
    }

    // -- update_centroid tests ---------------------------------------------

    #[tokio::test]
    async fn test_update_centroid_ema() {
        // No cold-start barrier (min_feedback_events = 0).
        let cat = make_categorizer(0.7, 10.0, 0);

        // Seed with [1.0, 0.0, 0.0].
        cat.seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;

        // Update towards [0.0, 1.0, 0.0] with positive weight = 1.0.
        // alpha = 1.0 * 0.05 = 0.05
        // new = (1 - 0.05) * [1, 0, 0] + 0.05 * [0, 1, 0] = [0.95, 0.05, 0.0]
        cat.update_centroid(EmailCategory::Work, &[0.0, 1.0, 0.0], 1.0)
            .await;

        let centroids = cat.get_centroids().await;
        let centroid = &centroids[&EmailCategory::Work];
        assert!(
            (centroid.vector[0] - 0.95).abs() < 1e-5,
            "Expected ~0.95, got {}",
            centroid.vector[0]
        );
        assert!(
            (centroid.vector[1] - 0.05).abs() < 1e-5,
            "Expected ~0.05, got {}",
            centroid.vector[1]
        );
        assert!(
            centroid.vector[2].abs() < 1e-5,
            "Expected ~0.0, got {}",
            centroid.vector[2]
        );
        assert_eq!(centroid.email_count, 1);
    }

    #[tokio::test]
    async fn test_update_centroid_negative_weight() {
        let cat = make_categorizer(0.7, 10.0, 0);
        cat.seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;

        // Negative weight uses beta = |weight| * 0.02 instead of 0.05.
        // weight = -1.0 => beta = 0.02
        // new = (1 - 0.02) * [1, 0, 0] + 0.02 * [0, 1, 0] = [0.98, 0.02, 0.0]
        cat.update_centroid(EmailCategory::Work, &[0.0, 1.0, 0.0], -1.0)
            .await;

        let centroids = cat.get_centroids().await;
        let centroid = &centroids[&EmailCategory::Work];
        assert!(
            (centroid.vector[0] - 0.98).abs() < 1e-5,
            "Expected ~0.98, got {}",
            centroid.vector[0]
        );
        assert!(
            (centroid.vector[1] - 0.02).abs() < 1e-5,
            "Expected ~0.02, got {}",
            centroid.vector[1]
        );
    }

    #[tokio::test]
    async fn test_update_centroid_bounded_shift() {
        // Very small max shift to trigger bounding.
        let cat = make_categorizer(0.7, 0.01, 0);

        cat.seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;

        // Large weight (alpha = 20.0 * 0.05 = 1.0, clamped) would move centroid
        // all the way to [0, 1, 0]. Unbounded shift = sqrt(2) ~ 1.414.
        // Since 1.414 > 0.01, the shift is scaled down.
        cat.update_centroid(EmailCategory::Work, &[0.0, 1.0, 0.0], 20.0)
            .await;

        let centroids = cat.get_centroids().await;
        let centroid = &centroids[&EmailCategory::Work];

        // The shift should be bounded to max_centroid_shift.
        let shift_sq: f32 = [1.0_f32, 0.0, 0.0]
            .iter()
            .zip(centroid.vector.iter())
            .map(|(old, new)| (new - old).powi(2))
            .sum();
        let shift = shift_sq.sqrt();

        assert!(
            shift <= 0.01 + 1e-5,
            "Shift magnitude {shift} should be <= 0.01"
        );
    }

    #[tokio::test]
    async fn test_update_centroid_cold_start_protection() {
        // Require 5 feedback events before updates activate.
        let cat = make_categorizer(0.7, 10.0, 5);

        cat.seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;

        // Send 4 updates -- all should be ignored due to cold start.
        for _ in 0..4 {
            cat.update_centroid(EmailCategory::Work, &[0.0, 1.0, 0.0], 1.0)
                .await;
        }

        let centroids = cat.get_centroids().await;
        let centroid = &centroids[&EmailCategory::Work];
        assert_eq!(
            centroid.vector,
            vec![1.0, 0.0, 0.0],
            "Centroid should not have moved (cold start)"
        );
        assert_eq!(centroid.email_count, 0);
        assert_eq!(cat.feedback_count(), 4);

        // The 5th update should activate and actually modify the centroid.
        cat.update_centroid(EmailCategory::Work, &[0.0, 1.0, 0.0], 1.0)
            .await;

        let centroids = cat.get_centroids().await;
        let centroid = &centroids[&EmailCategory::Work];
        assert_ne!(
            centroid.vector,
            vec![1.0, 0.0, 0.0],
            "Centroid should have moved after min_feedback_events reached"
        );
        assert_eq!(centroid.email_count, 1);
        assert_eq!(cat.feedback_count(), 5);
    }

    #[tokio::test]
    async fn test_update_centroid_no_centroid_seeded() {
        let cat = make_categorizer(0.7, 10.0, 0);

        // Updating a category with no seeded centroid is a no-op.
        cat.update_centroid(EmailCategory::Work, &[1.0, 0.0, 0.0], 1.0)
            .await;

        let centroids = cat.get_centroids().await;
        assert!(centroids.is_empty());
    }

    #[tokio::test]
    async fn test_categorize_selects_best_matching_centroid() {
        let cat = make_categorizer(0.0, 0.1, 0);

        // Use the embedding pipeline to get deterministic vectors for known texts.
        let work_text = "quarterly budget review meeting with the team regarding project deadlines";
        let personal_text = "hey just wanted to see how you are doing my friend";

        let work_vec = cat.embedding.embed(work_text).await.unwrap();
        let personal_vec = cat.embedding.embed(personal_text).await.unwrap();

        cat.seed_centroid(EmailCategory::Work, work_vec).await;
        cat.seed_centroid(EmailCategory::Personal, personal_vec)
            .await;

        // Categorize the work text -- should match Work.
        let result = cat.categorize(work_text).await.unwrap();
        assert_eq!(result.category, EmailCategory::Work);

        // Categorize the personal text -- should match Personal.
        let result = cat.categorize(personal_text).await.unwrap();
        assert_eq!(result.category, EmailCategory::Personal);
    }

    #[tokio::test]
    async fn test_get_centroids_returns_clone() {
        let cat = make_categorizer(0.7, 0.1, 0);
        cat.seed_centroid(EmailCategory::Work, vec![1.0, 0.0]).await;

        let snapshot = cat.get_centroids().await;
        assert_eq!(snapshot.len(), 1);

        // Adding more centroids does not affect the earlier snapshot.
        drop(snapshot);
        cat.seed_centroid(EmailCategory::Personal, vec![0.0, 1.0])
            .await;

        let snapshot2 = cat.get_centroids().await;
        assert_eq!(snapshot2.len(), 2);
    }

    // -- categorize_with_fallback tests ------------------------------------

    #[tokio::test]
    async fn test_categorizer_with_rule_fallback() {
        // High threshold so vector classification always fails.
        let cat = make_categorizer(0.99, 0.1, 0);

        // No generative model, rule-based should kick in for a GitHub email.
        let result = cat
            .categorize_with_fallback(
                "New pull request opened on your repo",
                "noreply@github.com",
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.category, EmailCategory::Notification);
        assert_eq!(result.method, "rule_based");
    }

    #[tokio::test]
    async fn test_categorizer_fallback_to_uncategorized() {
        let cat = make_categorizer(0.99, 0.1, 0);

        // Text and address that match no rules.
        let result = cat
            .categorize_with_fallback(
                "Hello, how are you doing today?",
                "friend@personal.com",
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.category, EmailCategory::Uncategorized);
        assert_eq!(result.method, "uncategorized");
    }

    #[tokio::test]
    async fn test_categorizer_fallback_finance_keyword() {
        let cat = make_categorizer(0.99, 0.1, 0);

        let result = cat
            .categorize_with_fallback(
                "Your invoice #12345 is attached",
                "billing@randomco.com",
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.category, EmailCategory::Finance);
        assert_eq!(result.method, "rule_based");
    }

    #[test]
    fn test_parse_email_category() {
        assert_eq!(parse_email_category("Work"), Some(EmailCategory::Work));
        assert_eq!(parse_email_category("finance"), Some(EmailCategory::Finance));
        assert_eq!(parse_email_category("SHOPPING"), Some(EmailCategory::Shopping));
        assert_eq!(parse_email_category("unknown"), None);
    }
}
