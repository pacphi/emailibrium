//! URL classification and analysis for extracted links.
//!
//! Classifies URLs into semantic categories based on domain patterns
//! and path heuristics.

use super::types::{ExtractedUrl, UrlCategory};

/// Stateless URL classifier.
pub struct LinkAnalyzer;

/// Known tracking domains (partial matches).
const TRACKING_DOMAINS: &[&str] = &[
    "doubleclick.net",
    "google-analytics.com",
    "mailchimp.com/track",
    "sendgrid.net/wf/click",
    "list-manage.com/track",
    "click.mailerlite.com",
    "links.m.example.com",
    "trk.klclick",
    "pardot.com",
    "hubspot.com/e",
    "t.co",
    "bit.ly",
    "goo.gl",
    "clicks.aweber.com",
];

/// Known tracking path segments.
const TRACKING_PATH_SEGMENTS: &[&str] = &[
    "track", "click", "redirect", "pixel", "beacon", "open", "wf/click",
];

/// Known unsubscribe path segments.
const UNSUBSCRIBE_SEGMENTS: &[&str] = &[
    "unsubscribe",
    "opt-out",
    "optout",
    "manage-preferences",
    "manage_preferences",
    "email-preferences",
    "subscription",
];

/// Known shopping domains.
const SHOPPING_DOMAINS: &[&str] = &[
    "amazon.com",
    "amazon.co",
    "ebay.com",
    "shopify.com",
    "etsy.com",
    "walmart.com",
    "target.com",
    "bestbuy.com",
    "aliexpress.com",
];

/// Known social media domains.
const SOCIAL_DOMAINS: &[&str] = &[
    "twitter.com",
    "x.com",
    "facebook.com",
    "linkedin.com",
    "instagram.com",
    "youtube.com",
    "tiktok.com",
    "reddit.com",
    "pinterest.com",
    "threads.net",
    "mastodon.social",
];

/// Known news domains.
const NEWS_DOMAINS: &[&str] = &[
    "nytimes.com",
    "washingtonpost.com",
    "bbc.com",
    "bbc.co.uk",
    "cnn.com",
    "reuters.com",
    "apnews.com",
    "theguardian.com",
    "wsj.com",
    "bloomberg.com",
    "techcrunch.com",
    "arstechnica.com",
];

impl LinkAnalyzer {
    /// Classify a URL into a semantic category.
    pub fn classify_url(url: &str) -> UrlCategory {
        let lower = url.to_lowercase();

        // Check unsubscribe first -- it is the most actionable for users.
        if UNSUBSCRIBE_SEGMENTS.iter().any(|seg| lower.contains(seg)) {
            return UrlCategory::Unsubscribe;
        }

        // Check tracking.
        if Self::is_tracking_url(url) {
            return UrlCategory::Tracking;
        }

        // Check social.
        if SOCIAL_DOMAINS.iter().any(|d| lower.contains(d)) {
            return UrlCategory::Social;
        }

        // Check shopping.
        if SHOPPING_DOMAINS.iter().any(|d| lower.contains(d)) {
            return UrlCategory::Shopping;
        }

        // Check news.
        if NEWS_DOMAINS.iter().any(|d| lower.contains(d)) {
            return UrlCategory::News;
        }

        UrlCategory::Other
    }

    /// Determine whether a URL is a tracking / click-redirect URL.
    pub fn is_tracking_url(url: &str) -> bool {
        let lower = url.to_lowercase();

        // Check known tracking domains.
        if TRACKING_DOMAINS.iter().any(|d| lower.contains(d)) {
            return true;
        }

        // Check tracking path segments.
        if TRACKING_PATH_SEGMENTS.iter().any(|seg| lower.contains(seg)) {
            return true;
        }

        // Heuristic: extremely long query strings are often tracking URLs.
        if let Some(query_start) = lower.find('?') {
            let query = &lower[query_start..];
            if query.len() > 200 {
                return true;
            }
        }

        false
    }

    /// Find the first unsubscribe link from a list of extracted URLs.
    pub fn extract_unsubscribe_link(urls: &[ExtractedUrl]) -> Option<String> {
        urls.iter()
            .find(|u| u.category == UrlCategory::Unsubscribe)
            .map(|u| u.url.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_tracking() {
        assert_eq!(
            LinkAnalyzer::classify_url("https://doubleclick.net/ad/click?id=123"),
            UrlCategory::Tracking
        );
        assert_eq!(
            LinkAnalyzer::classify_url("https://email.example.com/track/open?u=abc"),
            UrlCategory::Tracking
        );
    }

    #[test]
    fn test_classify_unsubscribe() {
        assert_eq!(
            LinkAnalyzer::classify_url("https://mail.example.com/unsubscribe?token=xyz"),
            UrlCategory::Unsubscribe
        );
        assert_eq!(
            LinkAnalyzer::classify_url("https://example.com/manage-preferences"),
            UrlCategory::Unsubscribe
        );
    }

    #[test]
    fn test_classify_shopping() {
        assert_eq!(
            LinkAnalyzer::classify_url("https://www.amazon.com/dp/B08N5WRWNW"),
            UrlCategory::Shopping
        );
        assert_eq!(
            LinkAnalyzer::classify_url("https://www.ebay.com/itm/12345"),
            UrlCategory::Shopping
        );
    }

    #[test]
    fn test_classify_social() {
        assert_eq!(
            LinkAnalyzer::classify_url("https://twitter.com/user/status/123"),
            UrlCategory::Social
        );
        assert_eq!(
            LinkAnalyzer::classify_url("https://www.linkedin.com/in/jdoe"),
            UrlCategory::Social
        );
    }

    #[test]
    fn test_classify_news() {
        assert_eq!(
            LinkAnalyzer::classify_url("https://www.nytimes.com/2025/01/01/article.html"),
            UrlCategory::News
        );
    }

    #[test]
    fn test_classify_other() {
        assert_eq!(
            LinkAnalyzer::classify_url("https://docs.example.com/api/v2"),
            UrlCategory::Other
        );
    }

    #[test]
    fn test_is_tracking_url() {
        assert!(LinkAnalyzer::is_tracking_url(
            "https://sendgrid.net/wf/click?upn=abc"
        ));
        assert!(!LinkAnalyzer::is_tracking_url("https://docs.example.com"));
    }

    #[test]
    fn test_extract_unsubscribe_link() {
        let urls = vec![
            ExtractedUrl {
                url: "https://example.com".to_string(),
                display_text: None,
                category: UrlCategory::Other,
                is_redirect: false,
            },
            ExtractedUrl {
                url: "https://mail.example.com/unsubscribe?t=1".to_string(),
                display_text: Some("Unsubscribe".to_string()),
                category: UrlCategory::Unsubscribe,
                is_redirect: false,
            },
        ];
        assert_eq!(
            LinkAnalyzer::extract_unsubscribe_link(&urls),
            Some("https://mail.example.com/unsubscribe?t=1".to_string())
        );
    }

    #[test]
    fn test_extract_unsubscribe_link_none() {
        let urls = vec![ExtractedUrl {
            url: "https://example.com".to_string(),
            display_text: None,
            category: UrlCategory::Other,
            is_redirect: false,
        }];
        assert_eq!(LinkAnalyzer::extract_unsubscribe_link(&urls), None);
    }
}
