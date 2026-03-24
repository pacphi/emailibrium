//! Adaptive quantization for memory optimization (ADR-007, S3-07).
//!
//! Provides three quantization tiers that activate automatically based on
//! vector count thresholds, with hysteresis to prevent thrashing near
//! boundaries.

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Quantization tier indicating the current compression level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuantizationTier {
    /// fp32, full precision, 1x size.
    None,
    /// int8 scalar quantization, ~4x compression.
    Scalar,
    /// Product quantization (PQ), ~16x compression.
    Product,
    /// 1-bit binary quantization, ~32x compression.
    Binary,
}

/// Configuration for adaptive quantization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantizationConfig {
    /// Quantization mode: "auto", "none", "scalar", "product", or "binary".
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Vector count threshold to activate scalar quantization.
    #[serde(default = "default_scalar_threshold")]
    pub scalar_threshold: u64,
    /// Vector count threshold to activate product quantization.
    #[serde(default = "default_product_threshold")]
    pub product_threshold: u64,
    /// Vector count threshold to activate binary quantization.
    #[serde(default = "default_binary_threshold")]
    pub binary_threshold: u64,
    /// Hysteresis percentage to prevent thrashing (0.10 = 10%).
    #[serde(default = "default_hysteresis_percent")]
    pub hysteresis_percent: f32,
}

fn default_mode() -> String {
    "auto".to_string()
}
fn default_scalar_threshold() -> u64 {
    50_000
}
fn default_product_threshold() -> u64 {
    200_000
}
fn default_binary_threshold() -> u64 {
    1_000_000
}
fn default_hysteresis_percent() -> f32 {
    0.10
}

impl Default for QuantizationConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            scalar_threshold: default_scalar_threshold(),
            product_threshold: default_product_threshold(),
            binary_threshold: default_binary_threshold(),
            hysteresis_percent: default_hysteresis_percent(),
        }
    }
}

// ---------------------------------------------------------------------------
// Scalar Quantization (int8)
// ---------------------------------------------------------------------------

/// A scalar-quantized vector using int8 per-dimension min-max scaling.
#[derive(Debug, Clone)]
pub struct QuantizedVector {
    /// Quantized int8 values.
    pub data: Vec<i8>,
    /// Minimum value observed across all dimensions (for dequantization).
    pub min_val: f32,
    /// Maximum value observed across all dimensions (for dequantization).
    pub max_val: f32,
    /// Original dimensionality.
    pub original_dims: usize,
}

/// Scalar (int8) quantizer using per-vector min-max scaling.
pub struct ScalarQuantizer;

impl ScalarQuantizer {
    /// Quantize an fp32 vector to int8 using per-vector min-max scaling.
    ///
    /// For each dimension: `int8_val = round((val - min) / (max - min) * 255) - 128`
    pub fn quantize(vector: &[f32]) -> QuantizedVector {
        if vector.is_empty() {
            return QuantizedVector {
                data: Vec::new(),
                min_val: 0.0,
                max_val: 0.0,
                original_dims: 0,
            };
        }

        let min_val = vector.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_val = vector.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let range = max_val - min_val;

        let data: Vec<i8> = if range == 0.0 {
            vec![0i8; vector.len()]
        } else {
            vector
                .iter()
                .map(|&val| {
                    let normalized = (val - min_val) / range * 255.0;
                    (normalized.round() as i16 - 128) as i8
                })
                .collect()
        };

        QuantizedVector {
            data,
            min_val,
            max_val,
            original_dims: vector.len(),
        }
    }

    /// Dequantize int8 back to fp32.
    pub fn dequantize(quantized: &QuantizedVector) -> Vec<f32> {
        let range = quantized.max_val - quantized.min_val;
        if range == 0.0 {
            return vec![quantized.min_val; quantized.original_dims];
        }

        quantized
            .data
            .iter()
            .map(|&val| {
                let normalized = (val as f32 + 128.0) / 255.0;
                normalized * range + quantized.min_val
            })
            .collect()
    }

    /// Compute approximate cosine similarity between two quantized vectors.
    ///
    /// Works directly on int8 values using i32 accumulation to avoid overflow.
    pub fn cosine_similarity_quantized(a: &QuantizedVector, b: &QuantizedVector) -> f32 {
        if a.data.len() != b.data.len() || a.data.is_empty() {
            return 0.0;
        }

        let mut dot: i64 = 0;
        let mut norm_a: i64 = 0;
        let mut norm_b: i64 = 0;

        for (&va, &vb) in a.data.iter().zip(b.data.iter()) {
            let va = va as i64;
            let vb = vb as i64;
            dot += va * vb;
            norm_a += va * va;
            norm_b += vb * vb;
        }

        let magnitude = ((norm_a as f64).sqrt()) * ((norm_b as f64).sqrt());
        if magnitude == 0.0 {
            return 0.0;
        }

        (dot as f64 / magnitude) as f32
    }
}

// ---------------------------------------------------------------------------
// Product Quantization (PQ)
// ---------------------------------------------------------------------------

/// A product-quantized code: one byte per subvector.
#[derive(Debug, Clone)]
pub struct PQCode {
    /// One code per subvector, indexing into the corresponding codebook.
    pub codes: Vec<u8>,
}

/// Product quantizer with per-subvector codebooks trained via K-Means.
pub struct ProductQuantizer {
    /// Number of subvectors the original vector is split into.
    pub num_subvectors: usize,
    /// Number of centroids per codebook (max 256 for u8 codes).
    pub codebook_size: usize,
    /// Codebooks: `[subvector_idx][centroid_idx][subvector_dims]`.
    pub codebooks: Vec<Vec<Vec<f32>>>,
}

impl ProductQuantizer {
    /// Train codebooks using K-Means on the provided training vectors.
    ///
    /// `num_subvectors` must evenly divide the vector dimensionality.
    /// `codebook_size` is typically 256 (8-bit codes).
    pub fn train(vectors: &[Vec<f32>], num_subvectors: usize, codebook_size: usize) -> Self {
        assert!(!vectors.is_empty(), "need at least one training vector");
        let dims = vectors[0].len();
        assert!(
            dims.is_multiple_of(num_subvectors),
            "dimensions must be divisible by num_subvectors"
        );
        let sub_dim = dims / num_subvectors;

        let mut codebooks = Vec::with_capacity(num_subvectors);

        for sub_idx in 0..num_subvectors {
            let start = sub_idx * sub_dim;
            let end = start + sub_dim;

            // Extract subvectors for this position from all training vectors.
            let sub_vectors: Vec<Vec<f32>> =
                vectors.iter().map(|v| v[start..end].to_vec()).collect();

            let codebook = simple_kmeans(&sub_vectors, codebook_size, 20);
            codebooks.push(codebook);
        }

        Self {
            num_subvectors,
            codebook_size,
            codebooks,
        }
    }

    /// Quantize a vector to PQ codes.
    pub fn quantize(&self, vector: &[f32]) -> PQCode {
        let sub_dim = vector.len() / self.num_subvectors;
        let mut codes = Vec::with_capacity(self.num_subvectors);

        for (sub_idx, codebook) in self.codebooks.iter().enumerate() {
            let start = sub_idx * sub_dim;
            let end = start + sub_dim;
            let sub_vec = &vector[start..end];

            let nearest = codebook
                .iter()
                .enumerate()
                .map(|(i, centroid)| {
                    let dist = euclidean_distance_sq(sub_vec, centroid);
                    (i, dist)
                })
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(0);

            codes.push(nearest as u8);
        }

        PQCode { codes }
    }

    /// Compute approximate distance using Asymmetric Distance Computation (ADC).
    ///
    /// Precomputes a distance table for the query, then sums table lookups.
    pub fn distance(&self, query: &[f32], code: &PQCode) -> f32 {
        let sub_dim = query.len() / self.num_subvectors;

        // Precompute distance table: distance from each query subvector to each centroid.
        let distance_table: Vec<Vec<f32>> = self
            .codebooks
            .iter()
            .enumerate()
            .map(|(sub_idx, codebook)| {
                let start = sub_idx * sub_dim;
                let end = start + sub_dim;
                let sub_query = &query[start..end];

                codebook
                    .iter()
                    .map(|centroid| euclidean_distance_sq(sub_query, centroid))
                    .collect()
            })
            .collect();

        // Sum lookups.
        code.codes
            .iter()
            .enumerate()
            .map(|(sub_idx, &c)| distance_table[sub_idx][c as usize])
            .sum()
    }
}

/// Squared Euclidean distance between two slices.
fn euclidean_distance_sq(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| {
            let d = x - y;
            d * d
        })
        .sum()
}

/// Simple K-Means clustering for PQ codebook training.
///
/// Returns `k` centroids. Uses fixed iteration count for simplicity.
fn simple_kmeans(data: &[Vec<f32>], k: usize, max_iters: usize) -> Vec<Vec<f32>> {
    let actual_k = k.min(data.len());
    let dim = data[0].len();

    // Initialize centroids by sampling evenly spaced points.
    let mut centroids: Vec<Vec<f32>> = (0..actual_k)
        .map(|i| {
            let idx = i * data.len() / actual_k;
            data[idx].clone()
        })
        .collect();

    for _ in 0..max_iters {
        // Assign each point to the nearest centroid.
        let mut assignments: Vec<Vec<usize>> = vec![Vec::new(); actual_k];
        for (idx, point) in data.iter().enumerate() {
            let nearest = centroids
                .iter()
                .enumerate()
                .map(|(i, c)| (i, euclidean_distance_sq(point, c)))
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(0);
            assignments[nearest].push(idx);
        }

        // Recompute centroids.
        let mut changed = false;
        for (ci, members) in assignments.iter().enumerate() {
            if members.is_empty() {
                continue;
            }
            let mut new_centroid = vec![0.0f32; dim];
            for &mi in members {
                for (d, val) in data[mi].iter().enumerate() {
                    new_centroid[d] += val;
                }
            }
            let n = members.len() as f32;
            for val in &mut new_centroid {
                *val /= n;
            }
            if new_centroid != centroids[ci] {
                changed = true;
            }
            centroids[ci] = new_centroid;
        }

        if !changed {
            break;
        }
    }

    centroids
}

// ---------------------------------------------------------------------------
// Binary Quantization
// ---------------------------------------------------------------------------

/// A binary-quantized vector: each dimension stored as a single bit.
#[derive(Debug, Clone)]
pub struct BinaryVector {
    /// Packed bits (sign of each dimension).
    pub data: Vec<u8>,
    /// Number of original dimensions.
    pub num_dims: usize,
}

/// Binary (1-bit) quantizer using sign encoding.
pub struct BinaryQuantizer;

impl BinaryQuantizer {
    /// Quantize to binary: `bit[i] = 1` if `vector[i] >= 0`, else `0`.
    pub fn quantize(vector: &[f32]) -> BinaryVector {
        let num_bytes = vector.len().div_ceil(8);
        let mut data = vec![0u8; num_bytes];

        for (i, &val) in vector.iter().enumerate() {
            if val >= 0.0 {
                data[i / 8] |= 1 << (7 - (i % 8));
            }
        }

        BinaryVector {
            data,
            num_dims: vector.len(),
        }
    }

    /// Hamming distance between two binary vectors (XOR + popcount).
    pub fn hamming_distance(a: &BinaryVector, b: &BinaryVector) -> u32 {
        a.data
            .iter()
            .zip(b.data.iter())
            .map(|(&x, &y)| (x ^ y).count_ones())
            .sum()
    }

    /// Approximate cosine similarity from Hamming distance.
    ///
    /// `cos ~= 1.0 - 2.0 * hamming_distance / num_bits`
    pub fn approx_cosine_similarity(a: &BinaryVector, b: &BinaryVector) -> f32 {
        let hamming = Self::hamming_distance(a, b);
        1.0 - 2.0 * hamming as f32 / a.num_dims as f32
    }
}

// ---------------------------------------------------------------------------
// Convenience dispatchers
// ---------------------------------------------------------------------------

/// Quantize an fp32 vector to a compact byte representation based on the tier.
///
/// - `None` → raw little-endian f32 bytes (no compression)
/// - `Scalar` → int8 per dimension + 8 bytes (min, max) header
/// - `Binary` → packed sign bits
///
/// Note: `Product` quantization requires a trained codebook and cannot be used
/// with this stateless dispatcher. Use `ProductQuantizer::train` + `quantize` directly.
pub fn quantize_vector(vector: &[f32], tier: QuantizationTier) -> Vec<u8> {
    match tier {
        QuantizationTier::None => {
            // Raw fp32 little-endian.
            vector.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        QuantizationTier::Scalar => {
            let q = ScalarQuantizer::quantize(vector);
            // Header: min_val (4B) + max_val (4B) + dims (4B), then data.
            let mut bytes = Vec::with_capacity(12 + q.data.len());
            bytes.extend_from_slice(&q.min_val.to_le_bytes());
            bytes.extend_from_slice(&q.max_val.to_le_bytes());
            bytes.extend_from_slice(&(q.original_dims as u32).to_le_bytes());
            bytes.extend(q.data.iter().map(|&v| v as u8));
            bytes
        }
        QuantizationTier::Binary => {
            let bv = BinaryQuantizer::quantize(vector);
            // Header: num_dims (4B), then packed bits.
            let mut bytes = Vec::with_capacity(4 + bv.data.len());
            bytes.extend_from_slice(&(bv.num_dims as u32).to_le_bytes());
            bytes.extend_from_slice(&bv.data);
            bytes
        }
        QuantizationTier::Product => {
            // PQ requires a trained codebook; fall through to raw fp32.
            // Callers needing PQ should use ProductQuantizer directly.
            vector.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
    }
}

/// Dequantize a byte representation back to fp32 based on the tier.
///
/// `dims` is the original vector dimensionality (required for `None` and `Product` tiers).
pub fn dequantize_vector(bytes: &[u8], tier: QuantizationTier, dims: usize) -> Vec<f32> {
    match tier {
        QuantizationTier::None | QuantizationTier::Product => {
            // Raw fp32 little-endian.
            bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect()
        }
        QuantizationTier::Scalar => {
            if bytes.len() < 12 {
                return vec![0.0; dims];
            }
            let min_val = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let max_val = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
            let original_dims =
                u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
            let data: Vec<i8> = bytes[12..].iter().map(|&b| b as i8).collect();
            let q = QuantizedVector {
                data,
                min_val,
                max_val,
                original_dims,
            };
            ScalarQuantizer::dequantize(&q)
        }
        QuantizationTier::Binary => {
            if bytes.len() < 4 {
                return vec![0.0; dims];
            }
            let num_dims = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
            // Binary dequantization: reconstruct sign vector (+1.0 / -1.0).
            let packed = &bytes[4..];
            (0..num_dims)
                .map(|i| {
                    let byte_idx = i / 8;
                    let bit_idx = 7 - (i % 8);
                    if byte_idx < packed.len() && (packed[byte_idx] >> bit_idx) & 1 == 1 {
                        1.0
                    } else {
                        -1.0
                    }
                })
                .collect()
        }
    }
}

// ---------------------------------------------------------------------------
// Quantization Engine (auto-tier selection)
// ---------------------------------------------------------------------------

/// Adaptive quantization engine that selects the compression tier based on
/// the current vector count.
pub struct QuantizationEngine {
    config: QuantizationConfig,
    current_tier: RwLock<QuantizationTier>,
}

impl QuantizationEngine {
    /// Create a new engine with the given configuration.
    pub fn new(config: QuantizationConfig) -> Self {
        let initial_tier = match config.mode.as_str() {
            "none" => QuantizationTier::None,
            "scalar" => QuantizationTier::Scalar,
            "product" => QuantizationTier::Product,
            "binary" => QuantizationTier::Binary,
            _ => QuantizationTier::None, // auto starts at None
        };

        Self {
            config,
            current_tier: RwLock::new(initial_tier),
        }
    }

    /// Determine the recommended tier based on vector count (ignoring hysteresis).
    pub fn recommended_tier(&self, count: u64) -> QuantizationTier {
        if self.config.mode != "auto" {
            return match self.config.mode.as_str() {
                "none" => QuantizationTier::None,
                "scalar" => QuantizationTier::Scalar,
                "product" => QuantizationTier::Product,
                "binary" => QuantizationTier::Binary,
                _ => QuantizationTier::None,
            };
        }

        if count >= self.config.binary_threshold {
            QuantizationTier::Binary
        } else if count >= self.config.product_threshold {
            QuantizationTier::Product
        } else if count >= self.config.scalar_threshold {
            QuantizationTier::Scalar
        } else {
            QuantizationTier::None
        }
    }

    /// Check if a tier transition is needed, accounting for hysteresis.
    ///
    /// Returns `Some(new_tier)` if a transition should occur, `None` otherwise.
    pub async fn check_transition(&self, count: u64) -> Option<QuantizationTier> {
        if self.config.mode != "auto" {
            return None;
        }

        let current = *self.current_tier.read().await;
        let h = self.config.hysteresis_percent;

        let new_tier = self.tier_with_hysteresis(count, current, h);

        if new_tier != current {
            let mut tier = self.current_tier.write().await;
            *tier = new_tier;
            Some(new_tier)
        } else {
            None
        }
    }

    /// Compute the tier accounting for hysteresis relative to the current tier.
    fn tier_with_hysteresis(
        &self,
        count: u64,
        current: QuantizationTier,
        h: f32,
    ) -> QuantizationTier {
        let scalar_up = (self.config.scalar_threshold as f64 * (1.0 + h as f64)) as u64;
        let product_up = (self.config.product_threshold as f64 * (1.0 + h as f64)) as u64;
        let binary_up = (self.config.binary_threshold as f64 * (1.0 + h as f64)) as u64;

        let h64 = (h * 1000.0).round() as f64 / 1000.0; // round to avoid f32->f64 drift
        let scalar_down = (self.config.scalar_threshold as f64 * (1.0 - h64)) as u64;
        let product_down = (self.config.product_threshold as f64 * (1.0 - h64)) as u64;
        let binary_down = (self.config.binary_threshold as f64 * (1.0 - h64)) as u64;

        match current {
            QuantizationTier::None => {
                if count > scalar_up {
                    if count > product_up {
                        if count > binary_up {
                            QuantizationTier::Binary
                        } else {
                            QuantizationTier::Product
                        }
                    } else {
                        QuantizationTier::Scalar
                    }
                } else {
                    QuantizationTier::None
                }
            }
            QuantizationTier::Scalar => {
                if count > product_up {
                    if count > binary_up {
                        QuantizationTier::Binary
                    } else {
                        QuantizationTier::Product
                    }
                } else if count < scalar_down {
                    QuantizationTier::None
                } else {
                    QuantizationTier::Scalar
                }
            }
            QuantizationTier::Product => {
                if count > binary_up {
                    QuantizationTier::Binary
                } else if count < product_down {
                    if count < scalar_down {
                        QuantizationTier::None
                    } else {
                        QuantizationTier::Scalar
                    }
                } else {
                    QuantizationTier::Product
                }
            }
            QuantizationTier::Binary => {
                if count < binary_down {
                    if count < product_down {
                        if count < scalar_down {
                            QuantizationTier::None
                        } else {
                            QuantizationTier::Scalar
                        }
                    } else {
                        QuantizationTier::Product
                    }
                } else {
                    QuantizationTier::Binary
                }
            }
        }
    }

    /// Get the current quantization tier.
    pub async fn current_tier(&self) -> QuantizationTier {
        *self.current_tier.read().await
    }

    /// Return the compression ratio for a given tier.
    pub fn compression_ratio(tier: QuantizationTier) -> f32 {
        match tier {
            QuantizationTier::None => 1.0,
            QuantizationTier::Scalar => 4.0,
            QuantizationTier::Product => 16.0,
            QuantizationTier::Binary => 32.0,
        }
    }

    /// Estimate memory usage in bytes for a given count, dimensionality, and tier.
    pub fn estimate_memory(count: u64, dims: usize, tier: QuantizationTier) -> u64 {
        let per_vector_bytes = match tier {
            QuantizationTier::None => (dims * 4) as u64, // fp32
            QuantizationTier::Scalar => dims as u64 + 8, // int8 + min/max
            QuantizationTier::Product => (dims / 8) as u64 + 8, // PQ codes + overhead
            QuantizationTier::Binary => dims.div_ceil(8) as u64, // packed bits
        };
        // Add 128 bytes metadata overhead per vector (same as InMemoryVectorStore).
        count * (per_vector_bytes + 128)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scalar_quantize_dequantize_roundtrip() {
        let original = vec![0.1, -0.5, 0.9, 0.0, -1.0, 1.0, 0.33, -0.77];
        let quantized = ScalarQuantizer::quantize(&original);
        let recovered = ScalarQuantizer::dequantize(&quantized);

        assert_eq!(recovered.len(), original.len());
        for (o, r) in original.iter().zip(recovered.iter()) {
            assert!(
                (o - r).abs() < 0.02,
                "expected {} ~= {}, diff = {}",
                o,
                r,
                (o - r).abs()
            );
        }
    }

    #[test]
    fn test_scalar_cosine_similarity_approximation() {
        let a = vec![1.0, 0.0, 0.0, 0.5, -0.3, 0.8, 0.1, -0.2];
        let b = vec![0.9, 0.1, -0.1, 0.4, -0.2, 0.7, 0.2, -0.1];

        // Exact cosine similarity.
        let exact = exact_cosine(&a, &b);

        let qa = ScalarQuantizer::quantize(&a);
        let qb = ScalarQuantizer::quantize(&b);
        let approx = ScalarQuantizer::cosine_similarity_quantized(&qa, &qb);

        let error = (exact - approx).abs();
        assert!(
            error < 0.05,
            "quantized cosine {} too far from exact {} (error {})",
            approx,
            exact,
            error
        );
    }

    #[test]
    fn test_scalar_compression_ratio() {
        let dims = 384;
        let original_size = dims * std::mem::size_of::<f32>();
        let original = vec![0.5f32; dims];
        let quantized = ScalarQuantizer::quantize(&original);
        let quantized_size = quantized.data.len() * std::mem::size_of::<i8>();

        let ratio = original_size as f32 / quantized_size as f32;
        assert!(
            ratio >= 3.9 && ratio <= 4.1,
            "expected ~4x compression, got {}x",
            ratio
        );
    }

    #[test]
    fn test_pq_train_and_quantize() {
        let dims = 16;
        let num_subvectors = 4;
        let codebook_size = 4; // small for test

        // Generate some training vectors.
        let vectors: Vec<Vec<f32>> = (0..20)
            .map(|i| (0..dims).map(|d| ((i * d) as f32).sin()).collect())
            .collect();

        let pq = ProductQuantizer::train(&vectors, num_subvectors, codebook_size);
        assert_eq!(pq.codebooks.len(), num_subvectors);
        assert_eq!(pq.codebooks[0].len(), codebook_size);
        assert_eq!(pq.codebooks[0][0].len(), dims / num_subvectors);

        let code = pq.quantize(&vectors[0]);
        assert_eq!(code.codes.len(), num_subvectors);
        for &c in &code.codes {
            assert!((c as usize) < codebook_size);
        }
    }

    #[test]
    fn test_pq_distance_approximation() {
        let dims = 16;
        let num_subvectors = 4;
        let codebook_size = 8;

        let vectors: Vec<Vec<f32>> = (0..50)
            .map(|i| (0..dims).map(|d| ((i * d) as f32 * 0.1).sin()).collect())
            .collect();

        let pq = ProductQuantizer::train(&vectors, num_subvectors, codebook_size);

        let query = &vectors[0];
        let code_self = pq.quantize(query);
        let code_far = pq.quantize(&vectors[25]);

        let dist_self = pq.distance(query, &code_self);
        let dist_far = pq.distance(query, &code_far);

        // Self-distance should be small (near zero).
        assert!(
            dist_self < dist_far,
            "self distance {} should be less than far distance {}",
            dist_self,
            dist_far
        );
    }

    #[test]
    fn test_binary_quantize() {
        let vector = vec![1.0, -0.5, 0.0, -0.1, 0.3, -2.0, 0.7, 0.01];
        let bv = BinaryQuantizer::quantize(&vector);

        assert_eq!(bv.num_dims, 8);
        assert_eq!(bv.data.len(), 1); // 8 bits = 1 byte

        // Expected bits: 1, 0, 1, 0, 1, 0, 1, 1 => 0b10101011 = 0xAB = 171
        assert_eq!(bv.data[0], 0b10101011);
    }

    #[test]
    fn test_binary_hamming_distance() {
        let a = vec![1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
        let b = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

        let ba = BinaryQuantizer::quantize(&a);
        let bb = BinaryQuantizer::quantize(&b);

        // a bits: 10101010, b bits: 11111111 => XOR = 01010101 => popcount = 4
        let dist = BinaryQuantizer::hamming_distance(&ba, &bb);
        assert_eq!(dist, 4);

        // Self-distance should be 0.
        assert_eq!(BinaryQuantizer::hamming_distance(&ba, &ba), 0);
    }

    #[test]
    fn test_binary_approx_cosine() {
        let a = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let b = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

        let ba = BinaryQuantizer::quantize(&a);
        let bb = BinaryQuantizer::quantize(&b);

        let sim = BinaryQuantizer::approx_cosine_similarity(&ba, &bb);
        assert!(
            (sim - 1.0).abs() < 1e-6,
            "identical vectors should have sim ~1.0"
        );

        // Opposite vectors.
        let c = vec![-1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0];
        let bc = BinaryQuantizer::quantize(&c);
        let sim_opp = BinaryQuantizer::approx_cosine_similarity(&ba, &bc);
        assert!(
            (sim_opp - (-1.0)).abs() < 1e-6,
            "opposite vectors should have sim ~-1.0, got {}",
            sim_opp
        );
    }

    #[test]
    fn test_auto_tier_below_scalar() {
        let config = QuantizationConfig::default();
        let engine = QuantizationEngine::new(config);
        assert_eq!(engine.recommended_tier(0), QuantizationTier::None);
        assert_eq!(engine.recommended_tier(49_999), QuantizationTier::None);
    }

    #[test]
    fn test_auto_tier_scalar_range() {
        let config = QuantizationConfig::default();
        let engine = QuantizationEngine::new(config);
        assert_eq!(engine.recommended_tier(50_000), QuantizationTier::Scalar);
        assert_eq!(engine.recommended_tier(100_000), QuantizationTier::Scalar);
        assert_eq!(engine.recommended_tier(199_999), QuantizationTier::Scalar);
    }

    #[test]
    fn test_auto_tier_product_range() {
        let config = QuantizationConfig::default();
        let engine = QuantizationEngine::new(config);
        assert_eq!(engine.recommended_tier(200_000), QuantizationTier::Product);
        assert_eq!(engine.recommended_tier(500_000), QuantizationTier::Product);
        assert_eq!(engine.recommended_tier(999_999), QuantizationTier::Product);
    }

    #[test]
    fn test_auto_tier_binary_range() {
        let config = QuantizationConfig::default();
        let engine = QuantizationEngine::new(config);
        assert_eq!(engine.recommended_tier(1_000_000), QuantizationTier::Binary);
        assert_eq!(engine.recommended_tier(5_000_000), QuantizationTier::Binary);
    }

    #[tokio::test]
    async fn test_auto_tier_hysteresis() {
        let config = QuantizationConfig {
            mode: "auto".to_string(),
            scalar_threshold: 100,
            product_threshold: 200,
            binary_threshold: 1000,
            hysteresis_percent: 0.10,
        };
        let engine = QuantizationEngine::new(config);

        // Start at None. At exactly 100, hysteresis prevents upgrade (need 110).
        assert!(engine.check_transition(100).await.is_none());
        assert_eq!(engine.current_tier().await, QuantizationTier::None);

        // At 110, still None because threshold is > 110 (need > 110).
        assert!(engine.check_transition(110).await.is_none());
        assert_eq!(engine.current_tier().await, QuantizationTier::None);

        // At 111, should transition to Scalar.
        let transition = engine.check_transition(111).await;
        assert_eq!(transition, Some(QuantizationTier::Scalar));
        assert_eq!(engine.current_tier().await, QuantizationTier::Scalar);

        // Now at Scalar, going back to 95 should not downgrade (hysteresis: need < 90).
        assert!(engine.check_transition(95).await.is_none());
        assert_eq!(engine.current_tier().await, QuantizationTier::Scalar);

        // At 89, should downgrade to None.
        let transition = engine.check_transition(89).await;
        assert_eq!(transition, Some(QuantizationTier::None));
        assert_eq!(engine.current_tier().await, QuantizationTier::None);
    }

    #[test]
    fn test_memory_estimation() {
        let dims = 384;
        let count = 10_000;

        let mem_none = QuantizationEngine::estimate_memory(count, dims, QuantizationTier::None);
        let mem_scalar = QuantizationEngine::estimate_memory(count, dims, QuantizationTier::Scalar);
        let mem_product =
            QuantizationEngine::estimate_memory(count, dims, QuantizationTier::Product);
        let mem_binary = QuantizationEngine::estimate_memory(count, dims, QuantizationTier::Binary);

        // Each tier should use less memory than the previous.
        assert!(
            mem_none > mem_scalar,
            "scalar should use less memory than none"
        );
        assert!(
            mem_scalar > mem_product,
            "product should use less memory than scalar"
        );
        assert!(
            mem_product > mem_binary,
            "binary should use less memory than product"
        );

        // Sanity: fp32 memory for 10k vectors of 384 dims.
        let expected_fp32 = 10_000 * (384 * 4 + 128);
        assert_eq!(mem_none, expected_fp32);
    }

    /// Helper: exact cosine similarity for comparison.
    fn exact_cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f64 = a
            .iter()
            .zip(b.iter())
            .map(|(&x, &y)| x as f64 * y as f64)
            .sum();
        let na: f64 = a.iter().map(|&x| (x as f64) * (x as f64)).sum();
        let nb: f64 = b.iter().map(|&x| (x as f64) * (x as f64)).sum();
        let mag = na.sqrt() * nb.sqrt();
        if mag == 0.0 {
            0.0
        } else {
            (dot / mag) as f32
        }
    }
}
