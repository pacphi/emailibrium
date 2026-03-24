//! Evaluation metrics for Emailibrium.
//!
//! Provides:
//! - Adjusted Rand Index (ARI) for clustering agreement measurement
//! - Silhouette coefficient (sample and mean) for cluster quality
//! - Precision / recall / F1 for subscription detection evaluation
//! - Information Retrieval metrics: Recall@K, Precision@K, NDCG@K, MRR (Section 5.1)
//! - Multi-class ConfusionMatrix for classification evaluation (Section 5.2)

use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Adjusted Rand Index
// ---------------------------------------------------------------------------

/// Adjusted Rand Index -- measures agreement between two clusterings.
///
/// Uses the contingency-table approach:
///   ARI = (RI - Expected_RI) / (max(RI) - Expected_RI)
///
/// Returns a value in `[-1.0, 1.0]`:
/// - `1.0`  = perfect agreement
/// - `0.0`  = random labelling
/// - negative = worse than random
pub fn adjusted_rand_index(labels_true: &[usize], labels_pred: &[usize]) -> f32 {
    let n = labels_true.len();
    assert_eq!(n, labels_pred.len(), "label vectors must have equal length");

    if n == 0 {
        return 0.0;
    }

    // Build contingency table n_ij (true_cluster, pred_cluster) -> count.
    let mut contingency: HashMap<(usize, usize), u64> = HashMap::new();
    let mut row_sums: HashMap<usize, u64> = HashMap::new();
    let mut col_sums: HashMap<usize, u64> = HashMap::new();

    for i in 0..n {
        let key = (labels_true[i], labels_pred[i]);
        *contingency.entry(key).or_insert(0) += 1;
        *row_sums.entry(labels_true[i]).or_insert(0) += 1;
        *col_sums.entry(labels_pred[i]).or_insert(0) += 1;
    }

    // Sum of C(n_ij, 2) over all cells.
    let sum_comb_nij: f64 = contingency.values().map(|&v| comb2(v)).sum();

    // Sum of C(a_i, 2) over rows.
    let sum_comb_a: f64 = row_sums.values().map(|&v| comb2(v)).sum();

    // Sum of C(b_j, 2) over columns.
    let sum_comb_b: f64 = col_sums.values().map(|&v| comb2(v)).sum();

    let comb_n = comb2(n as u64);

    if comb_n == 0.0 {
        return 0.0;
    }

    let expected = (sum_comb_a * sum_comb_b) / comb_n;
    let max_index = 0.5 * (sum_comb_a + sum_comb_b);
    let denominator = max_index - expected;

    if denominator.abs() < f64::EPSILON {
        // Perfect agreement or degenerate case.
        if (sum_comb_nij - expected).abs() < f64::EPSILON {
            return 1.0;
        }
        return 0.0;
    }

    ((sum_comb_nij - expected) / denominator) as f32
}

/// C(n, 2) = n * (n - 1) / 2.
#[inline]
fn comb2(n: u64) -> f64 {
    if n < 2 {
        return 0.0;
    }
    (n as f64) * (n as f64 - 1.0) / 2.0
}

// ---------------------------------------------------------------------------
// Silhouette coefficient
// ---------------------------------------------------------------------------

/// Silhouette coefficient for a single point.
///
/// - `a` = mean distance to same-cluster points
/// - `b` = mean distance to the nearest other cluster
/// - `s` = (b - a) / max(a, b)
///
/// Returns `0.0` if the point is in a singleton cluster or only one cluster
/// exists.
pub fn silhouette_sample(
    point_idx: usize,
    data: &[Vec<f32>],
    labels: &[usize],
    distance_fn: fn(&[f32], &[f32]) -> f32,
) -> f32 {
    let n = data.len();
    assert_eq!(n, labels.len());
    assert!(point_idx < n);

    let my_label = labels[point_idx];

    // Group indices by cluster.
    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, &lbl) in labels.iter().enumerate() {
        clusters.entry(lbl).or_default().push(i);
    }

    // Only one cluster -> silhouette is 0.
    if clusters.len() <= 1 {
        return 0.0;
    }

    let same = &clusters[&my_label];

    // Singleton cluster -> silhouette is 0.
    if same.len() <= 1 {
        return 0.0;
    }

    // a = average distance to same-cluster points (excluding self).
    let a: f32 = same
        .iter()
        .filter(|&&j| j != point_idx)
        .map(|&j| distance_fn(&data[point_idx], &data[j]))
        .sum::<f32>()
        / (same.len() - 1) as f32;

    // b = min over other clusters of average distance.
    let b: f32 = clusters
        .iter()
        .filter(|(&lbl, _)| lbl != my_label)
        .map(|(_, members)| {
            if members.is_empty() {
                f32::MAX
            } else {
                members
                    .iter()
                    .map(|&j| distance_fn(&data[point_idx], &data[j]))
                    .sum::<f32>()
                    / members.len() as f32
            }
        })
        .fold(f32::MAX, f32::min);

    let max_ab = a.max(b);
    if max_ab == 0.0 {
        return 0.0;
    }
    (b - a) / max_ab
}

/// Mean silhouette coefficient across all points.
pub fn silhouette_score(
    data: &[Vec<f32>],
    labels: &[usize],
    distance_fn: fn(&[f32], &[f32]) -> f32,
) -> f32 {
    let n = data.len();
    if n <= 1 {
        return 0.0;
    }

    let sum: f64 = (0..n)
        .map(|i| silhouette_sample(i, data, labels, distance_fn) as f64)
        .sum();

    (sum / n as f64) as f32
}

// ---------------------------------------------------------------------------
// Detection metrics (precision / recall / F1)
// ---------------------------------------------------------------------------

/// Precision, recall, and F1 score for binary detection.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectionMetrics {
    pub precision: f32,
    pub recall: f32,
    pub f1: f32,
}

/// Compute precision, recall, and F1 for predicted vs. actual positive sets.
///
/// - `precision` = |predicted ∩ actual| / |predicted|
/// - `recall`    = |predicted ∩ actual| / |actual|
/// - `f1`        = 2 * precision * recall / (precision + recall)
///
/// Returns zeros when denominators are zero.
pub fn detection_metrics(
    predicted_positive: &[String],
    actual_positive: &[String],
) -> DetectionMetrics {
    let predicted_set: HashSet<&str> = predicted_positive.iter().map(|s| s.as_str()).collect();
    let actual_set: HashSet<&str> = actual_positive.iter().map(|s| s.as_str()).collect();

    let true_pos = predicted_set.intersection(&actual_set).count() as f32;

    let precision = if predicted_set.is_empty() {
        0.0
    } else {
        true_pos / predicted_set.len() as f32
    };

    let recall = if actual_set.is_empty() {
        0.0
    } else {
        true_pos / actual_set.len() as f32
    };

    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };

    DetectionMetrics {
        precision,
        recall,
        f1,
    }
}

// ---------------------------------------------------------------------------
// Euclidean distance helper
// ---------------------------------------------------------------------------

/// Euclidean distance between two vectors.
pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = x - y;
            d * d
        })
        .sum::<f32>()
        .sqrt()
}

// ---------------------------------------------------------------------------
// Information Retrieval Metrics (RESEARCH.md Section 5.1)
// ---------------------------------------------------------------------------

/// Recall@K: fraction of relevant documents found in the top-K results.
///
/// Returns 0.0 if there are no relevant documents.
pub fn recall_at_k(relevant: &[String], retrieved: &[String], k: usize) -> f32 {
    if relevant.is_empty() {
        return 0.0;
    }
    let top_k: HashSet<_> = retrieved.iter().take(k).collect();
    let relevant_set: HashSet<_> = relevant.iter().collect();
    let hits = top_k.intersection(&relevant_set).count();
    hits as f32 / relevant.len() as f32
}

/// Precision@K: fraction of top-K results that are relevant.
///
/// Returns 0.0 if k is zero.
pub fn precision_at_k(relevant: &[String], retrieved: &[String], k: usize) -> f32 {
    if k == 0 {
        return 0.0;
    }
    let top_k: HashSet<_> = retrieved.iter().take(k).collect();
    let relevant_set: HashSet<_> = relevant.iter().collect();
    let hits = top_k.intersection(&relevant_set).count();
    hits as f32 / k as f32
}

/// NDCG@K: Normalized Discounted Cumulative Gain.
///
/// Uses binary relevance (1.0 if the document is relevant, 0.0 otherwise).
/// DCG  = sum(rel_i / log2(i + 2)) for i in 0..k
/// IDCG = DCG of the ideal (perfect) ranking
/// NDCG = DCG / IDCG
///
/// Returns 0.0 if there are no relevant documents.
pub fn ndcg_at_k(relevant: &[String], retrieved: &[String], k: usize) -> f32 {
    if relevant.is_empty() {
        return 0.0;
    }

    let relevant_set: HashSet<_> = relevant.iter().collect();

    // Compute DCG for the actual retrieved list.
    let dcg: f64 = retrieved
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, doc)| {
            let rel = if relevant_set.contains(doc) { 1.0 } else { 0.0 };
            rel / ((i as f64) + 2.0).log2()
        })
        .sum();

    // Compute IDCG: the best possible DCG with min(k, |relevant|) hits
    // placed at the top positions.
    let ideal_hits = k.min(relevant.len());
    let idcg: f64 = (0..ideal_hits)
        .map(|i| 1.0 / ((i as f64) + 2.0).log2())
        .sum();

    if idcg == 0.0 {
        return 0.0;
    }

    (dcg / idcg) as f32
}

/// MRR: Mean Reciprocal Rank.
///
/// Returns 1/rank of the first relevant document in the retrieved list,
/// or 0.0 if no relevant document is found.
pub fn mrr(relevant: &[String], retrieved: &[String]) -> f32 {
    let relevant_set: HashSet<_> = relevant.iter().collect();
    for (i, doc) in retrieved.iter().enumerate() {
        if relevant_set.contains(doc) {
            return 1.0 / (i as f32 + 1.0);
        }
    }
    0.0
}

// ---------------------------------------------------------------------------
// Confusion Matrix for Classification Evaluation (RESEARCH.md Section 5.2)
// ---------------------------------------------------------------------------

/// Multi-class confusion matrix that tracks (predicted, actual) counts.
#[derive(Debug, Clone)]
pub struct ConfusionMatrix {
    /// Counts keyed by (predicted, actual).
    pub matrix: HashMap<(String, String), u64>,
    /// The set of all observed class labels, in insertion order.
    pub classes: Vec<String>,
}

impl Default for ConfusionMatrix {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfusionMatrix {
    /// Create an empty confusion matrix.
    pub fn new() -> Self {
        Self {
            matrix: HashMap::new(),
            classes: Vec::new(),
        }
    }

    /// Record a single prediction.
    pub fn record(&mut self, predicted: &str, actual: &str) {
        let key = (predicted.to_string(), actual.to_string());
        *self.matrix.entry(key).or_insert(0) += 1;

        // Track unique classes in insertion order.
        if !self.classes.contains(&predicted.to_string()) {
            self.classes.push(predicted.to_string());
        }
        if !self.classes.contains(&actual.to_string()) {
            self.classes.push(actual.to_string());
        }
    }

    /// True positives for a class: predicted == actual == class.
    fn true_positives(&self, class: &str) -> u64 {
        self.matrix
            .get(&(class.to_string(), class.to_string()))
            .copied()
            .unwrap_or(0)
    }

    /// False positives for a class: predicted == class but actual != class.
    fn false_positives(&self, class: &str) -> u64 {
        self.classes
            .iter()
            .filter(|c| c.as_str() != class)
            .map(|actual| {
                self.matrix
                    .get(&(class.to_string(), actual.to_string()))
                    .copied()
                    .unwrap_or(0)
            })
            .sum()
    }

    /// False negatives for a class: actual == class but predicted != class.
    fn false_negatives(&self, class: &str) -> u64 {
        self.classes
            .iter()
            .filter(|c| c.as_str() != class)
            .map(|predicted| {
                self.matrix
                    .get(&(predicted.to_string(), class.to_string()))
                    .copied()
                    .unwrap_or(0)
            })
            .sum()
    }

    /// Precision for a specific class: TP / (TP + FP).
    pub fn precision(&self, class: &str) -> f32 {
        let tp = self.true_positives(class);
        let fp = self.false_positives(class);
        let denom = tp + fp;
        if denom == 0 {
            return 0.0;
        }
        tp as f32 / denom as f32
    }

    /// Recall for a specific class: TP / (TP + FN).
    pub fn recall(&self, class: &str) -> f32 {
        let tp = self.true_positives(class);
        let fn_ = self.false_negatives(class);
        let denom = tp + fn_;
        if denom == 0 {
            return 0.0;
        }
        tp as f32 / denom as f32
    }

    /// F1 score for a specific class: 2 * (precision * recall) / (precision + recall).
    pub fn f1(&self, class: &str) -> f32 {
        let p = self.precision(class);
        let r = self.recall(class);
        if p + r == 0.0 {
            return 0.0;
        }
        2.0 * p * r / (p + r)
    }

    /// Macro-averaged F1 across all classes.
    pub fn macro_f1(&self) -> f32 {
        if self.classes.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.classes.iter().map(|c| self.f1(c)).sum();
        sum / self.classes.len() as f32
    }

    /// Overall accuracy: total correct / total predictions.
    pub fn accuracy(&self) -> f32 {
        let total: u64 = self.matrix.values().sum();
        if total == 0 {
            return 0.0;
        }
        let correct: u64 = self.classes.iter().map(|c| self.true_positives(c)).sum();
        correct as f32 / total as f32
    }

    /// Total number of recorded predictions.
    pub fn total(&self) -> u64 {
        self.matrix.values().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(val: &str) -> String {
        val.to_string()
    }

    #[test]
    fn test_comb2() {
        assert_eq!(comb2(0), 0.0);
        assert_eq!(comb2(1), 0.0);
        assert_eq!(comb2(2), 1.0);
        assert_eq!(comb2(5), 10.0);
    }

    #[test]
    fn test_ari_identical() {
        let labels = vec![0, 0, 1, 1, 2, 2];
        let ari = adjusted_rand_index(&labels, &labels);
        assert!((ari - 1.0).abs() < 1e-6, "ARI of identical labellings = 1.0, got {ari}");
    }

    #[test]
    fn test_detection_metrics_empty() {
        let m = detection_metrics(&[], &[]);
        assert_eq!(m.precision, 0.0);
        assert_eq!(m.recall, 0.0);
        assert_eq!(m.f1, 0.0);
    }

    #[test]
    fn test_euclidean_distance() {
        let a = vec![0.0, 0.0];
        let b = vec![3.0, 4.0];
        assert!((euclidean_distance(&a, &b) - 5.0).abs() < 1e-6);
    }

    // -- recall_at_k ---------------------------------------------------------

    #[test]
    fn test_recall_at_k_perfect() {
        let relevant = vec![s("a"), s("b"), s("c")];
        let retrieved = vec![s("a"), s("b"), s("c"), s("d"), s("e")];
        assert!((recall_at_k(&relevant, &retrieved, 5) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_recall_at_k_partial() {
        let relevant = vec![s("a"), s("b"), s("c"), s("d")];
        let retrieved = vec![s("a"), s("x"), s("b"), s("y"), s("z")];
        assert!((recall_at_k(&relevant, &retrieved, 5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_recall_at_k_none() {
        let relevant = vec![s("a"), s("b")];
        let retrieved = vec![s("x"), s("y"), s("z")];
        assert!(recall_at_k(&relevant, &retrieved, 3).abs() < 1e-6);
    }

    // -- precision_at_k ------------------------------------------------------

    #[test]
    fn test_precision_at_k() {
        let relevant = vec![s("a"), s("b")];
        let retrieved = vec![s("a"), s("x"), s("b"), s("y"), s("z")];
        assert!((precision_at_k(&relevant, &retrieved, 5) - 0.4).abs() < 1e-6);
    }

    // -- ndcg_at_k -----------------------------------------------------------

    #[test]
    fn test_ndcg_perfect_ranking() {
        let relevant = vec![s("a"), s("b"), s("c")];
        let retrieved = vec![s("a"), s("b"), s("c"), s("d"), s("e")];
        let score = ndcg_at_k(&relevant, &retrieved, 5);
        assert!((score - 1.0).abs() < 1e-5, "Perfect NDCG, got {score}");
    }

    #[test]
    fn test_ndcg_inverted_ranking() {
        let relevant = vec![s("a"), s("b")];
        let retrieved = vec![s("x"), s("y"), s("z"), s("a"), s("b")];
        let score = ndcg_at_k(&relevant, &retrieved, 5);
        assert!(score < 1.0 && score > 0.0, "Inverted NDCG, got {score}");
    }

    // -- mrr -----------------------------------------------------------------

    #[test]
    fn test_mrr_first_result_relevant() {
        let relevant = vec![s("a")];
        let retrieved = vec![s("a"), s("b"), s("c")];
        assert!((mrr(&relevant, &retrieved) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_mrr_no_relevant() {
        let relevant = vec![s("z")];
        let retrieved = vec![s("a"), s("b"), s("c")];
        assert_eq!(mrr(&relevant, &retrieved), 0.0);
    }

    // -- ConfusionMatrix -----------------------------------------------------

    #[test]
    fn test_confusion_matrix_perfect() {
        let mut cm = ConfusionMatrix::new();
        for _ in 0..10 {
            cm.record("A", "A");
            cm.record("B", "B");
        }
        assert!((cm.accuracy() - 1.0).abs() < 1e-6);
        assert!((cm.macro_f1() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_confusion_matrix_single_misclassification() {
        let mut cm = ConfusionMatrix::new();
        cm.record("A", "A");
        cm.record("A", "A");
        cm.record("A", "B"); // FP for A, FN for B
        cm.record("B", "B");

        assert!((cm.precision("A") - 2.0 / 3.0).abs() < 1e-5);
        assert!((cm.recall("A") - 1.0).abs() < 1e-5);
        assert!((cm.precision("B") - 1.0).abs() < 1e-5);
        assert!((cm.recall("B") - 0.5).abs() < 1e-5);
        assert!((cm.accuracy() - 0.75).abs() < 1e-5);
    }

    #[test]
    fn test_confusion_matrix_empty() {
        let cm = ConfusionMatrix::new();
        assert_eq!(cm.accuracy(), 0.0);
        assert_eq!(cm.macro_f1(), 0.0);
        assert_eq!(cm.total(), 0);
    }
}
