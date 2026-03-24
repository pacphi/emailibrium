# Classification Accuracy Evaluation

**RESEARCH.md Section 5.2**

## Methodology

We evaluate classification accuracy using the `VectorCategorizer` with
seeded centroids and measure performance via a multi-class confusion matrix.

### Test Setup

1. Seed centroids for each `EmailCategory` using representative embeddings
2. Classify a held-out set of labeled test emails
3. Record predictions in a `ConfusionMatrix`
4. Compute per-class and aggregate metrics

### Evaluation Categories

- Work, Personal, Finance, Shopping, Social
- Newsletter, Marketing, Notification, Alerts, Promotions

## Metric Definitions

| Metric | Formula | Description |
|--------|---------|-------------|
| **Precision** | `TP / (TP + FP)` | Of predicted class X, how many are actually X |
| **Recall** | `TP / (TP + FN)` | Of actual class X, how many were predicted as X |
| **F1** | `2 * P * R / (P + R)` | Harmonic mean of precision and recall |
| **Macro-F1** | `mean(F1 per class)` | Unweighted average F1 across all classes |
| **Accuracy** | `correct / total` | Overall fraction of correct predictions |
| **LLM Fallback Rate** | `below_threshold / total` | Fraction requiring LLM classification |

## Expected Results

### Per-Category Metrics

| Category | Precision | Recall | F1 | Support |
|----------|-----------|--------|----|---------|
| Work | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Personal | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Finance | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Shopping | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Social | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Newsletter | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Marketing | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Notification | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Alerts | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Promotions | _TBD_ | _TBD_ | _TBD_ | _TBD_ |

### Aggregate Metrics

| Metric | Value |
|--------|-------|
| Macro-F1 | _TBD_ |
| Accuracy | _TBD_ |
| LLM Fallback Rate | _TBD_ |

> Fill in after running: `cargo test --test classification_evaluation -- --nocapture`

## Ablation Study Design

### Hypothesis

Vector-based centroid classification should achieve macro-F1 >= 0.75 with
an LLM fallback rate below 20%.

### Variables

- **Independent**: Confidence threshold (0.3, 0.5, 0.7, 0.9)
- **Dependent**: Macro-F1, accuracy, LLM fallback rate
- **Controlled**: Centroid seeding method, embedding model, test corpus

### Protocol

1. Seed centroids from labeled training emails (5-10 per category)
2. Classify 200 held-out test emails at each threshold
3. Record confusion matrix and fallback count
4. Plot threshold vs. accuracy and threshold vs. fallback rate

### Expected Outcomes

- Lower thresholds yield fewer fallbacks but potentially lower precision
- Higher thresholds yield higher precision but more LLM fallback
- Optimal threshold balances accuracy with acceptable fallback rate
- EMA centroid updates should improve F1 over time with user feedback
