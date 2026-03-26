//! Email HTML sanitization service (ADR-019, DDD-009).
//!
//! Provides defense-in-depth HTML sanitization using ammonia
//! with an email-specific whitelist. All body_html content must
//! pass through this service before database storage.

use std::collections::{HashMap, HashSet};

use base64::Engine;

/// Sanitize HTML email content for safe storage and rendering.
///
/// Allows standard email formatting tags (tables, images, styles)
/// while stripping dangerous elements (scripts, iframes, event handlers).
/// Sets `rel="noopener noreferrer"` on all links.
pub fn sanitize_email_html(raw_html: &str) -> String {
    let tags: HashSet<&str> = [
        "a",
        "b",
        "blockquote",
        "br",
        "center",
        "code",
        "div",
        "em",
        "font",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "i",
        "img",
        "li",
        "ol",
        "p",
        "pre",
        "span",
        "strong",
        "table",
        "tbody",
        "td",
        "th",
        "thead",
        "tr",
        "u",
        "ul",
        "sup",
        "sub",
    ]
    .into_iter()
    .collect();

    // Allow <style> tags via clean_content_tags (ammonia 4 handles CSS
    // sanitization separately from regular tag allowlisting).
    let clean_content_tags: HashSet<&str> = ["style", "script"].into_iter().collect();

    let mut tag_attrs = std::collections::HashMap::new();

    let a_attrs: HashSet<&str> = ["href", "target"].into_iter().collect();
    let img_attrs: HashSet<&str> = ["src", "alt", "width", "height", "style"]
        .into_iter()
        .collect();
    let td_attrs: HashSet<&str> = [
        "style", "width", "height", "align", "valign", "bgcolor", "colspan", "rowspan",
    ]
    .into_iter()
    .collect();
    let table_attrs: HashSet<&str> = [
        "style",
        "width",
        "border",
        "cellpadding",
        "cellspacing",
        "bgcolor",
        "align",
    ]
    .into_iter()
    .collect();
    let div_attrs: HashSet<&str> = ["style", "class", "align", "dir"].into_iter().collect();
    let span_attrs: HashSet<&str> = ["style", "class"].into_iter().collect();
    let font_attrs: HashSet<&str> = ["color", "size", "face"].into_iter().collect();
    let p_attrs: HashSet<&str> = ["style", "align", "dir"].into_iter().collect();

    tag_attrs.insert("a", a_attrs);
    tag_attrs.insert("img", img_attrs);
    tag_attrs.insert("td", td_attrs.clone());
    tag_attrs.insert("th", td_attrs);
    tag_attrs.insert("table", table_attrs);
    tag_attrs.insert("div", div_attrs);
    tag_attrs.insert("span", span_attrs);
    tag_attrs.insert("font", font_attrs);
    tag_attrs.insert("p", p_attrs);

    let url_schemes: HashSet<&str> = ["http", "https", "mailto", "data"].into_iter().collect();

    ammonia::Builder::new()
        .tags(tags)
        .clean_content_tags(clean_content_tags)
        .tag_attributes(tag_attrs)
        .link_rel(Some("noopener noreferrer"))
        .url_schemes(url_schemes)
        .clean(raw_html)
        .to_string()
}

/// Replace CID references in HTML with base64 data URIs.
///
/// This must be called BEFORE `sanitize_email_html()` so that ammonia's
/// URL scheme whitelist allows the resulting `data:` URIs.
pub fn resolve_cid_references(
    html: &str,
    inline_images: &HashMap<String, (String, Vec<u8>)>,
) -> String {
    let mut result = html.to_string();
    for (content_id, (content_type, data)) in inline_images {
        // CID references can appear as cid:xxx or cid:xxx@domain.
        let cid_ref = format!("cid:{}", content_id.trim_matches('<').trim_matches('>'));
        let data_uri = format!(
            "data:{};base64,{}",
            content_type,
            base64::engine::general_purpose::STANDARD.encode(data)
        );
        result = result.replace(&cid_ref, &data_uri);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strips_script_tags() {
        let input = "<p>Safe</p><script>alert('xss')</script>";
        let output = sanitize_email_html(input);
        assert!(output.contains("<p>Safe</p>"));
        assert!(!output.contains("<script>"));
        assert!(!output.contains("alert"));
    }

    #[test]
    fn test_strips_event_handlers() {
        let input = r#"<img src="x" onerror="alert(1)">"#;
        let output = sanitize_email_html(input);
        assert!(!output.contains("onerror"));
        // img tag should remain (src is allowed)
        assert!(output.contains("<img"));
    }

    #[test]
    fn test_strips_javascript_uri() {
        let input = r#"<a href="javascript:alert(1)">click</a>"#;
        let output = sanitize_email_html(input);
        assert!(!output.contains("javascript:"));
    }

    #[test]
    fn test_preserves_email_formatting() {
        let input = r#"<table style="width:100%"><tr><td>Cell</td></tr></table>"#;
        let output = sanitize_email_html(input);
        assert!(output.contains("<table"));
        assert!(output.contains("<tr>"));
        assert!(output.contains("<td"));
        assert!(output.contains("Cell"));

        // Links preserved
        let link_input = r#"<a href="https://example.com">Link</a>"#;
        let link_output = sanitize_email_html(link_input);
        assert!(link_output.contains("https://example.com"));
        assert!(link_output.contains("Link"));

        // Images preserved
        let img_input = r#"<img src="https://example.com/logo.png" alt="Logo">"#;
        let img_output = sanitize_email_html(img_input);
        assert!(img_output.contains("https://example.com/logo.png"));
        assert!(img_output.contains("Logo"));
    }

    #[test]
    fn test_preserves_inline_styles() {
        let input = r#"<div style="color: red">Styled</div>"#;
        let output = sanitize_email_html(input);
        assert!(output.contains("style"));
        assert!(output.contains("color: red"));
        assert!(output.contains("Styled"));
    }

    #[test]
    fn test_adds_link_rel() {
        let input = r#"<a href="https://example.com">Link</a>"#;
        let output = sanitize_email_html(input);
        assert!(output.contains(r#"rel="noopener noreferrer""#));
    }

    #[test]
    fn test_allows_data_uri_images() {
        let input = r#"<img src="data:image/png;base64,iVBOR">"#;
        let output = sanitize_email_html(input);
        assert!(output.contains("data:image/png;base64,iVBOR"));
    }

    #[test]
    fn test_strips_iframes() {
        let input = r#"<p>Text</p><iframe src="https://evil.com"></iframe>"#;
        let output = sanitize_email_html(input);
        assert!(!output.contains("<iframe"));
        assert!(output.contains("<p>Text</p>"));
    }

    #[test]
    fn test_strips_forms() {
        let input = r#"<form action="/steal"><input type="text"><button>Submit</button></form>"#;
        let output = sanitize_email_html(input);
        assert!(!output.contains("<form"));
        assert!(!output.contains("<input"));
    }

    #[test]
    fn test_empty_input() {
        let output = sanitize_email_html("");
        assert_eq!(output, "");
    }

    // --- CID resolution tests ---

    #[test]
    fn test_resolve_cid_single() {
        let html = r#"<img src="cid:logo123">"#;
        let mut map = HashMap::new();
        map.insert(
            "logo123".to_string(),
            ("image/png".to_string(), vec![0x89, 0x50, 0x4E, 0x47]),
        );
        let result = resolve_cid_references(html, &map);
        assert!(result.contains("data:image/png;base64,"));
        assert!(!result.contains("cid:"));
    }

    #[test]
    fn test_resolve_cid_with_angle_brackets() {
        let html = r#"<img src="cid:logo456">"#;
        let mut map = HashMap::new();
        map.insert(
            "<logo456>".to_string(),
            ("image/jpeg".to_string(), vec![0xFF, 0xD8]),
        );
        let result = resolve_cid_references(html, &map);
        assert!(result.contains("data:image/jpeg;base64,"));
        assert!(!result.contains("cid:"));
    }

    #[test]
    fn test_resolve_cid_multiple() {
        let html = r#"<img src="cid:img1"><img src="cid:img2">"#;
        let mut map = HashMap::new();
        map.insert("img1".to_string(), ("image/png".to_string(), vec![1]));
        map.insert("img2".to_string(), ("image/gif".to_string(), vec![2]));
        let result = resolve_cid_references(html, &map);
        assert!(result.contains("data:image/png;base64,"));
        assert!(result.contains("data:image/gif;base64,"));
        assert!(!result.contains("cid:"));
    }

    #[test]
    fn test_resolve_cid_missing_leaves_original() {
        let html = r#"<img src="cid:unknown">"#;
        let map = HashMap::new();
        let result = resolve_cid_references(html, &map);
        assert_eq!(result, html);
    }

    #[test]
    fn test_resolve_cid_empty_map() {
        let html = "<p>No images</p>";
        let map = HashMap::new();
        let result = resolve_cid_references(html, &map);
        assert_eq!(result, html);
    }
}
