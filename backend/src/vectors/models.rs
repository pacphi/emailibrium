//! AI model manifest and lifecycle management (ADR-013).
//!
//! Provides a static catalog of known embedding models, model status tracking,
//! and helpers for querying model availability on disk.

use serde::{Deserialize, Serialize};

/// Metadata about an available AI model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    /// Model identifier (e.g. "all-MiniLM-L6-v2").
    pub name: String,
    /// Provider backend: "onnx", "ollama", or "cloud".
    pub provider: String,
    /// Output embedding dimensions.
    pub dimensions: usize,
    /// Approximate parameter count (e.g. "22M", "33M").
    pub parameters: String,
    /// Approximate ONNX file size in bytes.
    pub onnx_size_bytes: u64,
    /// Maximum input token count.
    pub max_tokens: usize,
    /// Supported languages.
    pub languages: Vec<String>,
    /// Human-readable description.
    pub description: String,
}

/// Static catalog of known embedding models.
pub fn known_models() -> Vec<ModelManifest> {
    vec![
        ModelManifest {
            name: "all-MiniLM-L6-v2".to_string(),
            provider: "onnx".to_string(),
            dimensions: 384,
            parameters: "22M".to_string(),
            onnx_size_bytes: 90_000_000,
            max_tokens: 256,
            languages: vec!["en".to_string()],
            description: "Fast, lightweight English embedding. Best size/quality trade-off for email.".to_string(),
        },
        ModelManifest {
            name: "bge-small-en-v1.5".to_string(),
            provider: "onnx".to_string(),
            dimensions: 384,
            parameters: "33M".to_string(),
            onnx_size_bytes: 127_000_000,
            max_tokens: 512,
            languages: vec!["en".to_string()],
            description: "Higher quality English embedding with longer context window.".to_string(),
        },
        ModelManifest {
            name: "bge-base-en-v1.5".to_string(),
            provider: "onnx".to_string(),
            dimensions: 768,
            parameters: "109M".to_string(),
            onnx_size_bytes: 420_000_000,
            max_tokens: 512,
            languages: vec!["en".to_string()],
            description: "High quality English embedding. Requires more memory.".to_string(),
        },
    ]
}

/// Look up a model manifest by name. Returns `None` if not found.
pub fn get_manifest(name: &str) -> Option<ModelManifest> {
    known_models().into_iter().find(|m| m.name == name)
}

/// Runtime status of a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStatus {
    /// Model identifier.
    pub name: String,
    /// Whether the model files exist on disk.
    pub downloaded: bool,
    /// Whether this is the currently active embedding model.
    pub active: bool,
    /// Output embedding dimensions.
    pub dimensions: usize,
    /// Local cache path, if downloaded.
    pub cache_path: Option<String>,
}

/// Get the status of all known models relative to the active model and cache directory.
pub fn get_model_statuses(active_model: &str, cache_dir: &str) -> Vec<ModelStatus> {
    known_models()
        .iter()
        .map(|m| {
            let cache_path = format!("{}/{}", cache_dir, m.name);
            let downloaded = std::path::Path::new(&cache_path).exists();
            ModelStatus {
                name: m.name.clone(),
                downloaded,
                active: m.name == active_model,
                dimensions: m.dimensions,
                cache_path: if downloaded {
                    Some(cache_path)
                } else {
                    None
                },
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_models_catalog() {
        let models = known_models();
        assert!(
            models.len() >= 3,
            "Expected at least 3 known models, got {}",
            models.len()
        );

        for model in &models {
            assert!(!model.name.is_empty(), "Model name must not be empty");
            assert!(!model.provider.is_empty(), "Provider must not be empty");
            assert!(model.dimensions > 0, "Dimensions must be positive");
            assert!(!model.parameters.is_empty(), "Parameters must not be empty");
            assert!(model.onnx_size_bytes > 0, "ONNX size must be positive");
            assert!(model.max_tokens > 0, "Max tokens must be positive");
            assert!(
                !model.languages.is_empty(),
                "Languages must not be empty"
            );
            assert!(
                !model.description.is_empty(),
                "Description must not be empty"
            );
        }
    }

    #[test]
    fn test_get_manifest_by_name() {
        let manifest = get_manifest("all-MiniLM-L6-v2");
        assert!(manifest.is_some());
        let m = manifest.unwrap();
        assert_eq!(m.name, "all-MiniLM-L6-v2");
        assert_eq!(m.dimensions, 384);
        assert_eq!(m.provider, "onnx");

        let manifest2 = get_manifest("bge-small-en-v1.5");
        assert!(manifest2.is_some());
        assert_eq!(manifest2.unwrap().max_tokens, 512);
    }

    #[test]
    fn test_get_manifest_unknown() {
        let manifest = get_manifest("nonexistent-model");
        assert!(manifest.is_none());
    }

    #[test]
    fn test_model_statuses() {
        // Use a temp dir that does not contain model subdirectories.
        let cache_dir = "/tmp/emailibrium-test-models-nonexistent";
        let statuses = get_model_statuses("all-MiniLM-L6-v2", cache_dir);

        assert_eq!(statuses.len(), known_models().len());

        // The active model should be marked active.
        let active = statuses.iter().find(|s| s.name == "all-MiniLM-L6-v2");
        assert!(active.is_some());
        assert!(active.unwrap().active);

        // Non-active models should not be active.
        let non_active = statuses.iter().find(|s| s.name == "bge-small-en-v1.5");
        assert!(non_active.is_some());
        assert!(!non_active.unwrap().active);

        // None should be downloaded (directory doesn't exist).
        for status in &statuses {
            assert!(!status.downloaded);
            assert!(status.cache_path.is_none());
        }
    }
}
