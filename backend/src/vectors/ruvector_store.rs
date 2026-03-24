//! RuVector-backed vector store with HNSW indexing (ADR-003).
//!
//! Implements `VectorStoreBackend` using ruvector-core's HNSW index for
//! O(log n) approximate nearest neighbor search instead of brute-force scans.
//! Each `VectorCollection` gets its own isolated HNSW index and VectorDB.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use ruvector_core::types::{DbOptions, DistanceMetric, HnswConfig, SearchQuery, VectorEntry};
use ruvector_core::VectorDB;

use super::config::{IndexConfig, StoreConfig};
use super::error::VectorError;
use super::types::{
    ScoredResult, SearchParams, VectorCollection, VectorDocument, VectorId, VectorStats,
};

/// Maps an emailibrium `VectorCollection` to a stable directory name.
fn collection_dir(collection: &VectorCollection) -> &'static str {
    match collection {
        VectorCollection::EmailText => "email_text",
        VectorCollection::ImageText => "image_text",
        VectorCollection::ImageVisual => "image_visual",
        VectorCollection::AttachmentText => "attachment_text",
    }
}

/// All collection variants for iteration.
const ALL_COLLECTIONS: [VectorCollection; 4] = [
    VectorCollection::EmailText,
    VectorCollection::ImageText,
    VectorCollection::ImageVisual,
    VectorCollection::AttachmentText,
];

/// Per-collection state: an HNSW-backed VectorDB plus a metadata sidecar.
///
/// ruvector-core stores `VectorEntry { id, vector, metadata }` where metadata
/// uses `serde_json::Value`. We keep a parallel map of emailibrium-specific
/// fields (email_id, collection, created_at, full metadata) so we can
/// reconstruct `VectorDocument` on retrieval.
struct CollectionIndex {
    db: VectorDB,
    /// vector_id (uuid string) -> full VectorDocument for retrieval.
    documents: HashMap<String, VectorDocument>,
}

/// RuVector-backed vector store using per-collection HNSW indices.
///
/// Thread-safe via `RwLock` over the inner state. The HNSW parameters
/// (M, ef_construction, ef_search) are taken from the application config.
pub struct RuVectorStore {
    inner: Arc<RwLock<RuVectorInner>>,
}

struct RuVectorInner {
    /// One HNSW index per collection.
    collections: HashMap<VectorCollection, CollectionIndex>,
    /// Base path for persistence.
    base_path: PathBuf,
    /// HNSW configuration applied to every collection index.
    hnsw_config: HnswConfig,
    /// Embedding dimensions (must be consistent across all inserts).
    dimensions: usize,
}

impl RuVectorStore {
    /// Create a new RuVector store.
    ///
    /// Initialises one HNSW index per collection on disk under `store_config.path`.
    /// HNSW parameters are drawn from `index_config`; dimensions from `dimensions`.
    pub fn new(
        store_config: &StoreConfig,
        index_config: &IndexConfig,
        dimensions: usize,
    ) -> Result<Self, VectorError> {
        let base_path = PathBuf::from(&store_config.path);
        std::fs::create_dir_all(&base_path).map_err(|e| {
            VectorError::StoreFailed(format!("failed to create store directory: {e}"))
        })?;

        let hnsw_config = HnswConfig {
            m: index_config.m,
            ef_construction: index_config.ef_construction,
            ef_search: index_config.ef_search,
            max_elements: 10_000_000,
        };

        let mut collections = HashMap::new();

        for collection in &ALL_COLLECTIONS {
            let dir = base_path.join(collection_dir(collection));
            std::fs::create_dir_all(&dir).map_err(|e| {
                VectorError::StoreFailed(format!(
                    "failed to create collection directory {}: {e}",
                    dir.display()
                ))
            })?;

            let db_path = dir.join("vectors.db");
            let opts = DbOptions {
                dimensions,
                distance_metric: DistanceMetric::Cosine,
                storage_path: db_path.to_string_lossy().to_string(),
                hnsw_config: Some(hnsw_config.clone()),
                quantization: None,
            };

            let db = VectorDB::new(opts).map_err(|e| {
                VectorError::StoreFailed(format!(
                    "failed to initialise RuVector index for {}: {e}",
                    collection_dir(collection)
                ))
            })?;

            collections.insert(
                collection.clone(),
                CollectionIndex {
                    db,
                    documents: HashMap::new(),
                },
            );
        }

        Ok(Self {
            inner: Arc::new(RwLock::new(RuVectorInner {
                collections,
                base_path,
                hnsw_config,
                dimensions,
            })),
        })
    }
}

/// Convert emailibrium metadata (HashMap<String, String>) to ruvector metadata
/// (HashMap<String, serde_json::Value>).
fn to_rv_metadata(meta: &HashMap<String, String>) -> HashMap<String, serde_json::Value> {
    meta.iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect()
}

/// Check whether a document's metadata matches all required filter key-value pairs.
fn metadata_matches(doc: &VectorDocument, filters: &HashMap<String, String>) -> bool {
    filters
        .iter()
        .all(|(key, value)| doc.metadata.get(key) == Some(value))
}

#[async_trait]
impl super::store::VectorStoreBackend for RuVectorStore {
    async fn insert(&self, doc: VectorDocument) -> Result<VectorId, VectorError> {
        if doc.vector.is_empty() {
            return Err(VectorError::StoreFailed(
                "vector must not be empty".to_string(),
            ));
        }

        let id = doc.id.clone();
        let rv_id = id.0.to_string();

        let entry = VectorEntry {
            id: Some(rv_id.clone()),
            vector: doc.vector.clone(),
            metadata: Some(to_rv_metadata(&doc.metadata)),
        };

        let mut inner = self.inner.write().await;
        let coll = inner
            .collections
            .get_mut(&doc.collection)
            .ok_or_else(|| VectorError::CollectionNotFound(doc.collection.to_string()))?;

        coll.db
            .insert(entry)
            .map_err(|e| VectorError::StoreFailed(format!("ruvector insert failed: {e}")))?;

        coll.documents.insert(rv_id, doc);
        Ok(id)
    }

    async fn batch_insert(&self, docs: Vec<VectorDocument>) -> Result<Vec<VectorId>, VectorError> {
        // Validate all documents before inserting any.
        for (i, doc) in docs.iter().enumerate() {
            if doc.vector.is_empty() {
                return Err(VectorError::StoreFailed(format!(
                    "document at index {} has an empty vector",
                    i
                )));
            }
        }

        let mut inner = self.inner.write().await;
        let mut ids = Vec::with_capacity(docs.len());

        // Group by collection to batch inserts.
        let mut by_collection: HashMap<
            VectorCollection,
            Vec<(String, VectorEntry, VectorDocument)>,
        > = HashMap::new();

        for doc in docs {
            let rv_id = doc.id.0.to_string();
            let entry = VectorEntry {
                id: Some(rv_id.clone()),
                vector: doc.vector.clone(),
                metadata: Some(to_rv_metadata(&doc.metadata)),
            };
            by_collection
                .entry(doc.collection.clone())
                .or_default()
                .push((rv_id, entry, doc));
        }

        for (collection, entries) in by_collection {
            let coll = inner
                .collections
                .get_mut(&collection)
                .ok_or_else(|| VectorError::CollectionNotFound(collection.to_string()))?;

            let rv_entries: Vec<VectorEntry> = entries.iter().map(|(_, e, _)| e.clone()).collect();

            coll.db.insert_batch(rv_entries).map_err(|e| {
                VectorError::StoreFailed(format!("ruvector batch insert failed: {e}"))
            })?;

            for (rv_id, _, doc) in entries {
                ids.push(doc.id.clone());
                coll.documents.insert(rv_id, doc);
            }
        }

        Ok(ids)
    }

    async fn search(&self, params: &SearchParams) -> Result<Vec<ScoredResult>, VectorError> {
        if params.vector.is_empty() {
            return Err(VectorError::StoreFailed(
                "query vector must not be empty".to_string(),
            ));
        }

        let inner = self.inner.read().await;
        let coll = inner
            .collections
            .get(&params.collection)
            .ok_or_else(|| VectorError::CollectionNotFound(params.collection.to_string()))?;

        // Over-fetch to account for metadata filtering and min_score pruning.
        let fetch_k = if params.filters.is_some() {
            params.limit * 4
        } else {
            params.limit * 2
        }
        .max(params.limit);

        let query = SearchQuery {
            vector: params.vector.clone(),
            k: fetch_k,
            filter: None, // We handle metadata filtering ourselves for exact semantics.
            ef_search: Some(inner.hnsw_config.ef_search),
        };

        let rv_results = coll
            .db
            .search(query)
            .map_err(|e| VectorError::StoreFailed(format!("ruvector search failed: {e}")))?;

        // ruvector returns distance (lower=better for Cosine distance).
        // Convert to similarity: similarity = 1.0 - distance for Cosine.
        let mut results: Vec<ScoredResult> = rv_results
            .into_iter()
            .filter_map(|r| {
                let doc = coll.documents.get(&r.id)?;
                let similarity = 1.0 - r.score;

                // Apply min_score threshold.
                if let Some(min_score) = params.min_score {
                    if similarity < min_score {
                        return None;
                    }
                }

                // Apply metadata filters.
                if let Some(ref filters) = params.filters {
                    if !metadata_matches(doc, filters) {
                        return None;
                    }
                }

                Some(ScoredResult {
                    document: doc.clone(),
                    score: similarity,
                })
            })
            .collect();

        // Sort descending by similarity score.
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results.truncate(params.limit);
        Ok(results)
    }

    async fn get(&self, id: &VectorId) -> Result<Option<VectorDocument>, VectorError> {
        let inner = self.inner.read().await;
        let rv_id = id.0.to_string();

        for coll in inner.collections.values() {
            if let Some(doc) = coll.documents.get(&rv_id) {
                return Ok(Some(doc.clone()));
            }
        }
        Ok(None)
    }

    async fn get_by_email_id(&self, email_id: &str) -> Result<Option<VectorDocument>, VectorError> {
        let inner = self.inner.read().await;
        for coll in inner.collections.values() {
            for doc in coll.documents.values() {
                if doc.email_id == email_id {
                    return Ok(Some(doc.clone()));
                }
            }
        }
        Ok(None)
    }

    async fn delete(&self, id: &VectorId) -> Result<bool, VectorError> {
        let mut inner = self.inner.write().await;
        let rv_id = id.0.to_string();

        for coll in inner.collections.values_mut() {
            if coll.documents.remove(&rv_id).is_some() {
                let _ = coll.db.delete(&rv_id).map_err(|e| {
                    VectorError::StoreFailed(format!("ruvector delete failed: {e}"))
                })?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn update(&self, doc: VectorDocument) -> Result<(), VectorError> {
        if doc.vector.is_empty() {
            return Err(VectorError::StoreFailed(
                "vector must not be empty".to_string(),
            ));
        }

        let mut inner = self.inner.write().await;
        let rv_id = doc.id.0.to_string();

        let coll = inner
            .collections
            .get_mut(&doc.collection)
            .ok_or_else(|| VectorError::CollectionNotFound(doc.collection.to_string()))?;

        if !coll.documents.contains_key(&rv_id) {
            return Err(VectorError::NotFound(format!(
                "vector {} does not exist",
                doc.id
            )));
        }

        // Delete old entry, insert new one.
        let _ = coll.db.delete(&rv_id);

        let entry = VectorEntry {
            id: Some(rv_id.clone()),
            vector: doc.vector.clone(),
            metadata: Some(to_rv_metadata(&doc.metadata)),
        };

        coll.db
            .insert(entry)
            .map_err(|e| VectorError::StoreFailed(format!("ruvector update-insert failed: {e}")))?;

        coll.documents.insert(rv_id, doc);
        Ok(())
    }

    async fn health(&self) -> Result<bool, VectorError> {
        let _inner = self.inner.read().await;
        Ok(true)
    }

    async fn stats(&self) -> Result<VectorStats, VectorError> {
        let inner = self.inner.read().await;

        let mut total_vectors: u64 = 0;
        let mut collections: HashMap<String, u64> = HashMap::new();

        for (collection, coll_idx) in &inner.collections {
            let count = coll_idx.documents.len() as u64;
            if count > 0 {
                collections.insert(collection.to_string(), count);
            }
            total_vectors += count;
        }

        let dimensions = inner.dimensions;
        let vector_bytes = total_vectors * (dimensions as u64) * 4;
        // HNSW graph overhead: ~M * 2 * 4 bytes per node for neighbor lists.
        let hnsw_overhead = total_vectors * (inner.hnsw_config.m as u64) * 2 * 4;
        let metadata_overhead = total_vectors * 128;
        let memory_bytes = vector_bytes + hnsw_overhead + metadata_overhead;

        Ok(VectorStats {
            total_vectors,
            collections,
            dimensions,
            memory_bytes,
            index_type: "ruvector_hnsw".to_string(),
        })
    }

    async fn count(&self) -> Result<u64, VectorError> {
        let inner = self.inner.read().await;
        let total: usize = inner.collections.values().map(|c| c.documents.len()).sum();
        Ok(total as u64)
    }

    async fn list_by_collection(
        &self,
        collection: &VectorCollection,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<VectorDocument>, VectorError> {
        let inner = self.inner.read().await;
        let coll = inner
            .collections
            .get(collection)
            .ok_or_else(|| VectorError::CollectionNotFound(collection.to_string()))?;

        let docs: Vec<VectorDocument> = coll
            .documents
            .values()
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
    use crate::vectors::store::VectorStoreBackend;
    use chrono::Utc;
    use tempfile::TempDir;

    fn test_store_config(dir: &TempDir) -> StoreConfig {
        StoreConfig {
            path: dir.path().to_string_lossy().to_string(),
            enabled: true,
            backend: "ruvector".to_string(),
        }
    }

    fn test_index_config() -> IndexConfig {
        IndexConfig {
            m: 16,
            ef_construction: 100,
            ef_search: 50,
        }
    }

    fn make_doc(email_id: &str, vector: Vec<f32>, collection: VectorCollection) -> VectorDocument {
        VectorDocument {
            id: VectorId::new(),
            email_id: email_id.to_string(),
            vector,
            metadata: HashMap::new(),
            collection,
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_insert_and_get() {
        let dir = TempDir::new().unwrap();
        let store = RuVectorStore::new(&test_store_config(&dir), &test_index_config(), 3).unwrap();

        let doc = make_doc("email-1", vec![1.0, 0.0, 0.0], VectorCollection::EmailText);
        let id = doc.id.clone();

        let returned_id = store.insert(doc).await.unwrap();
        assert_eq!(returned_id, id);

        let retrieved = store.get(&id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().email_id, "email-1");
    }

    #[tokio::test]
    async fn test_search_returns_results() {
        let dir = TempDir::new().unwrap();
        let store = RuVectorStore::new(&test_store_config(&dir), &test_index_config(), 3).unwrap();

        store
            .insert(make_doc(
                "a",
                vec![1.0, 0.0, 0.0],
                VectorCollection::EmailText,
            ))
            .await
            .unwrap();
        store
            .insert(make_doc(
                "b",
                vec![0.0, 1.0, 0.0],
                VectorCollection::EmailText,
            ))
            .await
            .unwrap();
        store
            .insert(make_doc(
                "c",
                vec![0.9, 0.1, 0.0],
                VectorCollection::EmailText,
            ))
            .await
            .unwrap();

        let params = SearchParams {
            vector: vec![1.0, 0.0, 0.0],
            limit: 3,
            collection: VectorCollection::EmailText,
            filters: None,
            min_score: None,
        };

        let results = store.search(&params).await.unwrap();
        assert!(!results.is_empty());
        // Most similar to [1,0,0] should be "a".
        assert_eq!(results[0].document.email_id, "a");
    }

    #[tokio::test]
    async fn test_delete() {
        let dir = TempDir::new().unwrap();
        let store = RuVectorStore::new(&test_store_config(&dir), &test_index_config(), 2).unwrap();

        let doc = make_doc("email-1", vec![1.0, 0.0], VectorCollection::EmailText);
        let id = doc.id.clone();
        store.insert(doc).await.unwrap();

        assert!(store.delete(&id).await.unwrap());
        assert!(!store.delete(&id).await.unwrap());
        assert_eq!(store.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_stats_reports_hnsw_index_type() {
        let dir = TempDir::new().unwrap();
        let store = RuVectorStore::new(&test_store_config(&dir), &test_index_config(), 3).unwrap();

        store
            .insert(make_doc(
                "e1",
                vec![1.0, 0.0, 0.0],
                VectorCollection::EmailText,
            ))
            .await
            .unwrap();

        let stats = store.stats().await.unwrap();
        assert_eq!(stats.index_type, "ruvector_hnsw");
        assert_eq!(stats.total_vectors, 1);
        assert_eq!(stats.dimensions, 3);
    }

    #[tokio::test]
    async fn test_empty_vector_rejected() {
        let dir = TempDir::new().unwrap();
        let store = RuVectorStore::new(&test_store_config(&dir), &test_index_config(), 3).unwrap();

        let doc = make_doc("e1", vec![], VectorCollection::EmailText);
        assert!(store.insert(doc).await.is_err());
    }
}
