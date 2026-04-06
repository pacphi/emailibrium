//! Simple sliding-window rate limiter for MCP tools (ADR-028 Phase 6).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Simple sliding-window rate limiter for MCP tools.
pub struct ToolRateLimiter {
    windows: Mutex<HashMap<String, Vec<Instant>>>,
    default_limit: u32,
}

impl ToolRateLimiter {
    pub fn new(default_limit_per_minute: u32) -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            default_limit: default_limit_per_minute,
        }
    }

    /// Check if the tool call is allowed. Returns `Ok(())` or `Err` with a message.
    pub fn check(&self, tool_name: &str, limit_override: Option<u32>) -> Result<(), String> {
        let limit = limit_override.unwrap_or(self.default_limit);
        let now = Instant::now();
        let window = Duration::from_secs(60);

        let mut windows = self.windows.lock().unwrap();
        let calls = windows.entry(tool_name.to_string()).or_default();

        // Remove calls outside the window
        calls.retain(|t| now.duration_since(*t) < window);

        if calls.len() >= limit as usize {
            return Err(format!(
                "Rate limit exceeded for {tool_name}: {limit}/minute"
            ));
        }

        calls.push(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_calls_within_limit() {
        let limiter = ToolRateLimiter::new(5);
        for _ in 0..5 {
            assert!(limiter.check("test_tool", None).is_ok());
        }
    }

    #[test]
    fn rejects_calls_over_limit() {
        let limiter = ToolRateLimiter::new(3);
        for _ in 0..3 {
            assert!(limiter.check("test_tool", None).is_ok());
        }
        let result = limiter.check("test_tool", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Rate limit exceeded"));
    }

    #[test]
    fn respects_per_tool_override() {
        let limiter = ToolRateLimiter::new(100);
        // Override to 2 per minute
        assert!(limiter.check("strict_tool", Some(2)).is_ok());
        assert!(limiter.check("strict_tool", Some(2)).is_ok());
        assert!(limiter.check("strict_tool", Some(2)).is_err());
    }

    #[test]
    fn separate_windows_per_tool() {
        let limiter = ToolRateLimiter::new(2);
        assert!(limiter.check("tool_a", None).is_ok());
        assert!(limiter.check("tool_a", None).is_ok());
        assert!(limiter.check("tool_a", None).is_err());
        // tool_b should still be allowed
        assert!(limiter.check("tool_b", None).is_ok());
    }
}
