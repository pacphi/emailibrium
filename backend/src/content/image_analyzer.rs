//! Image analysis for CLIP-based embedding and description (ADR-006).
//!
//! This module provides image analysis using fastembed's CLIP model support
//! (ViT-B-32 via ONNX Runtime) for generating image embeddings that live
//! alongside text embeddings in the vector store.
//!
//! CLIP embeddings enable:
//! - Semantic image search ("find emails with receipts")
//! - Cross-modal retrieval (text query -> image results)
//! - Image similarity clustering
//!
//! Configuration: `content.clip.enabled`, `content.clip.model` in config.yaml.
//!
//! Dependencies: `fastembed` (already in Cargo.toml for text embeddings),
//! `image` for format decoding.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::types::ExtractedImage;

/// Configuration for CLIP image embedding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipConfig {
    /// Whether CLIP embedding is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// CLIP model name (must be supported by fastembed).
    /// Default: "clip-ViT-B-32"
    #[serde(default = "default_clip_model")]
    pub model: String,
    /// Embedding dimensions for the CLIP model.
    #[serde(default = "default_clip_dimensions")]
    pub dimensions: usize,
}

impl Default for ClipConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: default_clip_model(),
            dimensions: default_clip_dimensions(),
        }
    }
}

fn default_clip_model() -> String {
    "clip-ViT-B-32".to_string()
}

fn default_clip_dimensions() -> usize {
    512
}

/// Wraps fastembed's `ImageEmbedding` for CLIP vector generation.
///
/// The fastembed `ImageEmbedding::embed_bytes` call requires `&mut self`,
/// so we wrap it in a `Mutex` for interior mutability (same pattern as
/// `OnnxEmbeddingModel` in `vectors/embedding.rs`).
struct ClipEmbedder {
    model: Mutex<fastembed::ImageEmbedding>,
}

impl ClipEmbedder {
    /// Initialize the CLIP model. Downloads the ONNX model on first use.
    fn try_new() -> Result<Self, anyhow::Error> {
        use fastembed::{ImageEmbeddingModel, ImageInitOptions};

        let options = ImageInitOptions::new(ImageEmbeddingModel::ClipVitB32)
            .with_show_download_progress(true);

        let model = fastembed::ImageEmbedding::try_new(options)?;
        Ok(Self {
            model: Mutex::new(model),
        })
    }

    /// Generate a CLIP embedding for raw image bytes.
    ///
    /// Returns `None` if the image cannot be decoded or embedding fails.
    fn embed_bytes(&self, data: &[u8]) -> Option<Vec<f32>> {
        let mut model = self.model.lock().ok()?;
        let images: Vec<&[u8]> = vec![data];
        match model.embed_bytes(&images, None) {
            Ok(embeddings) => embeddings.into_iter().next(),
            Err(e) => {
                warn!(error = %e, "CLIP embedding generation failed");
                None
            }
        }
    }
}

/// Image analyser that wraps CLIP models when configured.
///
/// When CLIP is not enabled, falls back to metadata-only extraction
/// (content type, dimensions from image header).
pub struct ImageAnalyzer {
    clip_enabled: bool,
    clip_embedder: Option<ClipEmbedder>,
}

impl ImageAnalyzer {
    /// Create a new image analyzer with the given CLIP configuration.
    ///
    /// If CLIP is enabled, initializes the fastembed CLIP model (ViT-B-32).
    /// Model download happens on first initialization.
    pub fn new(config: &ClipConfig) -> Self {
        if config.enabled {
            match ClipEmbedder::try_new() {
                Ok(embedder) => {
                    info!(model = %config.model, dim = config.dimensions, "CLIP image embedding enabled");
                    Self {
                        clip_enabled: true,
                        clip_embedder: Some(embedder),
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to initialize CLIP model, falling back to disabled");
                    Self {
                        clip_enabled: false,
                        clip_embedder: None,
                    }
                }
            }
        } else {
            Self::disabled()
        }
    }

    /// Create a new image analyzer with CLIP disabled (default).
    pub fn disabled() -> Self {
        Self {
            clip_enabled: false,
            clip_embedder: None,
        }
    }

    /// Analyse an image attachment and extract metadata / embedding.
    ///
    /// When CLIP is enabled and a valid embedder is available, generates a
    /// vector embedding synchronously. For production workloads, prefer
    /// dispatching `ClipEmbeddingJob` via the background job queue instead
    /// of calling this inline (see `content/jobs.rs`).
    ///
    /// # Arguments
    /// * `data` - Raw image bytes.
    /// * `content_type` - MIME type (e.g., "image/png").
    pub async fn analyze(&self, data: &[u8], content_type: &str) -> ExtractedImage {
        let dimensions = Self::detect_image_dimensions(data);
        let description = dimensions.map(|(w, h)| {
            format!(
                "{} image, {}x{} pixels",
                content_type.strip_prefix("image/").unwrap_or("unknown"),
                w,
                h
            )
        });

        let clip_embedding = if self.clip_enabled {
            if let Some(ref embedder) = self.clip_embedder {
                // fastembed is synchronous; run on a blocking thread to avoid
                // starving the async runtime.
                let data_owned = data.to_vec();
                let model = &embedder.model;
                tokio::task::block_in_place(|| {
                    let mut guard = match model.lock() {
                        Ok(g) => g,
                        Err(e) => {
                            warn!(error = %e, "CLIP model mutex poisoned");
                            return None;
                        }
                    };
                    let images: Vec<&[u8]> = vec![&data_owned];
                    match guard.embed_bytes(&images, None) {
                        Ok(embeddings) => embeddings.into_iter().next(),
                        Err(e) => {
                            warn!(error = %e, "CLIP embedding generation failed");
                            None
                        }
                    }
                })
            } else {
                None
            }
        } else {
            None
        };

        ExtractedImage {
            content_id: None,
            ocr_text: None,
            ocr_confidence: 0.0,
            description,
            clip_embedding,
        }
    }

    /// Generate a CLIP embedding for raw image bytes without full analysis.
    ///
    /// Intended for use by the background `ClipEmbeddingJob` worker.
    /// Returns `None` if CLIP is disabled or embedding fails.
    pub fn embed_image(&self, data: &[u8]) -> Option<Vec<f32>> {
        if !self.clip_enabled {
            return None;
        }
        self.clip_embedder.as_ref()?.embed_bytes(data)
    }

    /// Detect image dimensions from raw bytes using the `image` crate.
    fn detect_image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
        match image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format() {
            Ok(reader) => match reader.into_dimensions() {
                Ok((w, h)) => Some((w, h)),
                Err(e) => {
                    warn!(error = %e, "Failed to read image dimensions");
                    None
                }
            },
            Err(e) => {
                warn!(error = %e, "Failed to guess image format");
                None
            }
        }
    }

    /// Static convenience method for backward compatibility.
    ///
    /// Performs metadata-only analysis without CLIP embedding.
    pub async fn analyze_static(data: &[u8], content_type: &str) -> ExtractedImage {
        let analyzer = Self::disabled();
        analyzer.analyze(data, content_type).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_analyze_disabled_returns_metadata() {
        let analyzer = ImageAnalyzer::disabled();
        let result = analyzer.analyze(b"fake-png-data", "image/png").await;
        assert_eq!(result.ocr_confidence, 0.0);
        assert!(result.ocr_text.is_none());
        assert!(result.clip_embedding.is_none());
    }

    #[tokio::test]
    async fn test_analyze_static_backward_compat() {
        let result = ImageAnalyzer::analyze_static(b"fake-png-data", "image/png").await;
        assert_eq!(result.ocr_confidence, 0.0);
        assert!(result.clip_embedding.is_none());
    }

    #[test]
    fn test_clip_config_defaults() {
        let config = ClipConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.model, "clip-ViT-B-32");
        assert_eq!(config.dimensions, 512);
    }

    #[tokio::test]
    async fn test_analyze_with_clip_disabled_via_config() {
        let config = ClipConfig {
            enabled: false,
            ..Default::default()
        };
        let analyzer = ImageAnalyzer::new(&config);
        let result = analyzer.analyze(b"fake-data", "image/png").await;
        assert!(result.clip_embedding.is_none());
    }

    #[test]
    fn test_embed_image_disabled() {
        let analyzer = ImageAnalyzer::disabled();
        assert!(analyzer.embed_image(b"fake-data").is_none());
    }

    #[test]
    fn test_detect_dimensions_invalid_data() {
        // Invalid data should return None, not panic.
        assert!(ImageAnalyzer::detect_image_dimensions(b"not an image").is_none());
    }

    #[test]
    fn test_detect_dimensions_valid_png() {
        // Generate a valid 1x1 PNG at test time using the image crate.
        let img = image::RgbImage::from_pixel(1, 1, image::Rgb([255u8, 255, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let dims = ImageAnalyzer::detect_image_dimensions(buf.get_ref());
        assert_eq!(dims, Some((1, 1)));
    }
}
