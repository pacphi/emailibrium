//! Clustering quality evaluation (docs/research/initial.md Section 5.3).
//! Measures silhouette coefficient, ARI, subscription detection precision/recall.

use emailibrium::vectors::metrics::{
    adjusted_rand_index, detection_metrics, euclidean_distance, silhouette_sample,
    silhouette_score, DetectionMetrics,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a cluster of `n` points centered at `center` with small perturbation.
fn make_cluster(center: &[f32], n: usize, spread: f32) -> Vec<Vec<f32>> {
    (0..n)
        .map(|i| {
            center
                .iter()
                .enumerate()
                .map(|(d, &c)| {
                    // Deterministic spread using index and dimension.
                    let offset = ((i * 7 + d * 13) % 100) as f32 / 100.0 - 0.5;
                    c + offset * spread
                })
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Silhouette tests
// ---------------------------------------------------------------------------

#[test]
fn test_silhouette_perfect_clusters() {
    // Two well-separated clusters in 2D.
    let mut data = Vec::new();
    let mut labels = Vec::new();

    // Cluster 0 centered at (0, 0).
    for p in make_cluster(&[0.0, 0.0], 20, 0.1) {
        data.push(p);
        labels.push(0);
    }
    // Cluster 1 centered at (100, 100).
    for p in make_cluster(&[100.0, 100.0], 20, 0.1) {
        data.push(p);
        labels.push(1);
    }

    let score = silhouette_score(&data, &labels, euclidean_distance);
    assert!(
        score > 0.9,
        "Well-separated clusters should have silhouette near 1.0, got {score}"
    );
}

#[test]
fn test_silhouette_overlapping_clusters() {
    // Two overlapping clusters -- should have lower silhouette.
    let mut data = Vec::new();
    let mut labels = Vec::new();

    for p in make_cluster(&[0.0, 0.0], 20, 5.0) {
        data.push(p);
        labels.push(0);
    }
    for p in make_cluster(&[1.0, 1.0], 20, 5.0) {
        data.push(p);
        labels.push(1);
    }

    let score = silhouette_score(&data, &labels, euclidean_distance);
    assert!(
        score < 0.5,
        "Overlapping clusters should have low silhouette, got {score}"
    );
}

#[test]
fn test_silhouette_single_cluster() {
    let data: Vec<Vec<f32>> = make_cluster(&[0.0, 0.0], 10, 1.0);
    let labels: Vec<usize> = vec![0; 10];

    let score = silhouette_score(&data, &labels, euclidean_distance);
    assert!(
        score.abs() < 1e-6,
        "Single cluster should have silhouette = 0.0, got {score}"
    );
}

#[test]
fn test_silhouette_sample_returns_bounded() {
    let mut data = Vec::new();
    let mut labels = Vec::new();

    for p in make_cluster(&[0.0, 0.0], 10, 0.5) {
        data.push(p);
        labels.push(0);
    }
    for p in make_cluster(&[10.0, 10.0], 10, 0.5) {
        data.push(p);
        labels.push(1);
    }

    for i in 0..data.len() {
        let s = silhouette_sample(i, &data, &labels, euclidean_distance);
        assert!(
            (-1.0..=1.0).contains(&s),
            "Silhouette sample must be in [-1, 1], got {s} for point {i}"
        );
    }
}

// ---------------------------------------------------------------------------
// Adjusted Rand Index tests
// ---------------------------------------------------------------------------

#[test]
fn test_adjusted_rand_index_perfect() {
    // Identical clusterings should give ARI = 1.0.
    let labels = vec![0, 0, 0, 1, 1, 1, 2, 2, 2];
    let ari = adjusted_rand_index(&labels, &labels);
    assert!(
        (ari - 1.0).abs() < 1e-5,
        "Perfect match should give ARI = 1.0, got {ari}"
    );
}

#[test]
fn test_adjusted_rand_index_permuted() {
    // Relabelled but structurally identical clusters should still give ARI = 1.0.
    let true_labels = vec![0, 0, 0, 1, 1, 1, 2, 2, 2];
    let pred_labels = vec![2, 2, 2, 0, 0, 0, 1, 1, 1];
    let ari = adjusted_rand_index(&true_labels, &pred_labels);
    assert!(
        (ari - 1.0).abs() < 1e-5,
        "Permuted labels with same structure should give ARI = 1.0, got {ari}"
    );
}

#[test]
fn test_adjusted_rand_index_random() {
    // Assign each point to its own cluster vs. all in one cluster.
    // ARI should be near 0 (or negative).
    let true_labels: Vec<usize> = (0..20).collect();
    let pred_labels: Vec<usize> = vec![0; 20];
    let ari = adjusted_rand_index(&true_labels, &pred_labels);
    assert!(
        ari.abs() < 0.15,
        "Highly dissimilar clusterings should give ARI near 0, got {ari}"
    );
}

#[test]
fn test_adjusted_rand_index_partially_correct() {
    // Partially overlapping clusterings.
    let true_labels = vec![0, 0, 0, 0, 1, 1, 1, 1];
    let pred_labels = vec![0, 0, 1, 1, 1, 1, 0, 0];
    let ari = adjusted_rand_index(&true_labels, &pred_labels);
    // This should be somewhere between -1 and 1 but not 1.0.
    assert!(
        ari < 0.5,
        "Partially wrong clustering should have moderate ARI, got {ari}"
    );
}

// ---------------------------------------------------------------------------
// Detection metrics tests
// ---------------------------------------------------------------------------

#[test]
fn test_detection_metrics_perfect() {
    let predicted = vec!["a@x.com".to_string(), "b@x.com".to_string()];
    let actual = vec!["a@x.com".to_string(), "b@x.com".to_string()];
    let m = detection_metrics(&predicted, &actual);
    assert_eq!(
        m,
        DetectionMetrics {
            precision: 1.0,
            recall: 1.0,
            f1: 1.0
        }
    );
}

#[test]
fn test_detection_metrics_partial() {
    // Predicted 2 out of 3 actual positives + 1 false positive.
    let predicted = vec![
        "a@x.com".to_string(),
        "b@x.com".to_string(),
        "d@x.com".to_string(), // false positive
    ];
    let actual = vec![
        "a@x.com".to_string(),
        "b@x.com".to_string(),
        "c@x.com".to_string(), // missed
    ];
    let m = detection_metrics(&predicted, &actual);
    // tp=2, fp=1, fn=1
    let expected_precision = 2.0 / 3.0;
    let expected_recall = 2.0 / 3.0;
    let expected_f1 =
        2.0 * expected_precision * expected_recall / (expected_precision + expected_recall);

    assert!(
        (m.precision - expected_precision).abs() < 1e-5,
        "precision: expected {expected_precision}, got {}",
        m.precision
    );
    assert!(
        (m.recall - expected_recall).abs() < 1e-5,
        "recall: expected {expected_recall}, got {}",
        m.recall
    );
    assert!(
        (m.f1 - expected_f1).abs() < 1e-5,
        "f1: expected {expected_f1}, got {}",
        m.f1
    );
}

#[test]
fn test_detection_metrics_no_true_positives() {
    let predicted = vec!["x@y.com".to_string()];
    let actual = vec!["a@y.com".to_string()];
    let m = detection_metrics(&predicted, &actual);
    assert_eq!(m.precision, 0.0);
    assert_eq!(m.recall, 0.0);
    assert_eq!(m.f1, 0.0);
}

// ---------------------------------------------------------------------------
// Subscription detection pipeline test
// ---------------------------------------------------------------------------

#[test]
fn test_subscription_detection_pipeline() {
    // Simulate: emails with List-Unsubscribe headers are subscriptions.
    // Our "detector" simply checks for presence of a List-Unsubscribe header.
    struct TestEmail {
        id: String,
        headers: Vec<(String, String)>,
    }

    let emails = [TestEmail {
            id: "email-1".into(),
            headers: vec![
                ("From".into(), "newsletter@example.com".into()),
                (
                    "List-Unsubscribe".into(),
                    "<mailto:unsub@example.com>".into(),
                ),
            ],
        },
        TestEmail {
            id: "email-2".into(),
            headers: vec![
                ("From".into(), "promo@store.com".into()),
                (
                    "List-Unsubscribe".into(),
                    "<https://store.com/unsub>".into(),
                ),
            ],
        },
        TestEmail {
            id: "email-3".into(),
            headers: vec![("From".into(), "friend@gmail.com".into())],
        },
        TestEmail {
            id: "email-4".into(),
            headers: vec![
                ("From".into(), "alerts@bank.com".into()),
                ("List-Unsubscribe".into(), "<mailto:unsub@bank.com>".into()),
            ],
        },
        TestEmail {
            id: "email-5".into(),
            headers: vec![("From".into(), "coworker@company.com".into())],
        }];

    // Ground truth: emails with List-Unsubscribe are actual subscriptions.
    let actual_subs: Vec<String> = emails
        .iter()
        .filter(|e| {
            e.headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("List-Unsubscribe"))
        })
        .map(|e| e.id.clone())
        .collect();

    // Simulated detector: finds emails where From contains known subscription domains.
    let subscription_domains = ["newsletter@", "promo@", "alerts@"];
    let predicted_subs: Vec<String> = emails
        .iter()
        .filter(|e| {
            e.headers
                .iter()
                .any(|(k, v)| k == "From" && subscription_domains.iter().any(|d| v.contains(d)))
        })
        .map(|e| e.id.clone())
        .collect();

    let m = detection_metrics(&predicted_subs, &actual_subs);

    // The detector should have found all 3 subscription emails
    // (newsletter, promo, alerts all match).
    assert!(
        m.precision >= 0.9,
        "Pipeline precision should be high, got {}",
        m.precision
    );
    assert!(
        m.recall >= 0.9,
        "Pipeline recall should be high, got {}",
        m.recall
    );
    assert!(m.f1 >= 0.9, "Pipeline F1 should be high, got {}", m.f1);
}
