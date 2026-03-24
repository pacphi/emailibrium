//! CLI model download support (ADR-013, ADR-011, item #33).
//!
//! Provides the `--download-models` CLI subcommand implementation for
//! pre-downloading ONNX models for offline use. Uses fastembed's model
//! download mechanism and verifies SHA-256 integrity after download.

use std::path::{Path, PathBuf};

use tracing::info;

use super::error::VectorError;
use super::model_integrity::{sha256_file, ModelEntry, ModelManifest};

// ---------------------------------------------------------------------------
// Supported models
// ---------------------------------------------------------------------------

/// Models available for download via fastembed.
pub const SUPPORTED_MODELS: &[&str] =
    &["all-MiniLM-L6-v2", "bge-small-en-v1.5", "bge-base-en-v1.5"];

/// Model dimensions lookup.
pub fn model_dimensions(model: &str) -> usize {
    match model {
        "all-MiniLM-L6-v2" => 384,
        "bge-small-en-v1.5" => 384,
        "bge-base-en-v1.5" => 768,
        _ => 384, // default
    }
}

// ---------------------------------------------------------------------------
// Download logic
// ---------------------------------------------------------------------------

/// Result of downloading a single model.
#[derive(Debug, Clone)]
pub struct DownloadResult {
    /// Model name.
    pub model: String,
    /// Whether the download was successful.
    pub success: bool,
    /// SHA-256 hash of the downloaded model (if available).
    pub sha256: Option<String>,
    /// Where the model was cached.
    pub cache_path: Option<PathBuf>,
    /// Error message if download failed.
    pub error: Option<String>,
}

/// Download a single model using fastembed's initialization mechanism.
///
/// Fastembed automatically downloads and caches models on first use.
/// This function triggers that download by initializing the model.
pub fn download_model(model_name: &str, cache_dir: Option<&Path>) -> DownloadResult {
    info!(model = %model_name, "Downloading model...");

    // Validate model name.
    if !SUPPORTED_MODELS.contains(&model_name) {
        return DownloadResult {
            model: model_name.to_string(),
            success: false,
            sha256: None,
            cache_path: None,
            error: Some(format!(
                "Unsupported model: {model_name}. Supported: {}",
                SUPPORTED_MODELS.join(", ")
            )),
        };
    }

    // Use fastembed to trigger model download.
    // fastembed::TextEmbedding::try_new() downloads the model if not cached.
    let model_enum = match model_name {
        "all-MiniLM-L6-v2" => fastembed::EmbeddingModel::AllMiniLML6V2,
        "bge-small-en-v1.5" => fastembed::EmbeddingModel::BGESmallENV15,
        "bge-base-en-v1.5" => fastembed::EmbeddingModel::BGEBaseENV15,
        _ => {
            return DownloadResult {
                model: model_name.to_string(),
                success: false,
                sha256: None,
                cache_path: None,
                error: Some(format!("No fastembed mapping for: {model_name}")),
            };
        }
    };

    let mut opts = fastembed::InitOptions::new(model_enum).with_show_download_progress(true);
    if let Some(dir) = cache_dir {
        opts = opts.with_cache_dir(dir.to_path_buf());
    }

    match fastembed::TextEmbedding::try_new(opts) {
        Ok(_) => {
            info!(model = %model_name, "Model downloaded and verified successfully");

            // Try to find the cached model file and compute its hash.
            let resolved_dir = cache_dir
                .map(|d| d.to_path_buf())
                .unwrap_or_else(|| super::model_integrity::resolve_models_dir(None));

            let model_path = resolved_dir.join(model_name).join("model.onnx");
            let sha256 = if model_path.exists() {
                sha256_file(&model_path).ok()
            } else {
                None
            };

            DownloadResult {
                model: model_name.to_string(),
                success: true,
                sha256,
                cache_path: Some(resolved_dir.join(model_name)),
                error: None,
            }
        }
        Err(e) => DownloadResult {
            model: model_name.to_string(),
            success: false,
            sha256: None,
            cache_path: None,
            error: Some(format!("Download failed: {e}")),
        },
    }
}

/// Download all supported models.
pub fn download_all_models(cache_dir: Option<&Path>) -> Vec<DownloadResult> {
    SUPPORTED_MODELS
        .iter()
        .map(|model| download_model(model, cache_dir))
        .collect()
}

/// Download models and update the manifest with computed hashes.
pub fn download_and_update_manifest(
    models: &[&str],
    cache_dir: Option<&Path>,
    manifest_path: &Path,
) -> Result<(Vec<DownloadResult>, ModelManifest), VectorError> {
    // Load or create manifest.
    let mut manifest = if manifest_path.exists() {
        ModelManifest::load_from_file(manifest_path)?
    } else {
        ModelManifest::default()
    };

    let results: Vec<DownloadResult> = models
        .iter()
        .map(|model| download_model(model, cache_dir))
        .collect();

    // Update manifest with computed hashes.
    for result in &results {
        if result.success {
            if let Some(ref hash) = result.sha256 {
                if let Some(entry) = manifest.models.get_mut(&result.model) {
                    entry.sha256 = hash.clone();
                } else {
                    manifest.models.insert(
                        result.model.clone(),
                        ModelEntry {
                            name: result.model.clone(),
                            sha256: hash.clone(),
                            path: format!("{}/model.onnx", result.model),
                            size_bytes: None,
                        },
                    );
                }
            }
        }
    }

    // Save updated manifest.
    manifest.save_to_file(manifest_path)?;

    Ok((results, manifest))
}

// ---------------------------------------------------------------------------
// CLI entry point
// ---------------------------------------------------------------------------

/// Execute the `--download-models` CLI command.
///
/// This is called from `main.rs` when the user passes `--download-models`.
pub fn run_download_models_cli(
    models: Option<Vec<String>>,
    models_dir: Option<String>,
) -> Result<(), VectorError> {
    let cache_dir = models_dir.as_deref().map(Path::new);

    let models_to_download: Vec<&str> = match &models {
        Some(m) => m.iter().map(|s| s.as_str()).collect(),
        None => SUPPORTED_MODELS.to_vec(),
    };

    println!("Emailibrium Model Downloader");
    println!("============================");
    println!();
    println!("Models to download: {}", models_to_download.join(", "));
    if let Some(dir) = &models_dir {
        println!("Cache directory: {dir}");
    }
    println!();

    let results: Vec<DownloadResult> = models_to_download
        .iter()
        .map(|model| {
            println!("Downloading {model}...");
            let result = download_model(model, cache_dir);
            if result.success {
                println!("  OK: {model}");
                if let Some(ref hash) = result.sha256 {
                    println!("  SHA-256: {hash}");
                }
                if let Some(ref path) = result.cache_path {
                    println!("  Cached at: {}", path.display());
                }
            } else {
                println!(
                    "  FAILED: {}",
                    result.error.as_deref().unwrap_or("unknown error")
                );
            }
            println!();
            result
        })
        .collect();

    let success_count = results.iter().filter(|r| r.success).count();
    let fail_count = results.len() - success_count;

    println!("Summary: {success_count} downloaded, {fail_count} failed");

    if fail_count > 0 {
        Err(VectorError::ConfigError(format!(
            "{fail_count} model(s) failed to download"
        )))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_models() {
        assert_eq!(SUPPORTED_MODELS.len(), 3);
        assert!(SUPPORTED_MODELS.contains(&"all-MiniLM-L6-v2"));
        assert!(SUPPORTED_MODELS.contains(&"bge-small-en-v1.5"));
        assert!(SUPPORTED_MODELS.contains(&"bge-base-en-v1.5"));
    }

    #[test]
    fn test_model_dimensions() {
        assert_eq!(model_dimensions("all-MiniLM-L6-v2"), 384);
        assert_eq!(model_dimensions("bge-small-en-v1.5"), 384);
        assert_eq!(model_dimensions("bge-base-en-v1.5"), 768);
        assert_eq!(model_dimensions("unknown"), 384);
    }

    #[test]
    fn test_download_unsupported_model() {
        let result = download_model("nonexistent-model", None);
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap().contains("Unsupported"));
    }
}
