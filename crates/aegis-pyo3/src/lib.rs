use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::StorageBackend;
use aegis_core::types::*;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyModule;

// ── Helper ──

fn py_err(msg: impl ToString) -> PyErr {
    PyRuntimeError::new_err(msg.to_string())
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

// ── Main engine class ──

#[pyclass(name = "Aegis")]
struct PyAegis {
    engine: Arc<GraphEngine>,
    closed: AtomicBool,
}

#[pymethods]
impl PyAegis {
    #[new]
    fn new(path: String, schema_yaml: String) -> PyResult<Self> {
        let config = SqliteConfig {
            path,
            max_readers: 4,
            busy_timeout_ms: 5000,
            wal_mode: true,
            mmap_size: 0,
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

    fn check(&self, subject: &str, permission: &str, resource: &str) -> PyResult<PyCheckResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(subject).map_err(py_err)?;
        let resource_id = ResourceId::new(resource).map_err(py_err)?;
        let result = self.engine.check(&subject_id, permission, &resource_id, None)
            .map_err(py_err)?;
        Ok(PyCheckResult {
            allowed: result.allowed,
            revision: result.revision.as_u64() as i64,
        })
    }

    fn write(&self, subject: &str, relation: &str, resource: &str) -> PyResult<PyWriteResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let tuple = validate_tuple(subject, relation, resource)?;
        let result = self.engine.write(&tuple).map_err(py_err)?;
        Ok(PyWriteResult {
            revision: result.revision.as_u64() as i64,
            node_id: result.node_id.to_string(),
            timestamp: result.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        })
    }

    fn delete(&self, subject: &str, relation: &str, resource: &str) -> PyResult<PyWriteResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let tuple = validate_tuple(subject, relation, resource)?;
        let key = TupleKey {
            subject: tuple.subject,
            relation: tuple.relation,
            object: tuple.object,
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
        })
    }

    fn explain(&self, subject: &str, permission: &str, resource: &str) -> PyResult<PyExplainResult> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(py_err("engine is closed"));
        }
        let subject_id = SubjectId::new(subject).map_err(py_err)?;
        let resource_id = ResourceId::new(resource).map_err(py_err)?;
        let result = self.engine.explain(&subject_id, permission, &resource_id, None)
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

    fn close(&self) -> PyResult<()> {
        if self.closed.swap(true, Ordering::Relaxed) {
            return Ok(()); // idempotent
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

// ── Helper functions ──

fn validate_tuple(subject: &str, relation: &str, resource: &str) -> PyResult<RelationshipTuple> {
    let subject_id = SubjectId::new(subject).map_err(py_err)?;
    let relation_id = Relation::new(relation).map_err(py_err)?;
    let resource_id = ResourceId::new(resource).map_err(py_err)?;
    Ok(RelationshipTuple::new(subject_id, relation_id, resource_id))
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
    Ok(())
}
