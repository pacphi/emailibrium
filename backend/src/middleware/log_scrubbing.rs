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
