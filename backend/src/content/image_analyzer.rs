//! Image analysis stub for OCR and CLIP-based description.
//!
//! This module provides the interface for image analysis. The actual ML model
//! integration (ocrs for OCR, fastembed for CLIP) is deferred to Sprint 3+.

use super::types::ExtractedImage;

/// Image analyser that will eventually wrap OCR and CLIP models.
pub struct ImageAnalyzer;

impl ImageAnalyzer {
    /// Analyse an image attachment and extract text / description.
    ///
    /// Currently returns a stub result with zero confidence.
    ///
    /// # Arguments
    /// * `_data` - Raw image bytes.
    /// * `_content_type` - MIME type (e.g., "image/png").
    pub async fn analyze(_data: &[u8], _content_type: &str) -> ExtractedImage {
        // TODO(S3): Integrate ocrs for OCR text extraction
        // TODO(S3): Integrate fastembed for CLIP-based image description
        ExtractedImage {
            content_id: None,
            ocr_text: None,
            ocr_confidence: 0.0,
            description: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_analyze_returns_stub() {
        let result = ImageAnalyzer::analyze(b"fake-png-data", "image/png").await;
        assert_eq!(result.ocr_confidence, 0.0);
        assert!(result.ocr_text.is_none());
        assert!(result.description.is_none());
    }
}
