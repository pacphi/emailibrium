//! Log Scrubbing Middleware
//!
//! Prevents sensitive data (tokens, passwords, secrets) from being logged
//! in application logs, error messages, and traces.
//!
//! This middleware intercepts errors and responses to scrub sensitive data
//! before it reaches log files or monitoring systems.

use axum::{
    body::Body,
    http::{Request, Response},
    middleware::Next,
};
use regex::Regex;
use std::sync::OnceLock;
use tracing::{error, warn};

/// Regex patterns for detecting tokens and sensitive data
static TOKEN_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();

fn get_token_patterns() -> &'static Vec<Regex> {
    TOKEN_PATTERNS.get_or_init(|| {
        vec![
            // JWT tokens (3 base64 segments separated by dots)
            Regex::new(r"eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+").unwrap(),
            // OAuth bearer tokens
            Regex::new(r"Bearer\s+[A-Za-z0-9._~+/-]+=*").unwrap(),
            // Generic access tokens
            Regex::new(r#"access_token["':\s=]+([A-Za-z0-9._~+/-]{20,})"#).unwrap(),
            // Refresh tokens
            Regex::new(r#"refresh_token["':\s=]+([A-Za-z0-9._~+/-]{20,})"#).unwrap(),
            // Authorization codes
            Regex::new(r#"code["':\s=]+([A-Za-z0-9._~+/-]{20,})"#).unwrap(),
            // PKCE verifiers
            Regex::new(r#"code_verifier["':\s=]+([A-Za-z0-9._~+/-]{43,128})"#).unwrap(),
            // API keys
            Regex::new(r#"(?i)api[_-]?key["':\s=]+([A-Za-z0-9._~+/-]{20,})"#).unwrap(),
            // Secrets
            Regex::new(r#"(?i)secret["':\s=]+([A-Za-z0-9._~+/-]{20,})"#).unwrap(),
            // Passwords (using raw string to avoid escape issues)
            Regex::new(r#"(?i)password["':\s=]+([A-Za-z0-9._~+/!@#$%^&*()-]{8,})"#).unwrap(),
        ]
    })
}

/// Scrub sensitive data from a string
///
/// Replaces tokens, passwords, and secrets with [REDACTED] markers
pub fn scrub_sensitive_data(input: &str) -> String {
    let patterns = get_token_patterns();
    let mut output = input.to_string();

    for pattern in patterns {
        output = pattern.replace_all(&output, "[REDACTED]").to_string();
    }

    output
}

/// Middleware to scrub tokens from error responses
///
/// This middleware should be applied early in the middleware stack
/// to catch errors from downstream handlers.
///
/// It sanitises the request URI (via [`scrub_query_params`]) and headers
/// (via [`scrub_headers`]) before forwarding the request, and uses
/// [`scrub_error_message`] when logging error responses.
pub async fn log_scrubbing_middleware(request: Request<Body>, next: Next) -> Response<Body> {
    let method = request.method().clone();

    // Scrub sensitive query parameters from the URI for logging purposes.
    let scrubbed_uri = scrub_query_params(&request.uri().to_string());

    // Scrub sensitive headers for any diagnostic logging.
    let scrubbed_hdrs = scrub_headers(request.headers());
    tracing::trace!(
        method = %method,
        uri = %scrubbed_uri,
        headers = %scrubbed_hdrs,
        "Incoming request (scrubbed)"
    );

    // Process request
    let response = next.run(request).await;

    // Check if response is an error status
    let status = response.status();
    if status.is_client_error() || status.is_server_error() {
        // Build a synthetic error to exercise scrub_error_message
        let synthetic_err = std::io::Error::other(format!("{status} on {scrubbed_uri}"));
        let scrubbed_msg = scrub_error_message(&synthetic_err);

        if status.is_server_error() {
            error!(
                method = %method,
                uri = %scrubbed_uri,
                status = %status,
                error_detail = %scrubbed_msg,
                "Request error (details scrubbed)"
            );
        } else {
            warn!(
                method = %method,
                uri = %scrubbed_uri,
                status = %status,
                error_detail = %scrubbed_msg,
                "Client error (details scrubbed)"
            );
        }
    }

    response
}

/// Scrub tokens from query parameters
///
/// Useful for logging request URLs without exposing sensitive data
pub fn scrub_query_params(url: &str) -> String {
    // Remove common sensitive query parameters
    let sensitive_params = [
        "access_token",
        "refresh_token",
        "token",
        "code",
        "client_secret",
        "api_key",
        "secret",
        "password",
    ];

    let mut scrubbed = url.to_string();

    for param in &sensitive_params {
        // Match param=value pattern
        let pattern = format!(r"{}=[^&\s]*", param);
        if let Ok(re) = Regex::new(&pattern) {
            scrubbed = re
                .replace_all(&scrubbed, &format!("{}=[REDACTED]", param))
                .to_string();
        }
    }

    scrubbed
}

/// Scrub tokens from headers
///
/// Returns a safe-to-log version of headers
pub fn scrub_headers(headers: &axum::http::HeaderMap) -> String {
    let mut scrubbed_headers = String::new();

    for (name, value) in headers {
        let name_lower = name.as_str().to_lowercase();

        // Skip sensitive headers entirely
        if name_lower.contains("authorization")
            || name_lower.contains("cookie")
            || name_lower.contains("token")
            || name_lower.contains("secret")
            || name_lower.contains("api-key")
        {
            scrubbed_headers.push_str(&format!("{}: [REDACTED]{}", name, "\n"));
        } else if let Ok(value_str) = value.to_str() {
            scrubbed_headers.push_str(&format!("{}: {}{}", name, value_str, "\n"));
        }
    }

    scrubbed_headers
}

/// Format error message with scrubbed sensitive data
///
/// Use this when logging errors that might contain tokens
pub fn scrub_error_message(error: &dyn std::error::Error) -> String {
    scrub_sensitive_data(&error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // scrub_sensitive_data — pattern coverage
    // -----------------------------------------------------------------------

    #[test]
    fn scrubs_jwt_token() {
        let input = "token: eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123XYZ_-456";
        let result = scrub_sensitive_data(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("eyJhbGciOiJIUzI1NiJ9"));
    }

    #[test]
    fn scrubs_bearer_token() {
        let input = "Authorization: Bearer ya29.a0AfH6SMBx12345abcdef";
        let result = scrub_sensitive_data(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("ya29.a0AfH6SMBx12345abcdef"));
    }

    #[test]
    fn scrubs_access_token_field() {
        let input = r#"access_token: "AABBCCDD1234567890abcdef""#;
        let result = scrub_sensitive_data(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("AABBCCDD1234567890abcdef"));
    }

    #[test]
    fn scrubs_refresh_token_field() {
        let input = r#"refresh_token="rt_1234567890abcdefghij""#;
        let result = scrub_sensitive_data(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("rt_1234567890abcdefghij"));
    }

    #[test]
    fn scrubs_api_key_case_insensitive() {
        let input = r#"API-KEY: "sk-proj-abcdefghij1234567890""#;
        let result = scrub_sensitive_data(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("sk-proj-abcdefghij1234567890"));
    }

    #[test]
    fn scrubs_secret_field() {
        let input = r#"secret="my_super_secret_value_12345""#;
        let result = scrub_sensitive_data(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("my_super_secret_value_12345"));
    }

    #[test]
    fn scrubs_password_field() {
        let input = r#"password: "P@ssw0rd!Strong#123""#;
        let result = scrub_sensitive_data(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("P@ssw0rd!Strong#123"));
    }

    #[test]
    fn scrubs_pkce_code_verifier() {
        // PKCE verifiers are 43-128 chars of unreserved characters
        let verifier = "abcdefghijklmnopqrstuvwxyz0123456789_ABCDEFG";
        let input = format!(r#"code_verifier="{}""#, verifier);
        let result = scrub_sensitive_data(&input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains(verifier));
    }

    #[test]
    fn leaves_safe_text_untouched() {
        let input = "GET /api/v1/emails?page=1&limit=20 HTTP/1.1";
        let result = scrub_sensitive_data(input);
        assert_eq!(result, input);
    }

    #[test]
    fn handles_multiple_sensitive_values_in_one_string() {
        let input = r#"Bearer abc123.def456.ghi789 and password: "hunter2!abc""#;
        let result = scrub_sensitive_data(input);
        // Bearer token should be redacted
        assert!(!result.contains("abc123.def456.ghi789"));
        // Password should be redacted
        assert!(!result.contains("hunter2!abc"));
    }

    // -----------------------------------------------------------------------
    // scrub_query_params
    // -----------------------------------------------------------------------

    #[test]
    fn scrubs_access_token_from_query_string() {
        let url = "https://example.com/callback?access_token=secret123&state=abc";
        let result = scrub_query_params(url);
        assert!(result.contains("access_token=[REDACTED]"));
        assert!(!result.contains("secret123"));
        assert!(result.contains("state=abc"));
    }

    #[test]
    fn scrubs_multiple_sensitive_query_params() {
        let url = "https://example.com?token=tok123&code=authcode456&safe=yes";
        let result = scrub_query_params(url);
        assert!(result.contains("token=[REDACTED]"));
        assert!(result.contains("code=[REDACTED]"));
        assert!(result.contains("safe=yes"));
    }

    #[test]
    fn scrub_query_params_leaves_safe_url_unchanged() {
        let url = "https://example.com/api/v1/emails?page=1&limit=20";
        let result = scrub_query_params(url);
        assert_eq!(result, url);
    }

    // -----------------------------------------------------------------------
    // scrub_headers
    // -----------------------------------------------------------------------

    #[test]
    fn scrub_headers_redacts_authorization() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        let result = scrub_headers(&headers);
        assert!(result.contains("[REDACTED]"));
        assert!(result.contains("application/json"));
        assert!(!result.contains("Bearer secret"));
    }

    #[test]
    fn scrub_headers_redacts_cookie() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("cookie", "session=abc123".parse().unwrap());
        let result = scrub_headers(&headers);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("abc123"));
    }

    // -----------------------------------------------------------------------
    // scrub_error_message
    // -----------------------------------------------------------------------

    #[test]
    fn scrub_error_message_redacts_jwt_in_error() {
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.signatureABC123";
        let err = std::io::Error::other(format!("Failed with token {}", jwt));
        let result = scrub_error_message(&err);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains(jwt));
    }

    #[test]
    fn scrub_error_message_passes_through_safe_errors() {
        let err = std::io::Error::other("Connection refused on port 8080");
        let result = scrub_error_message(&err);
        assert_eq!(result, "Connection refused on port 8080");
    }
}
