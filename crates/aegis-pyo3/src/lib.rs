use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aegis_core::engine::condition::ConditionEvalContext;
use aegis_core::engine::hooks::LogLevel;
use aegis_core::engine::ratelimit::{RateLimitConfig, TokenBucketRateLimiter};
use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::StorageBackend;
use aegis_core::types::*;
use aegis_core::types::PartitionId;

use chrono::{DateTime, Utc};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyModule;

fn py_err(msg: impl ToString) -> PyErr {
    PyRuntimeError::new_err(msg.to_string())
}

fn parse_consistency(s: Option<String>) -> PyResult<Option<ConsistencyMode>> {
    match s {
        None => Ok(None),
        Some(ref val) if val.eq_ignore_ascii_case("minimize_latency") => Ok(Some(ConsistencyMode::MinimizeLatency)),
        Some(ref val) if val.eq_ignore_ascii_case("fully_consistent") => Ok(Some(ConsistencyMode::FullyConsistent)),
        Some(ref val) => {
            if let Some(rev_str) = val.strip_prefix("at_revision:") {
                let rev_num: u64 = rev_str.parse()
                    .map_err(|_| py_err(format!("invalid consistency: {}", val)))?;
                Ok(Some(ConsistencyMode::AtRevision(Revision::from(rev_num))))
            } else {
                Err(py_err(format!("invalid consistency: {}", val)))
            }
        }
    }
}

// ── Result types ──

#[pyclass(name = "CheckResult")]
#[derive(Clone)]
struct PyCheckResult {
    #[pyo3(get)]
    allowed: bool,
    #[pyo3(get)]
    revision: i64,
}

#[pymethods]
impl PyCheckResult {
    fn __repr__(&self) -> String {
        format!("CheckResult(allowed={}, revision={})", self.allowed, self.revision)
    }
}

#[pyclass(name = "WriteResult")]
#[derive(Clone)]
struct PyWriteResult {
    #[pyo3(get)]
    revision: i64,
    #[pyo3(get)]
    node_id: String,
    #[pyo3(get)]
    timestamp: String,
}

#[pymethods]
impl PyWriteResult {
    fn __repr__(&self) -> String {
        format!("WriteResult(revision={})", self.revision)
    }
}

#[pyclass(name = "HealthReport")]
#[derive(Clone)]
struct PyHealthReport {
    #[pyo3(get)]
    healthy: bool,
    #[pyo3(get)]
    revision: i64,
    #[pyo3(get)]
    schema_version: i32,
    #[pyo3(get)]
    error: Option<String>,
    #[pyo3(get)]
    backend: String,
    #[pyo3(get)]
    backend_healthy: bool,
    #[pyo3(get)]
    cache_hit_rate: f64,
    #[pyo3(get)]
    cache_entries: i32,
    #[pyo3(get)]
    storage_integrity: bool,
    #[pyo3(get)]
    total_checks: f64,
    #[pyo3(get)]
    allowed_checks: f64,
    #[pyo3(get)]
    denied_checks: f64,
    #[pyo3(get)]
    error_checks: f64,
    #[pyo3(get)]
    uptime_ms: f64,
}

#[pymethods]
impl PyHealthReport {
    fn __repr__(&self) -> String {
        format!("HealthReport(healthy={}, revision={}, schema_version={})",
            self.healthy, self.revision, self.schema_version)
    }
}

#[pyclass(name = "ExplainTrace")]
#[derive(Clone)]
struct PyExplainTrace {
    #[pyo3(get)]
    subject: String,
    #[pyo3(get)]
    relation: String,
    #[pyo3(get)]
    object: String,
}

#[pymethods]
impl PyExplainTrace {
    fn __repr__(&self) -> String {
        format!("ExplainTrace({} {} {})", self.subject, self.relation, self.object)
    }
}

#[pyclass(name = "ExplainResult")]
#[derive(Clone)]
struct PyExplainResult {
    #[pyo3(get)]
    allowed: bool,
    #[pyo3(get)]
    revision: i64,
    #[pyo3(get)]
    trace: Vec<PyExplainTrace>,
    #[pyo3(get)]
    resolved_via: String,
    #[pyo3(get)]
    duration_ms: i64,
}

#[pymethods]
impl PyExplainResult {
    fn __repr__(&self) -> String {
        format!("ExplainResult(allowed={}, revision={}, resolved_via={})",
            self.allowed, self.revision, self.resolved_via)
    }
}

#[pyclass(name = "Tuple")]
#[derive(Clone)]
struct PyTuple {
    #[pyo3(get)]
    subject: String,
    #[pyo3(get)]
    relation: String,
    #[pyo3(get)]
    object: String,
}

#[pymethods]
impl PyTuple {
    fn __repr__(&self) -> String {
        format!("Tuple({} --{}--> {})", self.subject, self.relation, self.object)
    }
}

#[pyclass(name = "SchemaCheckReport")]
#[derive(Clone)]
struct PySchemaCheckReport {
    #[pyo3(get)]
    compatible: bool,
    #[pyo3(get)]
    warnings: Vec<String>,
    #[pyo3(get)]
    breaking: Vec<String>,
}

#[pymethods]
impl PySchemaCheckReport {
    fn __repr__(&self) -> String {
        format!("SchemaCheckReport(compatible={}, {} warnings, {} breaking)",
            self.compatible, self.warnings.len(), self.breaking.len())
    }
}

#[pyclass(name = "ExportResult")]
#[derive(Clone)]
struct PyExportResult {
    #[pyo3(get)]
    subject: String,
    #[pyo3(get)]
    active_tuples: Vec<PyTuple>,
    #[pyo3(get)]
    export_revision: i64,
    #[pyo3(get)]
    exported_at: String,
}

#[pymethods]
impl PyExportResult {
    fn __repr__(&self) -> String {
        format!("ExportResult(subject={}, {} tuples)", self.subject, self.active_tuples.len())
    }
}

#[pyclass(name = "AuditEntry")]
#[derive(Clone)]
struct PyAuditEntry {
    #[pyo3(get)]
    revision: i64,
    #[pyo3(get)]
    action: String,
    #[pyo3(get)]
    subject: String,
    #[pyo3(get)]
    relation: String,
    #[pyo3(get)]
    object: String,
    #[pyo3(get)]
    timestamp: String,
    #[pyo3(get)]
    identity: Option<String>,
}

#[pymethods]
impl PyAuditEntry {
    fn __repr__(&self) -> String {
        format!("AuditEntry(revision={}, action={})", self.revision, self.action)
    }
}

#[pyclass(name = "PaginatedTuples")]
#[derive(Clone)]
struct PyPaginatedTuples {
    #[pyo3(get)]
    tuples: Vec<PyTuple>,
    #[pyo3(get)]
    next_cursor: Option<f64>,
    #[pyo3(get)]
    revision: f64,
}

#[pymethods]
impl PyPaginatedTuples {
    fn __repr__(&self) -> String {
        format!("PaginatedTuples({} tuples, revision={})", self.tuples.len(), self.revision)
    }
}

// ── Helper conversions ──

fn tuple_to_py(t: &RelationshipTuple) -> PyTuple {
    PyTuple {
        subject: t.subject.as_str().to_string(),
        relation: t.relation.as_str().to_string(),
        object: t.object.as_str().to_string(),
    }
}

// ── Main engine class ──

#[pyclass(name = "Aegis")]
struct PyAegis {
    engine: Arc<GraphEngine>,
    closed: AtomicBool,
}

#[pymethods]
impl PyAegis {
    #[new]
    #[pyo3(signature = (path, schema_yaml, max_readers=None, busy_timeout_ms=None, wal_mode=None, mmap_size=None))]
    fn new(path: String, schema_yaml: String, max_readers: Option<u32>, busy_timeout_ms: Option<u32>, wal_mode: Option<bool>, mmap_size: Option<u64>) -> PyResult<Self> {
        let config = SqliteConfig {
            path,
            max_readers: max_readers.unwrap_or(4),
            busy_timeout_ms: busy_timeout_ms.unwrap_or(5000),
            wal_mode: wal_mode.unwrap_or(true),
            mmap_size: mmap_size.unwrap_or(0),
        };
        let mut storage = SqliteStorage::new(config).map_err(py_err)?;
        storage.initialize().map_err(py_err)?;
        let schema = parse_schema(&schema_yaml).map_err(py_err)?;
        let engine = GraphEngine::new(Box::new(storage), schema);
        Ok(PyAegis {
            engine: Arc::new(engine),
            closed: AtomicBool::new(false),
        })
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    fn initialize_result(&self) -> PyResult<PyHealthReport> {
        let report = self.engine.health();
        Ok(PyHealthReport {
            healthy: report.healthy,
            revision: report.revision.as_u64() as i64,
            schema_version: report.schema_version as i32,
            error: report.error.clone(),
            backend: report.backend.clone(),
            backend_healthy: report.backend_healthy,
            cache_hit_rate: report.cache_hit_rate,
            cache_entries: report.cache_entries as i32,
            storage_integrity: report.storage_integrity,
            total_checks: report.total_checks as f64,
            allowed_checks: report.allowed_checks as f64,
            denied_checks: report.denied_checks as f64,
            error_checks: report.error_checks as f64,
            uptime_ms: report.uptime_ms as f64,
        })
    }

    #[pyo3(signature = (subject, permission, resource, consistency=None))]
    fn check(&self, subject: &str, permission: &str, resource: &str, consistency: Option<String>) -> PyResult<PyCheckResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(subject).map_err(py_err)?;
        let resource_id = ResourceId::new(resource).map_err(py_err)?;
        let cm = parse_consistency(consistency)?;
        let result = self.engine.check(&subject_id, permission, &resource_id, cm)
            .map_err(py_err)?;
        Ok(PyCheckResult {
            allowed: result.allowed,
            revision: result.revision.as_u64() as i64,
        })
    }

    #[pyo3(signature = (subject, permission, resource, context, consistency=None))]
    fn check_with_context(&self, subject: &str, permission: &str, resource: &str, context: HashMap<String, HashMap<String, String>>, consistency: Option<String>) -> PyResult<PyCheckResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(subject).map_err(py_err)?;
        let resource_id = ResourceId::new(resource).map_err(py_err)?;
        let cm = parse_consistency(consistency)?;
        let ctx = ConditionEvalContext {
            subject_meta: context.get("subject_meta").cloned().unwrap_or_default(),
            resource_meta: context.get("resource_meta").cloned().unwrap_or_default(),
            env: context.get("env").cloned().unwrap_or_default(),
        };
        let result = self.engine.check_with_context(&subject_id, permission, &resource_id, cm, ctx)
            .map_err(py_err)?;
        Ok(PyCheckResult {
            allowed: result.allowed,
            revision: result.revision.as_u64() as i64,
        })
    }

    #[pyo3(signature = (subject, permission, resource, consistency=None))]
    fn check_dry_run(&self, subject: &str, permission: &str, resource: &str, consistency: Option<String>) -> PyResult<PyCheckResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(subject).map_err(py_err)?;
        let resource_id = ResourceId::new(resource).map_err(py_err)?;
        let cm = parse_consistency(consistency)?;
        let result = self.engine.check_dry_run(&subject_id, permission, &resource_id, cm)
            .map_err(py_err)?;
        Ok(PyCheckResult {
            allowed: result.allowed,
            revision: result.revision.as_u64() as i64,
        })
    }

    #[pyo3(signature = (subject, relation, resource, condition=None, metadata=None, valid_until=None))]
    fn write(&self, subject: &str, relation: &str, resource: &str, condition: Option<String>, metadata: Option<HashMap<String, String>>, valid_until: Option<String>) -> PyResult<PyWriteResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(subject).map_err(py_err)?;
        let relation_id = Relation::new(relation).map_err(py_err)?;
        let object_id = ResourceId::new(resource).map_err(py_err)?;
        let valid_until_dt = match valid_until {
            Some(ref s) => Some(
                DateTime::parse_from_rfc3339(s)
                    .map_err(|e| py_err(format!("invalid valid_until: {}", e)))?
                    .with_timezone(&Utc),
            ),
            None => None,
        };
        let tuple = RelationshipTuple {
            subject: subject_id,
            relation: relation_id,
            object: object_id,
            created_at: Utc::now(),
            metadata,
            valid_until: valid_until_dt,
            condition,
        };
        let result = self.engine.write(&tuple).map_err(py_err)?;
        Ok(PyWriteResult {
            revision: result.revision.as_u64() as i64,
            node_id: result.node_id.to_string(),
            timestamp: result.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        })
    }

    #[pyo3(signature = (subject, relation, resource, condition=None, metadata=None, valid_until=None))]
    fn write_dry_run(&self, subject: &str, relation: &str, resource: &str, condition: Option<String>, metadata: Option<HashMap<String, String>>, valid_until: Option<String>) -> PyResult<PyCheckResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(subject).map_err(py_err)?;
        let relation_id = Relation::new(relation).map_err(py_err)?;
        let object_id = ResourceId::new(resource).map_err(py_err)?;
        let valid_until_dt = match valid_until {
            Some(ref s) => Some(
                DateTime::parse_from_rfc3339(s)
                    .map_err(|e| py_err(format!("invalid valid_until: {}", e)))?
                    .with_timezone(&Utc),
            ),
            None => None,
        };
        let tuple = RelationshipTuple {
            subject: subject_id,
            relation: relation_id,
            object: object_id,
            created_at: Utc::now(),
            metadata,
            valid_until: valid_until_dt,
            condition,
        };
        let result = self.engine.write_dry_run(&tuple).map_err(py_err)?;
        Ok(PyCheckResult {
            allowed: false,
            revision: result.revision.as_u64() as i64,
        })
    }

    fn delete(&self, subject: &str, relation: &str, resource: &str) -> PyResult<PyWriteResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(subject).map_err(py_err)?;
        let relation_id = Relation::new(relation).map_err(py_err)?;
        let object_id = ResourceId::new(resource).map_err(py_err)?;
        let key = TupleKey {
            subject: subject_id,
            relation: relation_id,
            object: object_id,
        };
        let result = self.engine.delete(&key).map_err(py_err)?;
        Ok(PyWriteResult {
            revision: result.revision.as_u64() as i64,
            node_id: result.node_id.to_string(),
            timestamp: result.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        })
    }

    fn health(&self) -> PyResult<PyHealthReport> {
        let report = self.engine.health();
        Ok(PyHealthReport {
            healthy: report.healthy,
            revision: report.revision.as_u64() as i64,
            schema_version: report.schema_version as i32,
            error: report.error.clone(),
            backend: report.backend.clone(),
            backend_healthy: report.backend_healthy,
            cache_hit_rate: report.cache_hit_rate,
            cache_entries: report.cache_entries as i32,
            storage_integrity: report.storage_integrity,
            total_checks: report.total_checks as f64,
            allowed_checks: report.allowed_checks as f64,
            denied_checks: report.denied_checks as f64,
            error_checks: report.error_checks as f64,
            uptime_ms: report.uptime_ms as f64,
        })
    }

    #[pyo3(signature = (subject, permission, resource, consistency=None))]
    fn explain(&self, subject: &str, permission: &str, resource: &str, consistency: Option<String>) -> PyResult<PyExplainResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(subject).map_err(py_err)?;
        let resource_id = ResourceId::new(resource).map_err(py_err)?;
        let cm = parse_consistency(consistency)?;
        let result = self.engine.explain(&subject_id, permission, &resource_id, cm)
            .map_err(py_err)?;
        Ok(PyExplainResult {
            allowed: result.allowed,
            revision: result.revision.as_u64() as i64,
            trace: result.trace.iter().map(|t| PyExplainTrace {
                subject: t.subject.clone(),
                relation: t.relation.clone(),
                object: t.object.clone(),
            }).collect(),
            resolved_via: result.resolved_via,
            duration_ms: result.duration_ms as i64,
        })
    }

    #[pyo3(signature = (object, relation=None, consistency=None))]
    fn list_by_object(&self, object: &str, relation: Option<String>, consistency: Option<String>) -> PyResult<Vec<PyTuple>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let object_id = ResourceId::new(object).map_err(py_err)?;
        let rel = relation.as_deref().map(Relation::new).transpose().map_err(py_err)?;
        let cm = parse_consistency(consistency)?;
        let tuples = self.engine.list_by_object(&object_id, rel.as_ref(), cm)
            .map_err(py_err)?;
        Ok(tuples.iter().map(tuple_to_py).collect())
    }

    #[pyo3(signature = (subject, relation=None, consistency=None))]
    fn list_by_subject(&self, subject: &str, relation: Option<String>, consistency: Option<String>) -> PyResult<Vec<PyTuple>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(subject).map_err(py_err)?;
        let rel = relation.as_deref().map(Relation::new).transpose().map_err(py_err)?;
        let cm = parse_consistency(consistency)?;
        let tuples = self.engine.list_by_subject(&subject_id, rel.as_ref(), cm)
            .map_err(py_err)?;
        Ok(tuples.iter().map(tuple_to_py).collect())
    }

    fn list_by_relation(&self, object: &str, relation: &str) -> PyResult<Vec<PyTuple>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let object_id = ResourceId::new(object).map_err(py_err)?;
        let relation_id = Relation::new(relation).map_err(py_err)?;
        let tuples = self.engine.list_by_relation(&object_id, &relation_id)
            .map_err(py_err)?;
        Ok(tuples.iter().map(tuple_to_py).collect())
    }

    #[pyo3(signature = (permission, resource, page_offset=None, page_limit=None, include_paths=None))]
    fn who_can_access(&self, permission: &str, resource: &str, page_offset: Option<u64>, page_limit: Option<u64>, include_paths: Option<bool>) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let resource_id = ResourceId::new(resource).map_err(py_err)?;
        let cursor = page_offset.map(|offset| PaginationCursor { offset, revision: Revision::from(0) });
        let pagination = PaginationParams { limit: page_limit.unwrap_or(100), cursor };
        let include = include_paths.unwrap_or(false);
        let result = self.engine.who_can_access(permission, &resource_id, &pagination, include, 10, 5000)
            .map_err(py_err)?;
        serde_json::to_string(&result).map_err(py_err)
    }

    #[pyo3(signature = (tuples))]
    fn write_batch(&self, tuples: Vec<PyTuple>) -> PyResult<PyWriteResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let mut rel_tuples = Vec::with_capacity(tuples.len());
        for t in tuples {
            let subject_id = SubjectId::new(&t.subject).map_err(py_err)?;
            let relation_id = Relation::new(&t.relation).map_err(py_err)?;
            let object_id = ResourceId::new(&t.object).map_err(py_err)?;
            rel_tuples.push(RelationshipTuple::new(subject_id, relation_id, object_id));
        }
        let result = self.engine.write_batch(&rel_tuples).map_err(py_err)?;
        Ok(PyWriteResult {
            revision: result.revision.as_u64() as i64,
            node_id: result.node_id.to_string(),
            timestamp: result.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        })
    }

    fn migrate(&self, target_version: i32) -> PyResult<()> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        self.engine.migrate(target_version as u32).map_err(py_err)?;
        Ok(())
    }

    fn check_schema(&self, schema_yaml: String) -> PyResult<PySchemaCheckReport> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let new_schema = parse_schema(&schema_yaml).map_err(py_err)?;
        let report = self.engine.check_schema(&new_schema);
        Ok(PySchemaCheckReport {
            compatible: report.compatible,
            warnings: report.warnings,
            breaking: report.breaking,
        })
    }

    fn delete_object(&self, object: String) -> PyResult<PyWriteResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let object_id = ResourceId::new(&object).map_err(py_err)?;
        let result = self.engine.delete_object(&object_id).map_err(py_err)?;
        Ok(PyWriteResult {
            revision: result.revision.as_u64() as i64,
            node_id: result.node_id.to_string(),
            timestamp: result.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        })
    }

    fn export_subject(&self, subject: String) -> PyResult<PyExportResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(&subject).map_err(py_err)?;
        let tuples = self.engine.export_subject(&subject_id).map_err(py_err)?;
        let revision = self.engine.storage().current_revision(&PartitionId::default()).map_err(py_err)?;
        Ok(PyExportResult {
            subject: subject.clone(),
            active_tuples: tuples.iter().map(tuple_to_py).collect(),
            export_revision: revision.as_u64() as i64,
            exported_at: Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        })
    }

    #[pyo3(signature = (subject, policy, transfer_to_subject=None))]
    fn delete_subject_with_policy(&self, subject: String, policy: String, transfer_to_subject: Option<String>) -> PyResult<PyWriteResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(&subject).map_err(py_err)?;
        let transfer = match transfer_to_subject {
            Some(s) => Some(SubjectId::new(&s).map_err(py_err)?),
            None => None,
        };
        let result = self.engine.delete_subject_with_policy(&subject_id, &policy, transfer.as_ref())
            .map_err(py_err)?;
        Ok(PyWriteResult {
            revision: result.revision.as_u64() as i64,
            node_id: result.node_id.to_string(),
            timestamp: result.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        })
    }

    #[pyo3(signature = (object, from_revision=None, to_revision=None, limit=100.0))]
    fn query_audit(&self, object: String, from_revision: Option<i64>, to_revision: Option<i64>, limit: f64) -> PyResult<Vec<PyAuditEntry>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let object_id = ResourceId::new(&object).map_err(py_err)?;
        let from = from_revision.map(|r| Revision::from(r as u64));
        let to = to_revision.map(|r| Revision::from(r as u64));
        let pp = PaginationParams { limit: limit as u64, cursor: None };
        let entries = self.engine.query_audit(&object_id, from, to, &pp).map_err(py_err)?;
        Ok(entries.iter().map(|e| PyAuditEntry {
            revision: e.revision.as_u64() as i64,
            action: format!("{:?}", e.action).to_lowercase(),
            subject: e.subject.clone(),
            relation: e.relation.clone(),
            object: e.object.clone(),
            timestamp: e.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            identity: e.identity.clone(),
        }).collect())
    }

    #[pyo3(signature = (from_revision=None, to_revision=None, limit=100.0))]
    fn query_audit_all(&self, from_revision: Option<i64>, to_revision: Option<i64>, limit: f64) -> PyResult<Vec<PyAuditEntry>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let from = from_revision.map(|r| Revision::from(r as u64));
        let to = to_revision.map(|r| Revision::from(r as u64));
        let pp = PaginationParams { limit: limit as u64, cursor: None };
        let entries = self.engine.query_audit_all(from, to, &pp).map_err(py_err)?;
        Ok(entries.iter().map(|e| PyAuditEntry {
            revision: e.revision.as_u64() as i64,
            action: format!("{:?}", e.action).to_lowercase(),
            subject: e.subject.clone(),
            relation: e.relation.clone(),
            object: e.object.clone(),
            timestamp: e.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            identity: e.identity.clone(),
        }).collect())
    }

    fn reload_schema(&self, schema_yaml: String) -> PyResult<()> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let new_schema = parse_schema(&schema_yaml).map_err(py_err)?;
        self.engine.reload_schema(new_schema).map_err(py_err)?;
        Ok(())
    }

    fn invalidate_cache(&self) -> PyResult<()> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        self.engine.invalidate_cache();
        Ok(())
    }

    #[pyo3(signature = (checks_per_second=None, check_burst=None, writes_per_second=None, write_burst=None, max_traversal_depth=None, max_traversal_visits=None, max_keys=None))]
    fn set_rate_limiter(&self, checks_per_second: Option<u32>, check_burst: Option<u32>, writes_per_second: Option<u32>, write_burst: Option<u32>, max_traversal_depth: Option<usize>, max_traversal_visits: Option<usize>, max_keys: Option<usize>) -> PyResult<()> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let mut cfg = RateLimitConfig::default();
        if let Some(v) = checks_per_second { cfg.checks_per_second = v; }
        if let Some(v) = check_burst { cfg.check_burst = v; }
        if let Some(v) = writes_per_second { cfg.writes_per_second = v; }
        if let Some(v) = write_burst { cfg.write_burst = v; }
        if let Some(v) = max_traversal_depth { cfg.max_traversal_depth = v; }
        if let Some(v) = max_traversal_visits { cfg.max_traversal_visits = v; }
        if let Some(v) = max_keys { cfg.max_keys = v; }
        self.engine.set_rate_limiter(TokenBucketRateLimiter::new(cfg));
        Ok(())
    }

    #[pyo3(signature = (actor=None))]
    fn set_actor(&self, actor: Option<String>) {
        self.engine.set_actor(actor.as_deref());
    }

    fn active_actor(&self) -> Option<String> {
        self.engine.active_actor()
    }

    fn set_logger(&self, callback: PyObject) -> PyResult<()> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        self.engine.set_logger(move |level: LogLevel, target: &str, msg: &str| {
            let level_i32 = match level {
                LogLevel::Error => 0,
                LogLevel::Warn => 1,
                LogLevel::Info => 2,
                LogLevel::Debug => 3,
                LogLevel::Trace => 4,
            };
            Python::with_gil(|py| {
                let _ = callback.call1(py, (level_i32, target, msg));
            });
        });
        Ok(())
    }

    #[pyo3(signature = (subject, permission, resource, consistency=None))]
    fn explain_v2(&self, subject: &str, permission: &str, resource: &str, consistency: Option<String>) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(subject).map_err(py_err)?;
        let resource_id = ResourceId::new(resource).map_err(py_err)?;
        let cm = parse_consistency(consistency)?;
        let result = self.engine.explain_v2(&subject_id, permission, &resource_id, cm)
            .map_err(py_err)?;
        serde_json::to_string(&result).map_err(py_err)
    }

    fn list_policy_versions(&self) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let result = self.engine.list_policy_versions().map_err(py_err)?;
        serde_json::to_string(&result).map_err(py_err)
    }

    fn rollback_policy(&self, version: u32) -> PyResult<()> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        self.engine.rollback_policy(version).map_err(py_err)?;
        Ok(())
    }

    #[pyo3(signature = (schema_before_json, schema_after_json, max_checks=None))]
    fn access_diff(&self, schema_before_json: &str, schema_after_json: &str, max_checks: Option<u64>) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let schema_before: Schema = serde_json::from_str(schema_before_json).map_err(py_err)?;
        let schema_after: Schema = serde_json::from_str(schema_after_json).map_err(py_err)?;
        let result = self.engine.access_diff(&schema_before, &schema_after, None, max_checks)
            .map_err(py_err)?;
        serde_json::to_string(&result).map_err(py_err)
    }

    // ── V7 Policy Lifecycle ──

    fn create_policy_draft(&self, name: &str, description: &str) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let draft = self.engine.create_policy_draft(name, description).map_err(py_err)?;
        serde_json::to_string(&draft).map_err(py_err)
    }

    fn update_policy_draft(&self, id: &str, schema_json: &str) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let uid = uuid::Uuid::parse_str(id).map_err(py_err)?;
        let schema: Schema = serde_json::from_str(schema_json).map_err(py_err)?;
        let draft = self.engine.update_policy_draft(uid, schema).map_err(py_err)?;
        serde_json::to_string(&draft).map_err(py_err)
    }

    fn validate_policy_draft(&self, id: &str) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let uid = uuid::Uuid::parse_str(id).map_err(py_err)?;
        let report = self.engine.validate_policy_draft(uid).map_err(py_err)?;
        serde_json::to_string(&report).map_err(py_err)
    }

    fn submit_policy_draft_for_review(&self, id: &str) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let uid = uuid::Uuid::parse_str(id).map_err(py_err)?;
        let draft = self.engine.submit_policy_draft_for_review(uid).map_err(py_err)?;
        serde_json::to_string(&draft).map_err(py_err)
    }

    fn approve_policy_draft(&self, id: &str) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let uid = uuid::Uuid::parse_str(id).map_err(py_err)?;
        let draft = self.engine.approve_policy_draft(uid).map_err(py_err)?;
        serde_json::to_string(&draft).map_err(py_err)
    }

    fn reject_policy_draft(&self, id: &str, rejection_reason: &str) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let uid = uuid::Uuid::parse_str(id).map_err(py_err)?;
        let draft = self.engine.reject_policy_draft(uid, rejection_reason).map_err(py_err)?;
        serde_json::to_string(&draft).map_err(py_err)
    }

    fn publish_policy_draft(&self, id: &str) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let uid = uuid::Uuid::parse_str(id).map_err(py_err)?;
        let result = self.engine.publish_policy_draft(uid).map_err(py_err)?;
        serde_json::to_string(&result).map_err(py_err)
    }

    fn archive_policy_draft(&self, id: &str) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let uid = uuid::Uuid::parse_str(id).map_err(py_err)?;
        let draft = self.engine.archive_policy_draft(uid).map_err(py_err)?;
        serde_json::to_string(&draft).map_err(py_err)
    }

    #[pyo3(signature = (filter_status=None))]
    fn list_policy_drafts(&self, filter_status: Option<&str>) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let status = filter_status
            .map(|s| match s.to_lowercase().as_str() {
                "drafting" => Ok(aegis_core::engine::policy_lifecycle::DraftStatus::Drafting),
                "underreview" => Ok(aegis_core::engine::policy_lifecycle::DraftStatus::UnderReview),
                "approved" => Ok(aegis_core::engine::policy_lifecycle::DraftStatus::Approved),
                "rejected" => Ok(aegis_core::engine::policy_lifecycle::DraftStatus::Rejected),
                "published" => Ok(aegis_core::engine::policy_lifecycle::DraftStatus::Published),
                "archived" => Ok(aegis_core::engine::policy_lifecycle::DraftStatus::Archived),
                _ => Err(py_err(format!("unknown status: {}", s))),
            })
            .transpose()?;
        let drafts = self.engine.list_policy_drafts(status).map_err(py_err)?;
        serde_json::to_string(&drafts).map_err(py_err)
    }

    // ── V7 Scheduler ──

    #[pyo3(signature = (name, interval_seconds, queries_json, compare_schema_json=None))]
    fn create_analysis_schedule(&self, name: &str, interval_seconds: f64, queries_json: &str, compare_schema_json: Option<&str>) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let queries: Vec<aegis_core::types::analysis::CheckQuery> = serde_json::from_str(queries_json).map_err(py_err)?;
        let compare_schema = compare_schema_json
            .map(|s| serde_json::from_str(s).map_err(py_err))
            .transpose()?;
        let schedule = self.engine.create_analysis_schedule(name, interval_seconds as u64, queries, compare_schema)
            .map_err(py_err)?;
        serde_json::to_string(&schedule).map_err(py_err)
    }

    fn list_analysis_schedules(&self) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let schedules = self.engine.list_analysis_schedules().map_err(py_err)?;
        serde_json::to_string(&schedules).map_err(py_err)
    }

    fn delete_analysis_schedule(&self, id: &str) -> PyResult<bool> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let uid = uuid::Uuid::parse_str(id).map_err(py_err)?;
        self.engine.delete_analysis_schedule(uid).map_err(py_err)
    }

    #[pyo3(signature = (schedule_id=None))]
    fn run_analysis_now(&self, schedule_id: Option<&str>) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let uid = schedule_id
            .map(|id| uuid::Uuid::parse_str(id).map_err(py_err))
            .transpose()?;
        let runs = self.engine.run_analysis_now(uid).map_err(py_err)?;
        serde_json::to_string(&runs).map_err(py_err)
    }

    #[pyo3(signature = (limit=100.0))]
    fn get_analysis_runs(&self, limit: f64) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let runs = self.engine.get_analysis_runs(limit as usize).map_err(py_err)?;
        serde_json::to_string(&runs).map_err(py_err)
    }

    // ── V7 Enforcement History ──

    fn set_enforcement_history_config(&self, config_json: &str) -> PyResult<()> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let config: aegis_core::engine::enforcement_history::EnforcementHistoryConfig =
            serde_json::from_str(config_json).map_err(py_err)?;
        self.engine.set_enforcement_history_config(config).map_err(py_err)
    }

    fn get_enforcement_history_config(&self) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let config = self.engine.get_enforcement_history_config().map_err(py_err)?;
        serde_json::to_string(&config).map_err(py_err)
    }

    #[pyo3(signature = (limit=100.0))]
    fn enforcement_trends(&self, limit: f64) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let trends = self.engine.enforcement_trends(limit as usize).map_err(py_err)?;
        serde_json::to_string(&trends).map_err(py_err)
    }

    // ── V7 Subscribe ──

    fn subscribe(&self, event_types: Vec<String>) -> PyResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        use aegis_core::engine::watch::WatchEventType;
        let types: Vec<WatchEventType> = event_types
            .iter()
            .map(|s| match s.to_lowercase().as_str() {
                "tupleadded" => Ok(WatchEventType::TupleAdded),
                "tupleremoved" => Ok(WatchEventType::TupleRemoved),
                "policyversioncreated" => Ok(WatchEventType::PolicyVersionCreated),
                "policyrolledback" => Ok(WatchEventType::PolicyRolledBack),
                "integrityfinding" => Ok(WatchEventType::IntegrityFinding),
                "analysiscompleted" => Ok(WatchEventType::AnalysisCompleted),
                "ratelimitwarning" => Ok(WatchEventType::RateLimitWarning),
                _ => Err(py_err(format!("unknown event type: {}", s))),
            })
            .collect::<PyResult<Vec<_>>>()?;
        let sub = self.engine.subscribe(types);
        // Return subscription ID as JSON so user can poll/unsubscribe via other means
        Ok(serde_json::json!({"subscription_id": sub.id().to_string()}).to_string())
    }

    fn close(&self) -> PyResult<()> {
        if self.closed.swap(true, Ordering::Relaxed) {
            return Ok(());
        }
        self.engine.close().map_err(py_err)
    }

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(&self, _exc_type: Option<PyObject>, _exc_val: Option<PyObject>, _exc_tb: Option<PyObject>) -> PyResult<()> {
        let _ = self.close();
        Ok(())
    }
}

// ── Module registration ──

#[pymodule]
fn aegis(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyAegis>()?;
    m.add_class::<PyCheckResult>()?;
    m.add_class::<PyWriteResult>()?;
    m.add_class::<PyHealthReport>()?;
    m.add_class::<PyExplainTrace>()?;
    m.add_class::<PyExplainResult>()?;
    m.add_class::<PyTuple>()?;
    m.add_class::<PySchemaCheckReport>()?;
    m.add_class::<PyExportResult>()?;
    m.add_class::<PyAuditEntry>()?;
    m.add_class::<PyPaginatedTuples>()?;
    Ok(())
}
