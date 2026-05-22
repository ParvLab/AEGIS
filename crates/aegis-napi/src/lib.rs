use std::sync::Mutex;

use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::StorageBackend;
use aegis_core::types::{Relation, RelationshipTuple, ResourceId, SubjectId, TupleKey};
use napi_derive::napi;

static ENGINE: Mutex<Option<GraphEngine>> = Mutex::new(None);

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
    };
    let mut storage =
        SqliteStorage::new(config).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    storage
        .initialize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let schema = parse_schema(&schema_yaml)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let engine = GraphEngine::new(Box::new(storage), schema);
    let mut guard = ENGINE.lock().unwrap();
    *guard = Some(engine);
    Ok(())
}

#[napi]
pub fn check(
    subject: String,
    permission: String,
    resource: String,
) -> napi::Result<CheckResultNAP> {
    let guard = ENGINE.lock().unwrap();
    let engine = guard
        .as_ref()
        .ok_or_else(|| napi::Error::from_reason("engine not initialized"))?;
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
}

#[napi]
pub fn write(
    subject: String,
    relation: String,
    resource: String,
) -> napi::Result<WriteResultNAP> {
    let guard = ENGINE.lock().unwrap();
    let engine = guard
        .as_ref()
        .ok_or_else(|| napi::Error::from_reason("engine not initialized"))?;
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
}

#[napi]
pub fn delete(
    subject: String,
    relation: String,
    resource: String,
) -> napi::Result<WriteResultNAP> {
    let guard = ENGINE.lock().unwrap();
    let engine = guard
        .as_ref()
        .ok_or_else(|| napi::Error::from_reason("engine not initialized"))?;
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
}

#[napi]
pub fn list_by_object(
    object: String,
    relation: Option<String>,
) -> napi::Result<Vec<TupleNAP>> {
    let guard = ENGINE.lock().unwrap();
    let engine = guard
        .as_ref()
        .ok_or_else(|| napi::Error::from_reason("engine not initialized"))?;
    let object_id =
        ResourceId::new(&object).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let relation_opt = relation
        .as_deref()
        .map(Relation::new)
        .transpose()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let tuples = engine
        .storage()
        .list_by_object(&object_id, relation_opt.as_ref())
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(tuples
        .iter()
        .map(|t| TupleNAP {
            subject: t.subject.as_str().to_string(),
            relation: t.relation.as_str().to_string(),
            object: t.object.as_str().to_string(),
        })
        .collect())
}

#[napi]
pub fn explain(
    subject: String,
    permission: String,
    resource: String,
) -> napi::Result<ExplainResultNAP> {
    let guard = ENGINE.lock().unwrap();
    let engine = guard
        .as_ref()
        .ok_or_else(|| napi::Error::from_reason("engine not initialized"))?;
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
}

#[napi(object)]
pub struct HealthReportNAP {
    pub healthy: bool,
    pub revision: i64,
    pub schema_version: i32,
    pub backend: String,
    pub backend_healthy: bool,
    pub cache_hit_rate: f64,
    pub cache_entries: i32,
    pub storage_integrity: bool,
}

#[napi]
pub fn health() -> napi::Result<HealthReportNAP> {
    let guard = ENGINE.lock().unwrap();
    let engine = guard
        .as_ref()
        .ok_or_else(|| napi::Error::from_reason("engine not initialized"))?;
    let report = engine.health();
    Ok(HealthReportNAP {
        healthy: report.healthy,
        revision: report.revision.as_u64() as i64,
        schema_version: report.schema_version as i32,
        backend: report.backend,
        backend_healthy: report.backend_healthy,
        cache_hit_rate: report.cache_hit_rate,
        cache_entries: report.cache_entries as i32,
        storage_integrity: report.storage_integrity,
    })
}

#[napi]
pub fn check_dry_run(
    subject: String,
    permission: String,
    resource: String,
) -> napi::Result<CheckResultNAP> {
    let guard = ENGINE.lock().unwrap();
    let engine = guard
        .as_ref()
        .ok_or_else(|| napi::Error::from_reason("engine not initialized"))?;
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
}
