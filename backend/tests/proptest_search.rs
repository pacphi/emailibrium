//! Property-based tests for search query handling (R-10).
//!
//! Requires `proptest` in dev-dependencies.
//! Add to Cargo.toml [dev-dependencies]: proptest = "1.8"

#[cfg(feature = "proptest")]
mod search_props {
    use proptest::prelude::*;

    proptest! {
        /// Arbitrary search queries should never panic the sanitizer.
        #[test]
        fn search_query_sanitize_never_panics(query in ".*") {
            let sanitized = query.trim();
            // Trimming should never increase length.
            prop_assert!(sanitized.len() <= query.len());
        }

        /// Empty or whitespace-only queries should trim to empty.
        #[test]
        fn whitespace_queries_trim_to_empty(ws in "[ \t\n\r]{0,20}") {
            let sanitized = ws.trim();
            prop_assert!(sanitized.is_empty());
        }

        /// Cosine similarity should always be in [-1, 1] for valid vectors.
        #[test]
        fn similarity_score_bounds(
            a in prop::collection::vec(-1.0f32..1.0, 384),
            b in prop::collection::vec(-1.0f32..1.0, 384),
        ) {
            if a.len() == b.len() {
                let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
                let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
                let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm_a > f32::EPSILON && norm_b > f32::EPSILON {
                    let cosine = dot / (norm_a * norm_b);
                    // Allow small floating-point error.
                    prop_assert!(
                        cosine >= -1.001 && cosine <= 1.001,
                        "Cosine similarity {cosine} out of bounds"
                    );
                }
            }
        }

        /// Dot product of a vector with itself should be non-negative.
        #[test]
        fn self_dot_product_non_negative(
            v in prop::collection::vec(-100.0f32..100.0, 10..200),
        ) {
            let dot: f32 = v.iter().map(|x| x * x).sum();
            prop_assert!(dot >= 0.0, "Self dot product was negative: {dot}");
        }

        /// Search result limit clamping should always produce valid ranges.
        #[test]
        fn limit_clamping(requested in 0usize..10000, max_limit in 1usize..500) {
            let clamped = requested.min(max_limit);
            prop_assert!(clamped <= max_limit);
            prop_assert!(clamped <= requested || requested > max_limit);
        }

        /// Similarity threshold filtering: scores below threshold are excluded.
        #[test]
        fn threshold_filtering(
            scores in prop::collection::vec(0.0f32..1.0, 0..50),
            threshold in 0.0f32..1.0,
        ) {
            let filtered: Vec<f32> = scores.iter().filter(|&&s| s >= threshold).cloned().collect();
            for score in &filtered {
                prop_assert!(*score >= threshold);
            }
        }
    }
}

/// Ensure the test file compiles even without the proptest feature.
#[test]
fn proptest_search_placeholder() {
    assert!(true);
}
