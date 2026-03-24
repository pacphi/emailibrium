//! SQLite-backed vector backup and restore service (ADR-003: S1-04).
//!
//! Persists vector embeddings to the `vector_backups` table so that the
//! in-memory vector store can be rebuilt after restarts without re-embedding.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;

use super::encryption::EncryptedVectorStore;
use super::error::VectorError;
use super::store::VectorStoreBackend;
use super::types::*;
use crate::db::Database;

// ---------------------------------------------------------------------------
// f32 <-> bytes helpers
// ---------------------------------------------------------------------------

/// Serialize a slice of f32 values to little-endian bytes.
fn f32_vec_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Deserialize little-endian bytes back to a `Vec<f32>`.
fn bytes_to_f32_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

// ---------------------------------------------------------------------------
// VectorBackupService
// ---------------------------------------------------------------------------

/// Provides SQLite-based backup and restore of vector data.
///
/// Vectors are serialized as raw `f32` little-endian bytes and optionally
/// encrypted before storage. On restore, the reverse process is applied.
pub struct VectorBackupService {
    db: Arc<Database>,
    store: Arc<dyn VectorStoreBackend>,
    encryption: Option<Arc<EncryptedVectorStore>>,
}

impl VectorBackupService {
    /// Create a new backup service.
    pub fn new(
        db: Arc<Database>,
        store: Arc<dyn VectorStoreBackend>,
        encryption: Option<Arc<EncryptedVectorStore>>,
    ) -> Self {
        Self {
            db,
            store,
            encryption,
        }
    }

    /// Backup a single vector document to SQLite.
    ///
    /// Serializes the vector as raw bytes, optionally encrypts, and performs
    /// an INSERT OR REPLACE into the `vector_backups` table.
    pub async fn backup_vector(&self, doc: &VectorDocument) -> Result<(), VectorError> {
        let vector_data = match &self.encryption {
            Some(enc) => enc.encrypt_vector(&doc.vector)?,
            None => f32_vec_to_bytes(&doc.vector),
        };

        let metadata_json = if doc.metadata.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&doc.metadata)?)
        };

        let vector_id = doc.id.to_string();
        let collection = doc.collection.to_string();
        let dimensions = doc.vector.len() as i64;
        let now = Utc::now();

        sqlx::query(
            "INSERT OR REPLACE INTO vector_backups \
             (vector_id, email_id, collection, dimensions, vector_data, metadata_json, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(&vector_id)
        .bind(&doc.email_id)
        .bind(&collection)
        .bind(dimensions)
        .bind(&vector_data)
        .bind(&metadata_json)
        .bind(now)
        .bind(now)
        .execute(&self.db.pool)
        .await?;

        Ok(())
    }

    /// Backup all vectors currently in the store.
    ///
    /// Returns the number of vectors successfully backed up.
    pub async fn backup_all(&self) -> Result<u64, VectorError> {
        // Collect all documents from the store by listing each known collection.
        let collections = [
            VectorCollection::EmailText,
            VectorCollection::ImageText,
            VectorCollection::ImageVisual,
            VectorCollection::AttachmentText,
        ];

        let mut count: u64 = 0;

        for collection in &collections {
            let mut offset = 0;
            const PAGE_SIZE: usize = 500;

            loop {
                let docs = self
                    .store
                    .list_by_collection(collection, PAGE_SIZE, offset)
                    .await?;

                if docs.is_empty() {
                    break;
                }

                for doc in &docs {
                    self.backup_vector(doc).await?;
                    count += 1;
                }

                if docs.len() < PAGE_SIZE {
                    break;
                }
                offset += PAGE_SIZE;
            }
        }

        Ok(count)
    }

    /// Restore a single vector from the SQLite backup.
    ///
    /// Returns `None` if the vector ID is not found in the backup table.
    pub async fn restore_vector(
        &self,
        vector_id: &str,
    ) -> Result<Option<VectorDocument>, VectorError> {
        let row: Option<BackupRow> = sqlx::query_as(
            "SELECT vector_id, email_id, collection, dimensions, vector_data, metadata_json \
             FROM vector_backups WHERE vector_id = ?1",
        )
        .bind(vector_id)
        .fetch_optional(&self.db.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(self.row_to_document(r)?)),
            None => Ok(None),
        }
    }

    /// Restore all vectors from the SQLite backup.
    pub async fn restore_all(&self) -> Result<Vec<VectorDocument>, VectorError> {
        let rows: Vec<BackupRow> = sqlx::query_as(
            "SELECT vector_id, email_id, collection, dimensions, vector_data, metadata_json \
             FROM vector_backups",
        )
        .fetch_all(&self.db.pool)
        .await?;

        let mut docs = Vec::with_capacity(rows.len());
        for row in rows {
            docs.push(self.row_to_document(row)?);
        }
        Ok(docs)
    }

    /// Delete a backup entry by vector ID.
    pub async fn delete_backup(&self, vector_id: &str) -> Result<(), VectorError> {
        sqlx::query("DELETE FROM vector_backups WHERE vector_id = ?1")
            .bind(vector_id)
            .execute(&self.db.pool)
            .await?;

        Ok(())
    }

    // -- private helpers -----------------------------------------------------

    /// Convert a database row into a `VectorDocument`.
    fn row_to_document(&self, row: BackupRow) -> Result<VectorDocument, VectorError> {
        let vector = match &self.encryption {
            Some(enc) => enc.decrypt_vector(&row.vector_data)?,
            None => bytes_to_f32_vec(&row.vector_data),
        };

        let metadata: HashMap<String, String> = match &row.metadata_json {
            Some(json) => serde_json::from_str(json)?,
            None => HashMap::new(),
        };

        let collection = parse_collection(&row.collection)?;

        let id = uuid::Uuid::parse_str(&row.vector_id)
            .map(VectorId)
            .map_err(|e| VectorError::BackupError(format!("invalid vector_id UUID: {e}")))?;

        Ok(VectorDocument {
            id,
            email_id: row.email_id,
            vector,
            metadata,
            collection,
            created_at: Utc::now(),
        })
    }
}

/// Parse a collection string back into the enum variant.
fn parse_collection(s: &str) -> Result<VectorCollection, VectorError> {
    match s {
        "email_text" => Ok(VectorCollection::EmailText),
        "image_text" => Ok(VectorCollection::ImageText),
        "image_visual" => Ok(VectorCollection::ImageVisual),
        "attachment_text" => Ok(VectorCollection::AttachmentText),
        other => Err(VectorError::CollectionNotFound(other.to_string())),
    }
}

/// Internal row type for reading from the `vector_backups` table.
#[derive(sqlx::FromRow)]
struct BackupRow {
    vector_id: String,
    email_id: String,
    collection: String,
    #[allow(dead_code)]
    dimensions: i64,
    vector_data: Vec<u8>,
    metadata_json: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::vectors::store::InMemoryVectorStore;

    /// Create an in-memory SQLite database with the initial schema applied.
    ///
    /// Foreign key enforcement is disabled so backup tests do not need
    /// to insert parent rows into the `emails` table. We use a single
    /// max-connection pool and set the pragma before schema creation.
    async fn test_db() -> Database {
        use sqlx::sqlite::SqlitePoolOptions;

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(include_str!("../../migrations/001_initial_schema.sql"))
            .execute(&pool)
            .await
            .unwrap();

        Database { pool }
    }

    /// Create a test vector document.
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

    fn make_doc_with_metadata(
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

    #[tokio::test]
    async fn test_backup_and_restore_roundtrip() {
        let db = Arc::new(test_db().await);
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let service = VectorBackupService::new(db, store, None);

        let original = make_doc(
            "email-123",
            vec![0.1, 0.2, 0.3, 0.4],
            VectorCollection::EmailText,
        );
        let vector_id = original.id.to_string();

        // Backup
        service.backup_vector(&original).await.unwrap();

        // Restore
        let restored = service.restore_vector(&vector_id).await.unwrap();
        assert!(restored.is_some());

        let restored = restored.unwrap();
        assert_eq!(restored.email_id, "email-123");
        assert_eq!(restored.collection, VectorCollection::EmailText);
        assert_eq!(restored.vector.len(), 4);

        // Verify vector values round-trip exactly.
        for (a, b) in original.vector.iter().zip(restored.vector.iter()) {
            assert!((a - b).abs() < f32::EPSILON, "vector mismatch: {a} != {b}");
        }
    }

    #[tokio::test]
    async fn test_backup_and_restore_with_metadata() {
        let db = Arc::new(test_db().await);
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let service = VectorBackupService::new(db, store, None);

        let mut metadata = HashMap::new();
        metadata.insert("category".to_string(), "work".to_string());
        metadata.insert("sender".to_string(), "alice@example.com".to_string());

        let original = make_doc_with_metadata(
            "email-meta",
            vec![1.0, 2.0, 3.0],
            VectorCollection::ImageText,
            metadata.clone(),
        );
        let vector_id = original.id.to_string();

        service.backup_vector(&original).await.unwrap();

        let restored = service.restore_vector(&vector_id).await.unwrap().unwrap();
        assert_eq!(restored.metadata, metadata);
    }

    #[tokio::test]
    async fn test_backup_all() {
        let db = Arc::new(test_db().await);
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());

        // Insert documents into the store.
        let doc1 = make_doc("e1", vec![1.0, 0.0], VectorCollection::EmailText);
        let doc2 = make_doc("e2", vec![0.0, 1.0], VectorCollection::EmailText);
        let doc3 = make_doc("e3", vec![1.0, 1.0], VectorCollection::ImageText);

        store.insert(doc1).await.unwrap();
        store.insert(doc2).await.unwrap();
        store.insert(doc3).await.unwrap();

        let service = VectorBackupService::new(db.clone(), store.clone(), None);

        let count = service.backup_all().await.unwrap();
        assert_eq!(count, 3);

        // Verify all can be restored.
        let all = service.restore_all().await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn test_restore_nonexistent() {
        let db = Arc::new(test_db().await);
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let service = VectorBackupService::new(db, store, None);

        let result = service
            .restore_vector("00000000-0000-0000-0000-000000000000")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_backup() {
        let db = Arc::new(test_db().await);
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let service = VectorBackupService::new(db, store, None);

        let doc = make_doc("email-del", vec![1.0, 2.0], VectorCollection::EmailText);
        let vector_id = doc.id.to_string();

        service.backup_vector(&doc).await.unwrap();

        // Verify it exists.
        assert!(service.restore_vector(&vector_id).await.unwrap().is_some());

        // Delete.
        service.delete_backup(&vector_id).await.unwrap();

        // Verify it's gone.
        assert!(service.restore_vector(&vector_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_backup_replace_on_conflict() {
        let db = Arc::new(test_db().await);
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let service = VectorBackupService::new(db, store, None);

        let mut doc = make_doc("email-upd", vec![1.0, 2.0], VectorCollection::EmailText);
        let vector_id = doc.id.to_string();

        // First backup.
        service.backup_vector(&doc).await.unwrap();

        // Update the vector and backup again (INSERT OR REPLACE).
        doc.vector = vec![3.0, 4.0];
        service.backup_vector(&doc).await.unwrap();

        // Restored should have the updated vector.
        let restored = service.restore_vector(&vector_id).await.unwrap().unwrap();
        assert_eq!(restored.vector, vec![3.0, 4.0]);
    }

    #[test]
    fn test_f32_roundtrip() {
        let original = vec![0.1_f32, -0.5, 1.0, f32::MAX, f32::MIN, 0.0, f32::EPSILON];
        let bytes = f32_vec_to_bytes(&original);
        let restored = bytes_to_f32_vec(&bytes);
        assert_eq!(original, restored);
    }

    #[test]
    fn test_f32_empty() {
        let empty: Vec<f32> = vec![];
        let bytes = f32_vec_to_bytes(&empty);
        assert!(bytes.is_empty());
        let restored = bytes_to_f32_vec(&bytes);
        assert!(restored.is_empty());
    }

    #[test]
    fn test_parse_collection_variants() {
        assert_eq!(
            parse_collection("email_text").unwrap(),
            VectorCollection::EmailText
        );
        assert_eq!(
            parse_collection("image_text").unwrap(),
            VectorCollection::ImageText
        );
        assert_eq!(
            parse_collection("image_visual").unwrap(),
            VectorCollection::ImageVisual
        );
        assert_eq!(
            parse_collection("attachment_text").unwrap(),
            VectorCollection::AttachmentText
        );
        assert!(parse_collection("unknown").is_err());
    }
}
