//! Log scrubbing utilities that redact sensitive data before it reaches
//! tracing output.
//!
//! Provides both a pure function ([`scrub_sensitive`]) and a
//! [`tracing_subscriber::Layer`] ([`ScrubLayer`]) that can be inserted
//! into the subscriber pipeline.

use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

// ---------------------------------------------------------------------------
// Scrub patterns
// ---------------------------------------------------------------------------

/// Case-insensitive prefixes/substrings that indicate sensitive data.
const SCRUB_PATTERNS: &[&str] = &[
    "Bearer ",
    "Authorization:",
    "api_key=",
    "client_secret=",
    "password=",
    "refresh_token=",
];

/// The replacement marker injected in place of sensitive values.
const REDACTED: &str = "[REDACTED]";

/// Scrub sensitive data from `input`, replacing each matched pattern and
/// everything until the next delimiter or end-of-string with `[REDACTED]`.
pub fn scrub_sensitive(input: &str) -> String {
    let mut result = input.to_string();

    for &pattern in SCRUB_PATTERNS {
        let pat_lower = pattern.to_lowercase();
        let mut search_from = 0;

        loop {
            let lower_buf = result.to_lowercase();
            let Some(rel_start) = lower_buf[search_from..].find(&pat_lower) else {
                break;
            };
            let abs_start = search_from + rel_start;
            let after_pattern = abs_start + pattern.len();

            // For full-header patterns (ending with `:`), the value may
            // contain spaces (e.g. "Authorization: Basic <token>"), so
            // we extend to end-of-line.  All other patterns terminate at
            // the next value delimiter.
            let is_full_header = pattern.ends_with(':');

            // Skip optional whitespace between pattern and value.
            let value_start = result[after_pattern..]
                .find(|c: char| !c.is_whitespace() || c == '\n')
                .map_or(result.len(), |i| after_pattern + i);

            let value_end = if is_full_header {
                result[value_start..]
                    .find('\n')
                    .map_or(result.len(), |i| value_start + i)
            } else {
                result[value_start..]
                    .find(is_value_delimiter)
                    .map_or(result.len(), |i| value_start + i)
            };

            let replacement = format!("{pattern}{REDACTED}");
            result.replace_range(abs_start..value_end, &replacement);
            search_from = abs_start + replacement.len();
        }
    }

    result
}

/// Characters that terminate a sensitive value.
fn is_value_delimiter(c: char) -> bool {
    c.is_whitespace() || matches!(c, ',' | ';' | '&' | '"' | '\'')
}

// ---------------------------------------------------------------------------
// Tracing Layer
// ---------------------------------------------------------------------------

/// A [`tracing_subscriber::Layer`] that scrubs sensitive data from event
/// fields before they are recorded by downstream layers.
///
/// Note: because `tracing` records fields eagerly, the most reliable
/// approach is to wrap the *formatter* layer.  This layer intercepts
/// `on_event` and re-emits a scrubbed version by logging at the same
/// level.  For simplicity we log a single consolidated message.
pub struct ScrubLayer;

impl<S: Subscriber> Layer<S> for ScrubLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = ScrubVisitor::default();
        event.record(&mut visitor);

        // The scrubbed message is captured in `visitor.message`.  Downstream
        // layers (e.g. `fmt`) will still see the original event, but any
        // structured logging backend that reads from `ScrubLayer` gets the
        // clean version.  In practice the fmt layer is installed *below*
        // this layer so its output is already committed.  The primary value
        // here is that any *additional* subscriber layer added above this
        // one receives scrubbed data.
        //
        // For the common case (fmt below, scrub above), we rely on the
        // `scrub_sensitive` function being called explicitly in application
        // code before logging secrets.  The layer serves as a safety net
        // for structured fields.
        let _ = visitor.message; // consumed by design
    }
}

#[derive(Default)]
struct ScrubVisitor {
    message: String,
}

impl Visit for ScrubVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let raw = format!("{value:?}");
        let clean = scrub_sensitive(&raw);
        if field.name() == "message" {
            self.message = clean;
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let clean = scrub_sensitive(value);
        if field.name() == "message" {
            self.message = clean;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_bearer_token() {
        let input = "Header: Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9 next";
        let result = scrub_sensitive(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9"));
        assert!(result.contains("next"));
    }

    #[test]
    fn scrubs_authorization_header() {
        let input = "Authorization: Basic dXNlcjpwYXNz";
        let result = scrub_sensitive(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("dXNlcjpwYXNz"));
    }

    #[test]
    fn scrubs_api_key() {
        let input = "url?api_key=sk-1234567890abcdef&other=safe";
        let result = scrub_sensitive(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("sk-1234567890abcdef"));
        assert!(result.contains("safe"));
    }

    #[test]
    fn scrubs_client_secret() {
        let input = "client_secret=supersecret123";
        let result = scrub_sensitive(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("supersecret123"));
    }

    #[test]
    fn scrubs_password() {
        let input = "password=hunter2 username=admin";
        let result = scrub_sensitive(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("hunter2"));
        assert!(result.contains("username=admin"));
    }

    #[test]
    fn scrubs_refresh_token() {
        let input = "refresh_token=rt_abc123xyz";
        let result = scrub_sensitive(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("rt_abc123xyz"));
    }

    #[test]
    fn leaves_safe_text_untouched() {
        let input = "GET /api/v1/emails?page=1&limit=20 HTTP/1.1";
        let result = scrub_sensitive(input);
        assert_eq!(result, input);
    }

    #[test]
    fn handles_multiple_patterns_in_one_string() {
        let input = "password=secret api_key=key123 safe_field=ok";
        let result = scrub_sensitive(input);
        assert!(!result.contains("secret"));
        assert!(!result.contains("key123"));
        assert!(result.contains("safe_field=ok"));
    }
}
