# Performance Evaluation

## Methodology
- All benchmarks run via Criterion.rs with statistical analysis
- Hardware: [to be filled by runner]
- Rust version: 1.75+ with release profile

## Microbenchmarks

| Operation | Target | Measured | Status |
|-----------|--------|----------|--------|
| Single embed (384D) | < 5ms | TBD | |
| Batch embed (100) | < 50ms | TBD | |
| Vector search (1K) | < 1ms | TBD | |
| Vector search (10K) | < 5ms | TBD | |
| Vector search (100K) | < 10ms | TBD | |
| Cosine similarity (384D) | < 1us | TBD | |
| Categorize (10 centroids) | < 1ms | TBD | |
| Scalar quantize roundtrip | < 1us | TBD | |

## Search Scaling

| Index Size | p50 Target | p95 Target | p99 Target | Measured |
|------------|-----------|-----------|-----------|----------|
| 1K vectors | < 1ms | < 2ms | < 5ms | TBD |
| 10K vectors | < 5ms | < 10ms | < 20ms | TBD |
| 100K vectors | < 10ms | < 25ms | < 50ms | TBD |

## Ingestion Pipeline

| Mode | Target | Measured |
|------|--------|----------|
| Text-only | > 500 emails/sec | TBD |
| Multi-asset | > 50 emails/sec | TBD |

## Memory Profile

| Corpus Size | Target | Measured |
|-------------|--------|----------|
| 10K emails | ~115MB | TBD |
| 100K emails | ~700MB | TBD |

## Quantization Impact

| Tier | Compression | Recall@10 | Search Speed |
|------|-------------|-----------|-------------|
| None (fp32) | 1x | baseline | TBD |
| Scalar (int8) | 4x | ~99.5% | TBD |
| Binary (1-bit) | 32x | ~90% | TBD |

## Clustering Scaling

| Data Size | Target | Measured |
|-----------|--------|----------|
| 100 vectors | < 10ms | TBD |
| 1K vectors | < 100ms | TBD |
| 10K vectors | < 2s | TBD |

## How to Run

```bash
# Run all benchmarks
cargo bench --bench vector_benchmarks

# Run a specific benchmark group
cargo bench --bench vector_benchmarks -- search_scaling
cargo bench --bench vector_benchmarks -- ingestion_pipeline
cargo bench --bench vector_benchmarks -- quantization_comparison
cargo bench --bench vector_benchmarks -- clustering_scaling
```
