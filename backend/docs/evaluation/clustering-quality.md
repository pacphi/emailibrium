# Clustering Quality Evaluation

## Methodology

All clustering quality metrics are evaluated using the functions in
`src/vectors/metrics.rs` and exercised by the integration tests in
`tests/clustering_evaluation.rs`.

## Metrics

### Silhouette Coefficient

Measures how well each point fits within its assigned cluster vs. the nearest
alternative cluster.

| Scenario                | Expected Score | Description                       |
| ----------------------- | -------------- | --------------------------------- |
| Well-separated clusters | > 0.9          | Tight, distant clusters           |
| Overlapping clusters    | < 0.5          | Significant inter-cluster overlap |
| Single cluster          | 0.0            | No separation possible            |

Formula per point:

- `a` = mean distance to same-cluster points
- `b` = mean distance to nearest other cluster
- `s(i) = (b - a) / max(a, b)`
- Mean silhouette = average `s(i)` across all points

### Adjusted Rand Index (ARI)

Measures agreement between two clusterings, corrected for chance.

| Scenario                         | Expected ARI | Description               |
| -------------------------------- | ------------ | ------------------------- |
| Identical clusterings            | 1.0          | Perfect agreement         |
| Permuted labels (same structure) | 1.0          | Label-invariant           |
| Random vs. ground truth          | ~0.0         | No better than chance     |
| Partially correct                | 0.0 - 1.0    | Proportional to agreement |

### Subscription Detection (Precision / Recall / F1)

Evaluates the accuracy of subscription and recurring sender detection.

| Scenario                          | Precision | Recall | F1     |
| --------------------------------- | --------- | ------ | ------ |
| Perfect detection                 | 1.0       | 1.0    | 1.0    |
| Conservative (no false positives) | 1.0       | < 1.0  | < 1.0  |
| Aggressive (no false negatives)   | < 1.0     | 1.0    | < 1.0  |
| Balanced real-world               | > 0.85    | > 0.80 | > 0.82 |

## Test Suite

The integration tests in `tests/clustering_evaluation.rs` cover:

1. **test_silhouette_perfect_clusters** - Two well-separated 2D clusters yield score > 0.9
2. **test_silhouette_overlapping_clusters** - Overlapping clusters yield score < 0.5
3. **test_silhouette_single_cluster** - Single cluster yields exactly 0.0
4. **test_adjusted_rand_index_perfect** - Identical labels yield ARI = 1.0
5. **test_adjusted_rand_index_permuted** - Relabelled clusters still yield ARI = 1.0
6. **test_adjusted_rand_index_random** - Dissimilar clusterings yield ARI near 0.0
7. **test_detection_metrics_perfect** - All correct yields P = R = F1 = 1.0
8. **test_detection_metrics_partial** - Some missed yields lower recall
9. **test_subscription_detection_pipeline** - End-to-end with List-Unsubscribe headers

## How to Run

```bash
# Run all clustering evaluation tests
cargo test --test clustering_evaluation

# Run a specific test
cargo test --test clustering_evaluation test_silhouette_perfect_clusters
```
