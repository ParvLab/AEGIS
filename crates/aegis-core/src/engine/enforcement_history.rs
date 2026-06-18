use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use uuid::Uuid;

use crate::engine::GraphEngine;
use crate::error::{AegisError, AegisResult};

/// Sampling mode for enforcement history recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SamplingMode {
    /// Record all check results (high volume).
    All,
    /// Record only denied check results.
    DeniedOnly,
}

/// Configuration for enforcement history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforcementHistoryConfig {
    /// Whether enforcement history is enabled. Default: false (opt-in).
    pub enabled: bool,
    /// Sampling mode. Default: DeniedOnly.
    pub sampling: SamplingMode,
    /// Maximum events per minute across all subjects. 0 = unlimited.
    pub max_events_per_minute: u64,
    /// Maximum number of stored events. 0 = unlimited.
    pub max_rows: u64,
    /// Maximum age of stored events in days. 0 = unlimited.
    pub max_days: u64,
}

impl Default for EnforcementHistoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sampling: SamplingMode::DeniedOnly,
            max_events_per_minute: 10_000,
            max_rows: 100_000,
            max_days: 7,
        }
    }
}

/// A single enforcement event recorded during a check().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforcementEvent {
    pub id: Uuid,
    pub subject: String,
    pub permission: String,
    pub resource: String,
    pub allowed: bool,
    pub revision: u64,
    pub timestamp: String,
}

/// Trend summary for enforcement history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforcementTrends {
    pub total_events: u64,
    pub denied_count: u64,
    pub allowed_count: u64,
    pub by_resource: Vec<(String, u64)>,
    pub recent_events: Vec<EnforcementEvent>,
}

/// Sliding-window rate tracker for enforcement events.
pub(crate) struct RateTracker {
    window_duration_secs: u64,
    timestamps: VecDeque<std::time::Instant>,
    max_per_window: u64,
}

impl RateTracker {
    pub(crate) fn new(max_per_minute: u64) -> Self {
        Self {
            window_duration_secs: 60,
            timestamps: VecDeque::new(),
            max_per_window: max_per_minute,
        }
    }

    fn check_and_record(&mut self) -> bool {
        if self.max_per_window == 0 {
            return true;
        }
        let now = std::time::Instant::now();
        let cutoff = now - std::time::Duration::from_secs(self.window_duration_secs);
        while let Some(&t) = self.timestamps.front() {
            if t < cutoff {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
        if self.timestamps.len() < self.max_per_window as usize {
            self.timestamps.push_back(now);
            true
        } else {
            false
        }
    }

    fn update_max(&mut self, max_per_minute: u64) {
        self.max_per_window = max_per_minute;
    }
}

impl GraphEngine {
    /// Configure enforcement history recording.
    pub fn set_enforcement_history_config(
        &self,
        config: EnforcementHistoryConfig,
    ) -> AegisResult<()> {
        {
            let mut cfg = self
                .enforcement_config
                .lock()
                .map_err(|e| AegisError::Internal(e.to_string()))?;
            *cfg = config.clone();
        }
        {
            let mut rt = self
                .enforcement_rate_tracker
                .lock()
                .map_err(|e| AegisError::Internal(e.to_string()))?;
            rt.update_max(config.max_events_per_minute);
        }
        Ok(())
    }

    /// Get the current enforcement history configuration.
    pub fn get_enforcement_history_config(&self) -> AegisResult<EnforcementHistoryConfig> {
        let cfg = self
            .enforcement_config
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        Ok(cfg.clone())
    }

    /// Query enforcement trends.
    pub fn enforcement_trends(&self, limit: usize) -> AegisResult<EnforcementTrends> {
        let mut events = {
            let e = self
                .enforcement_events
                .lock()
                .map_err(|e| AegisError::Internal(e.to_string()))?;
            e.iter().rev().take(limit).cloned().collect::<Vec<_>>()
        };
        events.reverse();

        let total_events = events.len() as u64;
        let denied_count = events.iter().filter(|e| !e.allowed).count() as u64;
        let allowed_count = events.iter().filter(|e| e.allowed).count() as u64;

        let mut resource_counts: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        for e in &events {
            *resource_counts.entry(e.resource.clone()).or_default() += 1;
        }
        let mut by_resource: Vec<(String, u64)> = resource_counts.into_iter().collect();
        by_resource.sort_by(|a, b| b.1.cmp(&a.1));

        Ok(EnforcementTrends {
            total_events,
            denied_count,
            allowed_count,
            by_resource,
            recent_events: events,
        })
    }

    /// Internal: record an enforcement event if sampling and rate limits allow.
    pub(crate) fn record_enforcement_event(
        &self,
        subject: &str,
        permission: &str,
        resource: &str,
        allowed: bool,
        revision: u64,
    ) {
        let cfg = match self.enforcement_config.lock() {
            Ok(c) => c.clone(),
            Err(_) => return,
        };

        if !cfg.enabled {
            return;
        }

        // Sampling: skip allowed events unless mode is All
        if allowed && cfg.sampling == SamplingMode::DeniedOnly {
            return;
        }

        // Rate limit check
        let allow = match self.enforcement_rate_tracker.lock() {
            Ok(mut rt) => rt.check_and_record(),
            Err(_) => return,
        };

        if !allow {
            // Emit rate limit warning event
            #[cfg(not(target_arch = "wasm32"))]
            {
                let payload = serde_json::json!({
                    "event_type": "enforcement_history",
                    "current_rate_per_minute": cfg.max_events_per_minute,
                    "max_rate_per_minute": cfg.max_events_per_minute,
                });
                use crate::engine::watch::WatchEventType;
                self.emit_watch_event_with_payload(
                    WatchEventType::RateLimitWarning,
                    "",
                    "",
                    "",
                    0.into(),
                    Some(payload),
                );
            }
            return;
        }

        let event = EnforcementEvent {
            id: Uuid::new_v4(),
            subject: subject.to_string(),
            permission: permission.to_string(),
            resource: resource.to_string(),
            allowed,
            revision,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        if let Ok(mut events) = self.enforcement_events.lock() {
            // Enforce max_rows
            if cfg.max_rows > 0 && events.len() as u64 >= cfg.max_rows {
                events.pop_front();
            }
            let _ = self.storage.save_enforcement_event(&event);
            events.push_back(event);
        }

        // Periodically purge expired events (every ~1000 records)
        if cfg.max_days > 0
            && let Ok(mut events) = self.enforcement_events.lock()
            && events.len() % 1000 == 0
        {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(cfg.max_days as i64);
            let cutoff_str = cutoff.to_rfc3339();
            while let Some(front) = events.front() {
                if front.timestamp < cutoff_str {
                    events.pop_front();
                } else {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::GraphEngine;
    use crate::storage::StorageBackend;
    #[cfg(feature = "sqlite")]
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::types::*;
    use std::sync::Arc;

    fn make_engine() -> Arc<GraphEngine> {
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types: {
                let mut types = std::collections::HashMap::new();
                let mut relations = std::collections::HashMap::new();
                relations.insert(
                    "owner".to_string(),
                    crate::types::schema::RelationDef {
                        inherit_from: vec![],
                        description: None,
                    },
                );
                relations.insert(
                    "viewer".to_string(),
                    crate::types::schema::RelationDef {
                        inherit_from: vec![],
                        description: None,
                    },
                );
                let mut permissions = std::collections::HashMap::new();
                permissions.insert(
                    "read".to_string(),
                    crate::types::schema::PermissionDef {
                        union_of: vec!["viewer".to_string(), "owner".to_string()],
                        condition: None,
                        description: None,
                        ..Default::default()
                    },
                );
                types.insert(
                    "repo".to_string(),
                    crate::types::schema::TypeDef {
                        relations,
                        permissions,
                        ..Default::default()
                    },
                );
                types
            },
        };

        let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
        storage.initialize().unwrap();
        Arc::new(GraphEngine::new(Box::new(storage), schema))
    }

    #[test]
    fn test_default_config_is_disabled() {
        let engine = make_engine();
        let cfg = engine.get_enforcement_history_config().unwrap();
        assert!(!cfg.enabled);
        assert_eq!(cfg.sampling, SamplingMode::DeniedOnly);
    }

    #[test]
    fn test_enable_and_record() {
        let engine = make_engine();
        engine
            .set_enforcement_history_config(EnforcementHistoryConfig {
                enabled: true,
                sampling: SamplingMode::All,
                ..Default::default()
            })
            .unwrap();

        // Record some events directly (not through check() since we'd need tuples)
        engine.record_enforcement_event("user:alice", "read", "repo:fluxbus", true, 1);
        engine.record_enforcement_event("user:bob", "write", "repo:fluxbus", false, 2);

        let trends = engine.enforcement_trends(10).unwrap();
        assert_eq!(trends.total_events, 2);
        assert_eq!(trends.allowed_count, 1);
        assert_eq!(trends.denied_count, 1);
    }

    #[test]
    fn test_denied_only_sampling() {
        let engine = make_engine();
        engine
            .set_enforcement_history_config(EnforcementHistoryConfig {
                enabled: true,
                sampling: SamplingMode::DeniedOnly,
                ..Default::default()
            })
            .unwrap();

        engine.record_enforcement_event("user:alice", "read", "repo:fluxbus", true, 1);
        engine.record_enforcement_event("user:bob", "write", "repo:fluxbus", false, 2);

        let trends = engine.enforcement_trends(10).unwrap();
        assert_eq!(trends.total_events, 1);
        assert_eq!(trends.denied_count, 1);
    }

    #[test]
    fn test_disabled_records_nothing() {
        let engine = make_engine();
        engine.record_enforcement_event("user:alice", "read", "repo:fluxbus", false, 1);
        let trends = engine.enforcement_trends(10).unwrap();
        assert_eq!(trends.total_events, 0);
    }

    #[test]
    fn test_max_rows_eviction() {
        let engine = make_engine();
        engine
            .set_enforcement_history_config(EnforcementHistoryConfig {
                enabled: true,
                sampling: SamplingMode::All,
                max_rows: 3,
                ..Default::default()
            })
            .unwrap();

        engine.record_enforcement_event("a", "r", "x", true, 1);
        engine.record_enforcement_event("b", "r", "x", true, 2);
        engine.record_enforcement_event("c", "r", "x", true, 3);
        engine.record_enforcement_event("d", "r", "x", true, 4);

        let trends = engine.enforcement_trends(10).unwrap();
        assert_eq!(trends.total_events, 3);
        // Oldest ("a") should be evicted
        assert!(trends.recent_events.iter().any(|e| e.subject == "d"));
        assert!(!trends.recent_events.iter().any(|e| e.subject == "a"));
    }
}
