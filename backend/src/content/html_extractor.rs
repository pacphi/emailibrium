//! HTML content extraction -- strips tags, decodes entities, and extracts links/images.
//!
//! Uses the `regex` crate for HTML processing. For production use with adversarial
//! HTML, consider migrating to a proper HTML parser (scraper, html5ever).
//!
//! Dependency: `regex = "1"` in Cargo.toml.

use regex::Regex;

/// Stateless HTML extractor with methods for text, links, and images.
pub struct HtmlExtractor;

impl HtmlExtractor {
    /// Strip HTML tags, decode entities, and collapse whitespace to produce clean text.
    pub fn extract_text(html: &str) -> String {
        // Remove style and script blocks (including content)
        let re_style = Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
        let text = re_style.replace_all(html, "");

        let re_script = Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
        let text = re_script.replace_all(&text, "");

        // Replace <br>, </p>, </div>, </li>, </tr> with newlines for readability
        let re_block = Regex::new(r"(?i)<br\s*/?>|</p>|</div>|</li>|</tr>|</h[1-6]>").unwrap();
        let text = re_block.replace_all(&text, "\n");

        // Strip remaining HTML tags
        let re_tags = Regex::new(r"<[^>]+>").unwrap();
        let text = re_tags.replace_all(&text, "");

        // Decode common HTML entities
        let text = decode_html_entities(&text);

        // Collapse runs of whitespace (spaces/tabs) on each line, then collapse blank lines
        let re_hspace = Regex::new(r"[ \t]+").unwrap();
        let text = re_hspace.replace_all(&text, " ");

        let re_blank_lines = Regex::new(r"\n[ \t]*\n+").unwrap();
        let text = re_blank_lines.replace_all(&text, "\n\n");

        text.trim().to_string()
    }

    /// Extract all `<a href="...">text</a>` pairs from HTML.
    ///
    /// Returns a vec of (url, optional display text).
    pub fn extract_links(html: &str) -> Vec<(String, Option<String>)> {
        let re = Regex::new(r#"(?is)<a\s[^>]*href\s*=\s*"([^"]*)"[^>]*>(.*?)</a>"#).unwrap();
        re.captures_iter(html)
            .map(|cap| {
                let url = cap[1].to_string();
                let raw_text = cap[2].to_string();
                // Strip inner HTML tags from the display text
                let re_tags = Regex::new(r"<[^>]+>").unwrap();
                let display = re_tags.replace_all(&raw_text, "").trim().to_string();
                let display = if display.is_empty() {
                    None
                } else {
                    Some(display)
                };
                (url, display)
            })
            .collect()
    }

    /// Extract all `<img>` tags, returning (src, alt) pairs.
    pub fn extract_images(html: &str) -> Vec<(String, Option<String>)> {
        let re_img = Regex::new(r"(?is)<img\s[^>]*>").unwrap();
        let re_src = Regex::new(r#"(?i)src\s*=\s*"([^"]*)""#).unwrap();
        let re_alt = Regex::new(r#"(?i)alt\s*=\s*"([^"]*)""#).unwrap();

        re_img
            .find_iter(html)
            .filter_map(|m| {
                let tag = m.as_str();
                let src = re_src.captures(tag).map(|c| c[1].to_string())?;
                let alt = re_alt
                    .captures(tag)
                    .map(|c| c[1].to_string())
                    .filter(|s| !s.is_empty());
                Some((src, alt))
            })
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
}
