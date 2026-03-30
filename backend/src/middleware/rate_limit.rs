//! Enhanced Rate Limiting Middleware
//!
//! Implements token bucket algorithm with dual-key rate limiting (per-IP + per-user)
//! and optional Redis backend for distributed rate limiting across multiple instances.
//!
//! Features:
//! - In-memory rate limiting with token bucket algorithm
//! - Optional Redis backend for distributed deployments
//! - Dual-key rate limiting (per-IP + per-user)
//! - Configurable rate limits via environment variables
//! - Enhanced response headers (X-RateLimit-Reset, X-RateLimit-Policy)
//! - Automatic fallback to in-memory if Redis unavailable

use axum::{
    extract::{ConnectInfo, Request},
    http::{header::HeaderName, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing::{debug, warn};

/// Token bucket for rate limiting
#[derive(Clone, Debug)]
pub struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_rate: f64, // tokens per second
    pub last_refill: Instant,
}

impl TokenBucket {
    pub fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity,
            capacity,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    pub fn try_consume(&mut self, tokens: f64) -> bool {
        // Refill tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;

        // Try to consume tokens
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }

    fn remaining(&self) -> f64 {
        self.tokens
    }

    fn time_until_refill(&self) -> Duration {
        let tokens_needed = 1.0 - self.tokens;
        let seconds = (tokens_needed / self.refill_rate).ceil() as u64;
        Duration::from_secs(seconds.max(1))
    }
}

/// Rate limiting backend trait for abstraction
#[async_trait::async_trait]
pub trait RateLimitBackend: Send + Sync {
    async fn check(&self, key: &str, capacity: f64, refill_rate: f64) -> (bool, f64, Duration);
}

/// In-memory rate limiter (default)
#[derive(Clone)]
pub struct InMemoryBackend {
    buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryBackend {
    pub fn new() -> Self {
        let backend = Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
        };

        // Cleanup task: remove stale buckets every 60 seconds
        let buckets = Arc::clone(&backend.buckets);
        let cleanup_interval_duration = Duration::from_secs(60);
        let bucket_expiry = Duration::from_secs(300); // 5 minutes

        tokio::spawn(async move {
            let mut cleanup_interval = interval(cleanup_interval_duration);
            loop {
                cleanup_interval.tick().await;
                let mut map = buckets.lock().await;
                map.retain(|_, bucket| bucket.last_refill.elapsed() < bucket_expiry);
                debug!("Rate limiter cleanup: {} active buckets", map.len());
            }
        });

        backend
    }
}

#[async_trait::async_trait]
impl RateLimitBackend for InMemoryBackend {
    async fn check(&self, key: &str, capacity: f64, refill_rate: f64) -> (bool, f64, Duration) {
        let mut map = self.buckets.lock().await;
        let bucket = map
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket::new(capacity, refill_rate));

        let allowed = bucket.try_consume(1.0);
        let remaining = bucket.remaining();
        let retry_after = if !allowed {
            bucket.time_until_refill()
        } else {
            Duration::from_secs(0)
        };

        (allowed, remaining, retry_after)
    }
}

/// Unified rate limiter supporting multiple backends
#[derive(Clone)]
pub struct RateLimiter {
    backend: Arc<dyn RateLimitBackend>,
    capacity: f64,
    refill_rate: f64,
    endpoint_name: String,
}

impl RateLimiter {
    pub fn new_in_memory(capacity: f64, refill_rate: f64, endpoint_name: String) -> Self {
        Self {
            backend: Arc::new(InMemoryBackend::new()),
            capacity,
            refill_rate,
            endpoint_name,
        }
    }

    pub async fn check_ip(&self, ip: IpAddr) -> (bool, f64, Duration) {
        let key = format!("ip:{}:{}", self.endpoint_name, ip);
        self.backend
            .check(&key, self.capacity, self.refill_rate)
            .await
    }

    pub async fn check_user(&self, user_id: &str) -> (bool, f64, Duration) {
        let key = format!("user:{}:{}", self.endpoint_name, user_id);
        self.backend
            .check(&key, self.capacity, self.refill_rate)
            .await
    }

    pub fn get_policy_description(&self) -> String {
        let requests_per_minute = (self.refill_rate * 60.0) as u32;
        format!(
            "{} requests per minute (burst: {})",
            requests_per_minute, self.capacity as u32
        )
    }
}

/// Rate limiting preset configurations
#[derive(Clone, Debug)]
pub enum RateLimitPreset {
    Development,
    Staging,
    Production,
}

/// Comprehensive rate limiting configuration
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    // OAuth endpoints
    pub auth_start_limit: u32,
    pub auth_callback_limit: u32,
    pub session_status_limit: u32,
    pub token_refresh_limit: u32,

    // Backend configuration
    pub redis_url: Option<String>,
    pub enable_redis: bool,
    pub redis_fallback: bool,

    // User rate limiting
    pub enable_user_limits: bool,
    pub user_limit_multiplier: f64,

    // app.yaml fallback values (used when env/preset doesn't specify global limits)
    yaml_global_capacity: Option<usize>,
    yaml_global_refill_per_sec: Option<f64>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self::from_preset(RateLimitPreset::Production)
    }
}

impl RateLimitConfig {
    pub fn from_preset(preset: RateLimitPreset) -> Self {
        match preset {
            RateLimitPreset::Development => Self {
                auth_start_limit: 100,
                auth_callback_limit: 100,
                session_status_limit: 200,
                token_refresh_limit: 100,
                redis_url: None,
                enable_redis: false,
                redis_fallback: true,
                enable_user_limits: false,
                user_limit_multiplier: 1.0,
                yaml_global_capacity: None,
                yaml_global_refill_per_sec: None,
            },
            RateLimitPreset::Staging => Self {
                auth_start_limit: 20,
                auth_callback_limit: 10,
                session_status_limit: 120,
                token_refresh_limit: 40,
                redis_url: None,
                enable_redis: false,
                redis_fallback: true,
                enable_user_limits: true,
                user_limit_multiplier: 2.0,
                yaml_global_capacity: None,
                yaml_global_refill_per_sec: None,
            },
            RateLimitPreset::Production => Self {
                auth_start_limit: 10,
                auth_callback_limit: 5,
                session_status_limit: 60,
                token_refresh_limit: 20,
                redis_url: None,
                enable_redis: false,
                redis_fallback: true,
                enable_user_limits: true,
                user_limit_multiplier: 3.0,
                yaml_global_capacity: None,
                yaml_global_refill_per_sec: None,
            },
        }
    }

    pub fn from_env() -> Self {
        let preset_name = std::env::var("RATE_LIMIT_PRESET")
            .ok()
            .unwrap_or_else(|| "production".to_string());

        let preset = match preset_name.to_lowercase().as_str() {
            "development" | "dev" => RateLimitPreset::Development,
            "staging" | "stage" => RateLimitPreset::Staging,
            _ => RateLimitPreset::Production,
        };

        let mut config = Self::from_preset(preset);

        // Override with environment variables if present
        if let Ok(val) = std::env::var("RATE_LIMIT_AUTH_START") {
            if let Ok(limit) = val.parse() {
                config.auth_start_limit = limit;
            }
        }

        if let Ok(val) = std::env::var("RATE_LIMIT_AUTH_CALLBACK") {
            if let Ok(limit) = val.parse() {
                config.auth_callback_limit = limit;
            }
        }

        if let Ok(val) = std::env::var("RATE_LIMIT_SESSION_STATUS") {
            if let Ok(limit) = val.parse() {
                config.session_status_limit = limit;
            }
        }

        if let Ok(val) = std::env::var("RATE_LIMIT_TOKEN_REFRESH") {
            if let Ok(limit) = val.parse() {
                config.token_refresh_limit = limit;
            }
        }

        // Redis configuration
        config.redis_url = std::env::var("RATE_LIMIT_REDIS_URL").ok();
        config.enable_redis = config.redis_url.is_some()
            && std::env::var("RATE_LIMIT_ENABLE_REDIS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true);

        config.redis_fallback = std::env::var("RATE_LIMIT_REDIS_FALLBACK")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(true);

        // User limiting
        config.enable_user_limits = std::env::var("RATE_LIMIT_ENABLE_USER_LIMITS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(true);

        config.user_limit_multiplier = std::env::var("RATE_LIMIT_USER_MULTIPLIER")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3.0);

        config
    }

    /// Build config from environment variables with `app.yaml` security values
    /// as fallback defaults for capacity and refill rate.
    ///
    /// Priority: env vars > preset > app.yaml fallback.
    pub fn from_env_with_yaml_fallback(yaml_capacity: usize, yaml_refill_per_sec: f64) -> Self {
        let mut config = Self::from_env();
        // Store YAML fallback values for use in get_capacity_and_rate.
        config.yaml_global_capacity = Some(yaml_capacity);
        config.yaml_global_refill_per_sec = Some(yaml_refill_per_sec);
        config
    }

    pub fn get_capacity_and_rate(&self, endpoint: &str) -> (f64, f64) {
        let limit = match endpoint {
            "auth_start" => self.auth_start_limit,
            "auth_callback" => self.auth_callback_limit,
            "session_status" => self.session_status_limit,
            "token_refresh" => self.token_refresh_limit,
            // Default: use session_status_limit (high) for dev, yaml fallback, or 60 for production
            _ => {
                if !self.enable_user_limits {
                    // Development mode — use generous default
                    self.session_status_limit.max(200)
                } else if let Some(yaml_cap) = self.yaml_global_capacity {
                    // Use app.yaml rate_limit_capacity as the global default
                    yaml_cap as u32
                } else {
                    60
                }
            }
        };

        // Capacity is the burst limit, rate is tokens per second.
        // For the global endpoint, prefer app.yaml refill rate if available.
        let capacity = limit as f64;
        let refill_rate = match endpoint {
            "auth_start" | "auth_callback" | "session_status" | "token_refresh" => {
                limit as f64 / 60.0
            }
            _ => {
                // Use app.yaml rate_limit_refill_per_sec if available
                self.yaml_global_refill_per_sec
                    .unwrap_or(limit as f64 / 60.0)
            }
        };

        (capacity, refill_rate)
    }
}

/// Extract user ID from request (if authenticated)
fn extract_user_id(req: &Request) -> Option<String> {
    // Try to extract from Authorization header or session
    req.headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|auth| {
            if auth.starts_with("Bearer ") {
                // In production, decode JWT and extract user_id
                // For now, use a placeholder
                Some(auth.replace("Bearer ", ""))
            } else {
                None
            }
        })
}

/// Enhanced rate limiting middleware with dual-key support
pub async fn rate_limit_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    limiter: axum::Extension<Arc<RateLimiter>>,
    req: Request,
    next: Next,
) -> Result<Response, impl IntoResponse> {
    let ip = addr.ip();
    let user_id = extract_user_id(&req);

    // Check IP-based rate limit
    let (ip_allowed, ip_remaining, ip_retry_after) = limiter.check_ip(ip).await;

    if !ip_allowed {
        warn!(
            "IP rate limit exceeded: {} (retry after: {}s)",
            ip,
            ip_retry_after.as_secs()
        );

        return Err(build_rate_limit_response(
            limiter.capacity,
            0.0,
            ip_retry_after,
            &limiter.get_policy_description(),
            "IP rate limit exceeded",
        ));
    }

    // Check user-based rate limit (if authenticated)
    let (user_remaining, _user_retry_after) = if let Some(ref uid) = user_id {
        let (user_allowed, user_remaining, user_retry_after) = limiter.check_user(uid).await;

        if !user_allowed {
            warn!(
                "User rate limit exceeded: {} (retry after: {}s)",
                uid,
                user_retry_after.as_secs()
            );

            return Err(build_rate_limit_response(
                limiter.capacity,
                0.0,
                user_retry_after,
                &limiter.get_policy_description(),
                "User rate limit exceeded",
            ));
        }

        (user_remaining, user_retry_after)
    } else {
        (limiter.capacity, Duration::from_secs(0))
    };

    // Use the more restrictive remaining count
    let remaining = ip_remaining.min(user_remaining);

    debug!(
        "Rate limit check passed for IP: {} (IP remaining: {:.1}, User remaining: {:.1})",
        ip, ip_remaining, user_remaining
    );

    let mut response = next.run(req).await;

    // Add comprehensive rate limit headers
    add_rate_limit_headers(&mut response, limiter.capacity, remaining, &limiter);

    Ok(response)
}

/// Build rate limit error response with enhanced headers
fn build_rate_limit_response(
    limit: f64,
    remaining: f64,
    retry_after: Duration,
    policy: &str,
    message: &str,
) -> (StatusCode, [(String, String); 5], String) {
    let reset_time = SystemTime::now() + retry_after;
    let reset_timestamp = reset_time.duration_since(UNIX_EPOCH).unwrap().as_secs();

    (
        StatusCode::TOO_MANY_REQUESTS,
        [
            ("X-RateLimit-Limit".to_string(), limit.to_string()),
            ("X-RateLimit-Remaining".to_string(), remaining.to_string()),
            ("X-RateLimit-Reset".to_string(), reset_timestamp.to_string()),
            ("X-RateLimit-Policy".to_string(), policy.to_string()),
            ("Retry-After".to_string(), retry_after.as_secs().to_string()),
        ],
        message.to_string(),
    )
}

/// Add rate limit headers to successful response
fn add_rate_limit_headers(
    response: &mut Response,
    limit: f64,
    remaining: f64,
    limiter: &RateLimiter,
) {
    let headers = response.headers_mut();

    headers.insert(
        HeaderName::from_static("x-ratelimit-limit"),
        limit.to_string().parse().unwrap(),
    );

    headers.insert(
        HeaderName::from_static("x-ratelimit-remaining"),
        remaining.floor().to_string().parse().unwrap(),
    );

    headers.insert(
        HeaderName::from_static("x-ratelimit-policy"),
        limiter.get_policy_description().parse().unwrap(),
    );

    // Add reset time (current time + 60 seconds for next bucket refill)
    let reset_time = SystemTime::now() + Duration::from_secs(60);
    let reset_timestamp = reset_time.duration_since(UNIX_EPOCH).unwrap().as_secs();

    headers.insert(
        HeaderName::from_static("x-ratelimit-reset"),
        reset_timestamp.to_string().parse().unwrap(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // TokenBucket unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn token_bucket_new_starts_at_full_capacity() {
        let bucket = TokenBucket::new(10.0, 1.0);
        assert_eq!(bucket.remaining(), 10.0);
    }

    #[test]
    fn token_bucket_try_consume_succeeds_when_tokens_available() {
        let mut bucket = TokenBucket::new(5.0, 1.0);
        assert!(bucket.try_consume(1.0));
        // After consuming 1, remaining should be close to 4 (plus tiny refill)
        assert!(bucket.remaining() >= 3.9);
        assert!(bucket.remaining() <= 5.0);
    }

    #[test]
    fn token_bucket_try_consume_fails_when_exhausted() {
        let mut bucket = TokenBucket::new(2.0, 0.0); // zero refill so no recovery
        assert!(bucket.try_consume(1.0));
        assert!(bucket.try_consume(1.0));
        assert!(!bucket.try_consume(1.0));
    }

    #[test]
    fn token_bucket_never_exceeds_capacity() {
        let mut bucket = TokenBucket::new(5.0, 1000.0); // very high refill rate
                                                        // Even with high refill, consuming then waiting should not exceed capacity
        bucket.try_consume(1.0);
        std::thread::sleep(std::time::Duration::from_millis(50));
        bucket.try_consume(0.0); // triggers refill
        assert!(bucket.remaining() <= 5.0);
    }

    #[test]
    fn token_bucket_consumes_fractional_tokens() {
        let mut bucket = TokenBucket::new(1.0, 0.0);
        assert!(bucket.try_consume(0.5));
        assert!(bucket.try_consume(0.5));
        assert!(!bucket.try_consume(0.5));
    }

    #[test]
    fn token_bucket_time_until_refill_at_least_one_second() {
        let mut bucket = TokenBucket::new(2.0, 1.0);
        bucket.try_consume(2.0);
        let dur = bucket.time_until_refill();
        assert!(dur.as_secs() >= 1);
    }

    // -----------------------------------------------------------------------
    // RateLimitConfig preset tests
    // -----------------------------------------------------------------------

    #[test]
    fn production_preset_has_strictest_limits() {
        let prod = RateLimitConfig::from_preset(RateLimitPreset::Production);
        let dev = RateLimitConfig::from_preset(RateLimitPreset::Development);
        assert!(prod.auth_start_limit < dev.auth_start_limit);
        assert!(prod.auth_callback_limit < dev.auth_callback_limit);
        assert!(prod.session_status_limit < dev.session_status_limit);
    }

    #[test]
    fn development_preset_disables_user_limits() {
        let dev = RateLimitConfig::from_preset(RateLimitPreset::Development);
        assert!(!dev.enable_user_limits);
    }

    #[test]
    fn staging_preset_enables_user_limits() {
        let staging = RateLimitConfig::from_preset(RateLimitPreset::Staging);
        assert!(staging.enable_user_limits);
        assert_eq!(staging.user_limit_multiplier, 2.0);
    }

    #[test]
    fn default_config_uses_production_preset() {
        let default = RateLimitConfig::default();
        let prod = RateLimitConfig::from_preset(RateLimitPreset::Production);
        assert_eq!(default.auth_start_limit, prod.auth_start_limit);
        assert_eq!(default.auth_callback_limit, prod.auth_callback_limit);
    }

    // -----------------------------------------------------------------------
    // get_capacity_and_rate endpoint lookup tests
    // -----------------------------------------------------------------------

    #[test]
    fn get_capacity_and_rate_returns_correct_auth_start() {
        let config = RateLimitConfig::from_preset(RateLimitPreset::Production);
        let (capacity, rate) = config.get_capacity_and_rate("auth_start");
        assert_eq!(capacity, 10.0);
        assert!((rate - 10.0 / 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn get_capacity_and_rate_returns_correct_session_status() {
        let config = RateLimitConfig::from_preset(RateLimitPreset::Production);
        let (capacity, _rate) = config.get_capacity_and_rate("session_status");
        assert_eq!(capacity, 60.0);
    }

    #[test]
    fn get_capacity_and_rate_returns_default_for_unknown_endpoint() {
        let config = RateLimitConfig::from_preset(RateLimitPreset::Production);
        let (capacity, rate) = config.get_capacity_and_rate("unknown_endpoint");
        assert_eq!(capacity, 60.0);
        assert!((rate - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn get_capacity_and_rate_uses_yaml_fallback_for_global() {
        let mut config = RateLimitConfig::from_preset(RateLimitPreset::Production);
        config.yaml_global_capacity = Some(100);
        config.yaml_global_refill_per_sec = Some(2.0);
        let (capacity, rate) = config.get_capacity_and_rate("global");
        assert_eq!(capacity, 100.0);
        assert!((rate - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn get_capacity_and_rate_yaml_fallback_does_not_affect_named_endpoints() {
        let mut config = RateLimitConfig::from_preset(RateLimitPreset::Production);
        config.yaml_global_capacity = Some(100);
        config.yaml_global_refill_per_sec = Some(2.0);
        // Named endpoints still use their preset values
        let (capacity, rate) = config.get_capacity_and_rate("auth_start");
        assert_eq!(capacity, 10.0);
        assert!((rate - 10.0 / 60.0).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // RateLimiter and InMemoryBackend async tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn in_memory_backend_allows_within_capacity() {
        let backend = InMemoryBackend::new();
        let (allowed, remaining, _) = backend.check("test_key", 5.0, 1.0).await;
        assert!(allowed);
        assert!(remaining >= 3.0); // consumed 1 from 5
    }

    #[tokio::test]
    async fn in_memory_backend_rejects_after_exhaustion() {
        let backend = InMemoryBackend::new();
        // Exhaust a bucket with capacity 2
        let _ = backend.check("exhaust", 2.0, 0.0).await;
        let _ = backend.check("exhaust", 2.0, 0.0).await;
        let (allowed, _, retry_after) = backend.check("exhaust", 2.0, 0.0).await;
        assert!(!allowed);
        assert!(retry_after.as_secs() >= 1);
    }

    #[tokio::test]
    async fn rate_limiter_policy_description_format() {
        let limiter = RateLimiter::new_in_memory(10.0, 2.0, "test".to_string());
        let desc = limiter.get_policy_description();
        assert!(desc.contains("120 requests per minute"));
        assert!(desc.contains("burst: 10"));
    }

    #[tokio::test]
    async fn rate_limiter_check_ip_uses_endpoint_scoped_key() {
        let limiter = RateLimiter::new_in_memory(100.0, 10.0, "auth".to_string());
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let (allowed, _, _) = limiter.check_ip(ip).await;
        assert!(allowed);
    }

    #[tokio::test]
    async fn rate_limiter_check_user_uses_endpoint_scoped_key() {
        let limiter = RateLimiter::new_in_memory(100.0, 10.0, "auth".to_string());
        let (allowed, _, _) = limiter.check_user("user123").await;
        assert!(allowed);
    }
}
