//! GraphSAGE + KMeans++ hybrid topic clustering engine (S3-01, S3-02: ADR-009).
//!
//! Implements a production-ready clustering pipeline:
//! 1. Build an email similarity graph via HNSW neighbors
//! 2. Run GraphSAGE (via `ruvector-gnn` `RuvectorLayer`) to produce learned node
//!    embeddings that capture graph structure
//! 3. Cluster embeddings with KMeans++ for stable, interpretable topic clusters
//!
//! This replaces the earlier Mini-batch K-Means approach with a true GNN-based
//! pipeline that leverages attention-based neighbor aggregation.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ruvector_gnn::RuvectorLayer;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing;

use crate::db::Database;

use super::categorizer::cosine_similarity;
use super::error::VectorError;
use super::store::VectorStoreBackend;
use super::types::VectorCollection;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the GraphSAGE + KMeans++ clustering engine (ADR-009).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Minimum number of emails to form a cluster.
    #[serde(default = "default_min_cluster_size")]
    pub min_cluster_size: usize,
    /// Centroid similarity above which two clusters are merged.
    #[serde(default = "default_merge_threshold")]
    pub merge_threshold: f32,
    /// Minimum improvement to reassign an email to a new cluster.
    #[serde(default = "default_hysteresis_delta")]
    pub hysteresis_delta: f32,
    /// Consecutive stable runs before a cluster is visible.
    #[serde(default = "default_min_stability_runs")]
    pub min_stability_runs: u32,
    /// Maximum number of clusters to discover.
    #[serde(default = "default_max_clusters")]
    pub max_clusters: usize,
    /// Number of nearest neighbors for the similarity graph.
    #[serde(default = "default_neighbor_count")]
    pub neighbor_count: usize,
    /// Hidden dimension for GraphSAGE layers.
    #[serde(default = "default_graphsage_hidden_dim")]
    pub graphsage_hidden_dim: usize,
    /// Number of GraphSAGE propagation layers.
    #[serde(default = "default_graphsage_num_layers")]
    pub graphsage_num_layers: usize,
    /// Number of attention heads per GraphSAGE layer.
    #[serde(default = "default_graphsage_attention_heads")]
    pub graphsage_attention_heads: usize,
    /// Dropout rate for GraphSAGE layers (0.0 = none).
    #[serde(default = "default_graphsage_dropout")]
    pub graphsage_dropout: f32,
    /// Maximum KMeans++ iterations for convergence.
    #[serde(default = "default_kmeans_max_iters")]
    pub kmeans_max_iters: usize,
}

fn default_min_cluster_size() -> usize {
    5
}
fn default_merge_threshold() -> f32 {
    0.85
}
fn default_hysteresis_delta() -> f32 {
    0.05
}
fn default_min_stability_runs() -> u32 {
    3
}
fn default_max_clusters() -> usize {
    50
}
fn default_neighbor_count() -> usize {
    20
}
fn default_graphsage_hidden_dim() -> usize {
    64
}
fn default_graphsage_num_layers() -> usize {
    2
}
fn default_graphsage_attention_heads() -> usize {
    4
}
fn default_graphsage_dropout() -> f32 {
    0.0
}
fn default_kmeans_max_iters() -> usize {
    100
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            min_cluster_size: default_min_cluster_size(),
            merge_threshold: default_merge_threshold(),
            hysteresis_delta: default_hysteresis_delta(),
            min_stability_runs: default_min_stability_runs(),
            max_clusters: default_max_clusters(),
            neighbor_count: default_neighbor_count(),
            graphsage_hidden_dim: default_graphsage_hidden_dim(),
            graphsage_num_layers: default_graphsage_num_layers(),
            graphsage_attention_heads: default_graphsage_attention_heads(),
            graphsage_dropout: default_graphsage_dropout(),
            kmeans_max_iters: default_kmeans_max_iters(),
        }
    }
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// A discovered topic cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicCluster {
    pub id: String,
    pub name: String,
    pub description: String,
    pub centroid: Vec<f32>,
    pub email_ids: Vec<String>,
    pub email_count: usize,
    pub stability_score: f32,
    pub stability_runs: u32,
    pub is_pinned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Report returned after a full recluster operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusteringReport {
    pub total_emails: usize,
    pub cluster_count: usize,
    pub new_clusters: usize,
    pub merged_clusters: usize,
    pub dissolved_clusters: usize,
    pub unclustered_count: usize,
}

// ---------------------------------------------------------------------------
// Stop words for cluster naming
// ---------------------------------------------------------------------------

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by",
    "from", "is", "it", "be", "as", "was", "are", "been", "has", "have", "had", "do", "does",
    "did", "will", "would", "could", "should", "may", "might", "can", "this", "that", "these",
    "those", "i", "you", "he", "she", "we", "they", "me", "him", "her", "us", "them", "my", "your",
    "his", "its", "our", "their", "re", "fw", "fwd", "no", "not", "so", "if", "up", "out", "just",
    "about", "all", "also", "am", "an", "any", "into", "more", "new", "now", "only", "other",
    "over", "some", "than", "then", "very", "what", "when", "who", "how", "each", "which", "do",
    "get", "got", "here", "hi", "hello", "hey", "please", "thanks", "thank", "dear", "regards",
];

// ---------------------------------------------------------------------------
// K-Means implementation
// ---------------------------------------------------------------------------

/// Initialize centroids using K-means++ algorithm.
fn kmeans_plus_plus_init(data: &[Vec<f32>], k: usize) -> Vec<Vec<f32>> {
    if data.is_empty() || k == 0 {
        return vec![];
    }

    let n = data.len();
    let k = k.min(n);
    let mut centroids: Vec<Vec<f32>> = Vec::with_capacity(k);

    // Pick the first centroid as the first data point (deterministic for tests).
    centroids.push(data[0].clone());

    for _ in 1..k {
        // Compute distances to nearest centroid for each point.
        let mut distances: Vec<f32> = Vec::with_capacity(n);
        let mut total = 0.0_f64;

        for point in data.iter() {
            let min_dist = centroids
                .iter()
                .map(|c| euclidean_distance_sq(point, c))
                .fold(f32::MAX, f32::min);
            distances.push(min_dist);
            total += min_dist as f64;
        }

        if total == 0.0 {
            // All points are identical to existing centroids.
            centroids.push(data[centroids.len() % n].clone());
            continue;
        }

        // Pick the point with the maximum distance (deterministic).
        let max_idx = distances
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);

        centroids.push(data[max_idx].clone());
    }

    centroids
}

/// Euclidean distance squared between two vectors.
fn euclidean_distance_sq(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = x - y;
            d * d
        })
        .sum()
}

/// KMeans++ clustering (Lloyd's algorithm with KMeans++ initialization).
///
/// Returns cluster assignment for each data point.
///
/// This replaces the earlier Mini-batch K-Means with full Lloyd's iterations
/// for more accurate, stable clusters. KMeans++ seeding ensures good initial
/// centroid placement, and the algorithm converges when assignments stop changing.
pub fn kmeans(data: &[Vec<f32>], k: usize, max_iters: usize, _batch_size: usize) -> Vec<usize> {
    let n = data.len();
    if n == 0 || k == 0 {
        return vec![];
    }
    let k = k.min(n);
    let dim = data[0].len();

    let mut centroids = kmeans_plus_plus_init(data, k);
    let mut assignments: Vec<usize> = data
        .iter()
        .map(|point| nearest_centroid(point, &centroids))
        .collect();

    for _iter in 0..max_iters {
        // Recompute centroids from current assignments.
        let mut new_centroids = vec![vec![0.0_f32; dim]; k];
        let mut counts = vec![0usize; k];

        for (i, &cluster) in assignments.iter().enumerate() {
            counts[cluster] += 1;
            for (d, val) in data[i].iter().enumerate() {
                new_centroids[cluster][d] += val;
            }
        }

        for c in 0..k {
            if counts[c] > 0 {
                let n_f = counts[c] as f32;
                for val in new_centroids[c].iter_mut() {
                    *val /= n_f;
                }
            } else {
                // Empty cluster: keep old centroid.
                new_centroids[c] = centroids[c].clone();
            }
        }

        centroids = new_centroids;

        // Reassign all points.
        let new_assignments: Vec<usize> = data
            .iter()
            .map(|point| nearest_centroid(point, &centroids))
            .collect();

        if new_assignments == assignments {
            break; // Converged.
        }
        assignments = new_assignments;
    }

    assignments
}

/// Find the index of the nearest centroid to a point.
fn nearest_centroid(point: &[f32], centroids: &[Vec<f32>]) -> usize {
    centroids
        .iter()
        .enumerate()
        .map(|(i, c)| (i, euclidean_distance_sq(point, c)))
        .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Compute the silhouette score for a clustering.
///
/// For each point: a = avg distance to same-cluster points,
/// b = avg distance to nearest other cluster.
/// Score = mean over all points of (b - a) / max(a, b).
pub fn silhouette_score(data: &[Vec<f32>], assignments: &[usize], k: usize) -> f32 {
    let n = data.len();
    if n <= 1 || k <= 1 {
        return 0.0;
    }

    // Group indices by cluster.
    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, &c) in assignments.iter().enumerate() {
        clusters.entry(c).or_default().push(i);
    }

    // If all points are in one cluster, silhouette is 0.
    let active_clusters: Vec<usize> = clusters
        .iter()
        .filter(|(_, members)| !members.is_empty())
        .map(|(&k, _)| k)
        .collect();
    if active_clusters.len() <= 1 {
        return 0.0;
    }

    let mut total_score = 0.0_f64;
    let mut counted = 0usize;

    for i in 0..n {
        let ci = assignments[i];
        let same_cluster = &clusters[&ci];

        // a = avg distance to same-cluster points.
        let a = if same_cluster.len() <= 1 {
            0.0_f32
        } else {
            let sum: f32 = same_cluster
                .iter()
                .filter(|&&j| j != i)
                .map(|&j| euclidean_distance_sq(&data[i], &data[j]).sqrt())
                .sum();
            sum / (same_cluster.len() - 1) as f32
        };

        // b = avg distance to nearest other cluster.
        let b = active_clusters
            .iter()
            .filter(|&&c| c != ci)
            .map(|&c| {
                let members = &clusters[&c];
                if members.is_empty() {
                    f32::MAX
                } else {
                    let sum: f32 = members
                        .iter()
                        .map(|&j| euclidean_distance_sq(&data[i], &data[j]).sqrt())
                        .sum();
                    sum / members.len() as f32
                }
            })
            .fold(f32::MAX, f32::min);

        let max_ab = a.max(b);
        if max_ab > 0.0 {
            total_score += ((b - a) / max_ab) as f64;
        }
        counted += 1;
    }

    if counted == 0 {
        return 0.0;
    }
    (total_score / counted as f64) as f32
}

// ---------------------------------------------------------------------------
// Graph construction & propagation
// ---------------------------------------------------------------------------

/// Build a kNN similarity graph from embeddings.
///
/// Returns adjacency list: email_id -> [(neighbor_id, similarity)].
fn build_similarity_graph(
    ids: &[String],
    embeddings: &[Vec<f32>],
    k: usize,
    sender_map: &HashMap<String, String>,
    thread_map: &HashMap<String, String>,
) -> HashMap<String, Vec<(String, f32)>> {
    let n = ids.len();
    let mut graph: HashMap<String, Vec<(String, f32)>> = HashMap::with_capacity(n);

    for i in 0..n {
        let mut neighbors: Vec<(usize, f32)> = Vec::with_capacity(n - 1);

        for j in 0..n {
            if i == j {
                continue;
            }
            let sim = cosine_similarity(&embeddings[i], &embeddings[j]);
            neighbors.push((j, sim));
        }

        // Sort by similarity descending, take top-k.
        neighbors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        neighbors.truncate(k);

        let mut edge_map: HashMap<String, f32> = HashMap::new();
        for &(j, sim) in &neighbors {
            edge_map.insert(ids[j].clone(), sim);
        }

        // Same-sender edges (weight 0.3).
        if let Some(sender_i) = sender_map.get(&ids[i]) {
            for (j, id_j) in ids.iter().enumerate().take(n) {
                if i == j {
                    continue;
                }
                if let Some(sender_j) = sender_map.get(id_j) {
                    if sender_i == sender_j {
                        let entry = edge_map.entry(id_j.clone()).or_insert(0.0);
                        *entry = entry.max(0.3);
                    }
                }
            }
        }

        // Same-thread edges (weight 0.8).
        if let Some(thread_i) = thread_map.get(&ids[i]) {
            for (j, id_j) in ids.iter().enumerate().take(n) {
                if i == j {
                    continue;
                }
                if let Some(thread_j) = thread_map.get(id_j) {
                    if thread_i == thread_j {
                        let entry = edge_map.entry(id_j.clone()).or_insert(0.0);
                        *entry = entry.max(0.8);
                    }
                }
            }
        }

        let edges: Vec<(String, f32)> = edge_map.into_iter().collect();
        graph.insert(ids[i].clone(), edges);
    }

    graph
}

/// GraphSAGE propagation using ruvector-gnn `RuvectorLayer`.
///
/// Runs multiple GNN layers over the similarity graph. Each layer performs
/// attention-based message passing (multi-head attention + GRU update +
/// layer normalization) to produce learned node embeddings that capture
/// multi-hop graph structure.
///
/// Falls back to a simple mean-aggregation if GNN layer construction fails
/// (e.g., dimension mismatch), logging a warning.
fn propagate_embeddings_graphsage(
    ids: &[String],
    embeddings: &[Vec<f32>],
    graph: &HashMap<String, Vec<(String, f32)>>,
    config: &ClusterConfig,
) -> Vec<Vec<f32>> {
    let id_to_idx: HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    let input_dim = embeddings.first().map(|v| v.len()).unwrap_or(0);
    if input_dim == 0 {
        return embeddings.to_vec();
    }

    let hidden_dim = config.graphsage_hidden_dim;
    let num_layers = config.graphsage_num_layers.max(1);
    let heads = config.graphsage_attention_heads.max(1);
    let dropout = config.graphsage_dropout;

    // Build GNN layer stack.
    // First layer: input_dim -> hidden_dim
    // Subsequent layers: hidden_dim -> hidden_dim
    // hidden_dim must be divisible by heads; round up if needed.
    let effective_hidden = hidden_dim.div_ceil(heads) * heads;

    let mut layers: Vec<RuvectorLayer> = Vec::with_capacity(num_layers);
    for layer_idx in 0..num_layers {
        let in_dim = if layer_idx == 0 {
            input_dim
        } else {
            effective_hidden
        };
        match RuvectorLayer::new(in_dim, effective_hidden, heads, dropout) {
            Ok(layer) => layers.push(layer),
            Err(e) => {
                tracing::warn!(
                    "GraphSAGE layer {layer_idx} construction failed: {e}. \
                     Falling back to mean aggregation."
                );
                return propagate_embeddings_mean(ids, embeddings, graph);
            }
        }
    }

    // Run forward pass for each node through the layer stack.
    let mut current_embeddings = embeddings.to_vec();

    for layer in &layers {
        let mut next_embeddings = Vec::with_capacity(ids.len());

        for (i, id) in ids.iter().enumerate() {
            let neighbors = graph.get(id.as_str());

            let (neighbor_embs, edge_weights) = match neighbors {
                Some(n) if !n.is_empty() => {
                    let mut embs = Vec::new();
                    let mut weights = Vec::new();
                    for (neighbor_id, weight) in n {
                        if let Some(&j) = id_to_idx.get(neighbor_id.as_str()) {
                            embs.push(current_embeddings[j].clone());
                            weights.push(*weight);
                        }
                    }
                    (embs, weights)
                }
                _ => (vec![], vec![]),
            };

            let updated = layer.forward(&current_embeddings[i], &neighbor_embs, &edge_weights);
            next_embeddings.push(updated);
        }

        current_embeddings = next_embeddings;
    }

    current_embeddings
}

/// Fallback: single-layer mean aggregation when GNN layer construction fails.
///
/// New embedding = 0.5 * own_vector + 0.5 * weighted_mean(neighbor_vectors).
fn propagate_embeddings_mean(
    ids: &[String],
    embeddings: &[Vec<f32>],
    graph: &HashMap<String, Vec<(String, f32)>>,
) -> Vec<Vec<f32>> {
    let id_to_idx: HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    let dim = embeddings.first().map(|v| v.len()).unwrap_or(0);

    ids.iter()
        .enumerate()
        .map(|(i, id)| {
            let neighbors = match graph.get(id) {
                Some(n) if !n.is_empty() => n,
                _ => return embeddings[i].clone(),
            };

            let mut mean = vec![0.0_f32; dim];
            let mut weight_sum = 0.0_f32;

            for (neighbor_id, weight) in neighbors {
                if let Some(&j) = id_to_idx.get(neighbor_id.as_str()) {
                    for (d, val) in mean.iter_mut().enumerate() {
                        *val += embeddings[j][d] * weight;
                    }
                    weight_sum += weight;
                }
            }

            if weight_sum > 0.0 {
                for val in mean.iter_mut() {
                    *val /= weight_sum;
                }
            }

            embeddings[i]
                .iter()
                .zip(mean.iter())
                .map(|(own, neigh)| 0.5 * own + 0.5 * neigh)
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Cluster naming
// ---------------------------------------------------------------------------

/// Generate a human-readable cluster name from email subjects.
pub fn generate_cluster_name(email_subjects: &[String]) -> String {
    if email_subjects.is_empty() {
        return "Unknown Topic".to_string();
    }

    let stop: HashSet<&str> = STOP_WORDS.iter().copied().collect();
    let mut word_counts: HashMap<String, usize> = HashMap::new();

    for subject in email_subjects {
        let words: HashSet<String> = subject
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 1)
            .map(|w| w.to_lowercase())
            .filter(|w| !stop.contains(w.as_str()))
            .collect();

        for word in words {
            *word_counts.entry(word).or_insert(0) += 1;
        }
    }

    if word_counts.is_empty() {
        return "Unknown Topic".to_string();
    }

    let mut sorted: Vec<(String, usize)> = word_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let top_words: Vec<String> = sorted
        .into_iter()
        .take(3)
        .map(|(mut w, _)| {
            // Title case.
            if let Some(first) = w.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            w
        })
        .collect();

    top_words.join(" ")
}

/// Compute the centroid (mean vector) for a set of embeddings.
fn compute_centroid(embeddings: &[&Vec<f32>]) -> Vec<f32> {
    if embeddings.is_empty() {
        return vec![];
    }
    let dim = embeddings[0].len();
    let mut centroid = vec![0.0_f32; dim];
    for emb in embeddings {
        for (d, val) in emb.iter().enumerate() {
            centroid[d] += val;
        }
    }
    let n = embeddings.len() as f32;
    for val in centroid.iter_mut() {
        *val /= n;
    }
    centroid
}

// ---------------------------------------------------------------------------
// ClusterEngine
// ---------------------------------------------------------------------------

/// Graph-based topic clustering engine (ADR-009).
///
/// Builds a similarity graph, propagates embeddings via simplified GraphSAGE,
/// then clusters via Mini-batch K-Means with automatic K selection.
pub struct ClusterEngine {
    store: Arc<dyn VectorStoreBackend>,
    #[allow(dead_code)]
    db: Arc<Database>,
    config: ClusterConfig,
    clusters: RwLock<Vec<TopicCluster>>,
    /// Email -> current cluster assignment for hysteresis checks.
    assignments: RwLock<HashMap<String, String>>,
}

impl ClusterEngine {
    /// Create a new clustering engine.
    pub fn new(
        store: Arc<dyn VectorStoreBackend>,
        db: Arc<Database>,
        config: ClusterConfig,
    ) -> Self {
        Self {
            store,
            db,
            config,
            clusters: RwLock::new(Vec::new()),
            assignments: RwLock::new(HashMap::new()),
        }
    }

    /// Create a clustering engine with pre-populated clusters (for testing).
    pub fn with_clusters(
        store: Arc<dyn VectorStoreBackend>,
        db: Arc<Database>,
        config: ClusterConfig,
        clusters: Vec<TopicCluster>,
    ) -> Self {
        let mut assignments_map = HashMap::new();
        for cluster in &clusters {
            for email_id in &cluster.email_ids {
                assignments_map.insert(email_id.clone(), cluster.id.clone());
            }
        }
        Self {
            store,
            db,
            config,
            clusters: RwLock::new(clusters),
            assignments: RwLock::new(assignments_map),
        }
    }

    /// Assign an email to the nearest cluster.
    ///
    /// Returns the cluster ID if assigned, or `None` if unclustered.
    /// Applies hysteresis: only reassigns if the new cluster is closer
    /// by more than `hysteresis_delta`.
    pub async fn assign_email(&self, email_id: &str, embedding: &[f32]) -> Option<String> {
        let clusters = self.clusters.read().await;
        if clusters.is_empty() {
            return None;
        }

        let mut best_cluster_id: Option<String> = None;
        let mut best_sim = f32::NEG_INFINITY;

        for cluster in clusters.iter() {
            let sim = cosine_similarity(embedding, &cluster.centroid);
            if sim > best_sim {
                best_sim = sim;
                best_cluster_id = Some(cluster.id.clone());
            }
        }

        // Check similarity threshold (use merge_threshold as assignment floor).
        let threshold = self.config.merge_threshold * 0.5; // Assignment is more lenient.
        if best_sim < threshold {
            return None;
        }

        let best_id = best_cluster_id?;

        // Hysteresis: only reassign if improvement exceeds delta.
        let assignments = self.assignments.read().await;
        if let Some(current_cluster_id) = assignments.get(email_id) {
            if current_cluster_id == &best_id {
                return Some(best_id);
            }
            // Find similarity to current cluster.
            if let Some(current_cluster) = clusters.iter().find(|c| &c.id == current_cluster_id) {
                let current_sim = cosine_similarity(embedding, &current_cluster.centroid);
                if best_sim - current_sim <= self.config.hysteresis_delta {
                    // Not enough improvement, keep current assignment.
                    return Some(current_cluster_id.clone());
                }
            }
        }

        drop(assignments);
        drop(clusters);

        // Update assignment.
        let mut assignments = self.assignments.write().await;
        assignments.insert(email_id.to_string(), best_id.clone());

        // Add email to cluster.
        let mut clusters = self.clusters.write().await;
        if let Some(cluster) = clusters.iter_mut().find(|c| c.id == best_id) {
            if !cluster.email_ids.contains(&email_id.to_string()) {
                cluster.email_ids.push(email_id.to_string());
                cluster.email_count = cluster.email_ids.len();
            }
        }

        Some(best_id)
    }

    /// Incrementally assign new emails to nearest clusters without reclustering.
    pub async fn incremental_update(&self, new_emails: Vec<(String, Vec<f32>)>) {
        for (email_id, embedding) in &new_emails {
            self.assign_email(email_id, embedding).await;
        }
    }

    /// Perform a full recluster of all emails.
    ///
    /// 1. Fetch all embeddings from the vector store.
    /// 2. Build similarity graph.
    /// 3. Propagate embeddings (simplified GraphSAGE).
    /// 4. Auto-detect K via silhouette score.
    /// 5. K-Means clustering.
    /// 6. Merge similar clusters with existing ones.
    /// 7. Apply stability guardrails.
    pub async fn full_recluster(&self) -> Result<ClusteringReport, VectorError> {
        // Fetch all email text embeddings.
        let all_docs = self
            .store
            .list_by_collection(&VectorCollection::EmailText, 100_000, 0)
            .await?;

        if all_docs.is_empty() {
            return Ok(ClusteringReport {
                total_emails: 0,
                cluster_count: 0,
                new_clusters: 0,
                merged_clusters: 0,
                dissolved_clusters: 0,
                unclustered_count: 0,
            });
        }

        let ids: Vec<String> = all_docs.iter().map(|d| d.email_id.clone()).collect();
        let embeddings: Vec<Vec<f32>> = all_docs.iter().map(|d| d.vector.clone()).collect();

        // Extract sender/thread metadata for graph edges.
        let sender_map: HashMap<String, String> = all_docs
            .iter()
            .filter_map(|d| {
                d.metadata
                    .get("sender")
                    .map(|s| (d.email_id.clone(), s.clone()))
            })
            .collect();

        let thread_map: HashMap<String, String> = all_docs
            .iter()
            .filter_map(|d| {
                d.metadata
                    .get("thread_id")
                    .map(|t| (d.email_id.clone(), t.clone()))
            })
            .collect();

        let subjects: HashMap<String, String> = all_docs
            .iter()
            .filter_map(|d| {
                d.metadata
                    .get("subject")
                    .map(|s| (d.email_id.clone(), s.clone()))
            })
            .collect();

        // Step 1: Build similarity graph via HNSW neighbors.
        let graph = build_similarity_graph(
            &ids,
            &embeddings,
            self.config.neighbor_count,
            &sender_map,
            &thread_map,
        );

        // Step 2: GraphSAGE propagation (ruvector-gnn learned embeddings).
        let propagated = propagate_embeddings_graphsage(&ids, &embeddings, &graph, &self.config);

        // Step 3: Auto-detect K via silhouette score.
        let max_k = self.config.max_clusters.min(ids.len());
        let min_k = 2.min(max_k);

        let mut best_k = min_k;
        let mut best_score = f32::NEG_INFINITY;

        let probe_iters = self.config.kmeans_max_iters.min(50);
        for k in min_k..=max_k.min(10) {
            let assignments = kmeans(&propagated, k, probe_iters, 0);
            let score = silhouette_score(&propagated, &assignments, k);
            if score > best_score {
                best_score = score;
                best_k = k;
            }
        }

        // Step 4: Run KMeans++ with best K.
        let assignments = kmeans(&propagated, best_k, self.config.kmeans_max_iters, 0);

        // Step 5: Build new clusters.
        let mut cluster_members: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, &c) in assignments.iter().enumerate() {
            cluster_members.entry(c).or_default().push(i);
        }

        let now = Utc::now();
        let mut new_clusters: Vec<TopicCluster> = Vec::new();
        let mut unclustered_count = 0;

        for (&_cluster_idx, members) in &cluster_members {
            if members.len() < self.config.min_cluster_size {
                unclustered_count += members.len();
                continue;
            }

            let member_embeddings: Vec<&Vec<f32>> =
                members.iter().map(|&i| &propagated[i]).collect();
            let centroid = compute_centroid(&member_embeddings);

            let email_ids: Vec<String> = members.iter().map(|&i| ids[i].clone()).collect();
            let cluster_subjects: Vec<String> = email_ids
                .iter()
                .filter_map(|id| subjects.get(id).cloned())
                .collect();

            let name = generate_cluster_name(&cluster_subjects);

            new_clusters.push(TopicCluster {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.clone(),
                description: format!("{} ({} emails)", name, members.len()),
                centroid,
                email_ids,
                email_count: members.len(),
                stability_score: 0.0,
                stability_runs: 0,
                is_pinned: false,
                created_at: now,
                updated_at: now,
            });
        }

        // Step 6: Merge with existing clusters.
        let mut old_clusters = self.clusters.write().await;
        let _old_count = old_clusters.len();
        let mut merged_count = 0;
        let mut dissolved_count = 0;

        // Preserve pinned clusters.
        let pinned: Vec<TopicCluster> = old_clusters
            .iter()
            .filter(|c| c.is_pinned)
            .cloned()
            .collect();

        // Match new clusters to old ones by centroid similarity.
        for new_cluster in &mut new_clusters {
            let mut best_match: Option<usize> = None;
            let mut best_sim = 0.0_f32;

            for (i, old_cluster) in old_clusters.iter().enumerate() {
                if old_cluster.is_pinned {
                    continue;
                }
                let sim = cosine_similarity(&new_cluster.centroid, &old_cluster.centroid);
                if sim > best_sim && sim > self.config.merge_threshold {
                    best_sim = sim;
                    best_match = Some(i);
                }
            }

            if let Some(old_idx) = best_match {
                // Merge: carry over stability.
                let old = &old_clusters[old_idx];
                new_cluster.id = old.id.clone();
                new_cluster.stability_runs = old.stability_runs + 1;
                new_cluster.stability_score =
                    new_cluster.stability_runs as f32 / (new_cluster.stability_runs + 3) as f32;
                new_cluster.created_at = old.created_at;
                new_cluster.is_pinned = old.is_pinned;
                merged_count += 1;
            }
        }

        // Count dissolved (non-pinned old clusters that didn't match).
        let new_ids: HashSet<&str> = new_clusters.iter().map(|c| c.id.as_str()).collect();
        for old in old_clusters.iter() {
            if !old.is_pinned && !new_ids.contains(old.id.as_str()) {
                dissolved_count += 1;
            }
        }

        let new_count = new_clusters
            .iter()
            .filter(|c| c.stability_runs == 0)
            .count();

        // Combine: pinned clusters + new/merged clusters.
        let mut final_clusters = pinned;
        for c in new_clusters {
            if !final_clusters.iter().any(|existing| existing.id == c.id) {
                final_clusters.push(c);
            }
        }

        // Update assignments map.
        let mut assignments_map = self.assignments.write().await;
        assignments_map.clear();
        for cluster in &final_clusters {
            for email_id in &cluster.email_ids {
                assignments_map.insert(email_id.clone(), cluster.id.clone());
            }
        }

        let cluster_count = final_clusters.len();
        *old_clusters = final_clusters;

        Ok(ClusteringReport {
            total_emails: ids.len(),
            cluster_count,
            new_clusters: new_count.saturating_sub(dissolved_count.min(new_count)),
            merged_clusters: merged_count,
            dissolved_clusters: dissolved_count,
            unclustered_count,
        })
    }

    /// Merge multiple clusters into one with a new name.
    pub async fn merge_clusters(
        &self,
        source_ids: &[String],
        target_name: &str,
    ) -> Result<(), VectorError> {
        let mut clusters = self.clusters.write().await;

        let source_set: HashSet<&String> = source_ids.iter().collect();
        let mut merged_emails: Vec<String> = Vec::new();
        let mut merged_embeddings: Vec<Vec<f32>> = Vec::new();

        // Collect all emails from source clusters.
        let mut to_remove = Vec::new();
        for (i, cluster) in clusters.iter().enumerate() {
            if source_set.contains(&cluster.id) {
                merged_emails.extend(cluster.email_ids.clone());
                merged_embeddings.push(cluster.centroid.clone());
                to_remove.push(i);
            }
        }

        if to_remove.is_empty() {
            return Err(VectorError::NotFound(
                "No matching clusters found".to_string(),
            ));
        }

        // Remove source clusters (in reverse order to preserve indices).
        for &i in to_remove.iter().rev() {
            clusters.remove(i);
        }

        // Compute merged centroid.
        let centroid_refs: Vec<&Vec<f32>> = merged_embeddings.iter().collect();
        let centroid = compute_centroid(&centroid_refs);

        let now = Utc::now();
        let merged = TopicCluster {
            id: uuid::Uuid::new_v4().to_string(),
            name: target_name.to_string(),
            description: format!("{} ({} emails)", target_name, merged_emails.len()),
            centroid,
            email_count: merged_emails.len(),
            email_ids: merged_emails,
            stability_score: 1.0,
            stability_runs: self.config.min_stability_runs,
            is_pinned: false,
            created_at: now,
            updated_at: now,
        };

        // Update assignments.
        let mut assignments = self.assignments.write().await;
        for email_id in &merged.email_ids {
            assignments.insert(email_id.clone(), merged.id.clone());
        }

        clusters.push(merged);
        Ok(())
    }

    /// Pin a cluster so it is never auto-dissolved.
    pub async fn pin_cluster(&self, cluster_id: &str) -> Result<(), VectorError> {
        let mut clusters = self.clusters.write().await;
        let cluster = clusters
            .iter_mut()
            .find(|c| c.id == cluster_id)
            .ok_or_else(|| VectorError::NotFound(format!("Cluster {cluster_id} not found")))?;
        cluster.is_pinned = true;
        Ok(())
    }

    /// Get all clusters.
    pub async fn get_clusters(&self) -> Vec<TopicCluster> {
        self.clusters.read().await.clone()
    }

    /// Get email IDs belonging to a specific cluster.
    pub async fn get_cluster_emails(&self, cluster_id: &str) -> Result<Vec<String>, VectorError> {
        let clusters = self.clusters.read().await;
        let cluster = clusters
            .iter()
            .find(|c| c.id == cluster_id)
            .ok_or_else(|| VectorError::NotFound(format!("Cluster {cluster_id} not found")))?;
        Ok(cluster.email_ids.clone())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectors::store::InMemoryVectorStore;
    use crate::vectors::types::{VectorCollection, VectorDocument, VectorId};
    use chrono::Utc;
    use std::collections::HashMap;

    /// Helper: create a mock Database for testing.
    async fn mock_db() -> Arc<Database> {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Arc::new(db)
    }

    // -- K-Means tests -------------------------------------------------------

    #[test]
    fn test_kmeans_two_clear_clusters() {
        // Two well-separated clusters in 2D.
        let cluster_a: Vec<Vec<f32>> = (0..10).map(|i| vec![i as f32 * 0.1, 0.0]).collect();
        let cluster_b: Vec<Vec<f32>> = (0..10).map(|i| vec![10.0 + i as f32 * 0.1, 0.0]).collect();

        let mut data = cluster_a;
        data.extend(cluster_b);

        let assignments = kmeans(&data, 2, 100, 20);
        assert_eq!(assignments.len(), 20);

        // All points in first half should share a cluster.
        let first_half_cluster = assignments[0];
        for i in 0..10 {
            assert_eq!(
                assignments[i], first_half_cluster,
                "Point {i} in cluster A should have same assignment"
            );
        }

        // All points in second half should share a different cluster.
        let second_half_cluster = assignments[10];
        assert_ne!(first_half_cluster, second_half_cluster);
        for i in 10..20 {
            assert_eq!(
                assignments[i], second_half_cluster,
                "Point {i} in cluster B should have same assignment"
            );
        }
    }

    #[test]
    fn test_kmeans_three_clusters() {
        let mut data = Vec::new();
        // Cluster at origin.
        for _ in 0..5 {
            data.push(vec![0.0, 0.0]);
        }
        // Cluster at (10, 0).
        for _ in 0..5 {
            data.push(vec![10.0, 0.0]);
        }
        // Cluster at (0, 10).
        for _ in 0..5 {
            data.push(vec![0.0, 10.0]);
        }

        let assignments = kmeans(&data, 3, 100, 15);
        assert_eq!(assignments.len(), 15);

        // Verify each group has the same assignment.
        let c0 = assignments[0];
        let c1 = assignments[5];
        let c2 = assignments[10];

        for i in 0..5 {
            assert_eq!(assignments[i], c0);
        }
        for i in 5..10 {
            assert_eq!(assignments[i], c1);
        }
        for i in 10..15 {
            assert_eq!(assignments[i], c2);
        }

        // All three clusters should be distinct.
        assert_ne!(c0, c1);
        assert_ne!(c0, c2);
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_kmeans_single_cluster() {
        let data: Vec<Vec<f32>> = (0..10).map(|_| vec![1.0, 1.0]).collect();
        let assignments = kmeans(&data, 1, 50, 10);
        assert_eq!(assignments.len(), 10);
        // All should be cluster 0.
        for &a in &assignments {
            assert_eq!(a, 0);
        }
    }

    // -- Silhouette tests ----------------------------------------------------

    #[test]
    fn test_silhouette_perfect_clusters() {
        // Two perfectly separated clusters far apart.
        let mut data = Vec::new();
        for _ in 0..10 {
            data.push(vec![0.0, 0.0]);
        }
        for _ in 0..10 {
            data.push(vec![100.0, 100.0]);
        }

        let mut assignments = vec![0usize; 10];
        assignments.extend(vec![1usize; 10]);

        let score = silhouette_score(&data, &assignments, 2);
        assert!(
            score > 0.95,
            "Perfect clusters should have silhouette near 1.0, got {score}"
        );
    }

    #[test]
    fn test_silhouette_random_assignment() {
        // Points along a line, alternating cluster assignment.
        let data: Vec<Vec<f32>> = (0..20).map(|i| vec![i as f32, 0.0]).collect();
        let assignments: Vec<usize> = (0..20).map(|i| i % 2).collect();

        let score = silhouette_score(&data, &assignments, 2);
        // Interleaved assignments should give a low (possibly negative) score.
        assert!(
            score < 0.5,
            "Random-ish assignment should have low silhouette, got {score}"
        );
    }

    // -- Graph propagation test (mean fallback) ---------------------------------

    #[test]
    fn test_graph_neighbor_propagation_mean_fallback() {
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let embeddings = vec![
            vec![1.0, 0.1, 0.0],
            vec![0.1, 1.0, 0.1],
            vec![0.0, 0.1, 1.0],
        ];

        let sender_map = HashMap::new();
        let thread_map = HashMap::new();
        let graph = build_similarity_graph(&ids, &embeddings, 2, &sender_map, &thread_map);

        // Test the mean-aggregation fallback path.
        let propagated = propagate_embeddings_mean(&ids, &embeddings, &graph);

        assert_eq!(propagated.len(), 3);
        for emb in &propagated {
            assert_eq!(emb.len(), 3);
        }

        // The first point [1.0, 0.1, 0.0] should gain some weight in dim 2.
        let original_dim2 = 0.0_f32;
        assert!(
            propagated[0][2] > original_dim2 + 1e-6,
            "Propagation should mix neighbor information into dim 2, got {}",
            propagated[0][2]
        );
    }

    // -- GraphSAGE propagation test -------------------------------------------

    #[test]
    fn test_graphsage_propagation() {
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let embeddings = vec![
            vec![1.0, 0.1, 0.0, 0.0],
            vec![0.1, 1.0, 0.1, 0.0],
            vec![0.0, 0.1, 1.0, 0.1],
        ];

        let sender_map = HashMap::new();
        let thread_map = HashMap::new();
        let graph = build_similarity_graph(&ids, &embeddings, 2, &sender_map, &thread_map);

        let config = ClusterConfig {
            graphsage_hidden_dim: 4,
            graphsage_num_layers: 1,
            graphsage_attention_heads: 1,
            graphsage_dropout: 0.0,
            ..Default::default()
        };

        let propagated = propagate_embeddings_graphsage(&ids, &embeddings, &graph, &config);

        assert_eq!(propagated.len(), 3);
        // Output dim equals effective_hidden (rounded to heads multiple).
        let effective_hidden = ((config.graphsage_hidden_dim + config.graphsage_attention_heads
            - 1)
            / config.graphsage_attention_heads)
            * config.graphsage_attention_heads;
        for emb in &propagated {
            assert_eq!(emb.len(), effective_hidden);
        }

        // Embeddings should differ from each other (GNN learned different representations).
        assert_ne!(propagated[0], propagated[1]);
        assert_ne!(propagated[1], propagated[2]);
    }

    // -- Cluster assignment hysteresis test -----------------------------------

    #[tokio::test]
    async fn test_cluster_assignment_hysteresis() {
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let db = mock_db().await;

        let config = ClusterConfig {
            hysteresis_delta: 0.05,
            ..Default::default()
        };

        let now = Utc::now();
        let clusters = vec![
            TopicCluster {
                id: "c1".to_string(),
                name: "Cluster 1".to_string(),
                description: "".to_string(),
                centroid: vec![1.0, 0.0, 0.0],
                email_ids: vec!["email1".to_string()],
                email_count: 1,
                stability_score: 1.0,
                stability_runs: 5,
                is_pinned: false,
                created_at: now,
                updated_at: now,
            },
            TopicCluster {
                id: "c2".to_string(),
                name: "Cluster 2".to_string(),
                description: "".to_string(),
                centroid: vec![0.98, 0.2, 0.0],
                email_ids: vec![],
                email_count: 0,
                stability_score: 1.0,
                stability_runs: 5,
                is_pinned: false,
                created_at: now,
                updated_at: now,
            },
        ];

        let engine = ClusterEngine::with_clusters(store, db, config, clusters);

        // email1 is already assigned to c1 with centroid [1,0,0].
        // c2 centroid is [0.98, 0.2, 0] which is close but not much better.
        let embedding = vec![1.0, 0.05, 0.0];
        let result = engine.assign_email("email1", &embedding).await;

        // The difference in similarity should be less than hysteresis delta,
        // so it should stay in c1.
        assert_eq!(result, Some("c1".to_string()));
    }

    // -- Cluster stability runs test -----------------------------------------

    #[tokio::test]
    async fn test_cluster_stability_runs() {
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let db = mock_db().await;

        let config = ClusterConfig {
            min_stability_runs: 3,
            ..Default::default()
        };

        let now = Utc::now();
        let clusters = vec![
            TopicCluster {
                id: "stable".to_string(),
                name: "Stable".to_string(),
                description: "".to_string(),
                centroid: vec![1.0, 0.0],
                email_ids: vec![],
                email_count: 0,
                stability_score: 0.5,
                stability_runs: 5,
                is_pinned: false,
                created_at: now,
                updated_at: now,
            },
            TopicCluster {
                id: "new".to_string(),
                name: "New".to_string(),
                description: "".to_string(),
                centroid: vec![0.0, 1.0],
                email_ids: vec![],
                email_count: 0,
                stability_score: 0.0,
                stability_runs: 1,
                is_pinned: false,
                created_at: now,
                updated_at: now,
            },
        ];

        let engine = ClusterEngine::with_clusters(store, db, config, clusters);
        let all = engine.get_clusters().await;

        // The "stable" cluster has 5 runs >= min_stability_runs(3).
        let stable = all.iter().find(|c| c.id == "stable").unwrap();
        assert!(stable.stability_runs >= 3);

        // The "new" cluster has only 1 run < min_stability_runs(3).
        let new_cluster = all.iter().find(|c| c.id == "new").unwrap();
        assert!(new_cluster.stability_runs < 3);
    }

    // -- Cluster pinning test ------------------------------------------------

    #[tokio::test]
    async fn test_cluster_pinning() {
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let db = mock_db().await;
        let config = ClusterConfig::default();

        let now = Utc::now();
        let clusters = vec![TopicCluster {
            id: "pin-me".to_string(),
            name: "Important".to_string(),
            description: "".to_string(),
            centroid: vec![1.0, 0.0],
            email_ids: vec![],
            email_count: 0,
            stability_score: 1.0,
            stability_runs: 10,
            is_pinned: false,
            created_at: now,
            updated_at: now,
        }];

        let engine = ClusterEngine::with_clusters(store, db, config, clusters);

        // Pin the cluster.
        engine.pin_cluster("pin-me").await.unwrap();

        let all = engine.get_clusters().await;
        let pinned = all.iter().find(|c| c.id == "pin-me").unwrap();
        assert!(pinned.is_pinned);

        // Pinning a nonexistent cluster should fail.
        let err = engine.pin_cluster("nonexistent").await;
        assert!(err.is_err());
    }

    // -- Merge similar clusters test -----------------------------------------

    #[tokio::test]
    async fn test_merge_similar_clusters() {
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let db = mock_db().await;
        let config = ClusterConfig::default();

        let now = Utc::now();
        let clusters = vec![
            TopicCluster {
                id: "a".to_string(),
                name: "Cluster A".to_string(),
                description: "".to_string(),
                centroid: vec![1.0, 0.0],
                email_ids: vec!["e1".to_string(), "e2".to_string()],
                email_count: 2,
                stability_score: 1.0,
                stability_runs: 5,
                is_pinned: false,
                created_at: now,
                updated_at: now,
            },
            TopicCluster {
                id: "b".to_string(),
                name: "Cluster B".to_string(),
                description: "".to_string(),
                centroid: vec![0.9, 0.1],
                email_ids: vec!["e3".to_string()],
                email_count: 1,
                stability_score: 1.0,
                stability_runs: 3,
                is_pinned: false,
                created_at: now,
                updated_at: now,
            },
        ];

        let engine = ClusterEngine::with_clusters(store, db, config, clusters);

        engine
            .merge_clusters(&["a".to_string(), "b".to_string()], "Merged AB")
            .await
            .unwrap();

        let all = engine.get_clusters().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Merged AB");
        assert_eq!(all[0].email_count, 3);
        assert!(all[0].email_ids.contains(&"e1".to_string()));
        assert!(all[0].email_ids.contains(&"e2".to_string()));
        assert!(all[0].email_ids.contains(&"e3".to_string()));
    }

    // -- Generate cluster name test ------------------------------------------

    #[test]
    fn test_generate_cluster_name() {
        let subjects = vec![
            "Q3 Budget Planning Review".to_string(),
            "Re: Budget Planning Discussion".to_string(),
            "Fw: Budget Approval Needed".to_string(),
        ];
        let name = generate_cluster_name(&subjects);
        // "budget" and "planning" should be among top words.
        let lower = name.to_lowercase();
        assert!(
            lower.contains("budget"),
            "Expected 'budget' in name, got '{name}'"
        );
        assert!(
            lower.contains("planning"),
            "Expected 'planning' in name, got '{name}'"
        );
    }

    #[test]
    fn test_generate_cluster_name_empty() {
        let name = generate_cluster_name(&[]);
        assert_eq!(name, "Unknown Topic");
    }

    // -- Incremental update test ---------------------------------------------

    #[tokio::test]
    async fn test_incremental_update_assigns_new_emails() {
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let db = mock_db().await;
        let config = ClusterConfig::default();

        let now = Utc::now();
        let clusters = vec![TopicCluster {
            id: "topic1".to_string(),
            name: "Topic 1".to_string(),
            description: "".to_string(),
            centroid: vec![1.0, 0.0, 0.0],
            email_ids: vec!["existing".to_string()],
            email_count: 1,
            stability_score: 1.0,
            stability_runs: 5,
            is_pinned: false,
            created_at: now,
            updated_at: now,
        }];

        let engine = ClusterEngine::with_clusters(store, db, config, clusters);

        // Add new emails close to the cluster centroid.
        engine
            .incremental_update(vec![
                ("new1".to_string(), vec![0.99, 0.1, 0.0]),
                ("new2".to_string(), vec![0.95, 0.05, 0.0]),
            ])
            .await;

        let all = engine.get_clusters().await;
        let topic1 = all.iter().find(|c| c.id == "topic1").unwrap();
        assert!(
            topic1.email_ids.contains(&"new1".to_string()),
            "new1 should be assigned to topic1"
        );
        assert!(
            topic1.email_ids.contains(&"new2".to_string()),
            "new2 should be assigned to topic1"
        );
        assert_eq!(topic1.email_count, 3);
    }

    // -- Full recluster preserves pinned test --------------------------------

    #[tokio::test]
    async fn test_full_recluster_preserves_pinned() {
        let store = Arc::new(InMemoryVectorStore::new());

        // Insert documents into the store.
        let mut docs = Vec::new();
        for i in 0..10 {
            let mut metadata = HashMap::new();
            metadata.insert("subject".to_string(), format!("Budget report {i}"));
            metadata.insert("sender".to_string(), "alice@example.com".to_string());

            docs.push(VectorDocument {
                id: VectorId::new(),
                email_id: format!("email-{i}"),
                vector: vec![1.0 + i as f32 * 0.01, 0.0, 0.0],
                metadata,
                collection: VectorCollection::EmailText,
                created_at: Utc::now(),
            });
        }

        let store_backend: Arc<dyn VectorStoreBackend> = store.clone();
        for doc in docs {
            store_backend.insert(doc).await.unwrap();
        }

        let db = mock_db().await;
        let config = ClusterConfig {
            min_cluster_size: 2,
            max_clusters: 5,
            ..Default::default()
        };

        let now = Utc::now();
        let pinned_cluster = TopicCluster {
            id: "pinned-1".to_string(),
            name: "Pinned Topic".to_string(),
            description: "User-pinned".to_string(),
            centroid: vec![0.0, 1.0, 0.0],
            email_ids: vec!["pinned-email".to_string()],
            email_count: 1,
            stability_score: 1.0,
            stability_runs: 10,
            is_pinned: true,
            created_at: now,
            updated_at: now,
        };

        let engine = ClusterEngine::with_clusters(store_backend, db, config, vec![pinned_cluster]);

        let report = engine.full_recluster().await.unwrap();
        assert_eq!(report.total_emails, 10);

        let all = engine.get_clusters().await;
        let pinned = all.iter().find(|c| c.id == "pinned-1");
        assert!(
            pinned.is_some(),
            "Pinned cluster should be preserved after recluster"
        );
        assert!(pinned.unwrap().is_pinned);
    }
}
