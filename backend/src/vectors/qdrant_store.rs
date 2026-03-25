//! Qdrant-backed vector store via REST API (ADR-003 fallback).
//!
//! Implements `VectorStoreBackend` using Qdrant's REST API for HNSW-based
//! approximate nearest neighbor search. Uses reqwest (already in Cargo.toml)
//! instead of the qdrant-client crate to avoid adding dependencies.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::error::VectorError;
use super::types::{
    ScoredResult, SearchParams, VectorCollection, VectorDocument, VectorId, VectorStats,
};

/// Configuration for connecting to a Qdrant instance.
#[derive(Debug, Clone)]
pub struct QdrantConfig {
    /// Base URL for the Qdrant REST API (e.g. "http://localhost:6333").
    pub url: String,
    /// Collection name prefix. Each `VectorCollection` gets its own Qdrant
    /// collection named `{prefix}_{collection}`.
    pub collection_prefix: String,
    /// Optional API key for authenticated Qdrant deployments.
    pub api_key: Option<String>,
    /// Embedding dimensions (must be consistent across all inserts).
    pub dimensions: usize,
}

/// Maps a `VectorCollection` to a Qdrant collection name.
fn qdrant_collection_name(prefix: &str, collection: &VectorCollection) -> String {
    format!("{}_{}", prefix, collection)
}

/// All collection variants for iteration.
const ALL_COLLECTIONS: [VectorCollection; 4] = [
    VectorCollection::EmailText,
    VectorCollection::ImageText,
    VectorCollection::ImageVisual,
    VectorCollection::AttachmentText,
];

/// Qdrant REST API vector store backend.
///
/// Communicates with a running Qdrant instance via HTTP. Each
/// `VectorCollection` is mapped to a separate Qdrant collection.
/// Documents are cached in-memory for `get` / `get_by_email_id` lookups
/// since Qdrant points don't store all VectorDocument fields natively.
pub struct QdrantVectorStore {
    client: reqwest::Client,
    config: QdrantConfig,
    /// Local document cache keyed by vector UUID string.
    documents: Arc<RwLock<HashMap<String, VectorDocument>>>,
}

impl QdrantVectorStore {
    /// Create a new Qdrant store and ensure collections exist.
    pub async fn new(config: QdrantConfig) -> Result<Self, VectorError> {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(ref key) = config.api_key {
            headers.insert(
                "api-key",
                reqwest::header::HeaderValue::from_str(key).map_err(|e| {
                    VectorError::StoreFailed(format!("invalid Qdrant API key header: {e}"))
                })?,
            );
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| VectorError::StoreFailed(format!("failed to build HTTP client: {e}")))?;

        let store = Self {
            client,
            config,
            documents: Arc::new(RwLock::new(HashMap::new())),
        };

        // Ensure all collections exist.
        for collection in &ALL_COLLECTIONS {
            store.ensure_collection(collection).await?;
        }

        Ok(store)
    }

    /// Create a Qdrant collection if it does not already exist.
    async fn ensure_collection(&self, collection: &VectorCollection) -> Result<(), VectorError> {
        let name = qdrant_collection_name(&self.config.collection_prefix, collection);
        let url = format!("{}/collections/{}", self.config.url, name);

        // Check if collection exists.
        let resp =
            self.client.get(&url).send().await.map_err(|e| {
                VectorError::StoreFailed(format!("qdrant GET collection failed: {e}"))
            })?;

        if resp.status().is_success() {
            return Ok(());
        }

        // Create collection with HNSW + cosine distance.
        let body = serde_json::json!({
            "vectors": {
                "size": self.config.dimensions,
                "distance": "Cosine"
            }
        });

        let resp = self
            .client
            .put(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| VectorError::StoreFailed(format!("qdrant PUT collection failed: {e}")))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(VectorError::StoreFailed(format!(
                "qdrant create collection {name} failed: {text}"
            )));
        }

        Ok(())
    }

    /// Build the Qdrant points/upsert URL for a collection.
    fn upsert_url(&self, collection: &VectorCollection) -> String {
        let name = qdrant_collection_name(&self.config.collection_prefix, collection);
        format!("{}/collections/{}/points", self.config.url, name)
    }

    /// Build the Qdrant search URL for a collection.
    fn search_url(&self, collection: &VectorCollection) -> String {
        let name = qdrant_collection_name(&self.config.collection_prefix, collection);
        format!("{}/collections/{}/points/search", self.config.url, name)
    }

    /// Build the Qdrant points delete URL for a collection.
    fn delete_url(&self, collection: &VectorCollection) -> String {
        let name = qdrant_collection_name(&self.config.collection_prefix, collection);
        format!("{}/collections/{}/points/delete", self.config.url, name)
    }

    /// Convert document metadata to Qdrant payload JSON.
    fn doc_to_payload(doc: &VectorDocument) -> serde_json::Value {
        let mut payload = serde_json::Map::new();
        payload.insert(
            "email_id".to_string(),
            serde_json::Value::String(doc.email_id.clone()),
        );
        payload.insert(
            "collection".to_string(),
            serde_json::Value::String(doc.collection.to_string()),
        );
        payload.insert(
            "created_at".to_string(),
            serde_json::Value::String(doc.created_at.to_rfc3339()),
        );
        for (k, v) in &doc.metadata {
            payload.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        serde_json::Value::Object(payload)
    }
}

#[async_trait]
impl super::store::VectorStoreBackend for QdrantVectorStore {
    async fn insert(&self, doc: VectorDocument) -> Result<VectorId, VectorError> {
        if doc.vector.is_empty() {
            return Err(VectorError::StoreFailed(
                "vector must not be empty".to_string(),
            ));
        }

        let id = doc.id.clone();
        let point_id = id.0.to_string();

        let body = serde_json::json!({
            "points": [{
                "id": point_id,
                "vector": doc.vector,
                "payload": Self::doc_to_payload(&doc),
            }]
        });

        let url = self.upsert_url(&doc.collection);
        let resp = self
            .client
            .put(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| VectorError::StoreFailed(format!("qdrant upsert failed: {e}")))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(VectorError::StoreFailed(format!(
                "qdrant upsert failed: {text}"
            )));
        }

        self.documents.write().await.insert(point_id, doc);
        Ok(id)
    }

    async fn batch_insert(&self, docs: Vec<VectorDocument>) -> Result<Vec<VectorId>, VectorError> {
        for (i, doc) in docs.iter().enumerate() {
            if doc.vector.is_empty() {
                return Err(VectorError::StoreFailed(format!(
                    "document at index {} has an empty vector",
                    i
                )));
            }
        }

        // Group by collection for batched upserts.
        let mut by_collection: HashMap<VectorCollection, Vec<&VectorDocument>> = HashMap::new();
        for doc in &docs {
            by_collection
                .entry(doc.collection.clone())
                .or_default()
                .push(doc);
        }

        for (collection, batch) in &by_collection {
            let points: Vec<serde_json::Value> = batch
                .iter()
                .map(|doc| {
                    serde_json::json!({
                        "id": doc.id.0.to_string(),
                        "vector": doc.vector,
                        "payload": Self::doc_to_payload(doc),
                    })
                })
                .collect();

            let body = serde_json::json!({ "points": points });
            let url = self.upsert_url(collection);

            let resp = self
                .client
                .put(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| {
                    VectorError::StoreFailed(format!("qdrant batch upsert failed: {e}"))
                })?;

            if !resp.status().is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(VectorError::StoreFailed(format!(
                    "qdrant batch upsert failed: {text}"
                )));
            }
        }

        let mut cache = self.documents.write().await;
        let ids: Vec<VectorId> = docs
            .into_iter()
            .map(|doc| {
                let id = doc.id.clone();
                cache.insert(id.0.to_string(), doc);
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

        // Over-fetch to account for metadata filtering.
        let fetch_k = if params.filters.is_some() {
            params.limit * 4
        } else {
            params.limit * 2
        }
        .max(params.limit);

        let body = serde_json::json!({
            "vector": params.vector,
            "limit": fetch_k,
            "with_payload": true,
        });

        let url = self.search_url(&params.collection);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| VectorError::StoreFailed(format!("qdrant search failed: {e}")))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(VectorError::StoreFailed(format!(
                "qdrant search failed: {text}"
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| VectorError::StoreFailed(format!("qdrant search parse failed: {e}")))?;

        let cache = self.documents.read().await;
        let hits = json["result"].as_array().unwrap_or(&Vec::new()).clone();

        let mut results: Vec<ScoredResult> = hits
            .into_iter()
            .filter_map(|hit| {
                let point_id = hit["id"].as_str()?;
                let score = hit["score"].as_f64()? as f32;

                // Apply min_score threshold.
                if let Some(min_score) = params.min_score {
                    if score < min_score {
                        return None;
                    }
                }

                let doc = cache.get(point_id)?;

                // Apply metadata filters.
                if let Some(ref filters) = params.filters {
                    if !filters
                        .iter()
                        .all(|(key, value)| doc.metadata.get(key) == Some(value))
                    {
                        return None;
                    }
                }

                Some(ScoredResult {
                    document: doc.clone(),
                    score,
                })
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(params.limit);

        Ok(results)
    }

    async fn get(&self, id: &VectorId) -> Result<Option<VectorDocument>, VectorError> {
        let cache = self.documents.read().await;
        Ok(cache.get(&id.0.to_string()).cloned())
    }

    async fn get_by_email_id(&self, email_id: &str) -> Result<Option<VectorDocument>, VectorError> {
        let cache = self.documents.read().await;
        Ok(cache.values().find(|d| d.email_id == email_id).cloned())
    }

    async fn delete(&self, id: &VectorId) -> Result<bool, VectorError> {
        let rv_id = id.0.to_string();
        let mut cache = self.documents.write().await;

        let doc = match cache.remove(&rv_id) {
            Some(doc) => doc,
            None => return Ok(false),
        };

        let body = serde_json::json!({
            "points": [rv_id],
        });

        let url = self.delete_url(&doc.collection);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| VectorError::StoreFailed(format!("qdrant delete failed: {e}")))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(VectorError::StoreFailed(format!(
                "qdrant delete failed: {text}"
            )));
        }

        Ok(true)
    }

    async fn update(&self, doc: VectorDocument) -> Result<(), VectorError> {
        if doc.vector.is_empty() {
            return Err(VectorError::StoreFailed(
                "vector must not be empty".to_string(),
            ));
        }

        let rv_id = doc.id.0.to_string();
        {
            let cache = self.documents.read().await;
            if !cache.contains_key(&rv_id) {
                return Err(VectorError::NotFound(format!(
                    "vector {} does not exist",
                    doc.id
                )));
            }
        }

        // Upsert overwrites the existing point.
        self.insert(doc).await?;
        Ok(())
    }

    async fn health(&self) -> Result<bool, VectorError> {
        let url = format!("{}/healthz", self.config.url);
        let resp =
            self.client.get(&url).send().await.map_err(|e| {
                VectorError::StoreFailed(format!("qdrant health check failed: {e}"))
            })?;
        Ok(resp.status().is_success())
    }

    async fn stats(&self) -> Result<VectorStats, VectorError> {
        let cache = self.documents.read().await;
        let total_vectors = cache.len() as u64;

        let mut collections: HashMap<String, u64> = HashMap::new();
        for doc in cache.values() {
            *collections.entry(doc.collection.to_string()).or_insert(0) += 1;
        }

        let dimensions = self.config.dimensions;
        let vector_bytes = total_vectors * (dimensions as u64) * 4;
        let metadata_overhead = total_vectors * 128;
        let memory_bytes = vector_bytes + metadata_overhead;

        Ok(VectorStats {
            total_vectors,
            collections,
            dimensions,
            memory_bytes,
            index_type: "qdrant_hnsw".to_string(),
        })
    }

    async fn count(&self) -> Result<u64, VectorError> {
        let cache = self.documents.read().await;
        Ok(cache.len() as u64)
    }

    async fn list_by_collection(
        &self,
        collection: &VectorCollection,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<VectorDocument>, VectorError> {
        let cache = self.documents.read().await;
        let docs: Vec<VectorDocument> = cache
            .values()
            .filter(|d| d.collection == *collection)
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

    #[test]
    fn test_qdrant_collection_name() {
        let name = qdrant_collection_name("emailibrium", &VectorCollection::EmailText);
        assert_eq!(name, "emailibrium_email_text");

        let name = qdrant_collection_name("test", &VectorCollection::ImageVisual);
        assert_eq!(name, "test_image_visual");
    }

    #[test]
    fn test_qdrant_config_defaults() {
        let config = QdrantConfig {
            url: "http://localhost:6333".to_string(),
            collection_prefix: "emailibrium".to_string(),
            api_key: None,
            dimensions: 384,
        };
        assert_eq!(config.url, "http://localhost:6333");
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_doc_to_payload_includes_metadata() {
        use chrono::Utc;

        let mut metadata = HashMap::new();
        metadata.insert("category".to_string(), "work".to_string());

        let doc = VectorDocument {
            id: VectorId::new(),
            email_id: "test-email".to_string(),
            vector: vec![1.0, 0.0],
            metadata,
            collection: VectorCollection::EmailText,
            created_at: Utc::now(),
        };

        let payload = QdrantVectorStore::doc_to_payload(&doc);
        assert_eq!(payload["email_id"], "test-email");
        assert_eq!(payload["collection"], "email_text");
        assert_eq!(payload["category"], "work");
        assert!(payload["created_at"].as_str().is_some());
    }
}
