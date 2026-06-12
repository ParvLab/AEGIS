use std::sync::Mutex;

use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::{StorageBackend, TupleFilter};
use aegis_core::types::{Relation, RelationshipTuple, ResourceId, SubjectId, TupleKey, PaginationParams};
use napi_derive::napi;

fn lock_engine() -> napi::Result<std::sync::MutexGuard<'static, Option<GraphEngine>>> {
    ENGINE.lock().map_err(|e| {
        napi::Error::from_reason(format!("engine lock poisoned: {}", e))
    })
}

static ENGINE: Mutex<Option<GraphEngine>> = Mutex::new(None);

fn catch_engine_panic<F, T>(f: F) -> napi::Result<T>
where
    F: FnOnce() -> napi::Result<T>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            Err(napi::Error::from_reason(format!("engine panic: {}", msg)))
        }
    }
}

/// Convenience: lock engine, extract ref, run fallible fn, all panic-safe.
fn with_engine<F, T>(f: F) -> napi::Result<T>
where
    F: FnOnce(&GraphEngine) -> napi::Result<T>,
{
    catch_engine_panic(move || {
        let guard = lock_engine()?;
        let engine = engine_from_guard(&guard)?;
        f(engine)
    })
}

#[napi(object)]
pub struct CheckResultNAP {
    pub allowed: bool,
    pub revision: i64,
}

#[napi(object)]
pub struct WriteResultNAP {
    pub revision: i64,
}

#[napi(object)]
pub struct TupleNAP {
    pub subject: String,
    pub relation: String,
    pub object: String,
}

#[napi(object)]
pub struct ExplainResultNAP {
    pub allowed: bool,
    pub revision: i64,
    pub resolved_via: String,
    pub duration_ms: i64,
}

#[napi]
pub fn initialize(path: String, schema_yaml: String) -> napi::Result<()> {
    let config = SqliteConfig {
        path,
        max_readers: 4,
        busy_timeout_ms: 5000,
        wal_mode: true,
        mmap_size: 0,
    };
    let mut storage =
        SqliteStorage::new(config).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    storage
        .initialize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let schema = parse_schema(&schema_yaml)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let engine = GraphEngine::new(Box::new(storage), schema);
    let mut guard = lock_engine()?;
    *guard = Some(engine);
    Ok(())
}

fn engine_from_guard<'a>(guard: &'a Option<GraphEngine>) -> napi::Result<&'a GraphEngine> {
    guard.as_ref().ok_or_else(|| napi::Error::from_reason("engine not initialized"))
}

#[napi]
pub fn check(
    subject: String,
    permission: String,
    resource: String,
) -> napi::Result<CheckResultNAP> {
    with_engine(|engine| {
        let subject_id =
            SubjectId::new(&subject).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let resource_id =
            ResourceId::new(&resource).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let result = engine
            .check(&subject_id, &permission, &resource_id, None)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(CheckResultNAP {
            allowed: result.allowed,
            revision: result.revision.as_u64() as i64,
        })
    })
}

#[napi]
pub fn write(
    subject: String,
    relation: String,
    resource: String,
) -> napi::Result<WriteResultNAP> {
    with_engine(|engine| {
        let subject_id =
            SubjectId::new(&subject).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let relation_id =
            Relation::new(&relation).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let resource_id =
            ResourceId::new(&resource).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let tuple = RelationshipTuple::new(subject_id, relation_id, resource_id);
        let result = engine
            .write(&tuple)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(WriteResultNAP {
            revision: result.revision.as_u64() as i64,
        })
    })
}

#[napi]
pub fn delete(
    subject: String,
    relation: String,
    resource: String,
) -> napi::Result<WriteResultNAP> {
    with_engine(|engine| {
        let subject_id =
            SubjectId::new(&subject).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let relation_id =
            Relation::new(&relation).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let resource_id =
            ResourceId::new(&resource).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let key = TupleKey {
            subject: subject_id,
            relation: relation_id,
            object: resource_id,
        };
        let result = engine
            .delete(&key)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(WriteResultNAP {
            revision: result.revision.as_u64() as i64,
        })
    })
}

#[napi]
pub fn list_by_object(
    object: String,
    relation: Option<String>,
) -> napi::Result<Vec<TupleNAP>> {
    with_engine(|engine| {
        let object_id =
            ResourceId::new(&object).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let relation_opt = relation
            .as_deref()
            .map(Relation::new)
            .transpose()
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let tuples = engine
            .storage()
            .list_by_object(&object_id, relation_opt.as_ref(), &aegis_core::ConsistencyMode::MinimizeLatency)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(tuples
            .iter()
            .map(|t| TupleNAP {
                subject: t.subject.as_str().to_string(),
                relation: t.relation.as_str().to_string(),
                object: t.object.as_str().to_string(),
            })
            .collect())
    })
}

#[napi]
pub fn explain(
    subject: String,
    permission: String,
    resource: String,
) -> napi::Result<ExplainResultNAP> {
    with_engine(|engine| {
        let subject_id =
            SubjectId::new(&subject).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let resource_id =
            ResourceId::new(&resource).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let result = engine
            .explain(&subject_id, &permission, &resource_id, None)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(ExplainResultNAP {
            allowed: result.allowed,
            revision: result.revision.as_u64() as i64,
            resolved_via: result.resolved_via,
            duration_ms: result.duration_ms as i64,
        })
    })
}

#[napi(object)]
pub struct HealthReportNAP {
    pub healthy: bool,
    pub revision: i64,
    pub schema_version: i32,
    pub backend: String,
    pub backend_healthy: bool,
    pub telemetry_healthy: bool,
    pub cache_hit_rate: f64,
    pub cache_entries: i32,
    pub storage_integrity: bool,
}

#[napi]
pub fn health() -> napi::Result<HealthReportNAP> {
    with_engine(|engine| {
        let report = engine.health();
        Ok(HealthReportNAP {
            healthy: report.healthy,
            revision: report.revision.as_u64() as i64,
            schema_version: report.schema_version as i32,
            backend: report.backend,
            backend_healthy: report.backend_healthy,
            telemetry_healthy: report.telemetry_healthy,
            cache_hit_rate: report.cache_hit_rate,
            cache_entries: report.cache_entries as i32,
            storage_integrity: report.storage_integrity,
        })
    })
}

#[napi]
pub fn check_dry_run(
    subject: String,
    permission: String,
    resource: String,
) -> napi::Result<CheckResultNAP> {
    with_engine(|engine| {
        let subject_id =
            SubjectId::new(&subject).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let resource_id =
            ResourceId::new(&resource).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let result = engine
            .check_dry_run(&subject_id, &permission, &resource_id, None)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(CheckResultNAP {
            allowed: result.allowed,
            revision: result.revision.as_u64() as i64,
        })
    })
}

// --- Sprint 1.1 new bindings ---

#[napi]
pub fn list_by_subject(
    subject: String,
    relation: Option<String>,
) -> napi::Result<Vec<TupleNAP>> {
    with_engine(|engine| {
        let subject_id =
            SubjectId::new(&subject).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let relation_opt = relation
            .as_deref()
            .map(Relation::new)
            .transpose()
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let tuples = engine
            .list_by_subject(&subject_id, relation_opt.as_ref(), None)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(tuples
            .iter()
            .map(|t| TupleNAP {
                subject: t.subject.as_str().to_string(),
                relation: t.relation.as_str().to_string(),
                object: t.object.as_str().to_string(),
            })
            .collect())
    })
}

#[napi(object)]
pub struct QueryFilterNAP {
    pub subject_type: Option<String>,
    pub relation: Option<String>,
    pub object_type: Option<String>,
    pub metadata_key: Option<String>,
    pub metadata_value: Option<String>,
}

#[napi(object)]
pub struct PaginationNAP {
    pub limit: f64,
    pub cursor_offset: Option<f64>,
}

#[napi(object)]
pub struct PaginatedTuplesNAP {
    pub tuples: Vec<TupleNAP>,
    pub next_cursor: Option<f64>,
    pub revision: f64,
}

#[napi]
pub fn query(
    filter: QueryFilterNAP,
    pagination: PaginationNAP,
) -> napi::Result<PaginatedTuplesNAP> {
    with_engine(|engine| {
        let relation = match filter.relation {
            Some(r) => Some(Relation::new(&r).map_err(|e| napi::Error::from_reason(e.to_string()))?),
            None => None,
        };
        let tf = TupleFilter {
            subject_type: filter.subject_type,
            relation,
            object_type: filter.object_type,
            metadata_key: filter.metadata_key,
            metadata_value: filter.metadata_value,
        };
        let current_rev = engine.storage().current_revision()
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let pp = PaginationParams {
            limit: pagination.limit as u64,
            cursor: pagination.cursor_offset.map(|o| aegis_core::types::PaginationCursor {
                offset: o as u64,
                revision: current_rev,
            }),
        };
        let result = engine
            .query(&tf, &pp, None)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(PaginatedTuplesNAP {
            tuples: result
                .tuples
                .iter()
                .map(|t| TupleNAP {
                    subject: t.subject.as_str().to_string(),
                    relation: t.relation.as_str().to_string(),
                    object: t.object.as_str().to_string(),
                })
                .collect(),
            next_cursor: result.next_cursor.map(|c| c.offset as f64),
            revision: result.revision.as_u64() as f64,
        })
    })
}

#[napi]
pub fn write_batch(
    tuples: Vec<TupleNAP>,
) -> napi::Result<WriteResultNAP> {
    with_engine(|engine| {
        let mut rel_tuples = Vec::with_capacity(tuples.len());
        for t in tuples {
            let subject_id = SubjectId::new(&t.subject)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let relation_id = Relation::new(&t.relation)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let object_id = ResourceId::new(&t.object)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            rel_tuples.push(RelationshipTuple::new(subject_id, relation_id, object_id));
        }
        let result = engine
            .write_batch(&rel_tuples)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(WriteResultNAP {
            revision: result.revision.as_u64() as i64,
        })
    })
}

#[napi]
pub fn migrate(target_version: i32) -> napi::Result<()> {
    with_engine(|engine| {
        engine
            .migrate(target_version as u32)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(())
    })
}

#[napi(object)]
pub struct SchemaCheckReportNAP {
    pub compatible: bool,
    pub warnings: Vec<String>,
    pub breaking: Vec<String>,
}

#[napi]
pub fn check_schema(schema_yaml: String) -> napi::Result<SchemaCheckReportNAP> {
    with_engine(|engine| {
        let new_schema = parse_schema(&schema_yaml)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let report = engine.check_schema(&new_schema);
        Ok(SchemaCheckReportNAP {
            compatible: report.compatible,
            warnings: report.warnings,
            breaking: report.breaking,
        })
    })
}

#[napi]
pub fn delete_object(object: String) -> napi::Result<WriteResultNAP> {
    with_engine(|engine| {
        let object_id =
            ResourceId::new(&object).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let result = engine
            .delete_object(&object_id)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(WriteResultNAP {
            revision: result.revision.as_u64() as i64,
        })
    })
}
