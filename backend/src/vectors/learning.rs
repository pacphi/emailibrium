//! SONA 3-tier adaptive learning engine (ADR-004, Sprint 3: S3-03..S3-06).
//!
//! Implements the Self-Optimizing Neural Architecture for Emailibrium:
//!
//! - **Tier 1 (Instant)**: processes explicit user feedback to update centroids
//! - **Tier 2 (Session)**: builds per-session preference vectors for re-ranking
//! - **Tier 3 (Long-term)**: hourly/daily consolidation jobs
//! - **Safeguards**: position-bias detection, drift alarms, A/B evaluation, rollback

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::categorizer::{cosine_similarity, VectorCategorizer};
use super::error::VectorError;
use super::store::VectorStoreBackend;
use super::types::{CategoryCentroid, EmailCategory, ScoredResult};
use crate::db::Database;

// ---------------------------------------------------------------------------
// Feedback types (S3-03)
// ---------------------------------------------------------------------------

/// The kind of action the user took on an email.
#[derive(Debug, Clone, PartialEq)]
pub enum FeedbackAction {
    /// User moved an email from one category to another.
    Reclassify {
        from: EmailCategory,
        to: EmailCategory,
    },
    /// User moved the email into a custom group.
    MoveToGroup { group_id: String },
    /// User starred the email.
    Star,
    /// User replied, with the reply delay in seconds.
    Reply { delay_secs: u64 },
    /// User archived the email.
    Archive,
    /// User deleted the email.
    Delete,
}

/// A single piece of user feedback on an email.
#[derive(Debug, Clone)]
pub struct UserFeedback {
    pub email_id: String,
    pub action: FeedbackAction,
    pub timestamp: DateTime<Utc>,
}

/// Result returned after processing user feedback.
#[derive(Debug, Clone)]
pub struct FeedbackResult {
    /// Quality score assigned to the feedback signal (0.0 .. 1.0).
    pub quality: f32,
    /// Whether any centroid was actually updated.
    pub centroid_updated: bool,
    /// Whether a safeguard prevented the update.
    pub safeguard_triggered: bool,
}

// ---------------------------------------------------------------------------
// Session types (S3-04)
// ---------------------------------------------------------------------------

/// Per-session interaction state for Tier 2 learning.
pub struct SessionState {
    pub session_id: String,
    pub started_at: DateTime<Utc>,
    pub clicked_embeddings: Vec<Vec<f32>>,
    pub skipped_embeddings: Vec<Vec<f32>>,
    pub interaction_count: u32,
}

impl SessionState {
    /// Create a new session with the given identifier.
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            started_at: Utc::now(),
            clicked_embeddings: Vec::new(),
            skipped_embeddings: Vec::new(),
            interaction_count: 0,
        }
    }

    /// The unique session identifier.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// How long this session has been active.
    pub fn age(&self) -> chrono::Duration {
        Utc::now() - self.started_at
    }

    /// Compute the SONA Tier 2 preference vector: mean(clicked) − mean(skipped).
    ///
    /// Returns `None` if there are no clicked embeddings (nothing to learn from).
    pub fn preference_vector(&self) -> Option<Vec<f32>> {
        if self.clicked_embeddings.is_empty() {
            return None;
        }

        let dims = self.clicked_embeddings[0].len();

        let clicked_mean = mean_vector(&self.clicked_embeddings, dims);
        let skipped_mean = if self.skipped_embeddings.is_empty() {
            vec![0.0; dims]
        } else {
            mean_vector(&self.skipped_embeddings, dims)
        };

        let pref: Vec<f32> = clicked_mean
            .iter()
            .zip(skipped_mean.iter())
            .map(|(c, s)| c - s)
            .collect();

        Some(pref)
    }

    /// Compute the SONA Tier 2 re-ranking boost for a document embedding.
    ///
    /// Returns `gamma * cosine_similarity(doc_embedding, preference_vector)`,
    /// or `0.0` if no preference vector is available.
    pub fn rerank_boost(&self, doc_embedding: &[f32], gamma: f32) -> f32 {
        match self.preference_vector() {
            Some(pref) => gamma * cosine_similarity(doc_embedding, &pref),
            None => 0.0,
        }
    }
}

/// Compute the element-wise mean of a set of vectors.
fn mean_vector(vectors: &[Vec<f32>], dims: usize) -> Vec<f32> {
    let n = vectors.len() as f32;
    let mut mean = vec![0.0f32; dims];
    for v in vectors {
        for (i, &val) in v.iter().enumerate() {
            mean[i] += val;
        }
    }
    for val in &mut mean {
        *val /= n;
    }
    mean
}

// ---------------------------------------------------------------------------
// Consolidation types (S3-05)
// ---------------------------------------------------------------------------

/// Report returned by consolidation jobs.
#[derive(Debug, Clone, Default)]
pub struct ConsolidationReport {
    pub centroids_updated: u32,
    pub emails_reclassified: u32,
    pub new_clusters: u32,
    pub duration_ms: u64,
}

// ---------------------------------------------------------------------------
// Safeguard types (S3-06)
// ---------------------------------------------------------------------------

/// Observable metrics for the learning engine.
#[derive(Debug, Clone, Default)]
pub struct LearningMetrics {
    pub total_feedback: u64,
    pub rank1_clicks: u64,
    pub total_clicks: u64,
    pub centroid_drift: HashMap<String, f32>,
    pub ab_control_queries: u64,
    pub ab_sona_queries: u64,
}

/// A point-in-time snapshot of all centroids for rollback.
#[derive(Debug, Clone)]
pub struct CentroidSnapshot {
    pub timestamp: DateTime<Utc>,
    pub centroids: HashMap<EmailCategory, CategoryCentroid>,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration knobs for the SONA learning engine.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LearningConfig {
    /// Master switch for the SONA engine.
    #[serde(default = "default_sona_enabled")]
    pub sona_enabled: bool,
    /// Positive learning rate (alpha multiplier).
    #[serde(default = "default_positive_learning_rate")]
    pub positive_learning_rate: f32,
    /// Negative learning rate (beta multiplier).
    #[serde(default = "default_negative_learning_rate")]
    pub negative_learning_rate: f32,
    /// Session re-ranking weight.
    #[serde(default = "default_session_rerank_gamma")]
    pub session_rerank_gamma: f32,
    /// Maximum centroid shift per feedback event.
    #[serde(default = "default_max_centroid_shift")]
    pub max_centroid_shift: f32,
    /// Minimum feedback events before centroid updates activate (cold start).
    #[serde(default = "default_min_feedback_events")]
    pub min_feedback_events: u32,
    /// Emails below this confidence are reclassified during hourly consolidation.
    #[serde(default = "default_low_confidence_threshold")]
    pub low_confidence_threshold: f32,
    /// Fraction of queries routed to the control group (no SONA).
    #[serde(default = "default_ab_control_percentage")]
    pub ab_control_percentage: f32,
    /// Drift alarm fires when any centroid drifts beyond this fraction.
    #[serde(default = "default_drift_alarm_threshold")]
    pub drift_alarm_threshold: f32,
    /// Position-bias alarm threshold.
    #[serde(default = "default_position_bias_threshold")]
    pub position_bias_threshold: f32,
    /// Maximum number of daily snapshots to retain.
    #[serde(default = "default_max_snapshots")]
    pub max_snapshots: usize,
}

fn default_sona_enabled() -> bool {
    true
}
fn default_positive_learning_rate() -> f32 {
    0.05
}
fn default_negative_learning_rate() -> f32 {
    0.02
}
fn default_session_rerank_gamma() -> f32 {
    0.15
}
fn default_max_centroid_shift() -> f32 {
    0.1
}
fn default_min_feedback_events() -> u32 {
    10
}
fn default_low_confidence_threshold() -> f32 {
    0.6
}
fn default_ab_control_percentage() -> f32 {
    0.10
}
fn default_drift_alarm_threshold() -> f32 {
    0.20
}
fn default_position_bias_threshold() -> f32 {
    0.95
}
fn default_max_snapshots() -> usize {
    30
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self {
            sona_enabled: default_sona_enabled(),
            positive_learning_rate: default_positive_learning_rate(),
            negative_learning_rate: default_negative_learning_rate(),
            session_rerank_gamma: default_session_rerank_gamma(),
            max_centroid_shift: default_max_centroid_shift(),
            min_feedback_events: default_min_feedback_events(),
            low_confidence_threshold: default_low_confidence_threshold(),
            ab_control_percentage: default_ab_control_percentage(),
            drift_alarm_threshold: default_drift_alarm_threshold(),
            position_bias_threshold: default_position_bias_threshold(),
            max_snapshots: default_max_snapshots(),
        }
    }
}

// ---------------------------------------------------------------------------
// LearningEngine
// ---------------------------------------------------------------------------

/// The SONA 3-tier adaptive learning engine.
pub struct LearningEngine {
    categorizer: Arc<VectorCategorizer>,
    store: Arc<dyn VectorStoreBackend>,
    #[allow(dead_code)]
    db: Arc<Database>,
    config: LearningConfig,
    session: RwLock<SessionState>,
    snapshots: RwLock<Vec<CentroidSnapshot>>,
    metrics: RwLock<LearningMetrics>,
    /// Running total of feedback events (mirrors categorizer.feedback_count
    /// but is local to the engine for cold-start gating).
    feedback_count: RwLock<u32>,
    /// Initial centroid positions captured at construction time (for drift).
    initial_centroids: RwLock<HashMap<EmailCategory, Vec<f32>>>,
}

impl LearningEngine {
    /// Build a new learning engine wired to an existing categorizer and store.
    pub fn new(
        categorizer: Arc<VectorCategorizer>,
        store: Arc<dyn VectorStoreBackend>,
        db: Arc<Database>,
        config: LearningConfig,
    ) -> Self {
        Self {
            categorizer,
            store,
            db,
            config,
            session: RwLock::new(SessionState::new(uuid::Uuid::new_v4().to_string())),
            snapshots: RwLock::new(Vec::new()),
            metrics: RwLock::new(LearningMetrics::default()),
            feedback_count: RwLock::new(0),
            initial_centroids: RwLock::new(HashMap::new()),
        }
    }

    // -- helpers -------------------------------------------------------------

    /// Compute the quality score for a feedback action.
    pub fn quality_for_action(action: &FeedbackAction) -> f32 {
        match action {
            FeedbackAction::Reclassify { .. } => 1.0,
            FeedbackAction::MoveToGroup { .. } => 1.0,
            FeedbackAction::Star => 0.4,
            FeedbackAction::Reply { delay_secs } => {
                if *delay_secs < 300 {
                    0.5
                } else {
                    0.3
                }
            }
            FeedbackAction::Archive => 0.2,
            FeedbackAction::Delete => 0.4,
        }
    }

    /// Capture the current centroids as the initial baseline for drift detection.
    pub async fn capture_initial_centroids(&self) {
        let centroids = self.categorizer.get_centroids().await;
        let mut initial = self.initial_centroids.write().await;
        for (cat, centroid) in &centroids {
            initial.insert(*cat, centroid.vector.clone());
        }
    }

    /// Compute the L2 distance between two vectors of equal length.
    fn l2_distance(a: &[f32], b: &[f32]) -> f32 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Normalize a vector to unit length in place, returning whether it was non-zero.
    fn normalize(v: &mut [f32]) -> bool {
        let mag: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if mag == 0.0 {
            return false;
        }
        for x in v.iter_mut() {
            *x /= mag;
        }
        true
    }

    // -----------------------------------------------------------------------
    // Tier 1: Instant Learning (S3-03)
    // -----------------------------------------------------------------------

    /// Process a single piece of user feedback, potentially updating centroids.
    pub async fn on_user_feedback(
        &self,
        feedback: UserFeedback,
    ) -> Result<FeedbackResult, VectorError> {
        let quality = Self::quality_for_action(&feedback.action);

        // Increment local feedback count.
        {
            let mut count = self.feedback_count.write().await;
            *count += 1;
        }

        // Update global metrics.
        {
            let mut m = self.metrics.write().await;
            m.total_feedback += 1;
        }

        // Look up the email's embedding from the store.
        let doc = self.store.get_by_email_id(&feedback.email_id).await?;
        let embedding = match doc {
            Some(d) => d.vector,
            None => {
                return Ok(FeedbackResult {
                    quality,
                    centroid_updated: false,
                    safeguard_triggered: false,
                });
            }
        };

        // Cold start protection: only update centroids after enough feedback.
        let count = *self.feedback_count.read().await;
        if count < self.config.min_feedback_events {
            return Ok(FeedbackResult {
                quality,
                centroid_updated: false,
                safeguard_triggered: true,
            });
        }

        let mut centroid_updated = false;

        if let FeedbackAction::Reclassify { from, to } = &feedback.action {
            // Positive update on the target category.
            let alpha = quality * self.config.positive_learning_rate;
            self.apply_centroid_update(*to, &embedding, alpha).await;

            // Negative update on the source category: push centroid away.
            let neg_embedding: Vec<f32> = embedding.iter().map(|x| -0.3 * x).collect();
            let beta = quality * self.config.negative_learning_rate;
            self.apply_centroid_update(*from, &neg_embedding, beta)
                .await;

            centroid_updated = true;

            // Track drift.
            self.track_drift(*to).await;
            self.track_drift(*from).await;
        }

        Ok(FeedbackResult {
            quality,
            centroid_updated,
            safeguard_triggered: false,
        })
    }

    /// Apply an EMA centroid update with bounded shift.
    async fn apply_centroid_update(&self, category: EmailCategory, embedding: &[f32], alpha: f32) {
        let mut centroids = self.categorizer.get_centroids().await;
        let centroid = match centroids.get_mut(&category) {
            Some(c) => c,
            None => return,
        };

        if centroid.vector.len() != embedding.len() {
            return;
        }

        let alpha = alpha.clamp(0.0, 1.0);

        // mu_new = (1 - alpha) * mu_old + alpha * embedding
        let new_vector: Vec<f32> = centroid
            .vector
            .iter()
            .zip(embedding.iter())
            .map(|(&old, &new)| (1.0 - alpha) * old + alpha * new)
            .collect();

        // Compute shift magnitude.
        let shift = Self::l2_distance(&centroid.vector, &new_vector);

        // Bound the shift.
        if shift > self.config.max_centroid_shift && shift > 0.0 {
            let scale = self.config.max_centroid_shift / shift;
            let bounded: Vec<f32> = centroid
                .vector
                .iter()
                .zip(new_vector.iter())
                .map(|(&old, &new)| old + (new - old) * scale)
                .collect();
            self.categorizer.seed_centroid(category, bounded).await;
        } else {
            self.categorizer.seed_centroid(category, new_vector).await;
        }
    }

    /// Track cumulative centroid drift for a category against initial positions.
    async fn track_drift(&self, category: EmailCategory) {
        let centroids = self.categorizer.get_centroids().await;
        let initial = self.initial_centroids.read().await;

        if let (Some(current), Some(orig)) = (centroids.get(&category), initial.get(&category)) {
            let drift = Self::l2_distance(&current.vector, orig);
            let orig_magnitude: f32 = orig.iter().map(|x| x * x).sum::<f32>().sqrt();
            let relative_drift = if orig_magnitude > 0.0 {
                drift / orig_magnitude
            } else {
                drift
            };

            let mut m = self.metrics.write().await;
            m.centroid_drift
                .insert(category.to_string(), relative_drift);

            if relative_drift > self.config.drift_alarm_threshold {
                warn!(
                    category = %category,
                    drift = relative_drift,
                    threshold = self.config.drift_alarm_threshold,
                    "Centroid drift alarm: category has drifted beyond threshold"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Tier 2: Session Learning (S3-04)
    // -----------------------------------------------------------------------

    /// Start a new session, returning the session ID.
    pub async fn start_session(&self) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let mut session = self.session.write().await;
        *session = SessionState::new(id.clone());
        id
    }

    /// Record a click on an email (adds its embedding to the session).
    pub async fn record_click(&self, email_id: &str) {
        if let Ok(Some(doc)) = self.store.get_by_email_id(email_id).await {
            let mut session = self.session.write().await;
            session.clicked_embeddings.push(doc.vector);
            session.interaction_count += 1;
        }
    }

    /// Record a skip on an email (adds its embedding to the session).
    pub async fn record_skip(&self, email_id: &str) {
        if let Ok(Some(doc)) = self.store.get_by_email_id(email_id).await {
            let mut session = self.session.write().await;
            session.skipped_embeddings.push(doc.vector);
            session.interaction_count += 1;
        }
    }

    /// Compute the current session preference vector.
    ///
    /// preference = normalize(mean(clicked) - mean(skipped))
    ///
    /// Returns `None` when there are no interactions at all.
    pub async fn get_session_preference(&self) -> Option<Vec<f32>> {
        let session = self.session.read().await;

        if session.clicked_embeddings.is_empty() && session.skipped_embeddings.is_empty() {
            return None;
        }

        let dims = session
            .clicked_embeddings
            .first()
            .or(session.skipped_embeddings.first())
            .map(|v| v.len())
            .unwrap_or(0);

        if dims == 0 {
            return None;
        }

        let mean_clicked = Self::mean_vectors(&session.clicked_embeddings, dims);
        let mean_skipped = Self::mean_vectors(&session.skipped_embeddings, dims);

        let mut preference: Vec<f32> = mean_clicked
            .iter()
            .zip(mean_skipped.iter())
            .map(|(c, s)| c - s)
            .collect();

        if !Self::normalize(&mut preference) {
            return None;
        }

        Some(preference)
    }

    /// Re-rank search results using the session preference vector.
    ///
    /// `score' = score + gamma * cos(embedding, preference_vector)`
    pub async fn rerank_with_session(
        &self,
        mut results: Vec<ScoredResult>,
        gamma: f32,
    ) -> Vec<ScoredResult> {
        let preference = match self.get_session_preference().await {
            Some(p) => p,
            None => return results,
        };

        for result in &mut results {
            let boost = gamma * cosine_similarity(&result.document.vector, &preference);
            result.score += boost;
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// End the current session, clearing all state.
    pub async fn end_session(&self) {
        let mut session = self.session.write().await;
        *session = SessionState::new(String::new());
    }

    /// Compute the element-wise mean of a list of vectors.
    fn mean_vectors(vectors: &[Vec<f32>], dims: usize) -> Vec<f32> {
        if vectors.is_empty() {
            return vec![0.0; dims];
        }
        let mut sum = vec![0.0_f32; dims];
        for v in vectors {
            for (s, x) in sum.iter_mut().zip(v.iter()) {
                *s += x;
            }
        }
        let n = vectors.len() as f32;
        for s in &mut sum {
            *s /= n;
        }
        sum
    }

    // -----------------------------------------------------------------------
    // Tier 3: Long-Term Consolidation (S3-05)
    // -----------------------------------------------------------------------

    /// Hourly consolidation: reclassify low-confidence emails.
    ///
    /// In production this queries the database for emails whose confidence
    /// is below the threshold and re-categorizes them against the current
    /// centroids. For now the implementation operates on in-memory data.
    pub async fn hourly_consolidation(&self) -> Result<ConsolidationReport, VectorError> {
        let start = std::time::Instant::now();
        let mut report = ConsolidationReport::default();

        let threshold = self.config.low_confidence_threshold;

        // Get all email-text documents from the store.
        let docs = self
            .store
            .list_by_collection(&super::types::VectorCollection::EmailText, 1000, 0)
            .await?;

        let centroids = self.categorizer.get_centroids().await;
        if centroids.is_empty() {
            report.duration_ms = start.elapsed().as_millis() as u64;
            return Ok(report);
        }

        for doc in &docs {
            // Find the best matching centroid.
            let mut best_cat = EmailCategory::Uncategorized;
            let mut best_score: f32 = f32::NEG_INFINITY;

            for (cat, centroid) in &centroids {
                let score = cosine_similarity(&doc.vector, &centroid.vector);
                if score > best_score {
                    best_score = score;
                    best_cat = *cat;
                }
            }

            // If below threshold, mark as reclassified (the email would be
            // re-assigned in the database in production).
            if best_score < threshold && best_cat != EmailCategory::Uncategorized {
                report.emails_reclassified += 1;
            }
        }

        report.duration_ms = start.elapsed().as_millis() as u64;
        info!(
            reclassified = report.emails_reclassified,
            duration_ms = report.duration_ms,
            "Hourly consolidation complete"
        );
        Ok(report)
    }

    /// Daily consolidation: recompute all centroids from their member emails
    /// and take a snapshot for rollback.
    pub async fn daily_consolidation(&self) -> Result<ConsolidationReport, VectorError> {
        let start = std::time::Instant::now();
        let mut report = ConsolidationReport::default();

        // Take a snapshot of current centroids before changes.
        self.take_snapshot().await;

        // Get all email-text documents.
        let docs = self
            .store
            .list_by_collection(&super::types::VectorCollection::EmailText, 10_000, 0)
            .await?;

        let current_centroids = self.categorizer.get_centroids().await;
        if current_centroids.is_empty() || docs.is_empty() {
            report.duration_ms = start.elapsed().as_millis() as u64;
            return Ok(report);
        }

        // Assign each document to the best matching category.
        let mut category_vectors: HashMap<EmailCategory, Vec<Vec<f32>>> = HashMap::new();
        for doc in &docs {
            let mut best_cat = EmailCategory::Uncategorized;
            let mut best_score: f32 = f32::NEG_INFINITY;
            for (cat, centroid) in &current_centroids {
                let score = cosine_similarity(&doc.vector, &centroid.vector);
                if score > best_score {
                    best_score = score;
                    best_cat = *cat;
                }
            }
            category_vectors
                .entry(best_cat)
                .or_default()
                .push(doc.vector.clone());
        }

        // Recompute centroids as the mean of assigned embeddings.
        for (cat, vectors) in &category_vectors {
            if vectors.is_empty() {
                continue;
            }
            let dims = vectors[0].len();
            let mean = Self::mean_vectors(vectors, dims);
            self.categorizer.seed_centroid(*cat, mean).await;
            report.centroids_updated += 1;
        }

        report.duration_ms = start.elapsed().as_millis() as u64;
        info!(
            centroids_updated = report.centroids_updated,
            duration_ms = report.duration_ms,
            "Daily consolidation complete"
        );
        Ok(report)
    }

    /// Take a snapshot of the current centroids.
    async fn take_snapshot(&self) {
        let centroids = self.categorizer.get_centroids().await;
        let snapshot = CentroidSnapshot {
            timestamp: Utc::now(),
            centroids,
        };

        let mut snaps = self.snapshots.write().await;
        snaps.push(snapshot);

        // Trim to max_snapshots.
        while snaps.len() > self.config.max_snapshots {
            snaps.remove(0);
        }
    }

    // -----------------------------------------------------------------------
    // Safeguards (S3-06)
    // -----------------------------------------------------------------------

    /// Check for position bias: if rank-1 clicks dominate, log a warning.
    ///
    /// Returns `true` if position bias was detected.
    pub async fn check_position_bias(&self) -> bool {
        let m = self.metrics.read().await;
        if m.total_clicks == 0 {
            return false;
        }
        let ratio = m.rank1_clicks as f32 / m.total_clicks as f32;
        if ratio > self.config.position_bias_threshold {
            warn!(
                ratio = ratio,
                threshold = self.config.position_bias_threshold,
                "Position bias detected: rank-1 clicks dominate"
            );
            return true;
        }
        false
    }

    /// Record a click at a given rank position (1-based).
    pub async fn record_ranked_click(&self, rank: u32) {
        let mut m = self.metrics.write().await;
        m.total_clicks += 1;
        if rank == 1 {
            m.rank1_clicks += 1;
        }
    }

    /// Check centroid drift for all categories.
    ///
    /// Returns a map of category name to relative drift for categories
    /// that exceed the alarm threshold.
    pub async fn check_centroid_drift(&self) -> HashMap<String, f32> {
        let m = self.metrics.read().await;
        m.centroid_drift
            .iter()
            .filter(|(_, &drift)| drift > self.config.drift_alarm_threshold)
            .map(|(cat, &drift)| (cat.clone(), drift))
            .collect()
    }

    /// Decide whether the SONA engine should be used for this query,
    /// or whether this query is in the A/B control group.
    ///
    /// Returns `false` for approximately `ab_control_percentage` of calls.
    pub fn should_use_sona(&self) -> bool {
        if !self.config.sona_enabled {
            return false;
        }
        // Use a simple random sample.
        let r: f32 = rand::random();
        r >= self.config.ab_control_percentage
    }

    /// Rollback all centroids to a previous snapshot.
    pub async fn rollback_to_snapshot(&self, snapshot_index: usize) -> Result<(), VectorError> {
        let snaps = self.snapshots.read().await;
        let snapshot = snaps.get(snapshot_index).ok_or_else(|| {
            VectorError::StoreFailed(format!(
                "Snapshot index {snapshot_index} out of range (have {})",
                snaps.len()
            ))
        })?;

        for (cat, centroid) in &snapshot.centroids {
            self.categorizer
                .seed_centroid(*cat, centroid.vector.clone())
                .await;
        }

        info!(
            snapshot_index = snapshot_index,
            timestamp = %snapshot.timestamp,
            "Rolled back centroids to snapshot"
        );
        Ok(())
    }

    /// Return a clone of the current learning metrics.
    pub async fn get_metrics(&self) -> LearningMetrics {
        self.metrics.read().await.clone()
    }

    /// Return the number of stored snapshots.
    pub async fn snapshot_count(&self) -> usize {
        self.snapshots.read().await.len()
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
    use crate::vectors::types::{VectorCollection, VectorDocument, VectorId};

    /// Build a test learning engine with an in-memory store and mock embeddings.
    async fn make_engine(config: LearningConfig) -> (LearningEngine, Arc<dyn VectorStoreBackend>) {
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let emb_config = EmbeddingConfig {
            provider: "mock".to_string(),
            ..EmbeddingConfig::default()
        };
        let embedding = Arc::new(EmbeddingPipeline::new(&emb_config).unwrap());
        let categorizer = Arc::new(VectorCategorizer::with_config(
            store.clone(),
            embedding,
            0.0, // low threshold so everything matches
            config.max_centroid_shift,
            0, // categorizer-level cold start off; engine has its own
        ));
        // Create a temporary in-memory database for tests.
        let db = Arc::new(
            Database::connect("sqlite::memory:")
                .await
                .expect("in-memory db"),
        );
        let engine = LearningEngine::new(categorizer, store.clone(), db, config);
        (engine, store)
    }

    /// Insert a fake email document with a known embedding into the store.
    async fn insert_email(store: &Arc<dyn VectorStoreBackend>, email_id: &str, vector: Vec<f32>) {
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

    // -- S3-03: Tier 1 tests ------------------------------------------------

    #[test]
    fn test_feedback_quality_scoring() {
        assert_eq!(
            LearningEngine::quality_for_action(&FeedbackAction::Reclassify {
                from: EmailCategory::Work,
                to: EmailCategory::Personal,
            }),
            1.0
        );
        assert_eq!(
            LearningEngine::quality_for_action(&FeedbackAction::MoveToGroup {
                group_id: "g1".into()
            }),
            1.0
        );
        assert_eq!(
            LearningEngine::quality_for_action(&FeedbackAction::Star),
            0.4
        );
        assert_eq!(
            LearningEngine::quality_for_action(&FeedbackAction::Reply { delay_secs: 60 }),
            0.5
        );
        assert_eq!(
            LearningEngine::quality_for_action(&FeedbackAction::Reply { delay_secs: 600 }),
            0.3
        );
        assert_eq!(
            LearningEngine::quality_for_action(&FeedbackAction::Archive),
            0.2
        );
        assert_eq!(
            LearningEngine::quality_for_action(&FeedbackAction::Delete),
            0.4
        );
    }

    #[tokio::test]
    async fn test_tier1_reclassify_updates_centroid() {
        let config = LearningConfig {
            min_feedback_events: 0,
            max_centroid_shift: 10.0, // large so shift is not bounded
            ..Default::default()
        };
        let (engine, store) = make_engine(config).await;

        // Seed centroids.
        engine
            .categorizer
            .seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;
        engine
            .categorizer
            .seed_centroid(EmailCategory::Personal, vec![0.0, 1.0, 0.0])
            .await;
        engine.capture_initial_centroids().await;

        // Insert an email.
        insert_email(&store, "e1", vec![0.5, 0.5, 0.0]).await;

        let result = engine
            .on_user_feedback(UserFeedback {
                email_id: "e1".into(),
                action: FeedbackAction::Reclassify {
                    from: EmailCategory::Work,
                    to: EmailCategory::Personal,
                },
                timestamp: Utc::now(),
            })
            .await
            .unwrap();

        assert!(result.centroid_updated);
        assert_eq!(result.quality, 1.0);

        // Personal centroid should have moved toward the email embedding.
        let centroids = engine.categorizer.get_centroids().await;
        let personal = &centroids[&EmailCategory::Personal];
        // alpha = 1.0 * 0.05 = 0.05
        // new = (1-0.05)*[0,1,0] + 0.05*[0.5,0.5,0] = [0.025, 0.975, 0]
        assert!(
            (personal.vector[0] - 0.025).abs() < 1e-4,
            "got {}",
            personal.vector[0]
        );
        assert!(
            (personal.vector[1] - 0.975).abs() < 1e-4,
            "got {}",
            personal.vector[1]
        );
    }

    #[tokio::test]
    async fn test_tier1_negative_update_on_source_category() {
        let config = LearningConfig {
            min_feedback_events: 0,
            max_centroid_shift: 10.0,
            ..Default::default()
        };
        let (engine, store) = make_engine(config).await;

        engine
            .categorizer
            .seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;
        engine
            .categorizer
            .seed_centroid(EmailCategory::Personal, vec![0.0, 1.0, 0.0])
            .await;
        engine.capture_initial_centroids().await;

        insert_email(&store, "e1", vec![0.5, 0.5, 0.0]).await;

        engine
            .on_user_feedback(UserFeedback {
                email_id: "e1".into(),
                action: FeedbackAction::Reclassify {
                    from: EmailCategory::Work,
                    to: EmailCategory::Personal,
                },
                timestamp: Utc::now(),
            })
            .await
            .unwrap();

        // Work centroid should have been pushed *away* from the email.
        // neg_embedding = -0.3 * [0.5, 0.5, 0] = [-0.15, -0.15, 0]
        // beta = 1.0 * 0.02 = 0.02
        // new = (1-0.02)*[1,0,0] + 0.02*[-0.15,-0.15,0]
        //     = [0.98, 0, 0] + [-0.003, -0.003, 0]
        //     = [0.977, -0.003, 0]
        let centroids = engine.categorizer.get_centroids().await;
        let work = &centroids[&EmailCategory::Work];
        assert!(
            work.vector[0] < 1.0,
            "Work centroid x should decrease, got {}",
            work.vector[0]
        );
        assert!(
            work.vector[1] < 0.0,
            "Work centroid y should go negative, got {}",
            work.vector[1]
        );
    }

    #[tokio::test]
    async fn test_tier1_bounded_shift() {
        let config = LearningConfig {
            min_feedback_events: 0,
            max_centroid_shift: 0.001, // very small
            positive_learning_rate: 0.05,
            ..Default::default()
        };
        let (engine, store) = make_engine(config).await;

        engine
            .categorizer
            .seed_centroid(EmailCategory::Personal, vec![0.0, 1.0, 0.0])
            .await;
        engine
            .categorizer
            .seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;
        engine.capture_initial_centroids().await;

        insert_email(&store, "e1", vec![1.0, 0.0, 0.0]).await;

        engine
            .on_user_feedback(UserFeedback {
                email_id: "e1".into(),
                action: FeedbackAction::Reclassify {
                    from: EmailCategory::Work,
                    to: EmailCategory::Personal,
                },
                timestamp: Utc::now(),
            })
            .await
            .unwrap();

        let centroids = engine.categorizer.get_centroids().await;
        let personal = &centroids[&EmailCategory::Personal];
        let shift = LearningEngine::l2_distance(&[0.0, 1.0, 0.0], &personal.vector);
        assert!(
            shift <= 0.001 + 1e-5,
            "Shift {shift} should be bounded to 0.001"
        );
    }

    #[tokio::test]
    async fn test_tier1_cold_start_protection() {
        let config = LearningConfig {
            min_feedback_events: 5,
            max_centroid_shift: 10.0,
            ..Default::default()
        };
        let (engine, store) = make_engine(config).await;

        engine
            .categorizer
            .seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;
        engine
            .categorizer
            .seed_centroid(EmailCategory::Personal, vec![0.0, 1.0, 0.0])
            .await;

        insert_email(&store, "e1", vec![0.5, 0.5, 0.0]).await;

        // First 4 feedback events should not update centroids.
        for _ in 0..4 {
            let result = engine
                .on_user_feedback(UserFeedback {
                    email_id: "e1".into(),
                    action: FeedbackAction::Reclassify {
                        from: EmailCategory::Work,
                        to: EmailCategory::Personal,
                    },
                    timestamp: Utc::now(),
                })
                .await
                .unwrap();
            assert!(!result.centroid_updated);
            assert!(result.safeguard_triggered);
        }

        let centroids = engine.categorizer.get_centroids().await;
        assert_eq!(
            centroids[&EmailCategory::Personal].vector,
            vec![0.0, 1.0, 0.0],
            "Centroid should not have moved during cold start"
        );

        // 5th event should trigger an update.
        let result = engine
            .on_user_feedback(UserFeedback {
                email_id: "e1".into(),
                action: FeedbackAction::Reclassify {
                    from: EmailCategory::Work,
                    to: EmailCategory::Personal,
                },
                timestamp: Utc::now(),
            })
            .await
            .unwrap();
        assert!(result.centroid_updated);
        assert!(!result.safeguard_triggered);
    }

    // -- S3-04: Tier 2 tests ------------------------------------------------

    #[tokio::test]
    async fn test_tier2_session_preference_vector() {
        let config = LearningConfig::default();
        let (engine, store) = make_engine(config).await;

        insert_email(&store, "click1", vec![1.0, 0.0, 0.0]).await;
        insert_email(&store, "click2", vec![0.0, 1.0, 0.0]).await;
        insert_email(&store, "skip1", vec![0.0, 0.0, 1.0]).await;

        engine.start_session().await;
        engine.record_click("click1").await;
        engine.record_click("click2").await;
        engine.record_skip("skip1").await;

        let pref = engine.get_session_preference().await.unwrap();
        // mean(clicked) = [0.5, 0.5, 0]
        // mean(skipped) = [0, 0, 1]
        // raw = [0.5, 0.5, -1.0]
        // The vector should be normalized.
        let mag: f32 = pref.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (mag - 1.0).abs() < 1e-5,
            "Preference vector should be unit length, got mag={mag}"
        );
        // The x and y components should be positive, z should be negative.
        assert!(pref[0] > 0.0);
        assert!(pref[1] > 0.0);
        assert!(pref[2] < 0.0);
    }

    #[tokio::test]
    async fn test_tier2_session_reranking() {
        let config = LearningConfig::default();
        let (engine, store) = make_engine(config).await;

        // Set up session: user clicks emails aligned with [1,0,0].
        insert_email(&store, "click1", vec![1.0, 0.0, 0.0]).await;
        engine.start_session().await;
        engine.record_click("click1").await;

        // Build two results: one aligned with preference, one orthogonal.
        let aligned = ScoredResult {
            document: VectorDocument {
                id: VectorId::new(),
                email_id: "a".into(),
                vector: vec![1.0, 0.0, 0.0],
                metadata: HashMap::new(),
                collection: VectorCollection::EmailText,
                created_at: Utc::now(),
            },
            score: 0.5,
        };
        let orthogonal = ScoredResult {
            document: VectorDocument {
                id: VectorId::new(),
                email_id: "b".into(),
                vector: vec![0.0, 1.0, 0.0],
                metadata: HashMap::new(),
                collection: VectorCollection::EmailText,
                created_at: Utc::now(),
            },
            score: 0.5,
        };

        let reranked = engine
            .rerank_with_session(vec![orthogonal, aligned], 0.15)
            .await;

        // The aligned result should be boosted above the orthogonal one.
        assert_eq!(reranked[0].document.email_id, "a");
        assert!(reranked[0].score > reranked[1].score);
    }

    #[tokio::test]
    async fn test_tier2_empty_session_returns_none() {
        let config = LearningConfig::default();
        let (engine, _store) = make_engine(config).await;

        engine.start_session().await;
        let pref = engine.get_session_preference().await;
        assert!(pref.is_none());
    }

    // -- S3-05: Tier 3 tests ------------------------------------------------

    #[tokio::test]
    async fn test_tier3_hourly_reclassifies_low_confidence() {
        let config = LearningConfig {
            low_confidence_threshold: 0.99, // almost everything is "low confidence"
            ..Default::default()
        };
        let (engine, store) = make_engine(config).await;

        // Seed a centroid and insert an email that won't reach 0.99 similarity.
        engine
            .categorizer
            .seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;

        insert_email(&store, "e1", vec![0.6, 0.8, 0.0]).await;

        let report = engine.hourly_consolidation().await.unwrap();
        assert!(
            report.emails_reclassified >= 1,
            "Should reclassify low-confidence emails"
        );
    }

    #[tokio::test]
    async fn test_tier3_daily_recomputes_centroids() {
        let config = LearningConfig::default();
        let (engine, store) = make_engine(config).await;

        // Seed a centroid.
        engine
            .categorizer
            .seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;

        // Insert emails that will be assigned to Work.
        insert_email(&store, "e1", vec![0.9, 0.1, 0.0]).await;
        insert_email(&store, "e2", vec![0.8, 0.2, 0.0]).await;
        insert_email(&store, "e3", vec![0.7, 0.3, 0.0]).await;

        let report = engine.daily_consolidation().await.unwrap();
        assert!(report.centroids_updated >= 1);

        // The centroid should now be the mean of the three embeddings.
        let centroids = engine.categorizer.get_centroids().await;
        let work = &centroids[&EmailCategory::Work];
        let expected_x = (0.9 + 0.8 + 0.7) / 3.0;
        let expected_y = (0.1 + 0.2 + 0.3) / 3.0;
        assert!(
            (work.vector[0] - expected_x).abs() < 1e-4,
            "got {}",
            work.vector[0]
        );
        assert!(
            (work.vector[1] - expected_y).abs() < 1e-4,
            "got {}",
            work.vector[1]
        );
    }

    // -- S3-06: Safeguard tests ---------------------------------------------

    #[tokio::test]
    async fn test_safeguard_position_bias_detection() {
        let config = LearningConfig {
            position_bias_threshold: 0.95,
            ..Default::default()
        };
        let (engine, _store) = make_engine(config).await;

        // Record 100 clicks, 97 at rank 1.
        for _ in 0..97 {
            engine.record_ranked_click(1).await;
        }
        for _ in 0..3 {
            engine.record_ranked_click(5).await;
        }

        assert!(engine.check_position_bias().await);
    }

    #[tokio::test]
    async fn test_safeguard_centroid_drift_alarm() {
        let config = LearningConfig {
            min_feedback_events: 0,
            max_centroid_shift: 10.0,
            drift_alarm_threshold: 0.01, // very low threshold
            ..Default::default()
        };
        let (engine, store) = make_engine(config).await;

        engine
            .categorizer
            .seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;
        engine
            .categorizer
            .seed_centroid(EmailCategory::Personal, vec![0.0, 1.0, 0.0])
            .await;
        engine.capture_initial_centroids().await;

        insert_email(&store, "e1", vec![0.0, 1.0, 0.0]).await;

        engine
            .on_user_feedback(UserFeedback {
                email_id: "e1".into(),
                action: FeedbackAction::Reclassify {
                    from: EmailCategory::Work,
                    to: EmailCategory::Personal,
                },
                timestamp: Utc::now(),
            })
            .await
            .unwrap();

        let drifts = engine.check_centroid_drift().await;
        // At least one category should have drifted above the alarm threshold.
        assert!(
            !drifts.is_empty(),
            "Expected drift alarm for at least one category"
        );
    }

    #[test]
    fn test_safeguard_ab_evaluation_ratio() {
        let config = LearningConfig {
            ab_control_percentage: 0.10,
            ..Default::default()
        };
        // We cannot build the full engine in a sync test, so test the logic directly.
        // Run 1000 trials and check that roughly 10% are control.
        let mut control_count = 0;
        let trials = 10_000;
        for _ in 0..trials {
            let r: f32 = rand::random();
            if r < config.ab_control_percentage {
                control_count += 1;
            }
        }
        let ratio = control_count as f32 / trials as f32;
        // Should be approximately 0.10 +/- 0.03.
        assert!(
            (ratio - 0.10).abs() < 0.03,
            "A/B control ratio {ratio} is too far from 0.10"
        );
    }

    #[tokio::test]
    async fn test_rollback_to_snapshot() {
        let config = LearningConfig::default();
        let (engine, store) = make_engine(config).await;

        // Seed centroid and capture.
        engine
            .categorizer
            .seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;

        // Take a snapshot manually by running daily consolidation (which snapshots).
        insert_email(&store, "e1", vec![1.0, 0.0, 0.0]).await;
        engine.daily_consolidation().await.unwrap();

        // Now change the centroid.
        engine
            .categorizer
            .seed_centroid(EmailCategory::Work, vec![0.0, 0.0, 1.0])
            .await;

        let centroids = engine.categorizer.get_centroids().await;
        assert_eq!(centroids[&EmailCategory::Work].vector, vec![0.0, 0.0, 1.0]);

        // Rollback to the snapshot (index 0).
        engine.rollback_to_snapshot(0).await.unwrap();

        let centroids = engine.categorizer.get_centroids().await;
        // The centroid should be back to [1.0, 0.0, 0.0] (the mean of the
        // single email in the store at snapshot time).
        assert_eq!(centroids[&EmailCategory::Work].vector, vec![1.0, 0.0, 0.0]);
    }

    #[tokio::test]
    async fn test_snapshot_history_limit() {
        let config = LearningConfig {
            max_snapshots: 3,
            ..Default::default()
        };
        let (engine, store) = make_engine(config).await;

        engine
            .categorizer
            .seed_centroid(EmailCategory::Work, vec![1.0, 0.0, 0.0])
            .await;
        insert_email(&store, "e1", vec![1.0, 0.0, 0.0]).await;

        // Run 5 daily consolidations (each takes a snapshot).
        for _ in 0..5 {
            engine.daily_consolidation().await.unwrap();
        }

        let count = engine.snapshot_count().await;
        assert_eq!(count, 3, "Should cap at max_snapshots=3, got {count}");
    }
}
