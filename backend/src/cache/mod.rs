//! Redis cache layer for Emailibrium.
//!
//! Provides a thin async wrapper around `redis::aio::ConnectionManager` with
//! automatic reconnection, typed get/set via serde, and pub/sub support.
//!
//! The cache is **optional** -- the backend operates without Redis and callers
//! should treat a missing `RedisCache` as a permanent cache miss.

use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{de::DeserializeOwned, Serialize};
use tracing::{debug, info, warn};

/// Redis-backed cache with automatic reconnection via `ConnectionManager`.
#[derive(Clone)]
pub struct RedisCache {
    conn: ConnectionManager,
}

impl RedisCache {
    /// Connect to Redis at `url` (e.g. `redis://127.0.0.1:6379`).
    ///
    /// Retries up to 3 times with exponential back-off before giving up.
    pub async fn connect(url: &str) -> Result<Self, RedisError> {
        let client = redis::Client::open(url)
            .map_err(|e| RedisError::Connection(format!("Invalid Redis URL: {e}")))?;

        let mut last_err = String::new();
        for attempt in 0..3u32 {
            match ConnectionManager::new(client.clone()).await {
                Ok(conn) => {
                    info!("Connected to Redis at {url}");
                    return Ok(Self { conn });
                }
                Err(e) => {
                    last_err = e.to_string();
                    let delay = std::time::Duration::from_millis(100 * 2u64.pow(attempt));
                    warn!(
                        attempt = attempt + 1,
                        "Redis connection attempt failed: {e}, retrying in {delay:?}"
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }

        Err(RedisError::Connection(format!(
            "Failed to connect to Redis after 3 attempts: {last_err}"
        )))
    }

    /// Retrieve a JSON-serialized value by key.
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, RedisError> {
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn
            .get(key)
            .await
            .map_err(|e| RedisError::Command(format!("GET {key}: {e}")))?;

        match raw {
            Some(json) => {
                let value = serde_json::from_str(&json)
                    .map_err(|e| RedisError::Serialization(format!("deserialize {key}: {e}")))?;
                debug!(key, "Redis cache hit");
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Store a JSON-serialized value with a TTL.
    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_secs: u64,
    ) -> Result<(), RedisError> {
        let json = serde_json::to_string(value)
            .map_err(|e| RedisError::Serialization(format!("serialize {key}: {e}")))?;
        let mut conn = self.conn.clone();
        conn.set_ex::<_, _, ()>(key, &json, ttl_secs)
            .await
            .map_err(|e| RedisError::Command(format!("SET {key}: {e}")))?;
        debug!(key, ttl_secs, "Redis cache set");
        Ok(())
    }

    /// Delete a key.
    pub async fn delete(&self, key: &str) -> Result<(), RedisError> {
        let mut conn = self.conn.clone();
        conn.del::<_, ()>(key)
            .await
            .map_err(|e| RedisError::Command(format!("DEL {key}: {e}")))?;
        Ok(())
    }

    /// Publish a message to a Redis channel.
    pub async fn publish(&self, channel: &str, message: &str) -> Result<(), RedisError> {
        let mut conn = self.conn.clone();
        conn.publish::<_, _, ()>(channel, message)
            .await
            .map_err(|e| RedisError::Command(format!("PUBLISH {channel}: {e}")))?;
        Ok(())
    }

    /// Batch-retrieve multiple JSON-serialized values by key.
    ///
    /// Returns a `Vec` of the same length as `keys`, with `Some(T)` for cache
    /// hits and `None` for misses.  Uses a single Redis `MGET` round-trip.
    pub async fn mget<T: DeserializeOwned>(
        &self,
        keys: &[String],
    ) -> Result<Vec<Option<T>>, RedisError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn.clone();
        let raw_values: Vec<Option<String>> = conn
            .mget(keys)
            .await
            .map_err(|e| RedisError::Command(format!("MGET: {e}")))?;

        let mut results = Vec::with_capacity(raw_values.len());
        for (i, raw) in raw_values.into_iter().enumerate() {
            match raw {
                Some(json) => match serde_json::from_str(&json) {
                    Ok(value) => {
                        debug!(key = %keys[i], "Redis MGET cache hit");
                        results.push(Some(value));
                    }
                    Err(e) => {
                        warn!(key = %keys[i], "Redis MGET deserialization failed: {e}");
                        results.push(None);
                    }
                },
                None => results.push(None),
            }
        }
        Ok(results)
    }

    /// Batch-store multiple JSON-serialized key-value pairs with a TTL.
    ///
    /// Uses a Redis pipeline to issue all `SET EX` commands in a single
    /// round-trip, preserving per-key TTL semantics.
    pub async fn mset<T: Serialize>(
        &self,
        pairs: &[(String, T)],
        ttl_secs: u64,
    ) -> Result<(), RedisError> {
        if pairs.is_empty() {
            return Ok(());
        }
        let mut pipe = redis::pipe();
        for (key, value) in pairs {
            let json = serde_json::to_string(value)
                .map_err(|e| RedisError::Serialization(format!("serialize {key}: {e}")))?;
            pipe.cmd("SET")
                .arg(key)
                .arg(json)
                .arg("EX")
                .arg(ttl_secs)
                .ignore();
        }
        let mut conn = self.conn.clone();
        pipe.query_async::<()>(&mut conn)
            .await
            .map_err(|e| RedisError::Command(format!("MSET pipeline: {e}")))?;
        debug!(
            count = pairs.len(),
            ttl_secs, "Redis MSET pipeline executed"
        );
        Ok(())
    }

    /// Health check via PING.
    pub async fn health(&self) -> Result<bool, RedisError> {
        let mut conn = self.conn.clone();
        let pong: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .map_err(|e| RedisError::Command(format!("PING: {e}")))?;
        Ok(pong == "PONG")
    }
}

/// Errors from the Redis cache layer.
#[derive(Debug, thiserror::Error)]
pub enum RedisError {
    #[error("Redis connection error: {0}")]
    Connection(String),
    #[error("Redis command error: {0}")]
    Command(String),
    #[error("Redis serialization error: {0}")]
    Serialization(String),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redis_error_display() {
        let err = RedisError::Connection("refused".into());
        assert!(err.to_string().contains("refused"));
    }

    #[tokio::test]
    async fn test_connect_invalid_url_returns_error() {
        // A URL that cannot be parsed should fail fast.
        let result = RedisCache::connect("not-a-url://bad").await;
        assert!(result.is_err());
    }
}
