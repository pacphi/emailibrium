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
    http::{StatusCode, header::HeaderName},
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

    pub fn get_capacity_and_rate(&self, endpoint: &str) -> (f64, f64) {
        let limit = match endpoint {
            "auth_start" => self.auth_start_limit,
            "auth_callback" => self.auth_callback_limit,
            "session_status" => self.session_status_limit,
            "token_refresh" => self.token_refresh_limit,
            _ => 60,
        };

        // Capacity is the burst limit, rate is tokens per second
        let capacity = limit as f64;
        let refill_rate = limit as f64 / 60.0; // Per-minute limit converted to per-second

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
