# Domain Adaptation Evaluation

Reference: ADR-002 (Pluggable Embedding Model), docs/research/initial.md gap analysis.

## Overview

Emailibrium processes emails from many different domains (tech, finance, legal,
marketing, personal, etc.). The quality of the embedding model directly affects
classification accuracy, search relevance, and clustering coherence. This
document describes the evaluation methodology, baseline metrics, and the
procedure for switching embedding models when domain performance is
unsatisfactory.

## Evaluation Methodology

### Test Corpus

The evaluation uses representative subject lines from four domains:

| Domain    | Sample Size | Example                                             |
| --------- | ----------- | --------------------------------------------------- |
| Tech      | 5           | "RE: Pull request #234 - Refactor auth module"      |
| Finance   | 5           | "Q4 Budget Review - Final Numbers Approved"         |
| Legal     | 5           | "Contract renewal - Master Services Agreement v3"   |
| Marketing | 5           | "Campaign performance report - 23% CTR improvement" |

### Metrics

1. **Intra-domain similarity** -- average pairwise cosine similarity among
   emails within the same domain. Higher is better (emails in the same domain
   should cluster tightly).

2. **Inter-domain similarity** -- average cosine similarity between emails in
   different domains. Lower is better (domains should be well-separated).

3. **Separability delta** -- `intra - inter`. A positive delta indicates the
   model distinguishes domains. Target: delta > 0.10 with a production model.

4. **Centroid distance** -- cosine distance (`1 - similarity`) between domain
   centroids. Target: distance > 0.10 for all pairs.

5. **Query quality** -- cosine similarity between short/long queries and
   domain centroids. Validates that the query augmentation pipeline improves
   short-query retrieval.

### Domain Pairs x Similarity Scores (Baseline -- MockEmbeddingModel)

| Pair                  | Inter-domain Sim | Centroid Distance |
| --------------------- | ---------------- | ----------------- |
| Tech <-> Finance      | ~varies          | > 0.0             |
| Tech <-> Legal        | ~varies          | > 0.0             |
| Tech <-> Marketing    | ~varies          | > 0.0             |
| Finance <-> Legal     | ~varies          | > 0.0             |
| Finance <-> Marketing | ~varies          | > 0.0             |
| Legal <-> Marketing   | ~varies          | > 0.0             |

> Run `cargo test --test domain_evaluation -- --nocapture` to obtain exact
> values for the current embedding model.

## When to Consider Model Switching

Switch the embedding model when any of the following conditions is observed:

1. **Low separability delta** (< 0.05) -- the model cannot distinguish domains.
2. **Poor search recall** -- relevant emails do not appear in top-K results.
3. **Clustering overlap** -- k-means or HNSW produces mixed-domain clusters.
4. **New language requirements** -- the current model does not support the
   user's primary language(s).
5. **Dimension/performance trade-off** -- a smaller model (e.g., 384-dim)
   provides sufficient quality at lower latency.

## Embedding Model Switching Procedure

Changing the embedding model is a **breaking operation** -- old and new
embeddings are incompatible. Follow this procedure:

### 1. Update Configuration

```yaml
# config.yaml
embedding:
  provider: 'ollama' # or "openai", "local-onnx"
  model: 'nomic-embed-text' # new model name
  dimensions: 768 # must match the new model output
```

### 2. Re-embed All Emails

```bash
# Trigger a full re-embedding job (offline or background).
# This re-processes every email through the new model.
cargo run --release -- reindex --full
```

### 3. Recompute Category Centroids

After re-embedding, all category centroids must be recalculated from the new
vectors. The `reindex` job handles this automatically. If running manually:

```rust
// Pseudocode
for category in all_categories {
    let embeddings = store.get_vectors_by_category(category);
    let centroid = compute_centroid(&embeddings);
    categorizer.seed_centroid(category, centroid).await;
}
```

### 4. Rebuild HNSW Index

The HNSW graph structure depends on vector dimensions and distances. After
re-embedding, rebuild the index:

```bash
cargo run --release -- reindex --hnsw-only
```

### 5. Run Evaluation Suite

```bash
cargo test --test domain_evaluation -- --nocapture
```

Verify that the new model meets the quality thresholds defined above.

### 6. Monitor in Production

After deployment, monitor:

- Classification confidence scores (should increase)
- Search result click-through rates
- User feedback on categorization accuracy

## Multilingual Considerations

For users with multilingual inboxes, consider models with strong cross-lingual
transfer:

| Model                 | Dims | Languages | Notes                            |
| --------------------- | ---- | --------- | -------------------------------- |
| all-MiniLM-L6-v2      | 384  | English   | Default. Fast, English-only.     |
| all-MiniLM-L12-v2     | 384  | English   | Higher quality, same dims.       |
| multilingual-e5-large | 1024 | 100+      | Best multilingual quality.       |
| multilingual-e5-base  | 768  | 100+      | Good balance of quality/size.    |
| nomic-embed-text      | 768  | English   | Strong for long documents.       |
| bge-m3                | 1024 | 100+      | SOTA multilingual, dense+sparse. |

### Recommendation

- **English-only users**: Start with `all-MiniLM-L6-v2` (384-dim). Switch to
  `nomic-embed-text` (768-dim) if classification accuracy drops below 85%.
- **Multilingual users**: Use `multilingual-e5-base` (768-dim) as the default.
  Upgrade to `multilingual-e5-large` (1024-dim) or `bge-m3` if quality is
  insufficient for low-resource languages.
- **Latency-sensitive deployments**: Prefer smaller models (384-dim) and enable
  quantization (ADR-007) to reduce memory and search latency.

## Running the Evaluation

```bash
# Full evaluation with output
cargo test --test domain_evaluation -- --nocapture

# Specific test
cargo test --test domain_evaluation test_domain_clustering_separability -- --nocapture
```

Results are printed to stderr. Pipe to a file for archival:

```bash
cargo test --test domain_evaluation -- --nocapture 2> docs/evaluation/results-$(date +%Y%m%d).txt
```
