//! Tracking pixel detection in HTML emails.
//!
//! Detects invisible tracking images using size heuristics,
//! known tracking domains, and CSS visibility checks.

use regex::Regex;

use super::types::TrackingPixel;

/// Detects tracking pixels in email HTML.
pub struct TrackingDetector;

/// Known tracking pixel domains.
const TRACKING_PIXEL_DOMAINS: &[&str] = &[
    "mailchimp.com",
    "sendgrid.net",
    "doubleclick.net",
    "google-analytics.com",
    "facebook.com/tr",
    "list-manage.com",
    "exact-target.com",
    "pardot.com",
    "hubspot.com",
    "sailthru.com",
    "returnpath.net",
    "litmus.com",
    "bananatag.com",
    "yesware.com",
    "mailgun.net",
    "mandrillapp.com",
    "postmarkapp.com",
    "sparkpostmail.com",
    "cmail19.com",
    "cmail20.com",
];

impl TrackingDetector {
    /// Detect tracking pixels in HTML, given the already-extracted image list.
    ///
    /// # Arguments
    /// * `html` - The raw HTML body of the email.
    /// * `images` - Pre-extracted images as (src, alt) pairs.
    pub fn detect(html: &str, images: &[(String, Option<String>)]) -> Vec<TrackingPixel> {
        let mut pixels = Vec::new();

        // Strategy 1: Detect 1x1 pixel images from raw HTML attributes.
        pixels.extend(Self::detect_1x1_pixels(html));

        // Strategy 2: Detect hidden images (display:none, visibility:hidden).
        pixels.extend(Self::detect_hidden_images(html));

        // Strategy 3: Check extracted images against known tracking domains.
        for (src, _alt) in images {
            if let Some(pixel) = Self::check_known_domain(src) {
                // Avoid duplicates from strategies 1/2.
                if !pixels.iter().any(|p| p.url == pixel.url) {
                    pixels.push(pixel);
                }
            }
        }

        pixels
    }

    /// Detect images with explicit width="1" height="1" or 1px style dimensions.
    fn detect_1x1_pixels(html: &str) -> Vec<TrackingPixel> {
        let re_img = Regex::new(r"(?is)<img\s[^>]*>").unwrap();
        let re_src = Regex::new(r#"(?i)src\s*=\s*"([^"]*)""#).unwrap();
        let re_wh =
            Regex::new(r#"(?i)(width\s*=\s*"1"|height\s*=\s*"1"|width:\s*1px|height:\s*1px)"#)
                .unwrap();

        let mut results = Vec::new();
        for img_match in re_img.find_iter(html) {
            let tag = img_match.as_str();

            // Need both width and height indicators, or at least one explicit 1x1 pair.
            let size_matches: Vec<_> = re_wh.find_iter(tag).collect();
            if size_matches.len() >= 2 {
                if let Some(src_cap) = re_src.captures(tag) {
                    let url = src_cap[1].to_string();
                    let domain = extract_domain(&url);
                    results.push(TrackingPixel {
                        url,
                        domain,
                        detection_method: "1x1_pixel".to_string(),
                    });
                }
            }
        }
        results
    }

    /// Detect images hidden via CSS (display:none or visibility:hidden).
    fn detect_hidden_images(html: &str) -> Vec<TrackingPixel> {
        let re_img = Regex::new(r"(?is)<img\s[^>]*>").unwrap();
        let re_src = Regex::new(r#"(?i)src\s*=\s*"([^"]*)""#).unwrap();
        let re_hidden = Regex::new(r"(?i)(display\s*:\s*none|visibility\s*:\s*hidden)").unwrap();

        let mut results = Vec::new();
        for img_match in re_img.find_iter(html) {
            let tag = img_match.as_str();
            if re_hidden.is_match(tag) {
                if let Some(src_cap) = re_src.captures(tag) {
                    let url = src_cap[1].to_string();
                    let domain = extract_domain(&url);
                    results.push(TrackingPixel {
                        url,
                        domain,
                        detection_method: "hidden_image".to_string(),
                    });
                }
            }
        }
        results
    }

    /// Check if a URL belongs to a known tracking domain.
    fn check_known_domain(url: &str) -> Option<TrackingPixel> {
        let lower = url.to_lowercase();
        for domain in TRACKING_PIXEL_DOMAINS {
            if lower.contains(domain) {
                return Some(TrackingPixel {
                    url: url.to_string(),
                    domain: domain.to_string(),
                    detection_method: "known_domain".to_string(),
                });
            }
        }
        None
    }
}

/// Extract the domain portion from a URL string. Best-effort; returns the
/// full URL if parsing fails.
fn extract_domain(url: &str) -> String {
    // Strip protocol.
    let without_proto = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    // Take everything before the first '/'.
    without_proto.split('/').next().unwrap_or(url).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_1x1_pixel() {
        let html = r#"<img src="https://tracker.example.com/pixel.gif" width="1" height="1">"#;
        let images = vec![("https://tracker.example.com/pixel.gif".to_string(), None)];
        let pixels = TrackingDetector::detect(html, &images);
        assert!(!pixels.is_empty());
        assert_eq!(pixels[0].detection_method, "1x1_pixel");
        assert_eq!(pixels[0].url, "https://tracker.example.com/pixel.gif");
    }

    #[test]
    fn test_detect_known_domain() {
        let html = r#"<img src="https://open.mailchimp.com/abc123">"#;
        let images = vec![("https://open.mailchimp.com/abc123".to_string(), None)];
        let pixels = TrackingDetector::detect(html, &images);
        assert!(!pixels.is_empty());
        assert_eq!(pixels[0].detection_method, "known_domain");
    }

    #[test]
    fn test_detect_hidden_image() {
        let html = r#"<img src="https://tracker.example.com/open.gif" style="display:none">"#;
        let images = vec![];
        let pixels = TrackingDetector::detect(html, &images);
        assert!(!pixels.is_empty());
        assert_eq!(pixels[0].detection_method, "hidden_image");
    }

    #[test]
    fn test_no_tracking() {
        let html =
            r#"<img src="https://example.com/photo.jpg" alt="Photo" width="600" height="400">"#;
        let images = vec![(
            "https://example.com/photo.jpg".to_string(),
            Some("Photo".to_string()),
        )];
        let pixels = TrackingDetector::detect(html, &images);
        assert!(pixels.is_empty());
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            extract_domain("https://tracker.example.com/pixel.gif"),
            "tracker.example.com"
        );
        assert_eq!(extract_domain("http://example.com/path"), "example.com");
    }
}
