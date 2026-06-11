//! Rate limiting for engine operations using a token bucket algorithm.

use crate::error::{AegisError, AegisResult};
use std::sync::Mutex;
use std::time::Instant;

/// Configuration for rate limiting.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum number of check operations per second per key.
    pub checks_per_second: u32,
    /// Maximum burst size for check operations.
    pub check_burst: u32,
    /// Maximum number of write operations per second per key.
    pub writes_per_second: u32,
    /// Maximum burst size for write operations.
    pub write_burst: u32,
    /// Maximum traversal depth for BFS.
    pub max_traversal_depth: usize,
    /// Maximum number of tuples a traversal can visit before aborting.
    pub max_traversal_visits: usize,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            checks_per_second: 1000,
            check_burst: 2000,
            writes_per_second: 100,
            write_burst: 200,
            max_traversal_depth: 10,
            max_traversal_visits: 10000,
        }
    }
}

/// Operation type to rate limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitOp {
    Check,
    Write,
}

/// Per-key token bucket state.
struct BucketState {
    tokens: f64,
    last_refill: Instant,
}

/// Token bucket rate limiter.
pub struct TokenBucketRateLimiter {
    config: RateLimitConfig,
    buckets: Mutex<std::collections::HashMap<String, BucketState>>,
}

impl TokenBucketRateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Check if an operation is allowed for the given key.
    /// Returns `RateLimitExceeded` error if the rate limit is exceeded.
    pub fn check(&self, key: &str, op: RateLimitOp) -> AegisResult<()> {
        let mut buckets = self.buckets.lock().unwrap();
        let state = buckets.entry(key.to_string()).or_insert_with(|| {
            let initial = match op {
                RateLimitOp::Check => self.config.check_burst as f64,
                RateLimitOp::Write => self.config.write_burst as f64,
            };
            BucketState {
                tokens: initial,
                last_refill: Instant::now(),
            }
        });

        let (rate, burst) = match op {
            RateLimitOp::Check => (self.config.checks_per_second as f64, self.config.check_burst as f64),
            RateLimitOp::Write => (self.config.writes_per_second as f64, self.config.write_burst as f64),
        };

        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        state.tokens = (state.tokens + elapsed * rate).min(burst);
        state.last_refill = now;

        if state.tokens < 1.0 {
            tracing::warn!(
                "rate_limit.throttled key={} op={}",
                key,
                match op { RateLimitOp::Check => "check", RateLimitOp::Write => "write" },
            );
            return Err(AegisError::RateLimitExceeded(key.to_string()));
        }

        state.tokens -= 1.0;

        tracing::debug!(
            "rate_limit.allowed key={} op={} tokens_remaining={}",
            key,
            match op { RateLimitOp::Check => "check", RateLimitOp::Write => "write" },
            state.tokens,
        );

        Ok(())
    }

    pub fn max_traversal_depth(&self) -> usize {
        self.config.max_traversal_depth
    }

    pub fn max_traversal_visits(&self) -> usize {
        self.config.max_traversal_visits
    }

    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Clear all rate limiter state (e.g., on schema reload).
    pub fn reset(&self) {
        self.buckets.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_allows_within_bounds() {
        let config = RateLimitConfig {
            checks_per_second: 100,
            check_burst: 10,
            ..Default::default()
        };
        let limiter = TokenBucketRateLimiter::new(config);

        // Should allow within burst
        for i in 0..10 {
            assert!(
                limiter.check("tenant:alpha", RateLimitOp::Check).is_ok(),
                "request {} should be allowed",
                i
            );
        }
    }

    #[test]
    fn test_rate_limit_exceeds_burst() {
        let config = RateLimitConfig {
            checks_per_second: 100,
            check_burst: 5,
            ..Default::default()
        };
        let limiter = TokenBucketRateLimiter::new(config);

        // First 5 should succeed
        for i in 0..5 {
            assert!(
                limiter.check("key:1", RateLimitOp::Check).is_ok(),
                "request {} should be allowed",
                i
            );
        }

        // 6th should fail (burst exhausted)
        let result = limiter.check("key:1", RateLimitOp::Check);
        assert!(result.is_err());
        assert!(matches!(result, Err(AegisError::RateLimitExceeded(_))));
    }

    #[test]
    fn test_rate_limit_per_key_independence() {
        let config = RateLimitConfig {
            checks_per_second: 100,
            check_burst: 3,
            ..Default::default()
        };
        let limiter = TokenBucketRateLimiter::new(config);

        // Exhaust key:1
        for _ in 0..3 {
            limiter.check("key:1", RateLimitOp::Check).unwrap();
        }
        assert!(limiter.check("key:1", RateLimitOp::Check).is_err());

        // key:2 should still work
        for _ in 0..3 {
            limiter.check("key:2", RateLimitOp::Check).unwrap();
        }
    }

    #[test]
    fn test_rate_limit_reset() {
        let config = RateLimitConfig {
            checks_per_second: 100,
            check_burst: 3,
            ..Default::default()
        };
        let limiter = TokenBucketRateLimiter::new(config);

        for _ in 0..3 {
            limiter.check("key:1", RateLimitOp::Check).unwrap();
        }
        assert!(limiter.check("key:1", RateLimitOp::Check).is_err());

        limiter.reset();

        // After reset, should allow again
        for _ in 0..3 {
            limiter.check("key:1", RateLimitOp::Check).unwrap();
        }
    }

    #[test]
    fn test_traversal_depth_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_traversal_depth, 10);
        assert_eq!(config.max_traversal_visits, 10000);
    }

    #[test]
    fn test_write_rate_limit() {
        let config = RateLimitConfig {
            writes_per_second: 10,
            write_burst: 2,
            ..Default::default()
        };
        let limiter = TokenBucketRateLimiter::new(config);

        assert!(limiter.check("key:1", RateLimitOp::Write).is_ok());
        assert!(limiter.check("key:1", RateLimitOp::Write).is_ok());
        assert!(limiter.check("key:1", RateLimitOp::Write).is_err());
    }
}
