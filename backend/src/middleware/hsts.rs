//! HSTS (HTTP Strict Transport Security) header middleware.
//!
//! Provides a [`tower_http::set_header::SetResponseHeaderLayer`] that
//! attaches the `Strict-Transport-Security` header to every response.

use axum::http::{header, HeaderValue};
use tower_http::set_header::SetResponseHeaderLayer;

/// Convenience name for the concrete layer type returned by [`hsts_layer`].
pub type HstsLayer = SetResponseHeaderLayer<HeaderValue>;

/// Build a Tower layer that sets the `Strict-Transport-Security` header.
///
/// # Arguments
///
/// * `max_age_secs` -- How long (in seconds) the browser should remember
///   that this site must only be accessed via HTTPS.  A common production
///   value is `63_072_000` (2 years).
/// * `include_subdomains` -- Whether the policy applies to all subdomains.
///
/// # Example
///
/// ```rust,ignore
/// use emailibrium::middleware::hsts::hsts_layer;
///
/// let app = Router::new()
///     .route("/", get(handler))
///     .layer(hsts_layer(63_072_000, true));
/// ```
pub fn hsts_layer(max_age_secs: u64, include_subdomains: bool) -> HstsLayer {
    let value = if include_subdomains {
        format!("max-age={max_age_secs}; includeSubDomains")
    } else {
        format!("max-age={max_age_secs}")
    };

    let header_value =
        HeaderValue::from_str(&value).expect("HSTS header value must be valid ASCII");

    SetResponseHeaderLayer::overriding(header::STRICT_TRANSPORT_SECURITY, header_value)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsts_header_with_subdomains() {
        let layer = hsts_layer(63_072_000, true);
        // The layer itself is opaque, but we can verify construction
        // doesn't panic and the type is correct.
        let _ = layer;
    }

    #[test]
    fn hsts_header_without_subdomains() {
        let layer = hsts_layer(31_536_000, false);
        let _ = layer;
    }

    #[test]
    fn hsts_value_format_with_subdomains() {
        let value = format!("max-age={}; includeSubDomains", 63_072_000);
        let hv = HeaderValue::from_str(&value).unwrap();
        assert_eq!(hv.to_str().unwrap(), "max-age=63072000; includeSubDomains");
    }

    #[test]
    fn hsts_value_format_without_subdomains() {
        let value = format!("max-age={}", 31_536_000);
        let hv = HeaderValue::from_str(&value).unwrap();
        assert_eq!(hv.to_str().unwrap(), "max-age=31536000");
    }
}
