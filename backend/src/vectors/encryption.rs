//! Vector encryption at rest (S1-03: ADR-008).
//!
//! `EncryptedVectorStore` wraps any `VectorStoreBackend` and provides
//! AES-256-GCM encryption/decryption methods for at-rest storage.
//!
//! In-memory operations are delegated unchanged to the inner store.
//! Encryption is applied at the persistence boundary (backup/restore)
//! via the `encrypt_vector` and `decrypt_vector` methods.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use argon2::Argon2;
use async_trait::async_trait;
use rand::RngCore;
use std::sync::Arc;
use zeroize::Zeroizing;

use super::config::EncryptionConfig;
use super::error::VectorError;
use super::store::VectorStoreBackend;
use super::types::{
    ScoredResult, SearchParams, VectorCollection, VectorDocument, VectorId, VectorStats,
};

/// Fixed salt used for deterministic key derivation from the master password.
const KEY_DERIVATION_SALT: &[u8] = b"emailibrium-vector-encryption-v1";

/// AES-256-GCM nonce size in bytes (96 bits).
const NONCE_SIZE: usize = 12;

/// Derive a 256-bit key from a password and salt using Argon2id.
///
/// The returned key is wrapped in `Zeroizing` so it is automatically
/// zeroed from memory when dropped.
pub fn derive_key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, VectorError> {
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| VectorError::EncryptionError(format!("Key derivation failed: {e}")))?;
    Ok(key)
}

/// A vector store wrapper that delegates all in-memory operations to an
/// inner `VectorStoreBackend` and provides encryption/decryption for
/// at-rest persistence.
///
/// The encryption key is derived from a master password via Argon2id and
/// stored in a zeroize-on-drop wrapper so it never lingers in memory
/// after the store is dropped.
pub struct EncryptedVectorStore {
    inner: Arc<dyn VectorStoreBackend>,
    key: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for EncryptedVectorStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptedVectorStore")
            .field("inner", &"<VectorStoreBackend>")
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl EncryptedVectorStore {
    /// Create a new `EncryptedVectorStore` wrapping `inner`.
    ///
    /// Returns `VectorError::EncryptionError` if the master password is missing.
    pub fn new(
        inner: Arc<dyn VectorStoreBackend>,
        config: &EncryptionConfig,
    ) -> Result<Self, VectorError> {
        let password = config
            .master_password
            .as_deref()
            .ok_or_else(|| VectorError::EncryptionError("Master password required".to_string()))?;

        let key = derive_key(password, KEY_DERIVATION_SALT)?;

        Ok(Self { inner, key })
    }

    /// Encrypt an f32 vector into an opaque byte buffer.
    ///
    /// Layout: `[12-byte nonce | ciphertext]`
    ///
    /// Each call generates a fresh random 96-bit nonce, so encrypting the
    /// same vector twice produces different ciphertexts.
    pub fn encrypt_vector(&self, vector: &[f32]) -> Result<Vec<u8>, VectorError> {
        let cipher = Aes256Gcm::new_from_slice(self.key.as_ref())
            .map_err(|e| VectorError::EncryptionError(format!("Cipher init failed: {e}")))?;

        // Serialize f32 slice to raw little-endian bytes.
        let plaintext = f32_slice_to_bytes(vector);

        // Generate a random 96-bit nonce.
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|e| VectorError::EncryptionError(format!("Encryption failed: {e}")))?;

        // Prepend nonce to ciphertext.
        let mut output = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    /// Decrypt an opaque byte buffer back into an f32 vector.
    ///
    /// Expects the layout produced by `encrypt_vector`: `[12-byte nonce | ciphertext]`.
    pub fn decrypt_vector(&self, encrypted: &[u8]) -> Result<Vec<f32>, VectorError> {
        if encrypted.len() < NONCE_SIZE {
            return Err(VectorError::DecryptionError(
                "Ciphertext too short to contain nonce".to_string(),
            ));
        }

        let cipher = Aes256Gcm::new_from_slice(self.key.as_ref())
            .map_err(|e| VectorError::DecryptionError(format!("Cipher init failed: {e}")))?;

        let (nonce_bytes, ciphertext) = encrypted.split_at(NONCE_SIZE);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| VectorError::DecryptionError(format!("Decryption failed: {e}")))?;

        bytes_to_f32_vec(&plaintext)
    }
}

/// Serialize a slice of f32 values to raw little-endian bytes.
fn f32_slice_to_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for &v in values {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// Deserialize raw little-endian bytes back into a Vec<f32>.
fn bytes_to_f32_vec(bytes: &[u8]) -> Result<Vec<f32>, VectorError> {
    if bytes.len() % 4 != 0 {
        return Err(VectorError::DecryptionError(format!(
            "Decrypted data length {} is not a multiple of 4",
            bytes.len()
        )));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap();
            f32::from_le_bytes(arr)
        })
        .collect())
}

// ---------------------------------------------------------------------------
// VectorStoreBackend delegation -- all operations pass through unchanged
// ---------------------------------------------------------------------------

#[async_trait]
impl VectorStoreBackend for EncryptedVectorStore {
    async fn insert(&self, doc: VectorDocument) -> Result<VectorId, VectorError> {
        self.inner.insert(doc).await
    }

    async fn batch_insert(&self, docs: Vec<VectorDocument>) -> Result<Vec<VectorId>, VectorError> {
        self.inner.batch_insert(docs).await
    }

    async fn search(&self, params: &SearchParams) -> Result<Vec<ScoredResult>, VectorError> {
        self.inner.search(params).await
    }

    async fn get(&self, id: &VectorId) -> Result<Option<VectorDocument>, VectorError> {
        self.inner.get(id).await
    }

    async fn get_by_email_id(&self, email_id: &str) -> Result<Option<VectorDocument>, VectorError> {
        self.inner.get_by_email_id(email_id).await
    }

    async fn delete(&self, id: &VectorId) -> Result<bool, VectorError> {
        self.inner.delete(id).await
    }

    async fn update(&self, doc: VectorDocument) -> Result<(), VectorError> {
        self.inner.update(doc).await
    }

    async fn health(&self) -> Result<bool, VectorError> {
        self.inner.health().await
    }

    async fn stats(&self) -> Result<VectorStats, VectorError> {
        self.inner.stats().await
    }

    async fn count(&self) -> Result<u64, VectorError> {
        self.inner.count().await
    }

    async fn list_by_collection(
        &self,
        collection: &VectorCollection,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<VectorDocument>, VectorError> {
        self.inner
            .list_by_collection(collection, limit, offset)
            .await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectors::store::InMemoryVectorStore;
    use chrono::Utc;
    use std::collections::HashMap;

    fn test_config(password: Option<&str>) -> EncryptionConfig {
        EncryptionConfig {
            enabled: true,
            master_password: password.map(|s| s.to_string()),
        }
    }

    fn make_doc(email_id: &str, vector: Vec<f32>) -> VectorDocument {
        VectorDocument {
            id: VectorId::new(),
            email_id: email_id.to_string(),
            vector,
            metadata: HashMap::new(),
            collection: VectorCollection::EmailText,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_key_derivation_deterministic() {
        let key1 = derive_key("my-secret-password", KEY_DERIVATION_SALT).unwrap();
        let key2 = derive_key("my-secret-password", KEY_DERIVATION_SALT).unwrap();
        assert_eq!(key1.as_ref(), key2.as_ref());

        // Different password produces different key.
        let key3 = derive_key("different-password", KEY_DERIVATION_SALT).unwrap();
        assert_ne!(key1.as_ref(), key3.as_ref());
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let inner: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let store = EncryptedVectorStore::new(inner, &test_config(Some("test-password"))).unwrap();

        let original = vec![1.0_f32, 2.5, -3.7, 0.0, 42.42];
        let encrypted = store.encrypt_vector(&original).unwrap();

        // Encrypted data must differ from plaintext bytes.
        let plain_bytes = f32_slice_to_bytes(&original);
        assert_ne!(encrypted, plain_bytes);

        // Encrypted output must be larger (nonce + auth tag overhead).
        assert!(encrypted.len() > plain_bytes.len());

        let decrypted = store.decrypt_vector(&encrypted).unwrap();
        assert_eq!(original, decrypted);
    }

    #[test]
    fn test_encrypt_produces_different_ciphertexts() {
        let inner: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let store = EncryptedVectorStore::new(inner, &test_config(Some("test-password"))).unwrap();

        let vector = vec![1.0, 2.0, 3.0];
        let enc1 = store.encrypt_vector(&vector).unwrap();
        let enc2 = store.encrypt_vector(&vector).unwrap();

        // Random nonces mean different ciphertexts.
        assert_ne!(enc1, enc2);

        // Both must decrypt to the same vector.
        assert_eq!(store.decrypt_vector(&enc1).unwrap(), vector);
        assert_eq!(store.decrypt_vector(&enc2).unwrap(), vector);
    }

    #[test]
    fn test_decrypt_with_wrong_key_fails() {
        let inner1: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let store1 = EncryptedVectorStore::new(inner1, &test_config(Some("password-A"))).unwrap();

        let inner2: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let store2 = EncryptedVectorStore::new(inner2, &test_config(Some("password-B"))).unwrap();

        let vector = vec![1.0, 2.0, 3.0];
        let encrypted = store1.encrypt_vector(&vector).unwrap();

        let result = store2.decrypt_vector(&encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_truncated_data_fails() {
        let inner: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let store = EncryptedVectorStore::new(inner, &test_config(Some("test-password"))).unwrap();

        // Too short to contain a nonce.
        let result = store.decrypt_vector(&[0u8; 5]);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_encrypted_store_delegates_operations() {
        let inner: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let store =
            EncryptedVectorStore::new(inner.clone(), &test_config(Some("my-password"))).unwrap();

        // Insert through the encrypted store.
        let doc = make_doc("email-123", vec![1.0, 0.0, 0.0]);
        let id = store.insert(doc).await.unwrap();

        // Retrieve through the encrypted store.
        let retrieved = store.get(&id).await.unwrap().unwrap();
        assert_eq!(retrieved.email_id, "email-123");
        assert_eq!(retrieved.vector, vec![1.0, 0.0, 0.0]);

        // The inner store should also have the same document (plaintext in-memory).
        let inner_doc = inner.get(&id).await.unwrap().unwrap();
        assert_eq!(inner_doc.email_id, "email-123");

        // Health and stats should delegate.
        assert!(store.health().await.unwrap());
        let stats = store.stats().await.unwrap();
        assert_eq!(stats.total_vectors, 1);
    }

    #[test]
    fn test_encryption_requires_password() {
        let inner: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let result = EncryptedVectorStore::new(inner, &test_config(None));
        assert!(result.is_err());

        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Master password required"),
            "Expected 'Master password required', got: {msg}"
        );
    }

    #[test]
    fn test_encrypt_empty_vector() {
        let inner: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let store = EncryptedVectorStore::new(inner, &test_config(Some("password"))).unwrap();

        let encrypted = store.encrypt_vector(&[]).unwrap();
        let decrypted = store.decrypt_vector(&encrypted).unwrap();
        assert!(decrypted.is_empty());
    }
}
