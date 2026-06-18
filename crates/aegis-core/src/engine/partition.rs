//! Partition management for isolated authorization graphs.

use crate::engine::ratelimit::{RateLimitConfig, RateLimitOp, TokenBucketRateLimiter};
use crate::error::AegisResult;
use crate::types::PartitionId;
use std::collections::HashMap;
use std::sync::Mutex;

/// Manages per-partition state: rate limiters, caches, budgets.
pub struct PartitionManager {
    partitions: Mutex<HashMap<String, PartitionState>>,
    default_partition: PartitionState,
}

struct PartitionState {
    rate_limiter: TokenBucketRateLimiter,
}

impl Default for PartitionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PartitionManager {
    pub fn new() -> Self {
        Self {
            partitions: Mutex::new(HashMap::new()),
            default_partition: PartitionState {
                rate_limiter: TokenBucketRateLimiter::new(RateLimitConfig::default()),
            },
        }
    }

    pub fn get_or_create(&self, partition_id: &PartitionId) -> AegisResult<PartitionHandle> {
        let key = partition_id.to_string();
        let mut map = self
            .partitions
            .lock()
            .map_err(|_| crate::error::AegisError::Internal("partition lock poisoned".into()))?;
        if !map.contains_key(&key) {
            map.insert(
                key.clone(),
                PartitionState {
                    rate_limiter: TokenBucketRateLimiter::new(RateLimitConfig::default()),
                },
            );
        }
        Ok(PartitionHandle {
            partition_id: partition_id.clone(),
        })
    }

    pub fn check_rate_limit(&self, partition_id: &PartitionId) -> AegisResult<()> {
        let key = partition_id.to_string();
        if let Ok(map) = self.partitions.lock()
            && let Some(state) = map.get(&key)
        {
            return state.rate_limiter.check(&key, RateLimitOp::Check);
        }
        // If no partition-specific state, use default
        self.default_partition
            .rate_limiter
            .check(&key, RateLimitOp::Check)
    }
}

/// Handle to a specific partition for use during traversal and checks.
#[derive(Debug, Clone)]
pub struct PartitionHandle {
    pub partition_id: PartitionId,
}
