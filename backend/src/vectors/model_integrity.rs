#![allow(dead_code)]
//! Model integrity verification via SHA-256 checksums (ADR-013, item #32).
//!
//! Verifies that ONNX model files on disk have not been tampered with by
//! comparing their SHA-256 hash against a known-good manifest. This guards
//! against supply-chain attacks or corrupted downloads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use super::error::VectorError;

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// A single model entry in the integrity manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Model name (e.g., "all-MiniLM-L6-v2").
    pub name: String,
    /// Expected SHA-256 hex digest of the primary ONNX file.
    pub sha256: String,
    /// Relative path within the models directory.
    pub path: String,
    /// File size in bytes (for quick pre-check).
    pub size_bytes: Option<u64>,
}

/// Manifest of expected model checksums.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    /// Version of the manifest format.
    pub version: u32,
    /// Model entries keyed by model name.
    pub models: HashMap<String, ModelEntry>,
}

impl Default for ModelManifest {
    fn default() -> Self {
        let mut models = HashMap::new();

        // Known models supported by Emailibrium (ADR-011/ADR-013).
        // Note: checksums are placeholder values that should be updated
        // after the first verified download. The download-models CLI
        // command can generate the correct checksums.
        models.insert(
            "all-MiniLM-L6-v2".to_string(),
            ModelEntry {
                name: "all-MiniLM-L6-v2".to_string(),
                sha256: String::new(), // populated after first verified download
                path: "all-MiniLM-L6-v2/model.onnx".to_string(),
                size_bytes: None,
            },
        );
        models.insert(
            "bge-small-en-v1.5".to_string(),
            ModelEntry {
                name: "bge-small-en-v1.5".to_string(),
                sha256: String::new(),
                path: "bge-small-en-v1.5/model.onnx".to_string(),
                size_bytes: None,
            },
        );
        models.insert(
            "bge-base-en-v1.5".to_string(),
            ModelEntry {
                name: "bge-base-en-v1.5".to_string(),
                sha256: String::new(),
                path: "bge-base-en-v1.5/model.onnx".to_string(),
                size_bytes: None,
            },
        );

        Self { version: 1, models }
    }
}

impl ModelManifest {
    /// Load a manifest from a JSON file.
    pub fn load_from_file(path: &Path) -> Result<Self, VectorError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| VectorError::ConfigError(format!("Failed to read model manifest: {e}")))?;
        serde_json::from_str(&contents)
            .map_err(|e| VectorError::ConfigError(format!("Failed to parse model manifest: {e}")))
    }

    /// Save the manifest to a JSON file.
    pub fn save_to_file(&self, path: &Path) -> Result<(), VectorError> {
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(path, contents)
            .map_err(|e| VectorError::ConfigError(format!("Failed to write model manifest: {e}")))
    }
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

/// Result of verifying a single model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Model name.
    pub model: String,
    /// Whether the model passed integrity verification.
    pub verified: bool,
    /// Computed SHA-256 hash.
    pub computed_hash: String,
    /// Expected SHA-256 hash from the manifest.
    pub expected_hash: String,
    /// File path that was checked.
    pub file_path: String,
    /// Any error message if verification failed.
    pub error: Option<String>,
}

/// Compute the SHA-256 hash of a file.
pub fn sha256_file(path: &Path) -> Result<String, VectorError> {
    let file_bytes = std::fs::read(path).map_err(|e| {
        VectorError::ConfigError(format!(
            "Failed to read file for hashing: {}: {e}",
            path.display()
        ))
    })?;

    let mut hasher = Sha256::new();
    hasher.update(&file_bytes);
    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

/// Compute the SHA-256 hash of a byte slice.
pub fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("{:x}", result)
}

/// Verify a single model file against the manifest.
pub fn verify_model(models_dir: &Path, entry: &ModelEntry) -> VerificationResult {
    let file_path = models_dir.join(&entry.path);

    // Check if file exists.
    if !file_path.exists() {
        return VerificationResult {
            model: entry.name.clone(),
            verified: false,
            computed_hash: String::new(),
            expected_hash: entry.sha256.clone(),
            file_path: file_path.display().to_string(),
            error: Some("Model file not found".to_string()),
        };
    }

    // If expected hash is empty, we cannot verify but report the computed hash.
    if entry.sha256.is_empty() {
        match sha256_file(&file_path) {
            Ok(hash) => {
                info!(
                    model = %entry.name,
                    hash = %hash,
                    "Model hash computed (no expected hash in manifest)"
                );
                return VerificationResult {
                    model: entry.name.clone(),
                    verified: false,
                    computed_hash: hash,
                    expected_hash: String::new(),
                    file_path: file_path.display().to_string(),
                    error: Some(
                        "No expected hash in manifest; run --download-models to populate"
                            .to_string(),
                    ),
                };
            }
            Err(e) => {
                return VerificationResult {
                    model: entry.name.clone(),
                    verified: false,
                    computed_hash: String::new(),
                    expected_hash: entry.sha256.clone(),
                    file_path: file_path.display().to_string(),
                    error: Some(format!("Failed to compute hash: {e}")),
                };
            }
        }
    }

    // Optional size check for quick pre-filter.
    if let Some(expected_size) = entry.size_bytes {
        if let Ok(metadata) = std::fs::metadata(&file_path) {
            if metadata.len() != expected_size {
                return VerificationResult {
                    model: entry.name.clone(),
                    verified: false,
                    computed_hash: String::new(),
                    expected_hash: entry.sha256.clone(),
                    file_path: file_path.display().to_string(),
                    error: Some(format!(
                        "Size mismatch: expected {} bytes, got {} bytes",
                        expected_size,
                        metadata.len()
                    )),
                };
            }
        }
    }

    // Compute and compare SHA-256.
    match sha256_file(&file_path) {
        Ok(computed) => {
            let verified = computed == entry.sha256;
            if !verified {
                warn!(
                    model = %entry.name,
                    expected = %entry.sha256,
                    computed = %computed,
                    "Model integrity check FAILED: hash mismatch"
                );
            } else {
                info!(model = %entry.name, "Model integrity verified");
            }
            VerificationResult {
                model: entry.name.clone(),
                verified,
                computed_hash: computed,
                expected_hash: entry.sha256.clone(),
                file_path: file_path.display().to_string(),
                error: if verified {
                    None
                } else {
                    Some("SHA-256 hash mismatch: possible tampering detected".to_string())
                },
            }
        }
        Err(e) => VerificationResult {
            model: entry.name.clone(),
            verified: false,
            computed_hash: String::new(),
            expected_hash: entry.sha256.clone(),
            file_path: file_path.display().to_string(),
            error: Some(format!("Failed to compute hash: {e}")),
        },
    }
}

/// Verify all models in the manifest.
pub fn verify_all_models(models_dir: &Path, manifest: &ModelManifest) -> Vec<VerificationResult> {
    manifest
        .models
        .values()
        .map(|entry| verify_model(models_dir, entry))
        .collect()
}

/// Resolve the models directory from configuration or default paths.
pub fn resolve_models_dir(cache_dir: Option<&str>) -> PathBuf {
    if let Some(dir) = cache_dir {
        PathBuf::from(dir)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".cache")
            .join("emailibrium")
            .join("models")
    } else {
        PathBuf::from("data").join("models")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_sha256_bytes() {
        let hash = sha256_bytes(b"hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_sha256_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.bin");
        let mut file = std::fs::File::create(&file_path).unwrap();
        file.write_all(b"hello world").unwrap();
        drop(file);

        let hash = sha256_file(&file_path).unwrap();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_sha256_file_not_found() {
        let result = sha256_file(Path::new("/nonexistent/file.bin"));
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_model_file_not_found() {
        let entry = ModelEntry {
            name: "test-model".to_string(),
            sha256: "abc123".to_string(),
            path: "test/model.onnx".to_string(),
            size_bytes: None,
        };
        let result = verify_model(Path::new("/nonexistent"), &entry);
        assert!(!result.verified);
        assert!(result.error.as_deref().unwrap().contains("not found"));
    }

    #[test]
    fn test_verify_model_hash_match() {
        let dir = tempfile::tempdir().unwrap();
        let model_dir = dir.path().join("my-model");
        std::fs::create_dir_all(&model_dir).unwrap();
        let model_path = model_dir.join("model.onnx");
        std::fs::write(&model_path, b"fake model data").unwrap();

        let expected_hash = sha256_bytes(b"fake model data");

        let entry = ModelEntry {
            name: "my-model".to_string(),
            sha256: expected_hash.clone(),
            path: "my-model/model.onnx".to_string(),
            size_bytes: Some(15),
        };

        let result = verify_model(dir.path(), &entry);
        assert!(result.verified);
        assert_eq!(result.computed_hash, expected_hash);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_verify_model_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let model_dir = dir.path().join("bad-model");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.onnx"), b"tampered data").unwrap();

        let entry = ModelEntry {
            name: "bad-model".to_string(),
            sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            path: "bad-model/model.onnx".to_string(),
            size_bytes: None,
        };

        let result = verify_model(dir.path(), &entry);
        assert!(!result.verified);
        assert!(result.error.as_deref().unwrap().contains("mismatch"));
    }

    #[test]
    fn test_verify_model_size_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let model_dir = dir.path().join("sized-model");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.onnx"), b"data").unwrap();

        let entry = ModelEntry {
            name: "sized-model".to_string(),
            sha256: "abc".to_string(),
            path: "sized-model/model.onnx".to_string(),
            size_bytes: Some(999), // wrong size
        };

        let result = verify_model(dir.path(), &entry);
        assert!(!result.verified);
        assert!(result.error.as_deref().unwrap().contains("Size mismatch"));
    }

    #[test]
    fn test_verify_model_empty_hash() {
        let dir = tempfile::tempdir().unwrap();
        let model_dir = dir.path().join("no-hash");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.onnx"), b"data").unwrap();

        let entry = ModelEntry {
            name: "no-hash".to_string(),
            sha256: String::new(),
            path: "no-hash/model.onnx".to_string(),
            size_bytes: None,
        };

        let result = verify_model(dir.path(), &entry);
        assert!(!result.verified); // Cannot verify without expected hash.
        assert!(!result.computed_hash.is_empty()); // But hash was computed.
    }

    #[test]
    fn test_default_manifest() {
        let manifest = ModelManifest::default();
        assert_eq!(manifest.version, 1);
        assert!(manifest.models.contains_key("all-MiniLM-L6-v2"));
        assert!(manifest.models.contains_key("bge-small-en-v1.5"));
        assert!(manifest.models.contains_key("bge-base-en-v1.5"));
    }

    #[test]
    fn test_manifest_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");

        let manifest = ModelManifest::default();
        manifest.save_to_file(&path).unwrap();

        let loaded = ModelManifest::load_from_file(&path).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.models.len(), manifest.models.len());
    }

    #[test]
    fn test_resolve_models_dir_custom() {
        let dir = resolve_models_dir(Some("/custom/models"));
        assert_eq!(dir, PathBuf::from("/custom/models"));
    }

    #[test]
    fn test_resolve_models_dir_default() {
        let dir = resolve_models_dir(None);
        // Should end with models directory.
        assert!(
            dir.to_str().unwrap().ends_with("models"),
            "Expected models dir, got: {}",
            dir.display()
        );
    }

    #[test]
    fn test_verify_all_models() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = ModelManifest {
            version: 1,
            models: HashMap::from([
                (
                    "m1".to_string(),
                    ModelEntry {
                        name: "m1".to_string(),
                        sha256: "abc".to_string(),
                        path: "m1/model.onnx".to_string(),
                        size_bytes: None,
                    },
                ),
                (
                    "m2".to_string(),
                    ModelEntry {
                        name: "m2".to_string(),
                        sha256: "def".to_string(),
                        path: "m2/model.onnx".to_string(),
                        size_bytes: None,
                    },
                ),
            ]),
        };

        let results = verify_all_models(dir.path(), &manifest);
        assert_eq!(results.len(), 2);
        // Both should fail (files don't exist).
        assert!(results.iter().all(|r| !r.verified));
    }
}
