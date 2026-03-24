//! Criterion benchmarks for Sprint 1 vector operations.
//!
//! Run with: cargo bench --bench vector_benchmarks

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::runtime::Runtime;

use emailibrium::vectors::embedding::{EmbeddingModel, MockEmbeddingModel};
use emailibrium::vectors::store::{InMemoryVectorStore, VectorStoreBackend};
use emailibrium::vectors::types::{SearchParams, VectorCollection, VectorDocument, VectorId};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute cosine similarity between two vectors (mirrors store.rs impl).
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let magnitude = norm_a.sqrt() * norm_b.sqrt();
    if magnitude == 0.0 {
        return 0.0;
    }
    (dot / magnitude) as f32
}

/// Create a deterministic pseudo-random vector of the given dimension.
fn make_vector(seed: u64, dims: usize) -> Vec<f32> {
    const PRIMES: [u64; 8] = [7, 13, 31, 61, 127, 251, 509, 1021];
    let raw: Vec<f32> = (0..dims)
        .map(|i| {
            let prime = PRIMES[i % PRIMES.len()];
            let hash_val = (seed.wrapping_mul(prime).wrapping_add(i as u64)) % 10_000;
            (hash_val as f32 / 10_000.0) * 2.0 - 1.0
        })
        .collect();

    let magnitude = raw.iter().map(|v| v * v).sum::<f32>().sqrt();
    if magnitude == 0.0 {
        return raw;
    }
    raw.into_iter().map(|v| v / magnitude).collect()
}

/// Create a VectorDocument with the given seed and dimensions.
fn make_doc(seed: u64, dims: usize) -> VectorDocument {
    VectorDocument {
        id: VectorId::new(),
        email_id: format!("email-{seed}"),
        vector: make_vector(seed, dims),
        metadata: HashMap::new(),
        collection: VectorCollection::EmailText,
        created_at: Utc::now(),
    }
}

/// Build an InMemoryVectorStore pre-populated with `count` documents.
fn populated_store(rt: &Runtime, count: usize, dims: usize) -> Arc<InMemoryVectorStore> {
    let store = Arc::new(InMemoryVectorStore::new());
    let docs: Vec<VectorDocument> = (0..count).map(|i| make_doc(i as u64, dims)).collect();
    rt.block_on(async {
        store.batch_insert(docs).await.unwrap();
    });
    store
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_cosine_similarity(c: &mut Criterion) {
    let a = make_vector(1, 384);
    let b = make_vector(2, 384);

    c.bench_function("cosine_similarity_384d", |bencher| {
        bencher.iter(|| cosine_similarity(&a, &b));
    });
}

fn bench_embed_single(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let model = MockEmbeddingModel::new(384);

    c.bench_function("embed_single_mock_384d", |bencher| {
        bencher.iter(|| {
            rt.block_on(async {
                model
                    .embed("Hello world, this is a test email")
                    .await
                    .unwrap()
            })
        });
    });
}

fn bench_embed_batch(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let model = MockEmbeddingModel::new(384);
    let texts: Vec<String> = (0..100)
        .map(|i| format!("Email number {i} about various topics and subjects"))
        .collect();

    c.bench_function("embed_batch_100_mock_384d", |bencher| {
        bencher.iter(|| rt.block_on(async { model.embed_batch(&texts).await.unwrap() }));
    });
}

fn bench_store_insert(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("store_insert_single", |bencher| {
        let store = InMemoryVectorStore::new();
        let mut seed = 0u64;
        bencher.iter(|| {
            let doc = make_doc(seed, 384);
            seed += 1;
            rt.block_on(async { store.insert(doc).await.unwrap() })
        });
    });
}

fn bench_store_batch_insert(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("store_batch_insert");

    for &count in &[100, 1000] {
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &count,
            |bencher, &count| {
                bencher.iter(|| {
                    let store = InMemoryVectorStore::new();
                    let docs: Vec<VectorDocument> =
                        (0..count).map(|i| make_doc(i as u64, 384)).collect();
                    rt.block_on(async { store.batch_insert(docs).await.unwrap() })
                });
            },
        );
    }
    group.finish();
}

fn bench_store_search(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let query = make_vector(999_999, 384);
    let mut group = c.benchmark_group("store_search");

    for &count in &[100, 1000, 10_000] {
        let store = populated_store(&rt, count, 384);
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |bencher, _| {
            let params = SearchParams {
                vector: query.clone(),
                limit: 10,
                collection: VectorCollection::EmailText,
                filters: None,
                min_score: None,
            };
            bencher.iter(|| rt.block_on(async { store.search(&params).await.unwrap() }));
        });
    }
    group.finish();
}

fn bench_categorize(c: &mut Criterion) {
    // Simulate categorization: compute cosine similarity against 10 centroids
    // and pick the best match.
    let centroids: Vec<Vec<f32>> = (0..10).map(|i| make_vector(i * 1000, 384)).collect();
    let query = make_vector(42, 384);

    c.bench_function("categorize_10_centroids_384d", |bencher| {
        bencher.iter(|| {
            let mut best_score = f32::MIN;
            let mut _best_idx = 0;
            for (i, centroid) in centroids.iter().enumerate() {
                let score = cosine_similarity(&query, centroid);
                if score > best_score {
                    best_score = score;
                    _best_idx = i;
                }
            }
            best_score
        });
    });
}

// ---------------------------------------------------------------------------
// S7-04: Comprehensive benchmark groups
// ---------------------------------------------------------------------------

fn bench_search_scaling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let query = make_vector(999_999, 384);
    let mut group = c.benchmark_group("search_scaling");

    for &size in &[1_000usize, 10_000, 100_000] {
        let store = populated_store(&rt, size, 384);
        group.sample_size(10);
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |bencher, _| {
            let params = SearchParams {
                vector: query.clone(),
                limit: 10,
                collection: VectorCollection::EmailText,
                filters: None,
                min_score: None,
            };
            bencher.iter(|| rt.block_on(async { store.search(&params).await.unwrap() }));
        });
    }
    group.finish();
}

fn bench_ingestion_pipeline(c: &mut Criterion) {
    use emailibrium::vectors::embedding::EmbeddingModel;

    let rt = Runtime::new().unwrap();
    let model = MockEmbeddingModel::new(384);
    let store = Arc::new(InMemoryVectorStore::new());

    // Simulate end-to-end: embed + store per email (text-only).
    let emails: Vec<String> = (0..100)
        .map(|i| {
            format!(
                "Subject: Test email {i}\nFrom: sender{i}@example.com\n\n\
                 This is the body of test email number {i} about various topics."
            )
        })
        .collect();

    c.bench_function("ingestion_pipeline_100_emails", |bencher| {
        bencher.iter(|| {
            rt.block_on(async {
                for email in &emails {
                    let embedding = model.embed(email).await.unwrap();
                    let doc = VectorDocument {
                        id: VectorId::new(),
                        email_id: format!("bench-email"),
                        vector: embedding,
                        metadata: HashMap::new(),
                        collection: VectorCollection::EmailText,
                        created_at: Utc::now(),
                    };
                    store.insert(doc).await.unwrap();
                }
            })
        });
    });
}

fn bench_quantization_comparison(c: &mut Criterion) {
    use emailibrium::vectors::quantization::{BinaryQuantizer, ScalarQuantizer};

    let mut group = c.benchmark_group("quantization_comparison");
    let count = 10_000usize;
    let dims = 384;

    // Build fp32 vectors.
    let vectors: Vec<Vec<f32>> = (0..count).map(|i| make_vector(i as u64, dims)).collect();
    let query = make_vector(999_999, dims);

    // fp32 brute-force search.
    group.bench_function("fp32_search_10k", |bencher| {
        bencher.iter(|| {
            let mut best_score = f32::MIN;
            for v in &vectors {
                let score = cosine_similarity(&query, v);
                if score > best_score {
                    best_score = score;
                }
            }
            best_score
        });
    });

    // Scalar (int8) quantized search.
    let quantized_vectors: Vec<_> = vectors.iter().map(|v| ScalarQuantizer::quantize(v)).collect();
    let quantized_query = ScalarQuantizer::quantize(&query);

    group.bench_function("scalar_search_10k", |bencher| {
        bencher.iter(|| {
            let mut best_score = f32::MIN;
            for qv in &quantized_vectors {
                let score = ScalarQuantizer::cosine_similarity_quantized(&quantized_query, qv);
                if score > best_score {
                    best_score = score;
                }
            }
            best_score
        });
    });

    // Binary quantized search.
    let binary_vectors: Vec<_> = vectors.iter().map(|v| BinaryQuantizer::quantize(v)).collect();
    let binary_query = BinaryQuantizer::quantize(&query);

    group.bench_function("binary_search_10k", |bencher| {
        bencher.iter(|| {
            let mut best_score = f32::MIN;
            for bv in &binary_vectors {
                let score = BinaryQuantizer::approx_cosine_similarity(&binary_query, bv);
                if score > best_score {
                    best_score = score;
                }
            }
            best_score
        });
    });

    group.finish();
}

fn bench_clustering_scaling(c: &mut Criterion) {
    use emailibrium::vectors::clustering::kmeans;

    let mut group = c.benchmark_group("clustering_scaling");
    let dims = 384;

    for &size in &[100usize, 1_000, 10_000] {
        let data: Vec<Vec<f32>> = (0..size).map(|i| make_vector(i as u64, dims)).collect();
        let k = (size as f64).sqrt().ceil() as usize;
        let k = k.max(2).min(50);

        group.sample_size(10);
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |bencher, _| {
            bencher.iter(|| kmeans(&data, k, 10, 256));
        });
    }
    group.finish();
}

fn bench_memory_profile(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("memory_profile");

    for &count in &[1_000usize, 10_000] {
        group.sample_size(10);
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &count,
            |bencher, &count| {
                bencher.iter(|| {
                    let store = InMemoryVectorStore::new();
                    let docs: Vec<VectorDocument> =
                        (0..count).map(|i| make_doc(i as u64, 384)).collect();
                    rt.block_on(async { store.batch_insert(docs).await.unwrap() });
                    // Returning the store prevents optimizer from eliding.
                    store
                });
            },
        );
    }
    group.finish();
}

fn bench_scalar_quantize_roundtrip(c: &mut Criterion) {
    use emailibrium::vectors::quantization::ScalarQuantizer;

    let vector = make_vector(42, 384);
    c.bench_function("scalar_quantize_roundtrip_384d", |bencher| {
        bencher.iter(|| {
            let q = ScalarQuantizer::quantize(&vector);
            ScalarQuantizer::dequantize(&q)
        });
    });
}

// ---------------------------------------------------------------------------
// Criterion harness
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_cosine_similarity,
    bench_embed_single,
    bench_embed_batch,
    bench_store_insert,
    bench_store_batch_insert,
    bench_store_search,
    bench_categorize,
    bench_search_scaling,
    bench_ingestion_pipeline,
    bench_quantization_comparison,
    bench_clustering_scaling,
    bench_memory_profile,
    bench_scalar_quantize_roundtrip,
);
criterion_main!(benches);
