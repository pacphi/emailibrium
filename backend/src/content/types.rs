//! Core types for the content extraction pipeline.
//!
//! These types form the value objects of the Content Extraction bounded context (ADR-006).

use serde::{Deserialize, Serialize};

/// Raw email input for the content extraction pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEmail {
    /// Unique email identifier.
    pub id: String,
    /// Email subject line.
    pub subject: String,
    /// Sender address.
    pub from_addr: String,
    /// Plain-text body, if available.
    pub body_text: Option<String>,
    /// HTML body, if available.
    pub body_html: Option<String>,
    /// File attachments.
    pub attachments: Vec<RawAttachment>,
}

/// A raw attachment from an email.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawAttachment {
    /// Original filename.
    pub filename: String,
    /// MIME content type (e.g., "application/pdf").
    pub content_type: String,
    /// Raw attachment bytes.
    pub data: Vec<u8>,
    /// Whether this is an inline image (CID reference).
    pub is_inline: bool,
    /// Content-ID header value for inline attachments.
    pub content_id: Option<String>,
}

/// Aggregated result of all content extraction stages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentExtractionResult {
    /// Cleaned plain text extracted from the email body.
    pub clean_text: String,
    /// URLs found in the email with classification.
    pub extracted_urls: Vec<ExtractedUrl>,
    /// Images found in or attached to the email.
    pub images: Vec<ExtractedImage>,
    /// Non-image attachments with extracted text.
    pub attachments: Vec<ExtractedAttachment>,
    /// Detected tracking pixels.
    pub tracking_pixels: Vec<TrackingPixel>,
    /// Quality metrics for the extraction.
    pub quality: ExtractionQuality,
}

/// A URL extracted from the email with classification metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedUrl {
    /// The raw URL string.
    pub url: String,
    /// Anchor text or surrounding context.
    pub display_text: Option<String>,
    /// Semantic category of the URL.
    pub category: UrlCategory,
    /// Whether this URL appears to be a redirect/tracking redirect.
    pub is_redirect: bool,
}

/// Semantic category for a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UrlCategory {
    /// Analytics or click-tracking URL.
    Tracking,
    /// Unsubscribe / opt-out / manage-preferences link.
    Unsubscribe,
    /// E-commerce / shopping link.
    Shopping,
    /// News publication link.
    News,
    /// Social media link.
    Social,
    /// Internal / same-domain link.
    Internal,
    /// Uncategorised.
    Other,
}

impl std::fmt::Display for UrlCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UrlCategory::Tracking => write!(f, "tracking"),
            UrlCategory::Unsubscribe => write!(f, "unsubscribe"),
            UrlCategory::Shopping => write!(f, "shopping"),
            UrlCategory::News => write!(f, "news"),
            UrlCategory::Social => write!(f, "social"),
            UrlCategory::Internal => write!(f, "internal"),
            UrlCategory::Other => write!(f, "other"),
        }
    }
}

/// An image extracted from the email (inline or attachment).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedImage {
    /// Content-ID for inline images.
    pub content_id: Option<String>,
    /// OCR-extracted text, if any.
    pub ocr_text: Option<String>,
    /// Confidence score for OCR output (0.0 to 1.0).
    pub ocr_confidence: f32,
    /// Human-readable description (from CLIP or alt text).
    pub description: Option<String>,
    /// CLIP embedding vector for semantic image search (ADR-006, item #23).
    /// Populated asynchronously by the background CLIP embedding job.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_embedding: Option<Vec<f32>>,
}

/// Metadata and extracted text from a non-image attachment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedAttachment {
    /// Original filename.
    pub filename: String,
    /// Detected file type (e.g., "pdf", "docx").
    pub file_type: String,
    /// Text extracted from the attachment, if supported.
    pub extracted_text: Option<String>,
    /// Quality score for the extraction (0.0 to 1.0).
    pub extraction_quality: f32,
    /// Size in bytes.
    pub size_bytes: usize,
}

/// A detected tracking pixel in the email HTML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackingPixel {
    /// URL of the tracking pixel image.
    pub url: String,
    /// Domain serving the pixel.
    pub domain: String,
    /// How the pixel was detected (e.g., "1x1_pixel", "known_domain", "hidden_image").
    pub detection_method: String,
}

/// Quality metrics for the content extraction pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionQuality {
    /// Overall quality score (0.0 to 1.0).
    pub overall_score: f32,
    /// Ratio of extracted text length to raw HTML length.
    pub text_ratio: f32,
    /// Non-fatal warnings encountered during extraction.
    pub warnings: Vec<String>,
    /// Extraction stages that failed entirely.
    pub failed_extractions: Vec<String>,
}
