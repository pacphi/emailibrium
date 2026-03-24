# ADR-002: Embedding Model Selection Strategy

- **Status**: Proposed
- **Date**: 2026-03-23

## Context

Emailibrium requires an embedding model to convert email text and queries into vector representations for semantic search. The model must run locally (no cloud API dependency), produce embeddings fast enough for interactive search (~5ms target), and generate representations that capture email-domain semantics.

The plan selects all-MiniLM-L6-v2 (384 dimensions, 22M parameters) as the primary model. The research paper (docs/research/initial.md) identifies several gaps with this choice: no domain adaptation strategy for email-specific vocabulary, no multilingual support, and known weaknesses with jargon, abbreviations, and short queries common in email search contexts.

A separate model is needed for image embeddings (email attachments, inline images) due to the fundamentally different modality.

## Decision

Use all-MiniLM-L6-v2 as the default text embedding model, with a pluggable model interface that enables future upgrades without schema or index changes. Address known gaps with short-query augmentation and a model evaluation harness.

### Primary Text Model

- **Model**: all-MiniLM-L6-v2
- **Dimensions**: 384
- **Parameters**: 22M
- **Latency**: ~5ms per embedding on modern CPU
- **Quality**: ~95% of larger models on STS benchmarks (Wang et al. 2020)

### Image Model

- **Model**: CLIP ViT-B-32
- **Dimensions**: 512
- **Storage**: Separate HNSW collection due to dimension mismatch with text embeddings
- **Use case**: Searching email attachments and inline images by text description

### Pluggable Interface

```rust
trait EmbeddingModel {
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn dimensions(&self) -> usize;
    fn model_name(&self) -> &str;
}
```

All consumers depend on the trait, never the concrete model. This enables swapping models without cascading changes.

### Gap Mitigations

1. **Short query augmentation**: Queries shorter than 5 tokens are expanded with contextual padding before embedding. For example, a search for "budget" in the context of a Finance folder becomes "budget financial planning email" for embedding purposes. The original query is still used for FTS5.
2. **Model evaluation harness**: A benchmark suite that runs candidate models against a curated set of real email query/document pairs, measuring Recall@10, MRR, and latency. This enables data-driven model upgrades.
3. **Domain adaptation (Phase 3)**: Fine-tune the embedding model on user email data using contrastive learning on implicit feedback (clicked vs. skipped search results). Deferred to Phase 3 because it requires accumulated interaction data.

### Future Multilingual Support

- **Candidate**: multilingual-e5-large (768D, supports 100+ languages)
- **Trigger**: When non-English email volume exceeds 10% of a user's corpus
- **Impact**: Requires separate HNSW collection or re-indexing if dimensions change

## Consequences

### Positive

- 384 dimensions keep storage compact: 100K emails require ~150MB for vectors alone
- 5ms embedding latency fits within the 20ms total search latency budget (ADR-001)
- Pluggable interface prevents vendor/model lock-in
- Short query augmentation addresses the most common failure mode identified in the research
- Evaluation harness provides empirical basis for future model decisions

### Negative

- 384D loses nuance compared to 768D models, particularly for subtle semantic distinctions ("concerned about the timeline" vs. "worried about the schedule" may score similarly to unrelated text)
- No multilingual support at launch -- non-English emails will have degraded search quality
- Pluggable interface adds one layer of abstraction and dynamic dispatch overhead (~1 microsecond, negligible)
- CLIP and MiniLM produce different dimension vectors, requiring separate collections and preventing unified cross-modal search

### Neutral

- Model weights are ~90MB on disk, loaded once at startup and held in memory
- Batch embedding is available but single-query latency is already sufficient for interactive use

## Alternatives Considered

### all-mpnet-base-v2 (768D, 109M parameters)

- **Pros**: Better quality on STS benchmarks, richer semantic representations, widely used
- **Cons**: 4-5x slower (~20-25ms per embedding), exceeds latency budget alone, doubles vector storage cost
- **Verdict**: Could be offered as a "quality mode" behind the pluggable interface for batch operations

### BGE-small-en-v1.5 (384D, 33M parameters)

- **Pros**: Similar size tier, competitive quality, trained with RetroMAE pre-training
- **Cons**: Less community adoption, fewer production deployment reports, similar performance tier to MiniLM
- **Verdict**: Viable alternative, test in evaluation harness when available

### E5-small-v2 (384D)

- **Pros**: Microsoft-backed, newer training methodology, instruction-aware
- **Cons**: Less proven in production at time of evaluation, requires query prefix ("query: ") which adds preprocessing complexity
- **Verdict**: Monitor for maturity, candidate for Phase 2 evaluation

### No embedding model (keyword search only)

- **Pros**: Zero model complexity, no vector storage, simpler architecture
- **Cons**: No semantic search capability, fundamental feature gap, negates the value proposition of intelligent email search

## Research References

- Wang, W., Wei, F., Dong, L., Bao, H., Yang, N., & Zhou, M. (2020). "MiniLM: Deep Self-Attention Distillation for Task-Agnostic Compression of Pre-Trained Transformers." NeurIPS 2020. -- Establishes MiniLM distillation approach and quality-efficiency tradeoff.
- Reimers, N. & Gurevych, I. (2019). "Sentence-BERT: Sentence Embeddings using Siamese BERT-Networks." EMNLP 2019. -- Foundation for the sentence-transformers library and embedding model ecosystem.
- Radford, A., Kim, J. W., Hallacy, C., et al. (2021). "Learning Transferable Visual Models From Natural Language Supervision." ICML 2021. -- CLIP model for cross-modal (text-image) embeddings.
- docs/research/initial.md (Emailibrium internal) -- Identifies domain adaptation gap, multilingual gap, and short-query weakness as key concerns with the selected model.
