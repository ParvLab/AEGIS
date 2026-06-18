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
    /// Maximum number of tracked keys before LRU eviction.
    pub max_keys: usize,
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
            max_keys: 10_000,
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
    last_accessed: Instant,
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
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|e| AegisError::Internal(format!("rate limiter lock poisoned: {e}")))?;

        // Evict the least-recently-accessed entry if we need to insert a new key
        // and the map is at capacity.
        if !buckets.contains_key(key) && buckets.len() >= self.config.max_keys {
            if let Some(oldest_key) = buckets
                .iter()
                .min_by_key(|(_, state)| state.last_accessed)
                .map(|(k, _)| k.clone())
            {
                buckets.remove(&oldest_key);
            }
        }

        let state = buckets.entry(key.to_string()).or_insert_with(|| {
            let initial = match op {
                RateLimitOp::Check => self.config.check_burst as f64,
                RateLimitOp::Write => self.config.write_burst as f64,
            };
            BucketState {
                tokens: initial,
                last_refill: Instant::now(),
                last_accessed: Instant::now(),
            }
        });

        let (rate, burst) = match op {
            RateLimitOp::Check => (
                self.config.checks_per_second as f64,
                self.config.check_burst as f64,
            ),
            RateLimitOp::Write => (
                self.config.writes_per_second as f64,
                self.config.write_burst as f64,
            ),
        };

        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        state.tokens = (state.tokens + elapsed * rate).min(burst);
        state.last_refill = now;
        state.last_accessed = now;

        if state.tokens < 1.0 {
            let sanitized: String = key
                .chars()
                .filter(|&c| c.is_alphanumeric() || c == ':' || c == '_' || c == '-')
                .take(128)
                .collect();
            tracing::warn!(
                "rate_limit.throttled key={} op={}",
                sanitized,
                match op {
                    RateLimitOp::Check => "check",
                    RateLimitOp::Write => "write",
                },
            );
            return Err(AegisError::RateLimitExceeded(key.to_string()));
        }

        state.tokens -= 1.0;

        let sanitized: String = key
            .chars()
            .filter(|&c| c.is_alphanumeric() || c == ':' || c == '_' || c == '-')
            .take(128)
            .collect();
        tracing::debug!(
            "rate_limit.allowed key={} op={} tokens_remaining={}",
            sanitized,
            match op {
                RateLimitOp::Check => "check",
                RateLimitOp::Write => "write",
            },
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

    /// Remove buckets that haven't been accessed since the given duration.
    pub fn gc(&self, max_age: std::time::Duration) {
        if let Ok(mut buckets) = self.buckets.lock() {
            let cutoff = Instant::now() - max_age;
            buckets.retain(|_, state| state.last_accessed >= cutoff);
        }
    }

    /// Clear all rate limiter state (e.g., on schema reload).
    #[cfg(test)]
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

    #[test]
    fn test_rate_limiter_gc_removes_stale() {
        let config = RateLimitConfig::default();
        let limiter = TokenBucketRateLimiter::new(config);

        // Insert two keys
        limiter.check("key:a", RateLimitOp::Check).unwrap();
        limiter.check("key:b", RateLimitOp::Check).unwrap();

        // Sleep briefly so key:a becomes older
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Touch key:b again
        limiter.check("key:b", RateLimitOp::Check).unwrap();

        // GC with max_age = 5ms — key:a should be evicted but key:b should remain
        limiter.gc(std::time::Duration::from_millis(5));

        // Verify key:b still works
        assert!(limiter.check("key:b", RateLimitOp::Check).is_ok());
        // key:a was removed and can be created fresh
        assert!(limiter.check("key:a", RateLimitOp::Check).is_ok());
    }

    #[test]
    fn test_rate_limiter_max_keys_evicts_oldest() {
        let config = RateLimitConfig {
            max_keys: 3,
            ..Default::default()
        };
        let limiter = TokenBucketRateLimiter::new(config);

        // Insert 3 keys — all fit
        limiter.check("key:1", RateLimitOp::Check).unwrap();
        limiter.check("key:2", RateLimitOp::Check).unwrap();
        limiter.check("key:3", RateLimitOp::Check).unwrap();

        // Access key:1 again so it's most recently used
        limiter.check("key:1", RateLimitOp::Check).unwrap();

        // Insert a 4th key — should evict the least recently accessed (key:2)
        limiter.check("key:4", RateLimitOp::Check).unwrap();

        // key:2 should have been evicted; key:1 and key:3 and key:4 should remain
        assert!(limiter.check("key:1", RateLimitOp::Check).is_ok());
        assert!(limiter.check("key:3", RateLimitOp::Check).is_ok());
        assert!(limiter.check("key:4", RateLimitOp::Check).is_ok());
    }
}
