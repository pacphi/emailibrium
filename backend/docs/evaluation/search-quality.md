# Search Quality Evaluation

**RESEARCH.md Section 5.1**

## Methodology

We evaluate search quality by computing standard information retrieval metrics
across four search modes:

1. **FTS5-only** -- pure keyword (full-text) search
2. **Vector-only** -- pure semantic (embedding similarity) search
3. **Hybrid** -- FTS + vector with Reciprocal Rank Fusion (RRF)
4. **Hybrid+SONA** -- hybrid search with self-optimizing neural adaptation

### Test Corpus

- 100 synthetic emails across 5 categories (Work, Personal, Finance, Shopping, Social)
- 20 emails per category with varied vocabulary and phrasing
- 20 queries with known relevant email IDs (ground truth)

### Evaluation Protocol

For each query:
1. Run the query in all four search modes
2. Collect the top-K results (K = 5, 10, 20)
3. Compute metrics against the ground-truth relevant set

## Metric Definitions

| Metric | Formula | Description |
|--------|---------|-------------|
| **Recall@K** | `\|relevant ∩ top-K\| / \|relevant\|` | Fraction of relevant documents found in top-K |
| **Precision@K** | `\|relevant ∩ top-K\| / K` | Fraction of top-K that are relevant |
| **NDCG@K** | `DCG@K / IDCG@K` | Normalized Discounted Cumulative Gain (rank-aware) |
| **MRR** | `1 / rank(first relevant)` | Mean Reciprocal Rank of first relevant result |

### DCG / IDCG Calculation

- `DCG@K = sum(rel_i / log2(i + 2))` for `i = 0..K`
- `IDCG@K = DCG` of the ideal ranking (all relevant docs at top)
- Binary relevance: `rel_i = 1` if document is relevant, `0` otherwise

## Expected Results

| Mode | Recall@5 | Recall@10 | NDCG@5 | MRR | P@5 |
|------|----------|-----------|--------|-----|-----|
| FTS5-only | _TBD_ | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Vector-only | _TBD_ | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Hybrid (RRF) | _TBD_ | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Hybrid+SONA | _TBD_ | _TBD_ | _TBD_ | _TBD_ | _TBD_ |

> Fill in after running: `cargo test --test search_evaluation -- --nocapture`

## Ablation Study Design

### Hypothesis

Hybrid search (FTS + vector with RRF fusion) should achieve equal or higher
recall and NDCG compared to either modality in isolation.

### Variables

- **Independent**: Search mode (FTS-only, vector-only, hybrid, hybrid+SONA)
- **Dependent**: Recall@K, NDCG@K, MRR, Precision@K
- **Controlled**: Query set, corpus, embedding model, RRF parameter k=60

### Protocol

1. Fix the random seed for reproducibility
2. Run each query across all modes
3. Perform paired comparisons (Wilcoxon signed-rank test if n >= 20)
4. Report mean +/- std for each metric

### Expected Outcomes

- Vector-only should outperform FTS on semantic/paraphrased queries
- FTS should outperform vector on exact keyword matches
- Hybrid should be competitive with or better than the best single modality
- SONA adaptation should improve over baseline hybrid after training
