//! SQLite-backed emergency fallback vector store (ADR-003).
//!
//! Implements `VectorStoreBackend` using SQLite via sqlx. Vectors are stored
//! as BLOBs (raw f32 bytes) and search uses brute-force cosine similarity.
//! This is intentionally not performance-optimized — it exists as a reliable
//! last-resort fallback when both RuVector and Qdrant are unavailable.

use async_trait::async_trait;
use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;

use super::error::VectorError;
use super::types::{
    ScoredResult, SearchParams, VectorCollection, VectorDocument, VectorId, VectorStats,
};

/// SQLite emergency fallback vector store.
///
/// Stores vectors as BLOB columns (little-endian f32 bytes) in a single
/// `vector_documents` table. Search is brute-force O(n) cosine similarity,
/// acceptable only for emergency recovery scenarios.
pub struct SqliteVectorStore {
    pool: SqlitePool,
}

impl SqliteVectorStore {
    /// Create a new SQLite vector store, running migrations to ensure the
    /// `vector_documents` table exists.
    pub async fn new(pool: SqlitePool) -> Result<Self, VectorError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS vector_documents (
                id TEXT PRIMARY KEY,
                email_id TEXT NOT NULL,
                vector BLOB NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}',
                collection TEXT NOT NULL,
                created_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .map_err(|e| {
            VectorError::StoreFailed(format!("sqlite vector table creation failed: {e}"))
        })?;

        // Index on email_id for fast lookup.
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_vector_documents_email_id
            ON vector_documents(email_id)
            "#,
        )
        .execute(&pool)
        .await
        .map_err(|e| VectorError::StoreFailed(format!("sqlite index creation failed: {e}")))?;

        // Index on collection for filtered listing.
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_vector_documents_collection
            ON vector_documents(collection)
            "#,
        )
        .execute(&pool)
        .await
        .map_err(|e| VectorError::StoreFailed(format!("sqlite index creation failed: {e}")))?;

        Ok(Self { pool })
    }
}

/// Serialize a vector of f32 values to a BLOB (little-endian bytes).
fn vector_to_blob(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for &v in vector {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// Deserialize a BLOB back to a vector of f32 values.
fn blob_to_vector(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().expect("chunk is exactly 4 bytes");
            f32::from_le_bytes(arr)
        })
        .collect()
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

/// Parse a collection string back to a `VectorCollection` enum.
fn parse_collection(s: &str) -> Option<VectorCollection> {
    match s {
        "email_text" => Some(VectorCollection::EmailText),
        "image_text" => Some(VectorCollection::ImageText),
        "image_visual" => Some(VectorCollection::ImageVisual),
        "attachment_text" => Some(VectorCollection::AttachmentText),
        _ => None,
    }
}

/// Reconstruct a `VectorDocument` from SQLite row fields.
fn row_to_document(
    id_str: &str,
    email_id: &str,
    vector_blob: &[u8],
    metadata_json: &str,
    collection_str: &str,
    created_at_str: &str,
) -> Option<VectorDocument> {
    let uuid = uuid::Uuid::parse_str(id_str).ok()?;
    let collection = parse_collection(collection_str)?;
    let created_at = chrono::DateTime::parse_from_rfc3339(created_at_str)
        .ok()?
        .with_timezone(&Utc);
    let metadata: HashMap<String, String> = serde_json::from_str(metadata_json).unwrap_or_default();

    Some(VectorDocument {
        id: VectorId(uuid),
        email_id: email_id.to_string(),
        vector: blob_to_vector(vector_blob),
        metadata,
        collection,
        created_at,
    })
}

/// Check whether a document's metadata matches all required filter key-value pairs.
fn metadata_matches(doc: &VectorDocument, filters: &HashMap<String, String>) -> bool {
    filters
        .iter()
        .all(|(key, value)| doc.metadata.get(key) == Some(value))
}

#[async_trait]
impl super::store::VectorStoreBackend for SqliteVectorStore {
    async fn insert(&self, doc: VectorDocument) -> Result<VectorId, VectorError> {
        if doc.vector.is_empty() {
            return Err(VectorError::StoreFailed(
                "vector must not be empty".to_string(),
            ));
        }

        let id = doc.id.clone();
        let id_str = id.0.to_string();
        let blob = vector_to_blob(&doc.vector);
        let metadata_json =
            serde_json::to_string(&doc.metadata).unwrap_or_else(|_| "{}".to_string());
        let collection_str = doc.collection.to_string();
        let created_at_str = doc.created_at.to_rfc3339();

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO vector_documents (id, email_id, vector, metadata, collection, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id_str)
        .bind(&doc.email_id)
        .bind(&blob)
        .bind(&metadata_json)
        .bind(&collection_str)
        .bind(&created_at_str)
        .execute(&self.pool)
        .await
        .map_err(|e| VectorError::StoreFailed(format!("sqlite insert failed: {e}")))?;

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

        let mut ids = Vec::with_capacity(docs.len());

        // Use a transaction for atomicity.
        let mut tx = self.pool.begin().await.map_err(|e| {
            VectorError::StoreFailed(format!("sqlite begin transaction failed: {e}"))
        })?;

        for doc in docs {
            let id = doc.id.clone();
            let id_str = id.0.to_string();
            let blob = vector_to_blob(&doc.vector);
            let metadata_json =
                serde_json::to_string(&doc.metadata).unwrap_or_else(|_| "{}".to_string());
            let collection_str = doc.collection.to_string();
            let created_at_str = doc.created_at.to_rfc3339();

            sqlx::query(
                r#"
                INSERT OR REPLACE INTO vector_documents (id, email_id, vector, metadata, collection, created_at)
                VALUES (?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&id_str)
            .bind(&doc.email_id)
            .bind(&blob)
            .bind(&metadata_json)
            .bind(&collection_str)
            .bind(&created_at_str)
            .execute(&mut *tx)
            .await
            .map_err(|e| VectorError::StoreFailed(format!("sqlite batch insert failed: {e}")))?;

            ids.push(id);
        }

        tx.commit()
            .await
            .map_err(|e| VectorError::StoreFailed(format!("sqlite commit failed: {e}")))?;

        Ok(ids)
    }

    async fn search(&self, params: &SearchParams) -> Result<Vec<ScoredResult>, VectorError> {
        if params.vector.is_empty() {
            return Err(VectorError::StoreFailed(
                "query vector must not be empty".to_string(),
            ));
        }

        let collection_str = params.collection.to_string();

        // Load all vectors for the target collection (brute-force).
        let rows: Vec<(String, String, Vec<u8>, String, String, String)> = sqlx::query_as(
            r#"
            SELECT id, email_id, vector, metadata, collection, created_at
            FROM vector_documents
            WHERE collection = ?
            "#,
        )
        .bind(&collection_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| VectorError::StoreFailed(format!("sqlite search query failed: {e}")))?;

        let mut results: Vec<ScoredResult> = rows
            .iter()
            .filter_map(|(id_str, email_id, blob, meta, coll, created)| {
                let doc = row_to_document(id_str, email_id, blob, meta, coll, created)?;
                let score = cosine_similarity(&params.vector, &doc.vector);

                // Apply min_score threshold.
                if let Some(min_score) = params.min_score {
                    if score < min_score {
                        return None;
                    }
                }

                // Apply metadata filters.
                if let Some(ref filters) = params.filters {
                    if !metadata_matches(&doc, filters) {
                        return None;
                    }
                }

                Some(ScoredResult {
                    document: doc,
                    score,
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
        let id_str = id.0.to_string();

        let row: Option<(String, String, Vec<u8>, String, String, String)> = sqlx::query_as(
            r#"
            SELECT id, email_id, vector, metadata, collection, created_at
            FROM vector_documents
            WHERE id = ?
            "#,
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| VectorError::StoreFailed(format!("sqlite get failed: {e}")))?;

        Ok(row.and_then(|(id_s, email_id, blob, meta, coll, created)| {
            row_to_document(&id_s, &email_id, &blob, &meta, &coll, &created)
        }))
    }

    async fn get_by_email_id(&self, email_id: &str) -> Result<Option<VectorDocument>, VectorError> {
        let row: Option<(String, String, Vec<u8>, String, String, String)> = sqlx::query_as(
            r#"
            SELECT id, email_id, vector, metadata, collection, created_at
            FROM vector_documents
            WHERE email_id = ?
            LIMIT 1
            "#,
        )
        .bind(email_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| VectorError::StoreFailed(format!("sqlite get_by_email_id failed: {e}")))?;

        Ok(row.and_then(|(id_s, eid, blob, meta, coll, created)| {
            row_to_document(&id_s, &eid, &blob, &meta, &coll, &created)
        }))
    }

    async fn delete(&self, id: &VectorId) -> Result<bool, VectorError> {
        let id_str = id.0.to_string();

        let result = sqlx::query("DELETE FROM vector_documents WHERE id = ?")
            .bind(&id_str)
            .execute(&self.pool)
            .await
            .map_err(|e| VectorError::StoreFailed(format!("sqlite delete failed: {e}")))?;

        Ok(result.rows_affected() > 0)
    }

    async fn clear_all(&self) -> Result<u64, VectorError> {
        let result = sqlx::query("DELETE FROM vector_documents")
            .execute(&self.pool)
            .await
            .map_err(|e| VectorError::StoreFailed(format!("sqlite clear_all failed: {e}")))?;
        Ok(result.rows_affected())
    }

    async fn update(&self, doc: VectorDocument) -> Result<(), VectorError> {
        if doc.vector.is_empty() {
            return Err(VectorError::StoreFailed(
                "vector must not be empty".to_string(),
            ));
        }

        let id_str = doc.id.0.to_string();

        // Check existence first.
        let exists: Option<(String,)> =
            sqlx::query_as("SELECT id FROM vector_documents WHERE id = ?")
                .bind(&id_str)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| {
                    VectorError::StoreFailed(format!("sqlite update check failed: {e}"))
                })?;

        if exists.is_none() {
            return Err(VectorError::NotFound(format!(
                "vector {} does not exist",
                doc.id
            )));
        }

        let blob = vector_to_blob(&doc.vector);
        let metadata_json =
            serde_json::to_string(&doc.metadata).unwrap_or_else(|_| "{}".to_string());
        let collection_str = doc.collection.to_string();
        let created_at_str = doc.created_at.to_rfc3339();

        sqlx::query(
            r#"
            UPDATE vector_documents
            SET email_id = ?, vector = ?, metadata = ?, collection = ?, created_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&doc.email_id)
        .bind(&blob)
        .bind(&metadata_json)
        .bind(&collection_str)
        .bind(&created_at_str)
        .bind(&id_str)
        .execute(&self.pool)
        .await
        .map_err(|e| VectorError::StoreFailed(format!("sqlite update failed: {e}")))?;

        Ok(())
    }

    async fn health(&self) -> Result<bool, VectorError> {
        let _: (i64,) = sqlx::query_as("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| VectorError::StoreFailed(format!("sqlite health check failed: {e}")))?;
        Ok(true)
    }

    async fn stats(&self) -> Result<VectorStats, VectorError> {
        let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM vector_documents")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| VectorError::StoreFailed(format!("sqlite stats count failed: {e}")))?;
        let total_vectors = total as u64;

        // Per-collection counts.
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT collection, COUNT(*) FROM vector_documents GROUP BY collection")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| VectorError::StoreFailed(format!("sqlite stats group failed: {e}")))?;

        let mut collections: HashMap<String, u64> = HashMap::new();
        for (coll, cnt) in rows {
            if cnt > 0 {
                collections.insert(coll, cnt as u64);
            }
        }

        // Get dimensions from first vector.
        let dimensions: usize = if total_vectors > 0 {
            let (blob,): (Vec<u8>,) = sqlx::query_as("SELECT vector FROM vector_documents LIMIT 1")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| VectorError::StoreFailed(format!("sqlite stats dims failed: {e}")))?;
            blob.len() / 4
        } else {
            0
        };

        let vector_bytes = total_vectors * (dimensions as u64) * 4;
        let metadata_overhead = total_vectors * 128;
        let memory_bytes = vector_bytes + metadata_overhead;

        Ok(VectorStats {
            total_vectors,
            collections,
            dimensions,
            memory_bytes,
            index_type: "sqlite_brute_force".to_string(),
        })
    }

    async fn count(&self) -> Result<u64, VectorError> {
        let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM vector_documents")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| VectorError::StoreFailed(format!("sqlite count failed: {e}")))?;
        Ok(total as u64)
    }

    async fn list_by_collection(
        &self,
        collection: &VectorCollection,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<VectorDocument>, VectorError> {
        let collection_str = collection.to_string();

        let rows: Vec<(String, String, Vec<u8>, String, String, String)> = sqlx::query_as(
            r#"
            SELECT id, email_id, vector, metadata, collection, created_at
            FROM vector_documents
            WHERE collection = ?
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(&collection_str)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| VectorError::StoreFailed(format!("sqlite list_by_collection failed: {e}")))?;

        let docs: Vec<VectorDocument> = rows
            .iter()
            .filter_map(|(id_str, email_id, blob, meta, coll, created)| {
                row_to_document(id_str, email_id, blob, meta, coll, created)
            })
            .collect();

        Ok(docs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectors::store::VectorStoreBackend;

    async fn test_pool() -> SqlitePool {
        SqlitePool::connect("sqlite::memory:")
            .await
            .expect("failed to create in-memory SQLite pool")
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

    #[test]
    fn test_vector_blob_roundtrip() {
        let original = vec![1.0_f32, -0.5, 0.0, 3.14, -999.0];
        let blob = vector_to_blob(&original);
        let recovered = blob_to_vector(&blob);
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_parse_collection_roundtrip() {
        for coll in &[
            VectorCollection::EmailText,
            VectorCollection::ImageText,
            VectorCollection::ImageVisual,
            VectorCollection::AttachmentText,
        ] {
            let s = coll.to_string();
            let parsed = parse_collection(&s);
            assert_eq!(parsed, Some(coll.clone()));
        }
    }

    #[tokio::test]
    async fn test_sqlite_insert_and_get() {
        let pool = test_pool().await;
        let store = SqliteVectorStore::new(pool).await.unwrap();

        let doc = make_doc("email-1", vec![1.0, 0.0, 0.0], VectorCollection::EmailText);
        let id = doc.id.clone();

        let returned_id = store.insert(doc).await.unwrap();
        assert_eq!(returned_id, id);

        let retrieved = store.get(&id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().email_id, "email-1");
    }

    #[tokio::test]
    async fn test_sqlite_search_returns_results() {
        let pool = test_pool().await;
        let store = SqliteVectorStore::new(pool).await.unwrap();

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
    async fn test_sqlite_delete() {
        let pool = test_pool().await;
        let store = SqliteVectorStore::new(pool).await.unwrap();

        let doc = make_doc("email-1", vec![1.0, 0.0], VectorCollection::EmailText);
        let id = doc.id.clone();
        store.insert(doc).await.unwrap();

        assert!(store.delete(&id).await.unwrap());
        assert!(!store.delete(&id).await.unwrap());
        assert_eq!(store.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_sqlite_empty_vector_rejected() {
        let pool = test_pool().await;
        let store = SqliteVectorStore::new(pool).await.unwrap();

        let doc = make_doc("e1", vec![], VectorCollection::EmailText);
        assert!(store.insert(doc).await.is_err());
    }

    #[tokio::test]
    async fn test_sqlite_stats() {
        let pool = test_pool().await;
        let store = SqliteVectorStore::new(pool).await.unwrap();

        store
            .insert(make_doc(
                "e1",
                vec![1.0, 0.0, 0.0],
                VectorCollection::EmailText,
            ))
            .await
            .unwrap();

        let stats = store.stats().await.unwrap();
        assert_eq!(stats.index_type, "sqlite_brute_force");
        assert_eq!(stats.total_vectors, 1);
        assert_eq!(stats.dimensions, 3);
    }

    #[tokio::test]
    async fn test_sqlite_batch_insert() {
        let pool = test_pool().await;
        let store = SqliteVectorStore::new(pool).await.unwrap();

        let docs = vec![
            make_doc("e1", vec![1.0, 0.0], VectorCollection::EmailText),
            make_doc("e2", vec![0.0, 1.0], VectorCollection::EmailText),
            make_doc("e3", vec![1.0, 1.0], VectorCollection::ImageText),
        ];

        let ids = store.batch_insert(docs).await.unwrap();
        assert_eq!(ids.len(), 3);
        assert_eq!(store.count().await.unwrap(), 3);
    }

    #[tokio::test]
    async fn test_sqlite_get_by_email_id() {
        let pool = test_pool().await;
        let store = SqliteVectorStore::new(pool).await.unwrap();

        store
            .insert(make_doc(
                "unique-42",
                vec![1.0, 0.0, 0.0],
                VectorCollection::EmailText,
            ))
            .await
            .unwrap();

        let found = store.get_by_email_id("unique-42").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().email_id, "unique-42");

        let missing = store.get_by_email_id("nonexistent").await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_sqlite_health() {
        let pool = test_pool().await;
        let store = SqliteVectorStore::new(pool).await.unwrap();
        assert!(store.health().await.unwrap());
    }
}
