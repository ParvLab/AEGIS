use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::thread::JoinHandle;
use uuid::Uuid;

use crate::engine::GraphEngine;
use crate::error::{AegisError, AegisResult};
use crate::types::analysis::CheckQuery;

/// Configuration for the built-in scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// How often the scheduler thread wakes up to check for due schedules (seconds).
    pub tick_interval_seconds: u64,
    /// Maximum number of concurrent analysis runs.
    pub max_concurrent_runs: u32,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval_seconds: 10,
            max_concurrent_runs: 4,
        }
    }
}

/// JSON-deserializable configuration for creating an analysis schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisScheduleConfig {
    pub name: String,
    pub interval_seconds: u64,
    pub queries: Vec<crate::types::analysis::CheckQuery>,
    pub compare_schema: Option<crate::types::Schema>,
}

/// A scheduled analysis definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisSchedule {
    pub id: Uuid,
    pub name: String,
    /// Interval between runs in seconds.
    pub interval_seconds: u64,
    /// Queries to check during each run.
    pub queries: Vec<CheckQuery>,
    /// Optional schema to compare against the current active schema (access diff).
    pub compare_schema: Option<crate::types::Schema>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Status of an analysis run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnalysisRunStatus {
    Running,
    Completed,
    Failed,
}

/// A single execution of an analysis schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisRun {
    pub id: Uuid,
    pub schedule_id: Option<Uuid>,
    pub started_at: String,
    pub completed_at: String,
    pub status: AnalysisRunStatus,
    pub summary: serde_json::Value,
    pub error_message: Option<String>,
}

impl GraphEngine {
    /// Create a new analysis schedule and enable it.
    pub fn create_analysis_schedule(
        &self,
        name: &str,
        interval_seconds: u64,
        queries: Vec<CheckQuery>,
        compare_schema: Option<crate::types::Schema>,
    ) -> AegisResult<AnalysisSchedule> {
        let now = chrono::Utc::now().to_rfc3339();
        let schedule = AnalysisSchedule {
            id: Uuid::new_v4(),
            name: name.to_string(),
            interval_seconds,
            queries,
            compare_schema,
            enabled: true,
            created_at: now.clone(),
            updated_at: now,
        };
        let mut schedules = self
            .analysis_schedules
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let id = schedule.id;
        schedules.insert(id, schedule.clone());
        Ok(schedule)
    }

    /// List all analysis schedules.
    pub fn list_analysis_schedules(&self) -> AegisResult<Vec<AnalysisSchedule>> {
        let schedules = self
            .analysis_schedules
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let mut result: Vec<_> = schedules.values().cloned().collect();
        result.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(result)
    }

    /// Delete an analysis schedule by ID.
    pub fn delete_analysis_schedule(&self, id: Uuid) -> AegisResult<bool> {
        let mut schedules = self
            .analysis_schedules
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        Ok(schedules.remove(&id).is_some())
    }

    /// Run analysis immediately for a given schedule, or for all enabled schedules if None.
    pub fn run_analysis_now(&self, schedule_id: Option<Uuid>) -> AegisResult<Vec<AnalysisRun>> {
        let schedules: Vec<AnalysisSchedule> = {
            let s = self
                .analysis_schedules
                .lock()
                .map_err(|e| AegisError::Internal(e.to_string()))?;
            if let Some(id) = schedule_id {
                s.get(&id).cloned().into_iter().collect()
            } else {
                s.values().filter(|s| s.enabled).cloned().collect()
            }
        };

        let mut runs = Vec::new();
        for schedule in schedules {
            let run = self.execute_schedule(&schedule)?;
            runs.push(run);
        }
        Ok(runs)
    }

    /// Get recent analysis runs.
    pub fn get_analysis_runs(&self, limit: usize) -> AegisResult<Vec<AnalysisRun>> {
        let runs = self
            .analysis_runs
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let mut result: Vec<_> = runs.values().cloned().collect();
        result.sort_by(|a, b| b.completed_at.cmp(&a.completed_at));
        result.truncate(limit);
        Ok(result)
    }

    /// Start the background scheduler thread. Returns a handle that can be joined.
    pub fn start_scheduler(self: &Arc<Self>, config: SchedulerConfig) -> JoinHandle<()> {
        let engine = Arc::clone(self);
        std::thread::spawn(move || {
            let tick = std::time::Duration::from_secs(config.tick_interval_seconds);
            loop {
                std::thread::sleep(tick);
                if engine.closed.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let due_schedules = {
                    let schedules = match engine.analysis_schedules.lock() {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let now = chrono::Utc::now();
                    schedules
                        .values()
                        .filter(|s| s.enabled)
                        .filter_map(|s| {
                            let last_run = {
                                let runs = engine.analysis_runs.lock().ok()?;
                                runs.values()
                                    .filter(|r| r.schedule_id == Some(s.id))
                                    .max_by(|a, b| a.completed_at.cmp(&b.completed_at))
                                    .map(|r| {
                                        chrono::DateTime::parse_from_rfc3339(&r.completed_at)
                                            .map(|dt| dt.with_timezone(&chrono::Utc))
                                            .unwrap_or(
                                                now - std::time::Duration::from_secs(
                                                    s.interval_seconds * 2,
                                                ),
                                            )
                                    })
                                    .unwrap_or(
                                        now - std::time::Duration::from_secs(
                                            s.interval_seconds * 2,
                                        ),
                                    )
                            };
                            let elapsed = (now - last_run).num_seconds() as u64;
                            if elapsed >= s.interval_seconds {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                };
                for schedule in due_schedules {
                    if let Err(e) = engine.execute_schedule(&schedule) {
                        tracing::warn!("scheduled analysis failed for '{}': {}", schedule.name, e);
                    }
                }
            }
        })
    }

    fn execute_schedule(&self, schedule: &AnalysisSchedule) -> AegisResult<AnalysisRun> {
        let run_id = Uuid::new_v4();
        let started_at = chrono::Utc::now().to_rfc3339();

        // Record initial "running" state
        let run = AnalysisRun {
            id: run_id,
            schedule_id: Some(schedule.id),
            started_at: started_at.clone(),
            completed_at: started_at.clone(),
            status: AnalysisRunStatus::Running,
            summary: serde_json::Value::Null,
            error_message: None,
        };
        {
            let mut runs = self
                .analysis_runs
                .lock()
                .map_err(|e| AegisError::Internal(e.to_string()))?;
            runs.insert(run_id, run);
        }

        let result = self.compute_analysis(schedule);
        let completed_at = chrono::Utc::now().to_rfc3339();

        let (status, summary, error_message) = match result {
            Ok(summary) => (AnalysisRunStatus::Completed, summary, None),
            Err(e) => (
                AnalysisRunStatus::Failed,
                serde_json::Value::Null,
                Some(e.to_string()),
            ),
        };

        let completed_run = AnalysisRun {
            id: run_id,
            schedule_id: Some(schedule.id),
            started_at,
            completed_at: completed_at.clone(),
            status,
            summary: summary.clone(),
            error_message,
        };

        {
            let mut runs = self
                .analysis_runs
                .lock()
                .map_err(|e| AegisError::Internal(e.to_string()))?;
            runs.insert(run_id, completed_run.clone());
        }

        // Emit event
        use crate::engine::watch::WatchEventType;
        self.emit_watch_event_with_payload(
            WatchEventType::AnalysisCompleted,
            "",
            "",
            "",
            0.into(),
            Some(summary),
        );

        Ok(completed_run)
    }

    fn compute_analysis(&self, schedule: &AnalysisSchedule) -> AegisResult<serde_json::Value> {
        let mut output = serde_json::Map::new();

        // Run check queries
        let mut check_results = Vec::new();
        for q in &schedule.queries {
            let subject = crate::types::SubjectId::new(&q.subject)?;
            let resource = crate::types::ResourceId::new(&q.resource)?;
            let check = self.check(&subject, &q.permission, &resource, None)?;
            check_results.push(serde_json::json!({
                "subject": q.subject,
                "permission": q.permission,
                "resource": q.resource,
                "allowed": check.allowed,
            }));
        }
        output.insert(
            "checks".to_string(),
            serde_json::Value::Array(check_results),
        );

        // Run access diff if a compare schema is provided
        if let Some(ref compare_schema) = schedule.compare_schema {
            let current_schema = {
                let s = self
                    .schema
                    .read()
                    .map_err(|e| AegisError::Internal(e.to_string()))?;
                s.clone()
            };
            match self.access_diff(&current_schema, compare_schema, None, Some(1000)) {
                Ok(diff) => {
                    let diff_json = serde_json::to_value(&diff).unwrap_or(serde_json::Value::Null);
                    output.insert("access_diff".to_string(), diff_json);
                }
                Err(e) => {
                    output.insert(
                        "access_diff_error".to_string(),
                        serde_json::Value::String(e.to_string()),
                    );
                }
            }
        }

        output.insert(
            "schedule_id".to_string(),
            serde_json::Value::String(schedule.id.to_string()),
        );
        output.insert(
            "schedule_name".to_string(),
            serde_json::Value::String(schedule.name.clone()),
        );

        Ok(serde_json::Value::Object(output))
    }
}

#[cfg(all(test, feature = "sqlite"))]
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
    fn test_create_schedule() {
        let engine = make_engine();
        let schedule = engine
            .create_analysis_schedule(
                "hourly-review",
                3600,
                vec![CheckQuery {
                    subject: "user:alice".to_string(),
                    permission: "read".to_string(),
                    resource: "repo:fluxbus".to_string(),
                }],
                None,
            )
            .unwrap();
        assert_eq!(schedule.name, "hourly-review");
        assert_eq!(schedule.interval_seconds, 3600);
        assert!(schedule.enabled);
    }

    #[test]
    fn test_list_schedules() {
        let engine = make_engine();
        engine
            .create_analysis_schedule("a", 60, vec![], None)
            .unwrap();
        engine
            .create_analysis_schedule("b", 120, vec![], None)
            .unwrap();
        let list = engine.list_analysis_schedules().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_delete_schedule() {
        let engine = make_engine();
        let s = engine
            .create_analysis_schedule("test", 60, vec![], None)
            .unwrap();
        assert!(engine.delete_analysis_schedule(s.id).unwrap());
        assert!(!engine.delete_analysis_schedule(s.id).unwrap());
    }

    #[test]
    fn test_run_analysis_now() {
        let engine = make_engine();
        let schedule = engine
            .create_analysis_schedule(
                "test-run",
                60,
                vec![CheckQuery {
                    subject: "user:alice".to_string(),
                    permission: "read".to_string(),
                    resource: "repo:fluxbus".to_string(),
                }],
                None,
            )
            .unwrap();

        let runs = engine.run_analysis_now(Some(schedule.id)).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, AnalysisRunStatus::Completed);
        assert!(runs[0].summary.is_object());
    }

    #[test]
    fn test_get_analysis_runs() {
        let engine = make_engine();
        let schedule = engine
            .create_analysis_schedule("test-get", 60, vec![], None)
            .unwrap();
        engine.run_analysis_now(Some(schedule.id)).unwrap();
        let runs = engine.get_analysis_runs(10).unwrap();
        assert_eq!(runs.len(), 1);
    }
}
