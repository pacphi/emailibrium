//! Attachment text extraction and file-type detection.
//!
//! This module extracts text content from email attachments. Supported formats:
//! - **PDF**: Text extraction via `pdf-extract` crate
//! - **Plain text / CSV**: Direct UTF-8 decoding
//! - **DOCX/XLSX**: Documented as future work (requires calamine/dotext crates)
//!
//! Dependencies: `pdf-extract` in Cargo.toml.

use tracing::warn;

use super::types::ExtractedAttachment;

/// Extracts text and metadata from email attachments.
pub struct AttachmentExtractor;

impl AttachmentExtractor {
    /// Extract text content from an attachment.
    ///
    /// Detects file type and attempts text extraction for supported formats.
    /// Returns file metadata and any extracted text with a quality score.
    pub fn extract_text(data: &[u8], filename: &str, content_type: &str) -> ExtractedAttachment {
        let file_type = Self::detect_file_type(data, filename);
        let _ = content_type; // Reserved for content-type fallback detection.

        let (extracted_text, extraction_quality) = match file_type.as_str() {
            "pdf" => Self::extract_pdf_text(data),
            "txt" | "csv" => Self::extract_plain_text(data),
            _ => (None, 0.0),
        };

        ExtractedAttachment {
            filename: filename.to_string(),
            file_type,
            extracted_text,
            extraction_quality,
            size_bytes: data.len(),
        }
    }

    /// Extract text from a PDF document using `pdf-extract`.
    fn extract_pdf_text(data: &[u8]) -> (Option<String>, f32) {
        match pdf_extract::extract_text_from_mem(data) {
            Ok(text) => {
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    // PDF exists but contains no extractable text (e.g., scanned image).
                    (None, 0.1)
                } else {
                    // Score based on text density -- very short extractions may be
                    // low quality (e.g., only headers or page numbers).
                    let quality = if trimmed.len() > 100 {
                        0.9
                    } else if trimmed.len() > 20 {
                        0.7
                    } else {
                        0.5
                    };
                    (Some(trimmed), quality)
                }
            }
            Err(err) => {
                warn!(error = %err, "PDF text extraction failed");
                (None, 0.0)
            }
        }
    }

    /// Extract text from plain text or CSV files.
    fn extract_plain_text(data: &[u8]) -> (Option<String>, f32) {
        match std::str::from_utf8(data) {
            Ok(text) => {
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    (None, 0.0)
                } else {
                    (Some(trimmed), 1.0)
                }
            }
            Err(_) => {
                // Try lossy conversion for files with encoding issues.
                let text = String::from_utf8_lossy(data).trim().to_string();
                if text.is_empty() {
                    (None, 0.0)
                } else {
                    (Some(text), 0.6)
                }
            }
        }
    }

    /// Detect file type using extension (primary) and magic bytes (fallback).
    pub fn detect_file_type(data: &[u8], filename: &str) -> String {
        // Try extension first.
        if let Some(ext) = filename.rsplit('.').next() {
            let ext_lower = ext.to_lowercase();
            match ext_lower.as_str() {
                "pdf" => return "pdf".to_string(),
                "docx" => return "docx".to_string(),
                "doc" => return "doc".to_string(),
                "xlsx" => return "xlsx".to_string(),
                "xls" => return "xls".to_string(),
                "pptx" => return "pptx".to_string(),
                "csv" => return "csv".to_string(),
                "txt" => return "txt".to_string(),
                "png" => return "png".to_string(),
                "jpg" | "jpeg" => return "jpeg".to_string(),
                "gif" => return "gif".to_string(),
                "zip" => return "zip".to_string(),
                _ => {} // Fall through to magic bytes.
            }
        }

        // Magic byte detection.
        if data.len() >= 4 {
            // PDF: starts with %PDF
            if data.starts_with(b"%PDF") {
                return "pdf".to_string();
            }
            // ZIP-based formats (DOCX, XLSX, PPTX, ZIP): starts with PK\x03\x04
            if data.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
                // Peek for Office Open XML markers if we have enough data.
                let content = String::from_utf8_lossy(data);
                if content.contains("word/") {
                    return "docx".to_string();
                }
                if content.contains("xl/") {
                    return "xlsx".to_string();
                }
                if content.contains("ppt/") {
                    return "pptx".to_string();
                }
                return "zip".to_string();
            }
            // PNG: starts with \x89PNG
            if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
                return "png".to_string();
            }
            // JPEG: starts with \xFF\xD8\xFF
            if data.len() >= 3 && data.starts_with(&[0xFF, 0xD8, 0xFF]) {
                return "jpeg".to_string();
            }
            // GIF: starts with GIF8
            if data.starts_with(b"GIF8") {
                return "gif".to_string();
            }
        }

        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_pdf_by_extension() {
        assert_eq!(
            AttachmentExtractor::detect_file_type(b"", "report.pdf"),
            "pdf"
        );
    }

    #[test]
    fn test_detect_docx_by_extension() {
        assert_eq!(
            AttachmentExtractor::detect_file_type(b"", "resume.docx"),
            "docx"
        );
    }

    #[test]
    fn test_detect_pdf_by_magic_bytes() {
        assert_eq!(
            AttachmentExtractor::detect_file_type(b"%PDF-1.4", "noext"),
            "pdf"
        );
    }

    #[test]
    fn test_detect_png_by_magic_bytes() {
        let png_header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(
            AttachmentExtractor::detect_file_type(&png_header, "noext"),
            "png"
        );
    }

    #[test]
    fn test_detect_unknown() {
        assert_eq!(
            AttachmentExtractor::detect_file_type(b"\x00\x00\x00", "noext"),
            "unknown"
        );
    }

    #[test]
    fn test_extract_text_returns_metadata() {
        let result =
            AttachmentExtractor::extract_text(b"fake-pdf-data", "report.pdf", "application/pdf");
        assert_eq!(result.filename, "report.pdf");
        assert_eq!(result.file_type, "pdf");
        // pdf-extract will fail on invalid PDF data, so extracted_text should be None.
        assert!(result.extracted_text.is_none());
        assert_eq!(result.size_bytes, 13);
    }

    #[test]
    fn test_extract_plain_text_file() {
        let data = b"Hello, this is a plain text attachment.";
        let result = AttachmentExtractor::extract_text(data, "notes.txt", "text/plain");
        assert_eq!(result.file_type, "txt");
        assert_eq!(
            result.extracted_text,
            Some("Hello, this is a plain text attachment.".to_string())
        );
        assert_eq!(result.extraction_quality, 1.0);
    }

    #[test]
    fn test_extract_csv_file() {
        let data = b"name,email\nAlice,alice@example.com\nBob,bob@example.com";
        let result = AttachmentExtractor::extract_text(data, "contacts.csv", "text/csv");
        assert_eq!(result.file_type, "csv");
        assert!(result.extracted_text.is_some());
        assert!(result.extracted_text.unwrap().contains("Alice"));
        assert_eq!(result.extraction_quality, 1.0);
    }

    #[test]
    fn test_extract_empty_text_file() {
        let result = AttachmentExtractor::extract_text(b"", "empty.txt", "text/plain");
        assert_eq!(result.file_type, "txt");
        assert!(result.extracted_text.is_none());
    }
}
