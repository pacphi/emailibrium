//! Attachment text extraction and file-type detection.
//!
//! This module provides metadata extraction and file-type detection for
//! email attachments. Actual text extraction from PDF/DOCX/XLSX is stubbed
//! and will be integrated in Sprint 3+ (pdf-extract, calamine, dotext).

use super::types::ExtractedAttachment;

/// Extracts text and metadata from email attachments.
pub struct AttachmentExtractor;

impl AttachmentExtractor {
    /// Extract text content from an attachment.
    ///
    /// Currently returns metadata only; actual text extraction is stubbed.
    pub fn extract_text(data: &[u8], filename: &str, content_type: &str) -> ExtractedAttachment {
        let file_type = Self::detect_file_type(data, filename);
        let _ = content_type; // Will be used for content-type fallback in Sprint 3.

        ExtractedAttachment {
            filename: filename.to_string(),
            file_type,
            extracted_text: None, // TODO(S3): Integrate pdf-extract, calamine, dotext
            extraction_quality: 0.0,
            size_bytes: data.len(),
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
        assert!(result.extracted_text.is_none());
        assert_eq!(result.size_bytes, 13);
    }
}
