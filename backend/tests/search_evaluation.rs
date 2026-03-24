//! Search quality evaluation (docs/research/initial.md Section 5.1).
//!
//! Measures Recall@K, NDCG@K, MRR, Precision@K across different search modes.
//! These tests validate the IR metric implementations and provide a framework
//! for ablation studies comparing FTS-only, vector-only, and hybrid search.

use emailibrium::vectors::metrics::{mrr, ndcg_at_k, precision_at_k, recall_at_k};

fn s(val: &str) -> String {
    val.to_string()
}

// ---------------------------------------------------------------------------
// Recall@K
// ---------------------------------------------------------------------------

#[test]
fn test_recall_at_k_perfect() {
    // All relevant documents appear in top-K.
    let relevant = vec![s("d1"), s("d2"), s("d3")];
    let retrieved = vec![s("d1"), s("d2"), s("d3"), s("d4"), s("d5")];
    let score = recall_at_k(&relevant, &retrieved, 5);
    assert!(
        (score - 1.0).abs() < 1e-6,
        "All relevant in top-K should give recall 1.0, got {score}"
    );
}

#[test]
fn test_recall_at_k_partial() {
    // Only some relevant documents appear in top-K.
    let relevant = vec![s("d1"), s("d2"), s("d3"), s("d4")];
    let retrieved = vec![s("d1"), s("x1"), s("d3"), s("x2"), s("x3")];
    let score = recall_at_k(&relevant, &retrieved, 5);
    // 2 of 4 relevant found
    assert!(
        (score - 0.5).abs() < 1e-6,
        "Expected recall 0.5, got {score}"
    );
}

#[test]
fn test_recall_at_k_none() {
    // No relevant documents in retrieved list.
    let relevant = vec![s("d1"), s("d2")];
    let retrieved = vec![s("x1"), s("x2"), s("x3")];
    let score = recall_at_k(&relevant, &retrieved, 3);
    assert!(
        score.abs() < 1e-6,
        "No relevant in results should give recall 0.0, got {score}"
    );
}

#[test]
fn test_recall_at_k_empty_relevant() {
    let relevant: Vec<String> = vec![];
    let retrieved = vec![s("a"), s("b")];
    assert_eq!(recall_at_k(&relevant, &retrieved, 2), 0.0);
}

#[test]
fn test_recall_at_k_k_smaller_than_results() {
    let relevant = vec![s("d1"), s("d2"), s("d3")];
    // d3 is at position 4, but k=2 so it should not count.
    let retrieved = vec![s("d1"), s("d2"), s("x1"), s("d3")];
    let score = recall_at_k(&relevant, &retrieved, 2);
    // 2 of 3 relevant in top 2
    assert!(
        (score - 2.0 / 3.0).abs() < 1e-5,
        "Expected recall 2/3, got {score}"
    );
}

// ---------------------------------------------------------------------------
// NDCG@K
// ---------------------------------------------------------------------------

#[test]
fn test_ndcg_perfect_ranking() {
    let relevant = vec![s("a"), s("b"), s("c")];
    let retrieved = vec![s("a"), s("b"), s("c"), s("d"), s("e")];
    let score = ndcg_at_k(&relevant, &retrieved, 5);
    assert!(
        (score - 1.0).abs() < 1e-5,
        "Perfect ranking should yield NDCG ~1.0, got {score}"
    );
}

#[test]
fn test_ndcg_inverted_ranking() {
    let relevant = vec![s("a"), s("b")];
    // Relevant documents are pushed to the end.
    let retrieved = vec![s("x"), s("y"), s("z"), s("a"), s("b")];
    let score = ndcg_at_k(&relevant, &retrieved, 5);
    assert!(
        score < 1.0,
        "Inverted ranking should yield NDCG < 1.0, got {score}"
    );
    assert!(
        score > 0.0,
        "Some relevant docs found so NDCG > 0.0, got {score}"
    );
}

#[test]
fn test_ndcg_no_relevant_docs() {
    let relevant: Vec<String> = vec![];
    let retrieved = vec![s("a"), s("b")];
    assert_eq!(ndcg_at_k(&relevant, &retrieved, 2), 0.0);
}

#[test]
fn test_ndcg_single_relevant_at_top() {
    let relevant = vec![s("a")];
    let retrieved = vec![s("a"), s("b"), s("c")];
    let score = ndcg_at_k(&relevant, &retrieved, 3);
    assert!(
        (score - 1.0).abs() < 1e-5,
        "Single relevant at position 1 => NDCG = 1.0, got {score}"
    );
}

// ---------------------------------------------------------------------------
// MRR
// ---------------------------------------------------------------------------

#[test]
fn test_mrr_first_result_relevant() {
    let relevant = vec![s("a")];
    let retrieved = vec![s("a"), s("b"), s("c")];
    let score = mrr(&relevant, &retrieved);
    assert!(
        (score - 1.0).abs() < 1e-6,
        "First result relevant => MRR = 1.0, got {score}"
    );
}

#[test]
fn test_mrr_second_result_relevant() {
    let relevant = vec![s("b")];
    let retrieved = vec![s("a"), s("b"), s("c")];
    let score = mrr(&relevant, &retrieved);
    assert!(
        (score - 0.5).abs() < 1e-6,
        "Second result relevant => MRR = 0.5, got {score}"
    );
}

#[test]
fn test_mrr_no_relevant() {
    let relevant = vec![s("z")];
    let retrieved = vec![s("a"), s("b"), s("c")];
    let score = mrr(&relevant, &retrieved);
    assert!(
        score.abs() < 1e-6,
        "No relevant found => MRR = 0.0, got {score}"
    );
}

#[test]
fn test_mrr_multiple_relevant_returns_first() {
    // MRR cares only about the rank of the first relevant document.
    let relevant = vec![s("b"), s("c")];
    let retrieved = vec![s("a"), s("b"), s("c")];
    let score = mrr(&relevant, &retrieved);
    assert!(
        (score - 0.5).abs() < 1e-6,
        "First relevant is at position 2 => MRR = 0.5, got {score}"
    );
}

// ---------------------------------------------------------------------------
// Precision@K
// ---------------------------------------------------------------------------

#[test]
fn test_precision_at_k() {
    let relevant = vec![s("a"), s("b")];
    let retrieved = vec![s("a"), s("x"), s("b"), s("y"), s("z")];
    let score = precision_at_k(&relevant, &retrieved, 5);
    assert!(
        (score - 0.4).abs() < 1e-6,
        "2 hits in top 5 => precision 0.4, got {score}"
    );
}

#[test]
fn test_precision_at_k_perfect() {
    let relevant = vec![s("a"), s("b"), s("c")];
    let retrieved = vec![s("a"), s("b"), s("c")];
    let score = precision_at_k(&relevant, &retrieved, 3);
    assert!(
        (score - 1.0).abs() < 1e-6,
        "All top-K relevant => precision 1.0, got {score}"
    );
}

#[test]
fn test_precision_at_k_none() {
    let relevant = vec![s("a")];
    let retrieved = vec![s("x"), s("y")];
    let score = precision_at_k(&relevant, &retrieved, 2);
    assert!(
        score.abs() < 1e-6,
        "No hits => precision 0.0, got {score}"
    );
}

// ---------------------------------------------------------------------------
// Ablation: vector-only vs hybrid (synthetic, deterministic test)
// ---------------------------------------------------------------------------

#[test]
fn test_ablation_vector_vs_hybrid() {
    // Simulated ranked lists for a query with known relevant docs.
    let relevant = vec![s("r1"), s("r2"), s("r3")];

    // Vector-only retrieves some relevant and some irrelevant.
    let vector_only = vec![s("r1"), s("x1"), s("r2"), s("x2"), s("x3")];

    // Hybrid search (with keyword boost) retrieves more relevant docs higher.
    let hybrid = vec![s("r1"), s("r2"), s("r3"), s("x1"), s("x2")];

    let recall_vector = recall_at_k(&relevant, &vector_only, 5);
    let recall_hybrid = recall_at_k(&relevant, &hybrid, 5);

    assert!(
        recall_hybrid >= recall_vector,
        "Hybrid recall ({recall_hybrid}) should be >= vector-only recall ({recall_vector})"
    );

    let ndcg_vector = ndcg_at_k(&relevant, &vector_only, 5);
    let ndcg_hybrid = ndcg_at_k(&relevant, &hybrid, 5);

    assert!(
        ndcg_hybrid >= ndcg_vector,
        "Hybrid NDCG ({ndcg_hybrid}) should be >= vector-only NDCG ({ndcg_vector})"
    );
}
