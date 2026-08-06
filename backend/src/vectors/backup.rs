//! SQLite-backed vector backup and restore service (ADR-003: S1-04).
//!
//! Persists vector embeddings to the `vector_backups` table so that the
//! in-memory vector store can be rebuilt after restarts without re-embedding.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};

use super::encryption::EncryptedVectorStore;
use super::error::VectorError;
use super::store::VectorStoreBackend;
use super::types::*;
use crate::db::entities::vector_backups;
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
    conn: DatabaseConnection,
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
            conn: db.sea_orm(),
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
        // Every non-key column is in the insert, so the pre-port
        // `INSERT OR REPLACE` and this DO-UPDATE upsert are observably
        // identical. `created_at`/`updated_at` are plain TIMESTAMPs —
        // naive-UTC binds.
        let now = Utc::now().naive_utc();

        vector_backups::Entity::insert(vector_backups::ActiveModel {
            vector_id: Set(vector_id),
            email_id: Set(doc.email_id.clone()),
            collection: Set(collection),
            dimensions: Set(doc.vector.len() as i32),
            vector_data: Set(vector_data),
            metadata_json: Set(metadata_json),
            created_at: Set(Some(now)),
            updated_at: Set(Some(now)),
        })
        .on_conflict(
            OnConflict::column(vector_backups::Column::VectorId)
                .update_columns([
                    vector_backups::Column::EmailId,
                    vector_backups::Column::Collection,
                    vector_backups::Column::Dimensions,
                    vector_backups::Column::VectorData,
                    vector_backups::Column::MetadataJson,
                    vector_backups::Column::CreatedAt,
                    vector_backups::Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec_without_returning(&self.conn)
        .await
        .map_err(VectorError::Db)?;

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
        let row: Option<BackupRow> = vector_backups::Entity::find()
            .select_only()
            .column(vector_backups::Column::VectorId)
            .column(vector_backups::Column::EmailId)
            .column(vector_backups::Column::Collection)
            .column(vector_backups::Column::VectorData)
            .column(vector_backups::Column::MetadataJson)
            .filter(vector_backups::Column::VectorId.eq(vector_id))
            .into_tuple()
            .one(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        match row {
            Some(r) => Ok(Some(self.row_to_document(r)?)),
            None => Ok(None),
        }
    }

    /// Restore all vectors from the SQLite backup.
    pub async fn restore_all(&self) -> Result<Vec<VectorDocument>, VectorError> {
        let rows: Vec<BackupRow> = vector_backups::Entity::find()
            .select_only()
            .column(vector_backups::Column::VectorId)
            .column(vector_backups::Column::EmailId)
            .column(vector_backups::Column::Collection)
            .column(vector_backups::Column::VectorData)
            .column(vector_backups::Column::MetadataJson)
            .into_tuple()
            .all(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        let mut docs = Vec::with_capacity(rows.len());
        for row in rows {
            docs.push(self.row_to_document(row)?);
        }
        Ok(docs)
    }

    /// Delete a backup entry by vector ID.
    pub async fn delete_backup(&self, vector_id: &str) -> Result<(), VectorError> {
        vector_backups::Entity::delete_many()
            .filter(vector_backups::Column::VectorId.eq(vector_id))
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        Ok(())
    }

    // -- private helpers -----------------------------------------------------

    /// Convert a database row into a `VectorDocument`.
    fn row_to_document(&self, row: BackupRow) -> Result<VectorDocument, VectorError> {
        let (vector_id, email_id, collection, vector_data, metadata_json) = row;

        let vector = match &self.encryption {
            Some(enc) => enc.decrypt_vector(&vector_data)?,
            None => bytes_to_f32_vec(&vector_data),
        };

        let metadata: HashMap<String, String> = match &metadata_json {
            Some(json) => serde_json::from_str(json)?,
            None => HashMap::new(),
        };

        let collection = parse_collection(&collection)?;

        let id = uuid::Uuid::parse_str(&vector_id)
            .map(VectorId)
            .map_err(|e| VectorError::BackupError(format!("invalid vector_id UUID: {e}")))?;

        Ok(VectorDocument {
            id,
            email_id,
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

/// Row tuple for reading from the `vector_backups` table:
/// `(vector_id, email_id, collection, vector_data, metadata_json)` — the
/// unread `dimensions` column is no longer selected.
type BackupRow = (String, String, String, Vec<u8>, Option<String>);

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
        use sea_orm::ConnectionTrait;

        let db = crate::db::test_sqlite_database().await;
        let conn = db.sea_orm();

        conn.execute_unprepared("PRAGMA foreign_keys = OFF")
            .await
            .unwrap();

        let raw = include_str!("../../migrations/sqlite/001_initial_schema.sql");
        let cleaned: String = raw
            .lines()
            .map(|l| {
                if let Some(idx) = l.find("--") {
                    &l[..idx]
                } else {
                    l
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        for stmt in cleaned.split(';') {
            let s = stmt.trim();
            if !s.is_empty() {
                conn.execute_unprepared(s).await.unwrap();
            }
        }

        db
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
