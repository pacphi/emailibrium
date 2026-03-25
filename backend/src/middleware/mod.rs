//! Backend middleware
//!
//! Security and request processing middleware for the backend.

pub mod hsts;
pub mod log_scrub;
pub mod log_scrubbing;
pub mod rate_limit;
pub mod security_headers;

pub use log_scrubbing::{log_scrubbing_middleware, scrub_error_message, scrub_sensitive_data};
pub use rate_limit::{RateLimitConfig, RateLimiter, rate_limit_middleware};
pub use security_headers::security_headers_middleware;
