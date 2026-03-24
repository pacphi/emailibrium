//! Image analysis for OCR and CLIP-based description (ADR-006).
//!
//! This module provides the interface for image analysis. The actual ML model
//! integration (ocrs for OCR, fastembed for CLIP) requires additional
//! dependencies and is documented in ADR-006.

use super::types::ExtractedImage;

/// Image analyser that wraps OCR and CLIP models when configured.
pub struct ImageAnalyzer;

impl ImageAnalyzer {
    /// Analyse an image attachment and extract text / description.
    ///
    /// Returns a result indicating that OCR/CLIP integration is not yet
    /// configured. The `description` field communicates this honestly so
    /// callers can surface it in the UI rather than silently returning
    /// empty data.
    ///
    /// # Arguments
    /// * `_data` - Raw image bytes.
    /// * `_content_type` - MIME type (e.g., "image/png").
    pub async fn analyze(_data: &[u8], _content_type: &str) -> ExtractedImage {
        ExtractedImage {
            content_id: None,
            ocr_text: None,
            ocr_confidence: 0.0,
            description: Some(
                "Image analysis requires OCR/CLIP integration (see ADR-006)".to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_analyze_returns_not_configured() {
        let result = ImageAnalyzer::analyze(b"fake-png-data", "image/png").await;
        assert_eq!(result.ocr_confidence, 0.0);
        assert!(result.ocr_text.is_none());
        let desc = result.description.as_deref().unwrap();
        assert!(desc.contains("ADR-006"));
    }
}
