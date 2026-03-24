# ADR-007: Adaptive Quantization Strategy

- **Status**: Proposed
- **Date**: 2026-03-23
- **Context**: The plan proposes auto-scaling quantization based on corpus size. The research paper validates the recall/compression tradeoffs citing Jegou et al. 2011 and Guo et al. 2020.
- **Decision**: Implement automatic quantization tier selection with background index reconstruction and zero-downtime tier transitions.
- **Consequences**: Auto-scaling provides hands-off memory management. Hysteresis prevents thrashing. Atomic swap ensures zero-downtime during reconstruction. Binary+rerank at the highest tier is complex but necessary at >1M scale.
- **Alternatives Considered**: Fixed fp32 (simple but memory-prohibitive at scale), manual selection (burden on user), compression via dimensionality reduction (loses information).
- **Research References**: Jegou et al. 2011 (product quantization for nearest neighbor search); Guo et al. 2020 (accelerating large-scale inference with quantization).

## Detailed Design

### Quantization Tiers

| Corpus Size | Quantization    | Memory/Vector | Recall vs fp32                  |
| ----------- | --------------- | ------------- | ------------------------------- |
| < 50K       | None (fp32)     | 1,536 bytes   | 100% (baseline)                 |
| 50K-200K    | Scalar (int8)   | 384 bytes     | ~99.5%                          |
| 200K-1M     | Product (PQ)    | ~96 bytes     | ~97-98%                         |
| > 1M        | Binary + rerank | 48 bytes      | ~90% (top-100 reranked to ~97%) |

### Tier Transition Mechanism

1. Monitor email count via SQLite `COUNT(*)`.
2. When count crosses a threshold (with 10% hysteresis to prevent thrashing), schedule background reconstruction.
3. Background job builds the new quantized index alongside the existing one.
4. Atomic swap when the new index is complete; delete old index.
5. During reconstruction, queries continue on the old index (reads are non-blocking).

### Reconstruction Time Estimates

| Scale        | Quantization  | Estimated Time |
| ------------ | ------------- | -------------- |
| 100K vectors | Scalar (int8) | ~30s           |
| 500K vectors | Product (PQ)  | ~5min          |

### Binary + Rerank Strategy

For corpora exceeding 1M vectors:

- Retrieve top-100 candidates using binary codes (fast hamming distance).
- Re-score those 100 candidates against fp32 originals (stored separately) for final top-20 ranking.
- This recovers recall from ~90% (binary alone) to ~97% (after reranking).

### Validation

After each reconstruction, run a benchmark suite of 100 stored queries. Compare recall@10 against the previous tier. If recall drops more than 5%, alert the user and offer rollback.

### Configuration Override

Users can force a specific quantization tier in settings, bypassing automatic selection. This is useful for users who prefer consistent behavior or have specific memory constraints.

## Options Considered

### Option 1: Fixed fp32 (No Quantization)

- **Pros**: Simplest implementation. Maximum recall. No reconstruction overhead.
- **Cons**: Memory-prohibitive at scale. 1M vectors at 384D fp32 = ~1.5GB for vectors alone. Does not scale to large mailboxes.

### Option 2: Manual Tier Selection

- **Pros**: User has full control. No automatic transitions to worry about.
- **Cons**: Requires user to understand quantization tradeoffs. Most users will never change the default. Poor UX for a consumer-facing application.

### Option 3: Dimensionality Reduction (PCA/UMAP)

- **Pros**: Reduces memory by reducing vector dimensions rather than precision.
- **Cons**: Loses semantic information. Requires retraining or re-embedding. Not reversible. Harder to reason about recall impact.

### Option 4: Adaptive Quantization with Background Reconstruction (Selected)

- **Pros**: Hands-off for users. Graceful scaling from small to large corpora. Zero-downtime transitions. Hysteresis prevents oscillation. Validation ensures quality.
- **Cons**: Complex implementation. Multiple code paths for different quantization levels. Binary+rerank requires storing both binary and fp32 copies at the highest tier.
