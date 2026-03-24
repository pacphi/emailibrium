//! Multi-asset content extraction pipeline for Emailibrium (ADR-006).
//!
//! This module orchestrates extraction of text, links, images, attachments,
//! and tracking pixels from raw email input. Each sub-module handles one
//! extraction concern; the [`ContentPipeline`] facade composes them.

pub mod attachment_extractor;
pub mod html_extractor;
pub mod image_analyzer;
pub mod link_analyzer;
pub mod tracking_detector;
pub mod types;

use html_extractor::HtmlExtractor;
use image_analyzer::ImageAnalyzer;
use link_analyzer::LinkAnalyzer;
use tracking_detector::TrackingDetector;
use types::{ContentExtractionResult, ExtractedUrl, ExtractionQuality, RawEmail};

/// Top-level facade that runs all extraction stages on a raw email.
pub struct ContentPipeline;

impl ContentPipeline {
    /// Create a new pipeline instance.
    pub fn new() -> Self {
        Self
    }

    /// Run every extraction stage and return an aggregated result.
    pub async fn extract_all(&self, email: &RawEmail) -> ContentExtractionResult {
        let mut warnings: Vec<String> = Vec::new();
        let mut failed_extractions: Vec<String> = Vec::new();

        // --- 1. Text extraction ---
        let (clean_text, html_len) = match (&email.body_html, &email.body_text) {
            (Some(html), _) => {
                let text = HtmlExtractor::extract_text(html);
                let len = html.len();
                (text, len)
            }
            (None, Some(text)) => {
                let len = text.len();
                (text.clone(), len)
            }
            (None, None) => {
                warnings.push("Email has no body content".to_string());
                (String::new(), 0)
            }
        };

        // --- 2. Link extraction and classification ---
        let extracted_urls: Vec<ExtractedUrl> = if let Some(html) = &email.body_html {
            let raw_links = HtmlExtractor::extract_links(html);
            raw_links
                .into_iter()
                .map(|(url, display_text)| {
                    let category = LinkAnalyzer::classify_url(&url);
                    let is_redirect = LinkAnalyzer::is_tracking_url(&url);
                    ExtractedUrl {
                        url,
                        display_text,
                        category,
                        is_redirect,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        // --- 3. Image extraction and analysis ---
        let html_images = if let Some(html) = &email.body_html {
            HtmlExtractor::extract_images(html)
        } else {
            Vec::new()
        };

        let mut images = Vec::new();
        // Analyse inline image attachments.
        for attachment in &email.attachments {
            if attachment.is_inline && attachment.content_type.starts_with("image/") {
                let mut img =
                    ImageAnalyzer::analyze(&attachment.data, &attachment.content_type).await;
                img.content_id = attachment.content_id.clone();
                images.push(img);
            }
        }
        // For non-inline images found in HTML, we only have URLs (no bytes to analyse).
        // Record them with descriptions from alt text.
        for (_src, alt) in &html_images {
            images.push(types::ExtractedImage {
                content_id: None,
                ocr_text: None,
                ocr_confidence: 0.0,
                description: alt.clone(),
            });
        }

        // --- 4. Attachment extraction ---
        let attachments: Vec<_> = email
            .attachments
            .iter()
            .filter(|a| !a.is_inline)
            .map(|a| {
                attachment_extractor::AttachmentExtractor::extract_text(
                    &a.data,
                    &a.filename,
                    &a.content_type,
                )
            })
            .collect();

        if !email.attachments.is_empty() && attachments.iter().all(|a| a.extracted_text.is_none()) {
            failed_extractions.push("attachment_text_extraction".to_string());
        }

        // --- 5. Tracking pixel detection ---
        let tracking_pixels = if let Some(html) = &email.body_html {
            TrackingDetector::detect(html, &html_images)
        } else {
            Vec::new()
        };

        // --- 6. Quality scoring ---
        let text_ratio = if html_len > 0 {
            clean_text.len() as f32 / html_len as f32
        } else {
            0.0
        };

        let mut overall_score: f32 = 0.5; // Base score.
        if !clean_text.is_empty() {
            overall_score += 0.2;
        }
        if !extracted_urls.is_empty() {
            overall_score += 0.1;
        }
        if warnings.is_empty() {
            overall_score += 0.1;
        }
        if failed_extractions.is_empty() {
            overall_score += 0.1;
        }
        overall_score = overall_score.min(1.0);

        let quality = ExtractionQuality {
            overall_score,
            text_ratio,
            warnings,
            failed_extractions,
        };

        ContentExtractionResult {
            clean_text,
            extracted_urls,
            images,
            attachments,
            tracking_pixels,
            quality,
        }
    }
}

impl Default for ContentPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::types::{RawAttachment, RawEmail};
    use super::*;

    fn make_html_email() -> RawEmail {
        RawEmail {
            id: "test-001".to_string(),
            subject: "Test Email".to_string(),
            from_addr: "sender@example.com".to_string(),
            body_text: None,
            body_html: Some(
                r#"<html><body>
                <p>Hello <b>World</b></p>
                <a href="https://example.com">Visit us</a>
                <a href="https://mail.example.com/unsubscribe?t=1">Unsubscribe</a>
                <img src="https://tracker.example.com/pixel.gif" width="1" height="1">
                <img src="https://example.com/logo.png" alt="Logo">
                </body></html>"#
                    .to_string(),
            ),
            attachments: vec![RawAttachment {
                filename: "report.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: b"%PDF-1.4 fake".to_vec(),
                is_inline: false,
                content_id: None,
            }],
        }
    }

    fn make_plain_email() -> RawEmail {
        RawEmail {
            id: "test-002".to_string(),
            subject: "Plain Email".to_string(),
            from_addr: "sender@example.com".to_string(),
            body_text: Some("Hello, this is a plain text email.".to_string()),
            body_html: None,
            attachments: vec![],
        }
    }

    #[tokio::test]
    async fn test_extract_all_html_email() {
        let pipeline = ContentPipeline::new();
        let email = make_html_email();
        let result = pipeline.extract_all(&email).await;

        assert!(result.clean_text.contains("Hello"));
        assert!(result.clean_text.contains("World"));
        assert!(result.extracted_urls.len() >= 2);
        // Should have at least one unsubscribe link.
        assert!(result
            .extracted_urls
            .iter()
            .any(|u| u.category == types::UrlCategory::Unsubscribe));
        // Should have at least one tracking pixel (1x1).
        assert!(!result.tracking_pixels.is_empty());
        // Should have one attachment.
        assert_eq!(result.attachments.len(), 1);
        assert_eq!(result.attachments[0].filename, "report.pdf");
        // Quality score should be reasonable.
        assert!(result.quality.overall_score > 0.5);
    }

    #[tokio::test]
    async fn test_extract_all_plain_text() {
        let pipeline = ContentPipeline::new();
        let email = make_plain_email();
        let result = pipeline.extract_all(&email).await;

        assert_eq!(result.clean_text, "Hello, this is a plain text email.");
        assert!(result.extracted_urls.is_empty());
        assert!(result.tracking_pixels.is_empty());
        assert!(result.attachments.is_empty());
    }

    #[tokio::test]
    async fn test_quality_scoring() {
        let pipeline = ContentPipeline::new();

        // HTML email with content should score well.
        let html_email = make_html_email();
        let html_result = pipeline.extract_all(&html_email).await;
        assert!(html_result.quality.overall_score >= 0.7);
        assert!(html_result.quality.text_ratio > 0.0);

        // Empty email should score lower.
        let empty_email = RawEmail {
            id: "test-003".to_string(),
            subject: "Empty".to_string(),
            from_addr: "x@x.com".to_string(),
            body_text: None,
            body_html: None,
            attachments: vec![],
        };
        let empty_result = pipeline.extract_all(&empty_email).await;
        assert!(empty_result.quality.overall_score < html_result.quality.overall_score);
        assert!(!empty_result.quality.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_default_impl() {
        let pipeline = ContentPipeline::default();
        let email = make_plain_email();
        let result = pipeline.extract_all(&email).await;
        assert!(!result.clean_text.is_empty());
    }
}
