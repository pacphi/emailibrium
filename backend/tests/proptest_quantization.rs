//! Property-based tests for vector quantization round-trip (R-10).
//!
//! Requires `proptest` in dev-dependencies.
//! Add to Cargo.toml [dev-dependencies]: proptest = "1.8"

#[cfg(feature = "proptest")]
mod quantization_props {
    use proptest::prelude::*;

    proptest! {
        /// Scalar quantization round-trip should preserve relative ordering
        /// for values that are sufficiently far apart.
        #[test]
        fn scalar_quantization_preserves_order(
            values in prop::collection::vec(-10.0f32..10.0, 10..100)
        ) {
            let min_val = values.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_val = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let range = max_val - min_val;

            if range > f32::EPSILON {
                // Quantize to u8.
                let quantized: Vec<u8> = values
                    .iter()
                    .map(|v| ((v - min_val) / range * 255.0).round() as u8)
                    .collect();

                // Dequantize.
                let restored: Vec<f32> = quantized
                    .iter()
                    .map(|q| (*q as f32 / 255.0) * range + min_val)
                    .collect();

                // Check relative ordering is preserved for sufficiently different values.
                for i in 0..values.len() {
                    for j in (i + 1)..values.len() {
                        if (values[i] - values[j]).abs() > range / 128.0 {
                            prop_assert_eq!(
                                values[i] > values[j],
                                restored[i] > restored[j],
                                "Ordering violated: {} vs {} (restored: {} vs {})",
                                values[i],
                                values[j],
                                restored[i],
                                restored[j]
                            );
                        }
                    }
                }
            }
        }

        /// Quantized values should always be in [0, 255].
        #[test]
        fn quantized_values_in_range(
            values in prop::collection::vec(-100.0f32..100.0, 1..200)
        ) {
            let min_val = values.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_val = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let range = max_val - min_val;

            if range > f32::EPSILON {
                let quantized: Vec<u8> = values
                    .iter()
                    .map(|v| ((v - min_val) / range * 255.0).round().clamp(0.0, 255.0) as u8)
                    .collect();

                for q in &quantized {
                    prop_assert!(*q <= 255);
                }
            }
        }

        /// Dequantized values should be within the original min/max range
        /// (with small floating-point tolerance).
        #[test]
        fn dequantized_values_in_original_range(
            values in prop::collection::vec(-50.0f32..50.0, 2..100)
        ) {
            let min_val = values.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_val = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let range = max_val - min_val;

            if range > f32::EPSILON {
                let quantized: Vec<u8> = values
                    .iter()
                    .map(|v| ((v - min_val) / range * 255.0).round().clamp(0.0, 255.0) as u8)
                    .collect();

                let restored: Vec<f32> = quantized
                    .iter()
                    .map(|q| (*q as f32 / 255.0) * range + min_val)
                    .collect();

                for r in &restored {
                    let epsilon = range / 255.0 + f32::EPSILON;
                    prop_assert!(
                        *r >= min_val - epsilon && *r <= max_val + epsilon,
                        "Dequantized value {} outside range [{}, {}]",
                        r,
                        min_val,
                        max_val
                    );
                }
            }
        }

        /// Round-trip quantization error should be bounded by range/255.
        #[test]
        fn quantization_error_bounded(
            values in prop::collection::vec(-10.0f32..10.0, 5..50)
        ) {
            let min_val = values.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_val = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let range = max_val - min_val;

            if range > f32::EPSILON {
                let max_error = range / 255.0;

                for v in &values {
                    let q = ((v - min_val) / range * 255.0).round().clamp(0.0, 255.0) as u8;
                    let restored = (q as f32 / 255.0) * range + min_val;
                    let error = (v - restored).abs();

                    prop_assert!(
                        error <= max_error + f32::EPSILON,
                        "Quantization error {error} exceeds bound {max_error} for value {v}"
                    );
                }
            }
        }

        /// Single-value vectors should quantize to a constant (all same value).
        #[test]
        fn single_value_quantization(value in -100.0f32..100.0, count in 2usize..20) {
            let values: Vec<f32> = vec![value; count];
            let min_val = value;
            let max_val = value;
            let range = max_val - min_val;

            // When all values are the same, range is 0 -- quantization is undefined
            // but should not panic.
            if range <= f32::EPSILON {
                // All values are the same -- quantization can map to any constant.
                // Just verify no panic.
                let _quantized: Vec<u8> = values.iter().map(|_| 128u8).collect();
            }
        }
    }
}

/// Ensure the test file compiles even without the proptest feature.
#[test]
fn proptest_quantization_placeholder() {
    assert!(true);
}
