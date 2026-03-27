//! Security Headers Middleware
//!
//! Adds comprehensive security headers to all HTTP responses to protect against
//! common web vulnerabilities including XSS, clickjacking, MIME sniffing, and more.
//!
//! Features:
//! - Helmet-like comprehensive security headers
//! - Configurable CSP for OAuth redirects and desktop app
//! - Support for Tauri desktop app CORS
//! - CSP report URI for policy violations
//! - Strict HSTS with preload option
//! - Modern security best practices

use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};
use std::sync::OnceLock;
use tracing::debug;

/// Global CSP policy cache
static CSP_POLICY: OnceLock<String> = OnceLock::new();

/// Security headers configuration
#[derive(Debug, Clone)]
pub struct SecurityHeadersConfig {
    /// HSTS max-age in seconds (default: 31536000 = 1 year)
    pub hsts_max_age: u64,
    /// Enable HSTS preload
    pub hsts_preload: bool,
    /// CSP report URI (optional)
    pub csp_report_uri: Option<String>,
    /// Allow inline styles (needed for some frameworks)
    pub allow_inline_styles: bool,
    /// Additional CSP connect-src origins (for API calls)
    pub connect_src_origins: Vec<String>,
}

impl Default for SecurityHeadersConfig {
    fn default() -> Self {
        Self {
            hsts_max_age: 31536000, // 1 year
            hsts_preload: false,
            csp_report_uri: None,
            allow_inline_styles: true,
            connect_src_origins: vec![],
        }
    }
}

impl SecurityHeadersConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            hsts_max_age: std::env::var("HSTS_MAX_AGE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(31536000),
            hsts_preload: std::env::var("HSTS_PRELOAD")
                .ok()
                .map(|s| s.to_lowercase() == "true")
                .unwrap_or(false),
            csp_report_uri: std::env::var("CSP_REPORT_URI").ok(),
            allow_inline_styles: std::env::var("CSP_ALLOW_INLINE_STYLES")
                .ok()
                .map(|s| s.to_lowercase() != "false")
                .unwrap_or(true),
            connect_src_origins: std::env::var("CSP_CONNECT_SRC_ORIGINS")
                .ok()
                .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default(),
        }
    }

    /// Build CSP policy string
    pub fn build_csp_policy(&self) -> String {
        let mut policy_parts = vec![
            // Default: only allow resources from same origin
            "default-src 'self'".to_string(),
            // Scripts: only from same origin (no inline scripts for security)
            "script-src 'self'".to_string(),
            // Only allow self-hosted or HTTPS images + data URIs for inline images
            "img-src 'self' data: https:".to_string(),
            // Fonts from same origin
            "font-src 'self'".to_string(),
            // Objects/plugins disabled
            "object-src 'none'".to_string(),
            // Base URI restricted to same origin
            "base-uri 'self'".to_string(),
            // Form actions only to same origin
            "form-action 'self'".to_string(),
            // No framing allowed (alternative to X-Frame-Options)
            "frame-ancestors 'none'".to_string(),
            // Upgrade insecure requests to HTTPS
            "upgrade-insecure-requests".to_string(),
            // Block mixed content
            "block-all-mixed-content".to_string(),
        ];

        // Styles: allow same origin + optional inline styles (needed for some frameworks)
        if self.allow_inline_styles {
            policy_parts.push("style-src 'self' 'unsafe-inline'".to_string());
        } else {
            policy_parts.push("style-src 'self'".to_string());
        }

        // Connect-src: API calls to self + OAuth providers + configured origins
        let mut connect_src = vec![
            "'self'",
            // Google OAuth
            "https://accounts.google.com",
            "https://oauth2.googleapis.com",
            "https://www.googleapis.com",
            // Microsoft OAuth
            "https://login.microsoftonline.com",
            "https://graph.microsoft.com",
        ];

        // Add custom origins from config
        for origin in &self.connect_src_origins {
            connect_src.push(origin.as_str());
        }

        policy_parts.push(format!("connect-src {}", connect_src.join(" ")));

        // Add report URI if configured
        if let Some(ref report_uri) = self.csp_report_uri {
            policy_parts.push(format!("report-uri {}", report_uri));
        }

        policy_parts.join("; ")
    }
}

/// Get or initialize CSP policy
fn get_csp_policy() -> &'static str {
    CSP_POLICY.get_or_init(|| {
        let config = SecurityHeadersConfig::from_env();
        config.build_csp_policy()
    })
}

/// Security headers middleware
///
/// Adds comprehensive security headers following Helmet.js best practices:
/// - Strict-Transport-Security (HSTS) with optional preload
/// - X-Content-Type-Options: nosniff
/// - X-Frame-Options: DENY
/// - X-XSS-Protection: 1; mode=block
/// - Content-Security-Policy with OAuth provider support
/// - Referrer-Policy: strict-origin-when-cross-origin
/// - Permissions-Policy: restricts browser features
///
/// Configuration via environment variables:
/// - HSTS_MAX_AGE: HSTS max-age in seconds (default: 31536000)
/// - HSTS_PRELOAD: Enable HSTS preload (default: false)
/// - CSP_REPORT_URI: CSP violation report endpoint (optional)
/// - CSP_ALLOW_INLINE_STYLES: Allow inline styles (default: true)
/// - CSP_CONNECT_SRC_ORIGINS: Additional API origins (comma-separated)
pub async fn security_headers_middleware(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;

    let headers = response.headers_mut();

    // Load config from environment (cached in CSP_POLICY)
    let config = SecurityHeadersConfig::from_env();

    // 1. Strict-Transport-Security (HSTS)
    // Forces HTTPS for all future requests to this domain
    let hsts_value = if config.hsts_preload {
        format!(
            "max-age={}; includeSubDomains; preload",
            config.hsts_max_age
        )
    } else {
        format!("max-age={}; includeSubDomains", config.hsts_max_age)
    };
    headers.insert(
        "Strict-Transport-Security",
        HeaderValue::from_str(&hsts_value)
            .unwrap_or_else(|_| HeaderValue::from_static("max-age=31536000; includeSubDomains")),
    );

    // 2. X-Frame-Options: DENY
    // Prevents clickjacking by disallowing the page to be framed
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));

    // 3. X-Content-Type-Options: nosniff
    // Prevents browsers from MIME-sniffing responses
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );

    // 4. X-XSS-Protection: 1; mode=block
    // Legacy XSS protection (still useful for older browsers)
    headers.insert(
        "X-XSS-Protection",
        HeaderValue::from_static("1; mode=block"),
    );

    // 5. Content-Security-Policy (CSP)
    // Comprehensive policy that allows OAuth redirects and desktop app
    let csp_policy = get_csp_policy();
    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_str(csp_policy)
            .unwrap_or_else(|_| HeaderValue::from_static("default-src 'self'")),
    );

    // 6. Referrer-Policy: strict-origin-when-cross-origin
    // Controls how much referrer information is included with requests
    headers.insert(
        "Referrer-Policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );

    // 7. Permissions-Policy (formerly Feature-Policy)
    // Disables unnecessary browser features to reduce attack surface
    headers.insert(
        "Permissions-Policy",
        HeaderValue::from_static(
            "geolocation=(), microphone=(), camera=(), usb=(), payment=(), magnetometer=()",
        ),
    );

    debug!("Added comprehensive security headers to response");

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // SecurityHeadersConfig defaults
    // -----------------------------------------------------------------------

    #[test]
    fn default_config_has_one_year_hsts() {
        let config = SecurityHeadersConfig::default();
        assert_eq!(config.hsts_max_age, 31536000);
    }

    #[test]
    fn default_config_disables_hsts_preload() {
        let config = SecurityHeadersConfig::default();
        assert!(!config.hsts_preload);
    }

    #[test]
    fn default_config_allows_inline_styles() {
        let config = SecurityHeadersConfig::default();
        assert!(config.allow_inline_styles);
    }

    #[test]
    fn default_config_has_no_report_uri() {
        let config = SecurityHeadersConfig::default();
        assert!(config.csp_report_uri.is_none());
    }

    #[test]
    fn default_config_has_no_extra_connect_origins() {
        let config = SecurityHeadersConfig::default();
        assert!(config.connect_src_origins.is_empty());
    }

    // -----------------------------------------------------------------------
    // CSP policy construction
    // -----------------------------------------------------------------------

    #[test]
    fn csp_policy_contains_default_src_self() {
        let config = SecurityHeadersConfig::default();
        let policy = config.build_csp_policy();
        assert!(policy.contains("default-src 'self'"));
    }

    #[test]
    fn csp_policy_does_not_contain_unsafe_eval() {
        let config = SecurityHeadersConfig::default();
        let policy = config.build_csp_policy();
        assert!(
            !policy.contains("unsafe-eval"),
            "CSP must never contain unsafe-eval: {}",
            policy
        );
    }

    #[test]
    fn csp_policy_blocks_framing() {
        let config = SecurityHeadersConfig::default();
        let policy = config.build_csp_policy();
        assert!(policy.contains("frame-ancestors 'none'"));
    }

    #[test]
    fn csp_policy_disables_object_src() {
        let config = SecurityHeadersConfig::default();
        let policy = config.build_csp_policy();
        assert!(policy.contains("object-src 'none'"));
    }

    #[test]
    fn csp_policy_includes_google_oauth_origins() {
        let config = SecurityHeadersConfig::default();
        let policy = config.build_csp_policy();
        assert!(policy.contains("https://accounts.google.com"));
        assert!(policy.contains("https://oauth2.googleapis.com"));
    }

    #[test]
    fn csp_policy_includes_microsoft_oauth_origins() {
        let config = SecurityHeadersConfig::default();
        let policy = config.build_csp_policy();
        assert!(policy.contains("https://login.microsoftonline.com"));
        assert!(policy.contains("https://graph.microsoft.com"));
    }

    #[test]
    fn csp_policy_upgrades_insecure_requests() {
        let config = SecurityHeadersConfig::default();
        let policy = config.build_csp_policy();
        assert!(policy.contains("upgrade-insecure-requests"));
    }

    #[test]
    fn csp_policy_allows_inline_styles_when_configured() {
        let config = SecurityHeadersConfig {
            allow_inline_styles: true,
            ..SecurityHeadersConfig::default()
        };
        let policy = config.build_csp_policy();
        assert!(policy.contains("style-src 'self' 'unsafe-inline'"));
    }

    #[test]
    fn csp_policy_disallows_inline_styles_when_disabled() {
        let config = SecurityHeadersConfig {
            allow_inline_styles: false,
            ..SecurityHeadersConfig::default()
        };
        let policy = config.build_csp_policy();
        assert!(policy.contains("style-src 'self'"));
        assert!(!policy.contains("unsafe-inline"));
    }

    #[test]
    fn csp_policy_includes_report_uri_when_set() {
        let config = SecurityHeadersConfig {
            csp_report_uri: Some("https://example.com/csp-report".to_string()),
            ..SecurityHeadersConfig::default()
        };
        let policy = config.build_csp_policy();
        assert!(policy.contains("report-uri https://example.com/csp-report"));
    }

    #[test]
    fn csp_policy_omits_report_uri_when_none() {
        let config = SecurityHeadersConfig::default();
        let policy = config.build_csp_policy();
        assert!(!policy.contains("report-uri"));
    }

    #[test]
    fn csp_policy_includes_custom_connect_origins() {
        let config = SecurityHeadersConfig {
            connect_src_origins: vec![
                "https://custom-api.example.com".to_string(),
                "https://other.example.com".to_string(),
            ],
            ..SecurityHeadersConfig::default()
        };
        let policy = config.build_csp_policy();
        assert!(policy.contains("https://custom-api.example.com"));
        assert!(policy.contains("https://other.example.com"));
    }
}
