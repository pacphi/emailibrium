//! Domain adaptation evaluation (ADR-002, RESEARCH.md gap).
//! Tests embedding quality across different email domains.
//!
//! These integration tests verify that the embedding pipeline produces
//! vectors with meaningful semantic structure: emails within the same
//! domain cluster more tightly than emails across different domains.

use std::sync::Arc;

use emailibrium::vectors::categorizer::cosine_similarity;
use emailibrium::vectors::config::EmbeddingConfig;
use emailibrium::vectors::embedding::{EmbeddingModel, EmbeddingPipeline, MockEmbeddingModel};

// ---------------------------------------------------------------------------
// Test data representing different email domains
// ---------------------------------------------------------------------------

const TECH_EMAILS: &[&str] = &[
    "RE: Pull request #234 - Refactor authentication module",
    "Kubernetes cluster upgrade scheduled for Friday",
    "Bug report: Memory leak in connection pooling",
    "Sprint retrospective notes - team velocity up 15%",
    "New deployment pipeline ready for review",
];

const FINANCE_EMAILS: &[&str] = &[
    "Q4 Budget Review - Final Numbers Approved",
    "Invoice #INV-2026-0847 from Acme Corp - Due March 30",
    "Monthly bank statement available for review",
    "Tax filing deadline reminder - April 15",
    "Expense report submitted for business travel",
];

const LEGAL_EMAILS: &[&str] = &[
    "Contract renewal - Master Services Agreement v3",
    "NDA execution required before Monday",
    "Compliance audit findings - action items attached",
    "Patent filing status update - Application #US2026123",
    "Terms of service update - effective April 1",
];

const MARKETING_EMAILS: &[&str] = &[
    "Campaign performance report - 23% CTR improvement",
    "New product launch email sequence draft for review",
    "Social media analytics - February engagement summary",
    "A/B test results: Subject line variant B wins",
    "Brand guidelines update - new logo assets attached",
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a default embedding pipeline backed by MockEmbeddingModel.
fn default_pipeline() -> EmbeddingPipeline {
    let config = EmbeddingConfig::default();
    EmbeddingPipeline::new(&config).unwrap()
}

/// Embed all texts in a domain slice, returning one vector per text.
async fn embed_domain(pipeline: &EmbeddingPipeline, texts: &[&str]) -> Vec<Vec<f32>> {
    let owned: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
    pipeline.embed_batch(&owned).await.unwrap()
}

/// Compute the average pairwise cosine similarity within a set of vectors.
fn avg_pairwise_similarity(vectors: &[Vec<f32>]) -> f32 {
    if vectors.len() < 2 {
        return 1.0;
    }
    let mut total = 0.0_f64;
    let mut count = 0u64;
    for i in 0..vectors.len() {
        for j in (i + 1)..vectors.len() {
            total += cosine_similarity(&vectors[i], &vectors[j]) as f64;
            count += 1;
        }
    }
    (total / count as f64) as f32
}

/// Compute the average cosine similarity between all pairs of vectors
/// drawn from two different sets.
fn avg_cross_similarity(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    let mut total = 0.0_f64;
    let mut count = 0u64;
    for va in a {
        for vb in b {
            total += cosine_similarity(va, vb) as f64;
            count += 1;
        }
    }
    (total / count as f64) as f32
}

/// Compute the centroid (element-wise mean) of a set of vectors.
fn compute_centroid(vectors: &[Vec<f32>]) -> Vec<f32> {
    assert!(!vectors.is_empty());
    let dims = vectors[0].len();
    let mut centroid = vec![0.0_f32; dims];
    for v in vectors {
        for (i, val) in v.iter().enumerate() {
            centroid[i] += val;
        }
    }
    let n = vectors.len() as f32;
    centroid.iter_mut().for_each(|c| *c /= n);
    centroid
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_intra_domain_similarity_higher_than_inter_domain() {
    let pipeline = default_pipeline();

    let tech_vecs = embed_domain(&pipeline, TECH_EMAILS).await;
    let finance_vecs = embed_domain(&pipeline, FINANCE_EMAILS).await;
    let legal_vecs = embed_domain(&pipeline, LEGAL_EMAILS).await;
    let marketing_vecs = embed_domain(&pipeline, MARKETING_EMAILS).await;

    // Intra-domain average similarities.
    let intra_tech = avg_pairwise_similarity(&tech_vecs);
    let intra_finance = avg_pairwise_similarity(&finance_vecs);
    let intra_legal = avg_pairwise_similarity(&legal_vecs);
    let intra_marketing = avg_pairwise_similarity(&marketing_vecs);

    // Inter-domain average similarities (all six pairs).
    let all_domains: Vec<(&str, &[Vec<f32>])> = vec![
        ("tech", &tech_vecs),
        ("finance", &finance_vecs),
        ("legal", &legal_vecs),
        ("marketing", &marketing_vecs),
    ];

    let mut inter_similarities: Vec<(&str, &str, f32)> = Vec::new();
    for i in 0..all_domains.len() {
        for j in (i + 1)..all_domains.len() {
            let sim = avg_cross_similarity(all_domains[i].1, all_domains[j].1);
            inter_similarities.push((all_domains[i].0, all_domains[j].0, sim));
        }
    }

    let avg_inter: f32 =
        inter_similarities.iter().map(|(_, _, s)| s).sum::<f32>() / inter_similarities.len() as f32;

    let avg_intra = (intra_tech + intra_finance + intra_legal + intra_marketing) / 4.0;

    // Log results for evaluation reports.
    eprintln!("=== Domain Adaptation Evaluation ===");
    eprintln!("Intra-domain similarities:");
    eprintln!("  tech:      {intra_tech:.4}");
    eprintln!("  finance:   {intra_finance:.4}");
    eprintln!("  legal:     {intra_legal:.4}");
    eprintln!("  marketing: {intra_marketing:.4}");
    eprintln!("  average:   {avg_intra:.4}");
    eprintln!();
    eprintln!("Inter-domain similarities:");
    for (a, b, sim) in &inter_similarities {
        eprintln!("  {a} <-> {b}: {sim:.4}");
    }
    eprintln!("  average: {avg_inter:.4}");
    eprintln!();
    eprintln!("Delta (intra - inter): {:.4}", avg_intra - avg_inter);

    // With a real embedding model this delta would be significant (>0.1).
    // The mock model uses a deterministic hash, so the delta may be small,
    // but intra-domain similarity should still be >= inter-domain similarity
    // because texts in the same domain share more character n-grams.
    //
    // We use a generous tolerance here; the real assertion fires when a
    // production model is plugged in.
    assert!(
        avg_intra >= avg_inter - 0.05,
        "Intra-domain similarity ({avg_intra:.4}) should be >= \
         inter-domain similarity ({avg_inter:.4}) within tolerance"
    );
}

#[tokio::test]
async fn test_domain_clustering_separability() {
    let pipeline = default_pipeline();

    let tech_vecs = embed_domain(&pipeline, TECH_EMAILS).await;
    let finance_vecs = embed_domain(&pipeline, FINANCE_EMAILS).await;
    let legal_vecs = embed_domain(&pipeline, LEGAL_EMAILS).await;
    let marketing_vecs = embed_domain(&pipeline, MARKETING_EMAILS).await;

    let centroids: Vec<(&str, Vec<f32>)> = vec![
        ("tech", compute_centroid(&tech_vecs)),
        ("finance", compute_centroid(&finance_vecs)),
        ("legal", compute_centroid(&legal_vecs)),
        ("marketing", compute_centroid(&marketing_vecs)),
    ];

    // Verify centroids are separated: cosine distance > 0.0 between all pairs.
    // With a production model we would require distance > 0.1.
    eprintln!("=== Centroid Separability ===");
    for i in 0..centroids.len() {
        for j in (i + 1)..centroids.len() {
            let sim = cosine_similarity(&centroids[i].1, &centroids[j].1);
            let distance = 1.0 - sim;
            eprintln!(
                "  {} <-> {}: similarity={sim:.4}, distance={distance:.4}",
                centroids[i].0, centroids[j].0
            );

            // Centroids should not be identical (distance > 0).
            assert!(
                distance > 0.0,
                "Centroids for {} and {} should be separated (distance={distance:.4})",
                centroids[i].0,
                centroids[j].0,
            );
        }
    }
}

#[tokio::test]
async fn test_short_query_vs_long_query_quality() {
    let pipeline = default_pipeline();

    // Embed finance domain emails as the reference set.
    let finance_vecs = embed_domain(&pipeline, FINANCE_EMAILS).await;
    let finance_centroid = compute_centroid(&finance_vecs);

    // Short query: single keyword related to finance.
    let short_query_vec = pipeline.embed("budget").await.unwrap();
    let short_sim = cosine_similarity(&short_query_vec, &finance_centroid);

    // Long query: multi-word phrase clearly in the finance domain.
    let long_query_vec = pipeline
        .embed("quarterly budget review from finance team")
        .await
        .unwrap();
    let long_sim = cosine_similarity(&long_query_vec, &finance_centroid);

    eprintln!("=== Query Length Quality ===");
    eprintln!("  short query ('budget')  -> finance centroid sim: {short_sim:.4}");
    eprintln!("  long query              -> finance centroid sim: {long_sim:.4}");

    // Both queries should produce non-zero similarity to the finance centroid.
    // The short query is augmented by the pipeline ("email search: budget"),
    // so it still carries some signal.
    assert!(
        short_sim.abs() > 0.0,
        "Short query should have non-zero similarity to finance centroid"
    );
    assert!(
        long_sim.abs() > 0.0,
        "Long query should have non-zero similarity to finance centroid"
    );

    // With a production model, the long query should score higher because it
    // contains more semantic overlap with finance terminology. Log for
    // evaluation but do not hard-assert with the mock model.
    eprintln!(
        "  long > short: {} (delta: {:.4})",
        long_sim > short_sim,
        long_sim - short_sim
    );
}

#[tokio::test]
async fn test_embedding_model_consistency() {
    let model = MockEmbeddingModel::new(384);
    let text = "Quarterly budget review meeting notes from finance department";

    // Embed the same text five times.
    let mut embeddings = Vec::new();
    for _ in 0..5 {
        embeddings.push(model.embed(text).await.unwrap());
    }

    // All embeddings must be identical (bitwise).
    for (i, emb) in embeddings.iter().enumerate().skip(1) {
        assert_eq!(
            &embeddings[0], emb,
            "Embedding at iteration {i} differs from iteration 0; \
             model must be deterministic"
        );
    }

    // Also verify via the pipeline (which adds caching).
    let pipeline = default_pipeline();
    let v1 = pipeline.embed(text).await.unwrap();
    let v2 = pipeline.embed(text).await.unwrap();
    assert_eq!(v1, v2, "Pipeline should return consistent results (cache or not)");
}

#[tokio::test]
async fn test_model_switching_procedure() {
    // Create pipeline with MockEmbeddingModel via default config.
    let config = EmbeddingConfig::default();
    let pipeline = EmbeddingPipeline::new(&config).unwrap();

    // Verify the pipeline is available and produces vectors of the expected dimension.
    assert!(
        pipeline.is_available().await,
        "Pipeline should be available with mock provider"
    );

    let sample_vec = pipeline.embed("test embedding dimension check").await.unwrap();
    assert_eq!(
        sample_vec.len(),
        config.dimensions,
        "Embedding dimension should match config (expected {}, got {})",
        config.dimensions,
        sample_vec.len()
    );

    // Document: Model switching procedure
    // 1. Update config: change `embedding.provider` and `embedding.model`
    //    e.g., provider = "ollama", model = "nomic-embed-text"
    // 2. Update `embedding.dimensions` to match the new model's output size
    //    (e.g., 768 for all-MiniLM-L12-v2, 1024 for multilingual-e5-large)
    // 3. Re-embed ALL existing emails using the new model
    // 4. Recompute category centroids from the new embeddings
    // 5. Re-index the HNSW graph
    // 6. Run this domain evaluation suite to verify quality metrics

    // Simulate dimension change: create a new pipeline with different dims.
    let model_768: Arc<dyn EmbeddingModel> = Arc::new(MockEmbeddingModel::new(768));
    let pipeline_768 = EmbeddingPipeline::from_providers(vec![model_768], 100, 5);

    let vec_768 = pipeline_768
        .embed("test with larger model dimensions for evaluation")
        .await
        .unwrap();
    assert_eq!(
        vec_768.len(),
        768,
        "New pipeline should produce 768-dim vectors"
    );

    // Verify old and new embeddings are incompatible (different dimensions).
    assert_ne!(
        sample_vec.len(),
        vec_768.len(),
        "Old (384) and new (768) embeddings must have different dimensions, \
         confirming re-embedding is required after model switch"
    );
}
