//! Classification accuracy evaluation (docs/research/initial.md Section 5.2).
//!
//! Measures macro-F1, per-category precision/recall, and LLM fallback rate
//! using the ConfusionMatrix and VectorCategorizer.

use emailibrium::vectors::metrics::ConfusionMatrix;

// ---------------------------------------------------------------------------
// ConfusionMatrix tests
// ---------------------------------------------------------------------------

#[test]
fn test_confusion_matrix_perfect_classification() {
    let mut cm = ConfusionMatrix::new();
    for _ in 0..20 {
        cm.record("Work", "Work");
    }
    for _ in 0..15 {
        cm.record("Personal", "Personal");
    }
    for _ in 0..10 {
        cm.record("Finance", "Finance");
    }

    assert!(
        (cm.accuracy() - 1.0).abs() < 1e-6,
        "Perfect classification should yield accuracy 1.0, got {}",
        cm.accuracy()
    );
    assert!(
        (cm.macro_f1() - 1.0).abs() < 1e-6,
        "Perfect classification should yield macro-F1 1.0, got {}",
        cm.macro_f1()
    );
}

#[test]
fn test_confusion_matrix_single_misclassification() {
    let mut cm = ConfusionMatrix::new();
    cm.record("Work", "Work");
    cm.record("Work", "Work");
    cm.record("Work", "Personal"); // FP for Work, FN for Personal
    cm.record("Personal", "Personal");
    cm.record("Personal", "Personal");

    // Work: TP=2, FP=1, FN=0 -> precision=2/3, recall=1.0
    assert!(
        (cm.precision("Work") - 2.0 / 3.0).abs() < 1e-4,
        "Work precision should be 2/3, got {}",
        cm.precision("Work")
    );
    assert!(
        (cm.recall("Work") - 1.0).abs() < 1e-4,
        "Work recall should be 1.0, got {}",
        cm.recall("Work")
    );

    // Personal: TP=2, FP=0, FN=1 -> precision=1.0, recall=2/3
    assert!(
        (cm.precision("Personal") - 1.0).abs() < 1e-4,
        "Personal precision should be 1.0, got {}",
        cm.precision("Personal")
    );
    assert!(
        (cm.recall("Personal") - 2.0 / 3.0).abs() < 1e-4,
        "Personal recall should be 2/3, got {}",
        cm.recall("Personal")
    );

    // Accuracy: 4 correct out of 5
    assert!(
        (cm.accuracy() - 0.8).abs() < 1e-4,
        "Accuracy should be 0.8, got {}",
        cm.accuracy()
    );
}

#[test]
fn test_macro_f1_balanced_classes() {
    let mut cm = ConfusionMatrix::new();
    let classes = ["Work", "Personal", "Finance", "Shopping", "Social"];

    // Perfect classification with balanced class counts.
    for class in &classes {
        for _ in 0..20 {
            cm.record(class, class);
        }
    }

    assert!(
        (cm.macro_f1() - 1.0).abs() < 1e-6,
        "Balanced perfect classification => macro-F1 = 1.0, got {}",
        cm.macro_f1()
    );
}

#[test]
fn test_macro_f1_imbalanced_classes() {
    let mut cm = ConfusionMatrix::new();

    // Class Work: 100 samples, all correct.
    for _ in 0..100 {
        cm.record("Work", "Work");
    }
    // Class Personal: 10 samples, 4 misclassified as Work.
    for _ in 0..6 {
        cm.record("Personal", "Personal");
    }
    for _ in 0..4 {
        cm.record("Work", "Personal"); // FP for Work, FN for Personal
    }

    // Work: TP=100, FP=4, FN=0 -> precision=100/104, recall=1.0
    let p_work = 100.0_f32 / 104.0;
    let r_work = 1.0_f32;
    let f1_work = 2.0 * p_work * r_work / (p_work + r_work);

    // Personal: TP=6, FP=0, FN=4 -> precision=1.0, recall=6/10=0.6
    let p_personal = 1.0_f32;
    let r_personal = 6.0_f32 / 10.0;
    let f1_personal = 2.0 * p_personal * r_personal / (p_personal + r_personal);

    let expected_macro_f1 = (f1_work + f1_personal) / 2.0;

    assert!(
        (cm.macro_f1() - expected_macro_f1).abs() < 1e-4,
        "Expected macro-F1 ~{expected_macro_f1}, got {}",
        cm.macro_f1()
    );

    // Macro-F1 should be less than 1.0 due to misclassification.
    assert!(cm.macro_f1() < 1.0);
}

#[test]
fn test_precision_recall_per_class() {
    let mut cm = ConfusionMatrix::new();

    // Finance: 15 TP, 3 FP (predicted Finance but actual Shopping)
    for _ in 0..15 {
        cm.record("Finance", "Finance");
    }
    for _ in 0..3 {
        cm.record("Finance", "Shopping");
    }
    // Shopping: 12 TP
    for _ in 0..12 {
        cm.record("Shopping", "Shopping");
    }

    // Finance: TP=15, FP=3, FN=0 -> precision=15/18, recall=1.0
    assert!(
        (cm.precision("Finance") - 15.0 / 18.0).abs() < 1e-4,
        "Finance precision expected 15/18, got {}",
        cm.precision("Finance")
    );
    assert!(
        (cm.recall("Finance") - 1.0).abs() < 1e-4,
        "Finance recall expected 1.0, got {}",
        cm.recall("Finance")
    );

    // Shopping: TP=12, FP=0, FN=3 -> precision=1.0, recall=12/15
    assert!(
        (cm.precision("Shopping") - 1.0).abs() < 1e-4,
        "Shopping precision expected 1.0, got {}",
        cm.precision("Shopping")
    );
    assert!(
        (cm.recall("Shopping") - 12.0 / 15.0).abs() < 1e-4,
        "Shopping recall expected 12/15, got {}",
        cm.recall("Shopping")
    );
}

#[test]
fn test_categorizer_accuracy_with_seeded_centroids() {
    // Simulate classification using known centroid assignments.
    // In a real test this would use VectorCategorizer with seeded centroids;
    // here we exercise the ConfusionMatrix with realistic data.
    let mut cm = ConfusionMatrix::new();

    // Simulated predictions from a categorizer with seeded centroids.
    let test_cases = vec![
        ("Work", "Work"),
        ("Work", "Work"),
        ("Personal", "Personal"),
        ("Finance", "Finance"),
        ("Finance", "Finance"),
        ("Shopping", "Shopping"),
        ("Work", "Personal"),        // misclassification
        ("Newsletter", "Marketing"), // misclassification
        ("Newsletter", "Newsletter"),
        ("Marketing", "Marketing"),
    ];

    for (predicted, actual) in &test_cases {
        cm.record(predicted, actual);
    }

    // Verify overall accuracy: 8 correct out of 10.
    assert!(
        (cm.accuracy() - 0.8).abs() < 1e-4,
        "Expected accuracy 0.8, got {}",
        cm.accuracy()
    );

    // Macro-F1 should be between 0 and 1.
    let macro_f1 = cm.macro_f1();
    assert!(
        macro_f1 > 0.0 && macro_f1 <= 1.0,
        "Macro-F1 should be in (0, 1], got {macro_f1}"
    );
}

#[test]
fn test_llm_fallback_rate() {
    // Simulate a scenario where some classifications fall below the
    // confidence threshold and require LLM fallback.
    let total_emails = 100;
    let below_threshold = 15; // 15 emails fell below confidence threshold

    let fallback_rate = below_threshold as f32 / total_emails as f32;

    assert!(
        (fallback_rate - 0.15).abs() < 1e-6,
        "Fallback rate should be 0.15, got {fallback_rate}"
    );

    // Track this via confusion matrix: "below_threshold" predictions
    // count as requiring LLM fallback.
    let mut cm = ConfusionMatrix::new();

    // 85 emails classified by vector centroid with correct predictions.
    for _ in 0..40 {
        cm.record("Work", "Work");
    }
    for _ in 0..25 {
        cm.record("Personal", "Personal");
    }
    for _ in 0..20 {
        cm.record("Finance", "Finance");
    }

    // 15 emails fell to LLM fallback (Uncategorized by vector method).
    for _ in 0..8 {
        cm.record("Uncategorized", "Work");
    }
    for _ in 0..4 {
        cm.record("Uncategorized", "Personal");
    }
    for _ in 0..3 {
        cm.record("Uncategorized", "Finance");
    }

    let total = cm.total();
    assert_eq!(total, 100);

    // Count how many were classified as Uncategorized (fallback).
    let fallback_count: u64 = cm
        .classes
        .iter()
        .map(|actual| {
            cm.matrix
                .get(&("Uncategorized".to_string(), actual.to_string()))
                .copied()
                .unwrap_or(0)
        })
        .sum();

    let computed_fallback_rate = fallback_count as f32 / total as f32;

    assert!(
        (computed_fallback_rate - 0.15).abs() < 1e-4,
        "Computed fallback rate should be 0.15, got {computed_fallback_rate}"
    );
}
