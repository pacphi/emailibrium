//! HTML content extraction -- strips tags, decodes entities, and extracts links/images.
//!
//! Uses `ammonia` for HTML sanitization and `scraper` for structured parsing
//! with CSS selectors. Falls back to regex for edge cases.
//!
//! Dependencies: `ammonia`, `scraper`, `regex` in Cargo.toml.

use regex::Regex;
use scraper::{Html, Selector};

/// Stateless HTML extractor with methods for text, links, and images.
pub struct HtmlExtractor;

impl HtmlExtractor {
    /// Strip HTML tags, decode entities, and collapse whitespace to produce clean text.
    ///
    /// Uses `ammonia` to sanitize the HTML (removing script/style/dangerous content)
    /// then extracts plain text via `scraper`'s DOM traversal.
    pub fn extract_text(html: &str) -> String {
        // Step 1: Sanitize with ammonia -- removes scripts, styles, event handlers,
        // and dangerous elements while preserving safe text content.
        let sanitized = ammonia::Builder::new()
            .tags(std::collections::HashSet::new()) // strip ALL tags
            .clean(html)
            .to_string();

        // Step 2: Decode remaining entities and collapse whitespace.
        let text = decode_html_entities(&sanitized);

        let re_hspace = Regex::new(r"[ \t]+").unwrap();
        let text = re_hspace.replace_all(&text, " ");

        let re_blank_lines = Regex::new(r"\n[ \t]*\n+").unwrap();
        let text = re_blank_lines.replace_all(&text, "\n\n");

        text.trim().to_string()
    }

    /// Sanitize HTML for safe display (e.g., in a webview preview).
    ///
    /// Strips dangerous elements (scripts, iframes, event handlers) but
    /// preserves safe formatting tags (p, b, i, a, img, etc.).
    pub fn sanitize_html(html: &str) -> String {
        ammonia::clean(html)
    }

    /// Extract all `<a href="...">text</a>` pairs from HTML using CSS selectors.
    ///
    /// Returns a vec of (url, optional display text).
    pub fn extract_links(html: &str) -> Vec<(String, Option<String>)> {
        let document = Html::parse_document(html);
        let selector = Selector::parse("a[href]").unwrap();

        document
            .select(&selector)
            .map(|el| {
                let url = el.value().attr("href").unwrap_or("").to_string();
                let display: String = el.text().collect::<Vec<_>>().join(" ");
                let display = display.trim().to_string();
                let display = if display.is_empty() {
                    None
                } else {
                    Some(display)
                };
                (url, display)
            })
            .collect()
    }

    /// Extract all `<img>` tags using CSS selectors, returning (src, alt) pairs.
    pub fn extract_images(html: &str) -> Vec<(String, Option<String>)> {
        let document = Html::parse_document(html);
        let selector = Selector::parse("img[src]").unwrap();

        document
            .select(&selector)
            .map(|el| {
                let src = el.value().attr("src").unwrap_or("").to_string();
                let alt = el
                    .value()
                    .attr("alt")
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                (src, alt)
            })
            .collect()
    }

    /// Extract structured text from specific HTML elements using CSS selectors.
    ///
    /// Useful for extracting specific content like email headers, signatures, etc.
    pub fn extract_by_selector(html: &str, css_selector: &str) -> Vec<String> {
        let document = Html::parse_document(html);
        let selector = match Selector::parse(css_selector) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        document
            .select(&selector)
            .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

/// Decode the most common HTML entities to their plain-text equivalents.
fn decode_html_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_tags() {
        let html = "<p>Hello <b>world</b></p>";
        let text = HtmlExtractor::extract_text(html);
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn test_decode_entities() {
        let html = "&amp; &lt; &gt; &quot; &nbsp; &apos;";
        let text = HtmlExtractor::extract_text(html);
        // &nbsp; decodes to a space, then adjacent spaces are collapsed.
        assert_eq!(text, "& < > \" '");
    }

    #[test]
    fn test_extract_links() {
        let html = r#"<a href="https://example.com">Click here</a> and <a href="https://other.com">Other</a>"#;
        let links = HtmlExtractor::extract_links(html);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].0, "https://example.com");
        assert_eq!(links[0].1, Some("Click here".to_string()));
        assert_eq!(links[1].0, "https://other.com");
        assert_eq!(links[1].1, Some("Other".to_string()));
    }

    #[test]
    fn test_extract_images() {
        let html = r#"<img src="logo.png" alt="Logo"> <img src="photo.jpg">"#;
        let images = HtmlExtractor::extract_images(html);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].0, "logo.png");
        assert_eq!(images[0].1, Some("Logo".to_string()));
        assert_eq!(images[1].0, "photo.jpg");
        assert_eq!(images[1].1, None);
    }

    #[test]
    fn test_empty_html() {
        assert_eq!(HtmlExtractor::extract_text(""), "");
        assert!(HtmlExtractor::extract_links("").is_empty());
        assert!(HtmlExtractor::extract_images("").is_empty());
    }

    #[test]
    fn test_plain_text_passthrough() {
        let plain = "Just plain text, no HTML tags at all.";
        let text = HtmlExtractor::extract_text(plain);
        assert_eq!(text, plain);
    }

    #[test]
    fn test_script_and_style_removed() {
        let html = r#"<style>.x { color: red; }</style><p>Hello</p><script>alert('x')</script>"#;
        let text = HtmlExtractor::extract_text(html);
        assert_eq!(text, "Hello");
    }

    #[test]
    fn test_link_with_inner_html() {
        let html = r#"<a href="https://example.com"><b>Bold link</b></a>"#;
        let links = HtmlExtractor::extract_links(html);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].1, Some("Bold link".to_string()));
    }

    #[test]
    fn test_sanitize_html_strips_scripts() {
        let html = r#"<p>Safe</p><script>alert('xss')</script><b>Also safe</b>"#;
        let sanitized = HtmlExtractor::sanitize_html(html);
        assert!(sanitized.contains("Safe"));
        assert!(sanitized.contains("Also safe"));
        assert!(!sanitized.contains("script"));
        assert!(!sanitized.contains("alert"));
    }

    #[test]
    fn test_extract_by_selector() {
        let html = r#"<div class="header">Email Header</div><div class="body">Body text</div>"#;
        let headers = HtmlExtractor::extract_by_selector(html, "div.header");
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0], "Email Header");
    }
}
