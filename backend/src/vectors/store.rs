//! Vector store backend trait and in-memory implementation.
//!
//! Defines the `VectorStoreBackend` facade trait (ADR-003) and provides
//! an `InMemoryVectorStore` for development, testing, and small deployments.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::error::VectorError;
use super::types::{
    ScoredResult, SearchParams, VectorCollection, VectorDocument, VectorId, VectorStats,
};

/// Facade trait for all vector store backends (ADR-003).
///
/// Implementations must be `Send + Sync` so they can be shared across
/// Axum handlers behind an `Arc<dyn VectorStoreBackend>`.
#[async_trait]
pub trait VectorStoreBackend: Send + Sync {
    /// Insert a single document, returning its assigned ID.
    async fn insert(&self, doc: VectorDocument) -> Result<VectorId, VectorError>;

    /// Insert multiple documents atomically. Validates all documents first;
    /// if any validation fails, none are inserted.
    async fn batch_insert(&self, docs: Vec<VectorDocument>) -> Result<Vec<VectorId>, VectorError>;

    /// Perform a similarity search using the given parameters.
    async fn search(&self, params: &SearchParams) -> Result<Vec<ScoredResult>, VectorError>;

    /// Retrieve a document by its vector ID.
    async fn get(&self, id: &VectorId) -> Result<Option<VectorDocument>, VectorError>;

    /// Retrieve a document by the source email ID.
    async fn get_by_email_id(&self, email_id: &str) -> Result<Option<VectorDocument>, VectorError>;

    /// Delete a document by ID. Returns `true` if the document existed.
    async fn delete(&self, id: &VectorId) -> Result<bool, VectorError>;

    /// Update an existing document in place. Fails if the document does not exist.
    async fn update(&self, doc: VectorDocument) -> Result<(), VectorError>;

    /// Check if the store is healthy and operational.
    async fn health(&self) -> Result<bool, VectorError>;

    /// Return statistics about the store contents.
    async fn stats(&self) -> Result<VectorStats, VectorError>;

    /// Return the total number of stored documents.
    async fn count(&self) -> Result<u64, VectorError>;

    /// List documents belonging to a specific collection with pagination.
    async fn list_by_collection(
        &self,
        collection: &VectorCollection,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<VectorDocument>, VectorError>;
}

/// Thread-safe in-memory vector store backed by a `HashMap` behind an `RwLock`.
///
/// Search uses brute-force cosine similarity (O(n) scan), which is acceptable
/// for small-to-medium datasets during development and testing.
pub struct InMemoryVectorStore {
    documents: Arc<RwLock<HashMap<VectorId, VectorDocument>>>,
}

impl InMemoryVectorStore {
    pub fn new() -> Self {
        Self {
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryVectorStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute cosine similarity between two vectors.
///
/// Returns 0.0 if either vector has zero magnitude to avoid division by zero.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

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

    let magnitude = (norm_a.sqrt()) * (norm_b.sqrt());
    if magnitude == 0.0 {
        return 0.0;
    }

    (dot / magnitude) as f32
}

/// Check whether a document's metadata matches all required filter key-value pairs.
fn metadata_matches(doc: &VectorDocument, filters: &HashMap<String, String>) -> bool {
    filters
        .iter()
        .all(|(key, value)| doc.metadata.get(key) == Some(value))
}

#[async_trait]
impl VectorStoreBackend for InMemoryVectorStore {
    async fn insert(&self, doc: VectorDocument) -> Result<VectorId, VectorError> {
        if doc.vector.is_empty() {
            return Err(VectorError::StoreFailed(
                "vector must not be empty".to_string(),
            ));
        }
        let id = doc.id.clone();
        let mut store = self.documents.write().await;
        store.insert(id.clone(), doc);
        Ok(id)
    }

    async fn batch_insert(&self, docs: Vec<VectorDocument>) -> Result<Vec<VectorId>, VectorError> {
        // Validate all documents before inserting any (atomic-ish semantics).
        for (i, doc) in docs.iter().enumerate() {
            if doc.vector.is_empty() {
                return Err(VectorError::StoreFailed(format!(
                    "document at index {} has an empty vector",
                    i
                )));
            }
        }

        let mut store = self.documents.write().await;
        let ids: Vec<VectorId> = docs
            .into_iter()
            .map(|doc| {
                let id = doc.id.clone();
                store.insert(id.clone(), doc);
                id
            })
            .collect();

        Ok(ids)
    }

    async fn search(&self, params: &SearchParams) -> Result<Vec<ScoredResult>, VectorError> {
        if params.vector.is_empty() {
            return Err(VectorError::StoreFailed(
                "query vector must not be empty".to_string(),
            ));
        }

        let store = self.documents.read().await;

        let mut results: Vec<ScoredResult> = store
            .values()
            .filter(|doc| {
                // Filter by collection
                if doc.collection != params.collection {
                    return false;
                }
                // Filter by metadata
                if let Some(ref filters) = params.filters {
                    if !metadata_matches(doc, filters) {
                        return false;
                    }
                }
                true
            })
            .filter_map(|doc| {
                let score = cosine_similarity(&params.vector, &doc.vector);

                // Filter by minimum score threshold
                if let Some(min_score) = params.min_score {
                    if score < min_score {
                        return None;
                    }
                }

                Some(ScoredResult {
                    document: doc.clone(),
                    score,
                })
            })
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Apply limit
        results.truncate(params.limit);

        Ok(results)
    }

    async fn get(&self, id: &VectorId) -> Result<Option<VectorDocument>, VectorError> {
        let store = self.documents.read().await;
        Ok(store.get(id).cloned())
    }

    async fn get_by_email_id(&self, email_id: &str) -> Result<Option<VectorDocument>, VectorError> {
        let store = self.documents.read().await;
        Ok(store.values().find(|doc| doc.email_id == email_id).cloned())
    }

    async fn delete(&self, id: &VectorId) -> Result<bool, VectorError> {
        let mut store = self.documents.write().await;
        Ok(store.remove(id).is_some())
    }

    async fn update(&self, doc: VectorDocument) -> Result<(), VectorError> {
        if doc.vector.is_empty() {
            return Err(VectorError::StoreFailed(
                "vector must not be empty".to_string(),
            ));
        }
        let mut store = self.documents.write().await;
        if !store.contains_key(&doc.id) {
            return Err(VectorError::NotFound(format!(
                "vector {} does not exist",
                doc.id
            )));
        }
        store.insert(doc.id.clone(), doc);
        Ok(())
    }

    async fn health(&self) -> Result<bool, VectorError> {
        // The in-memory store is always healthy if it can acquire the lock.
        let _store = self.documents.read().await;
        Ok(true)
    }

    async fn stats(&self) -> Result<VectorStats, VectorError> {
        let store = self.documents.read().await;

        let total_vectors = store.len() as u64;

        // Count per collection
        let mut collections: HashMap<String, u64> = HashMap::new();
        let mut dimensions: usize = 0;

        for doc in store.values() {
            *collections.entry(doc.collection.to_string()).or_insert(0) += 1;
            if dimensions == 0 && !doc.vector.is_empty() {
                dimensions = doc.vector.len();
            }
        }

        // Memory estimation: vectors * dimensions * 4 bytes (f32) + metadata overhead
        // Metadata overhead: ~128 bytes per document for HashMap, String allocations, etc.
        let vector_bytes = total_vectors * (dimensions as u64) * 4;
        let metadata_overhead = total_vectors * 128;
        let memory_bytes = vector_bytes + metadata_overhead;

        Ok(VectorStats {
            total_vectors,
            collections,
            dimensions,
            memory_bytes,
            index_type: "in_memory_brute_force".to_string(),
        })
    }

    async fn count(&self) -> Result<u64, VectorError> {
        let store = self.documents.read().await;
        Ok(store.len() as u64)
    }

    async fn list_by_collection(
        &self,
        collection: &VectorCollection,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<VectorDocument>, VectorError> {
        let store = self.documents.read().await;

        let docs: Vec<VectorDocument> = store
            .values()
            .filter(|doc| doc.collection == *collection)
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();

        Ok(docs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    /// Helper to create a test document with a given vector and collection.
    fn make_doc(
        email_id: &str,
        vector: Vec<f32>,
        collection: VectorCollection,
        metadata: HashMap<String, String>,
    ) -> VectorDocument {
        VectorDocument {
            id: VectorId::new(),
            email_id: email_id.to_string(),
            vector,
            metadata,
            collection,
            created_at: Utc::now(),
        }
    }

    fn empty_metadata() -> HashMap<String, String> {
        HashMap::new()
    }

    #[tokio::test]
    async fn test_insert_and_get() {
        let store = InMemoryVectorStore::new();
        let doc = make_doc(
            "email-1",
            vec![1.0, 0.0, 0.0],
            VectorCollection::EmailText,
            empty_metadata(),
        );
        let id = doc.id.clone();

        let returned_id = store.insert(doc).await.unwrap();
        assert_eq!(returned_id, id);

        let retrieved = store.get(&id).await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.email_id, "email-1");
        assert_eq!(retrieved.vector, vec![1.0, 0.0, 0.0]);

        // Getting a nonexistent ID returns None
        let missing = store.get(&VectorId::new()).await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_insert_empty_vector_fails() {
        let store = InMemoryVectorStore::new();
        let doc = make_doc(
            "email-1",
            vec![],
            VectorCollection::EmailText,
            empty_metadata(),
        );
        let result = store.insert(doc).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_batch_insert() {
        let store = InMemoryVectorStore::new();
        let docs = vec![
            make_doc(
                "email-1",
                vec![1.0, 0.0],
                VectorCollection::EmailText,
                empty_metadata(),
            ),
            make_doc(
                "email-2",
                vec![0.0, 1.0],
                VectorCollection::EmailText,
                empty_metadata(),
            ),
            make_doc(
                "email-3",
                vec![1.0, 1.0],
                VectorCollection::ImageText,
                empty_metadata(),
            ),
        ];

        let ids = store.batch_insert(docs).await.unwrap();
        assert_eq!(ids.len(), 3);

        let count = store.count().await.unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_batch_insert_atomic_validation() {
        let store = InMemoryVectorStore::new();
        let docs = vec![
            make_doc(
                "email-1",
                vec![1.0, 0.0],
                VectorCollection::EmailText,
                empty_metadata(),
            ),
            make_doc(
                "email-2",
                vec![],
                VectorCollection::EmailText,
                empty_metadata(),
            ), // invalid
            make_doc(
                "email-3",
                vec![1.0, 1.0],
                VectorCollection::EmailText,
                empty_metadata(),
            ),
        ];

        let result = store.batch_insert(docs).await;
        assert!(result.is_err());

        // None should have been inserted
        let count = store.count().await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_search_cosine_similarity() {
        let store = InMemoryVectorStore::new();

        // Insert vectors with known directions
        // doc_a points along x-axis: [1, 0, 0]
        // doc_b points along y-axis: [0, 1, 0]
        // doc_c points at 45 degrees: [1, 1, 0] (normalized ~[0.707, 0.707, 0])
        let doc_a = make_doc(
            "a",
            vec![1.0, 0.0, 0.0],
            VectorCollection::EmailText,
            empty_metadata(),
        );
        let doc_b = make_doc(
            "b",
            vec![0.0, 1.0, 0.0],
            VectorCollection::EmailText,
            empty_metadata(),
        );
        let doc_c = make_doc(
            "c",
            vec![1.0, 1.0, 0.0],
            VectorCollection::EmailText,
            empty_metadata(),
        );

        store.insert(doc_a).await.unwrap();
        store.insert(doc_b).await.unwrap();
        store.insert(doc_c).await.unwrap();

        // Query along x-axis: doc_a should be most similar, then doc_c, then doc_b
        let params = SearchParams {
            vector: vec![1.0, 0.0, 0.0],
            limit: 10,
            collection: VectorCollection::EmailText,
            filters: None,
            min_score: None,
        };

        let results = store.search(&params).await.unwrap();
        assert_eq!(results.len(), 3);

        // First result should be doc_a with score ~1.0
        assert_eq!(results[0].document.email_id, "a");
        assert!((results[0].score - 1.0).abs() < 1e-6);

        // Second result should be doc_c with score ~0.707
        assert_eq!(results[1].document.email_id, "c");
        assert!((results[1].score - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);

        // Third result should be doc_b with score ~0.0
        assert_eq!(results[2].document.email_id, "b");
        assert!(results[2].score.abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_search_with_filters() {
        let store = InMemoryVectorStore::new();

        let mut meta_work = HashMap::new();
        meta_work.insert("category".to_string(), "work".to_string());

        let mut meta_personal = HashMap::new();
        meta_personal.insert("category".to_string(), "personal".to_string());

        store
            .insert(make_doc(
                "e1",
                vec![1.0, 0.0],
                VectorCollection::EmailText,
                meta_work.clone(),
            ))
            .await
            .unwrap();
        store
            .insert(make_doc(
                "e2",
                vec![0.9, 0.1],
                VectorCollection::EmailText,
                meta_personal.clone(),
            ))
            .await
            .unwrap();
        store
            .insert(make_doc(
                "e3",
                vec![0.8, 0.2],
                VectorCollection::EmailText,
                meta_work.clone(),
            ))
            .await
            .unwrap();

        let mut filters = HashMap::new();
        filters.insert("category".to_string(), "work".to_string());

        let params = SearchParams {
            vector: vec![1.0, 0.0],
            limit: 10,
            collection: VectorCollection::EmailText,
            filters: Some(filters),
            min_score: None,
        };

        let results = store.search(&params).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results
            .iter()
            .all(|r| r.document.metadata.get("category") == Some(&"work".to_string())));
    }

    #[tokio::test]
    async fn test_search_min_score_threshold() {
        let store = InMemoryVectorStore::new();

        store
            .insert(make_doc(
                "a",
                vec![1.0, 0.0, 0.0],
                VectorCollection::EmailText,
                empty_metadata(),
            ))
            .await
            .unwrap();
        store
            .insert(make_doc(
                "b",
                vec![0.0, 1.0, 0.0],
                VectorCollection::EmailText,
                empty_metadata(),
            ))
            .await
            .unwrap();
        store
            .insert(make_doc(
                "c",
                vec![0.9, 0.1, 0.0],
                VectorCollection::EmailText,
                empty_metadata(),
            ))
            .await
            .unwrap();

        let params = SearchParams {
            vector: vec![1.0, 0.0, 0.0],
            limit: 10,
            collection: VectorCollection::EmailText,
            filters: None,
            min_score: Some(0.5),
        };

        let results = store.search(&params).await.unwrap();
        // doc_a (score ~1.0) and doc_c (score ~0.995) should pass; doc_b (score ~0.0) should not
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.score >= 0.5));
    }

    #[tokio::test]
    async fn test_search_collection_filter() {
        let store = InMemoryVectorStore::new();

        store
            .insert(make_doc(
                "e1",
                vec![1.0, 0.0],
                VectorCollection::EmailText,
                empty_metadata(),
            ))
            .await
            .unwrap();
        store
            .insert(make_doc(
                "e2",
                vec![1.0, 0.0],
                VectorCollection::ImageText,
                empty_metadata(),
            ))
            .await
            .unwrap();
        store
            .insert(make_doc(
                "e3",
                vec![0.9, 0.1],
                VectorCollection::EmailText,
                empty_metadata(),
            ))
            .await
            .unwrap();

        let params = SearchParams {
            vector: vec![1.0, 0.0],
            limit: 10,
            collection: VectorCollection::ImageText,
            filters: None,
            min_score: None,
        };

        let results = store.search(&params).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].document.email_id, "e2");
    }

    #[tokio::test]
    async fn test_delete() {
        let store = InMemoryVectorStore::new();
        let doc = make_doc(
            "email-1",
            vec![1.0, 0.0],
            VectorCollection::EmailText,
            empty_metadata(),
        );
        let id = doc.id.clone();

        store.insert(doc).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 1);

        let deleted = store.delete(&id).await.unwrap();
        assert!(deleted);
        assert_eq!(store.count().await.unwrap(), 0);

        // Deleting again returns false
        let deleted_again = store.delete(&id).await.unwrap();
        assert!(!deleted_again);
    }

    #[tokio::test]
    async fn test_update() {
        let store = InMemoryVectorStore::new();
        let doc = make_doc(
            "email-1",
            vec![1.0, 0.0],
            VectorCollection::EmailText,
            empty_metadata(),
        );
        let id = doc.id.clone();

        store.insert(doc).await.unwrap();

        // Update with new vector
        let updated = VectorDocument {
            id: id.clone(),
            email_id: "email-1".to_string(),
            vector: vec![0.0, 1.0],
            metadata: empty_metadata(),
            collection: VectorCollection::EmailText,
            created_at: Utc::now(),
        };

        store.update(updated).await.unwrap();

        let retrieved = store.get(&id).await.unwrap().unwrap();
        assert_eq!(retrieved.vector, vec![0.0, 1.0]);
    }

    #[tokio::test]
    async fn test_update_nonexistent_fails() {
        let store = InMemoryVectorStore::new();
        let doc = make_doc(
            "email-1",
            vec![1.0, 0.0],
            VectorCollection::EmailText,
            empty_metadata(),
        );
        let result = store.update(doc).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_by_email_id() {
        let store = InMemoryVectorStore::new();
        let doc = make_doc(
            "unique-email-42",
            vec![1.0, 0.0, 0.0],
            VectorCollection::EmailText,
            empty_metadata(),
        );

        store.insert(doc).await.unwrap();

        let found = store.get_by_email_id("unique-email-42").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().email_id, "unique-email-42");

        let missing = store.get_by_email_id("nonexistent").await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_stats() {
        let store = InMemoryVectorStore::new();

        // Empty store stats
        let stats = store.stats().await.unwrap();
        assert_eq!(stats.total_vectors, 0);
        assert!(stats.collections.is_empty());
        assert_eq!(stats.dimensions, 0);
        assert_eq!(stats.memory_bytes, 0);
        assert_eq!(stats.index_type, "in_memory_brute_force");

        // Insert some documents
        store
            .insert(make_doc(
                "e1",
                vec![1.0, 0.0, 0.0],
                VectorCollection::EmailText,
                empty_metadata(),
            ))
            .await
            .unwrap();
        store
            .insert(make_doc(
                "e2",
                vec![0.0, 1.0, 0.0],
                VectorCollection::EmailText,
                empty_metadata(),
            ))
            .await
            .unwrap();
        store
            .insert(make_doc(
                "e3",
                vec![0.0, 0.0, 1.0],
                VectorCollection::ImageText,
                empty_metadata(),
            ))
            .await
            .unwrap();

        let stats = store.stats().await.unwrap();
        assert_eq!(stats.total_vectors, 3);
        assert_eq!(stats.dimensions, 3);
        assert_eq!(stats.collections.get("email_text"), Some(&2));
        assert_eq!(stats.collections.get("image_text"), Some(&1));
        // Memory: 3 vectors * 3 dimensions * 4 bytes + 3 * 128 overhead = 36 + 384 = 420
        assert_eq!(stats.memory_bytes, 3 * 3 * 4 + 3 * 128);
    }

    #[tokio::test]
    async fn test_health() {
        let store = InMemoryVectorStore::new();
        let healthy = store.health().await.unwrap();
        assert!(healthy);
    }

    #[tokio::test]
    async fn test_list_by_collection() {
        let store = InMemoryVectorStore::new();

        for i in 0..5 {
            store
                .insert(make_doc(
                    &format!("email-{}", i),
                    vec![i as f32, 0.0],
                    VectorCollection::EmailText,
                    empty_metadata(),
                ))
                .await
                .unwrap();
        }
        store
            .insert(make_doc(
                "img-1",
                vec![1.0, 1.0],
                VectorCollection::ImageText,
                empty_metadata(),
            ))
            .await
            .unwrap();

        let all_email = store
            .list_by_collection(&VectorCollection::EmailText, 100, 0)
            .await
            .unwrap();
        assert_eq!(all_email.len(), 5);

        // Test pagination
        let page = store
            .list_by_collection(&VectorCollection::EmailText, 2, 0)
            .await
            .unwrap();
        assert_eq!(page.len(), 2);

        let page2 = store
            .list_by_collection(&VectorCollection::EmailText, 2, 3)
            .await
            .unwrap();
        assert_eq!(page2.len(), 2);

        let page3 = store
            .list_by_collection(&VectorCollection::EmailText, 2, 4)
            .await
            .unwrap();
        assert_eq!(page3.len(), 1);

        // Different collection
        let images = store
            .list_by_collection(&VectorCollection::ImageText, 100, 0)
            .await
            .unwrap();
        assert_eq!(images.len(), 1);
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let store = Arc::new(InMemoryVectorStore::new());
        let mut handles = Vec::new();

        // Spawn 10 writer tasks
        for i in 0..10 {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                let doc = make_doc(
                    &format!("concurrent-{}", i),
                    vec![i as f32, (i * 2) as f32, (i * 3) as f32],
                    VectorCollection::EmailText,
                    empty_metadata(),
                );
                store.insert(doc).await.unwrap();
            }));
        }

        // Spawn 5 reader tasks that run concurrently with writers
        for _ in 0..5 {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                let _ = store.count().await.unwrap();
                let _ = store.health().await.unwrap();
                let _ = store.stats().await.unwrap();
            }));
        }

        // Spawn 3 search tasks
        for _ in 0..3 {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                let params = SearchParams {
                    vector: vec![1.0, 2.0, 3.0],
                    limit: 5,
                    collection: VectorCollection::EmailText,
                    filters: None,
                    min_score: None,
                };
                let _ = store.search(&params).await.unwrap();
            }));
        }

        // All tasks should complete without panics or deadlocks
        for handle in handles {
            handle.await.unwrap();
        }

        // All 10 documents should have been inserted
        let count = store.count().await.unwrap();
        assert_eq!(count, 10);
    }
}
