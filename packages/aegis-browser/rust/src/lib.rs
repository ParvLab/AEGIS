use std::sync::Mutex;
use aegis_core::engine::GraphEngine;
use aegis_core::storage::{InMemoryStorage, PolicyVersion, StorageBackend, TupleFilter};
use aegis_core::types::{
    ConsistencyMode, PaginationCursor, PaginationParams, Relation, RelationshipTuple, ResourceId,
    Revision, Schema, SubjectId, TupleKey,
};
use aegis_core::types::analysis::*;
use wasm_bindgen::prelude::*;

static ENGINE: Mutex<Option<GraphEngine>> = Mutex::new(None);

fn to_js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

fn fmt_token(r: &aegis_core::types::RevisionToken) -> String {
    format!("{}:{}", r.revision, r.node_id)
}

#[wasm_bindgen]
pub fn init_sync(schema_json: &str, _in_memory: bool) -> Result<String, JsValue> {
    let schema: Schema =
        serde_json::from_str(schema_json).map_err(|e| JsValue::from_str(&format!("schema: {}", e)))?;
    let mut storage = InMemoryStorage::new();
    storage.initialize().map_err(to_js_err)?;
    let engine = GraphEngine::new(Box::new(storage), schema);
    *ENGINE.lock().unwrap() = Some(engine);
    Ok("ok".to_string())
}

#[wasm_bindgen]
pub fn init_async(schema_json: &str) -> Result<String, JsValue> {
    let schema: Schema =
        serde_json::from_str(schema_json).map_err(|e| JsValue::from_str(&format!("schema: {}", e)))?;
    let mut storage = InMemoryStorage::new();
    storage.initialize().map_err(to_js_err)?;
    let async_storage = aegis_core::storage::async_traits::InMemoryAsyncStorage::new();
    let engine = GraphEngine::new(Box::new(storage), schema).with_async_storage(Box::new(async_storage));
    *ENGINE.lock().unwrap() = Some(engine);
    Ok("ok".to_string())
}

fn with_engine<F, R>(f: F) -> Result<R, JsValue>
where
    F: FnOnce(&GraphEngine) -> Result<R, aegis_core::error::AegisError>,
{
    let guard = ENGINE.lock().unwrap();
    match guard.as_ref() {
        Some(e) => f(e).map_err(to_js_err),
        None => Err(JsValue::from_str("engine not initialized, call init_sync first")),
    }
}

#[wasm_bindgen]
pub fn set_partition(partition_id: &str) -> Result<String, JsValue> {
    let pid = aegis_core::types::PartitionId::new(partition_id).map_err(to_js_err)?;
    with_engine(|e| e.with_partition(pid).map(|_| "ok".to_string()))
}

#[wasm_bindgen]
pub fn active_partition() -> Result<String, JsValue> {
    with_engine(|e| Ok(e.active_partition_id().as_str().to_string()))
}

#[wasm_bindgen]
pub fn check(subject: &str, permission: &str, resource: &str) -> Result<bool, JsValue> {
    let subject = SubjectId::new(subject).map_err(to_js_err)?;
    let resource = ResourceId::new(resource).map_err(to_js_err)?;
    with_engine(|e| Ok(e.check(&subject, permission, &resource, None)?.allowed))
}

#[wasm_bindgen]
pub fn write_relation(subject: &str, relation: &str, resource: &str) -> Result<String, JsValue> {
    let subject = SubjectId::new(subject).map_err(to_js_err)?;
    let resource = ResourceId::new(resource).map_err(to_js_err)?;
    let rel = Relation::new(relation).map_err(to_js_err)?;
    let tuple = RelationshipTuple::new(subject, rel, resource);
    with_engine(|e| e.write(&tuple).map(|r| fmt_token(&r)))
}

#[wasm_bindgen]
pub fn delete_relation(subject: &str, relation: &str, resource: &str) -> Result<String, JsValue> {
    let subject = SubjectId::new(subject).map_err(to_js_err)?;
    let resource = ResourceId::new(resource).map_err(to_js_err)?;
    let rel = Relation::new(relation).map_err(to_js_err)?;
    let key = TupleKey { subject, relation: rel, object: resource };
    with_engine(|e| e.delete(&key).map(|r| fmt_token(&r)))
}

#[wasm_bindgen]
pub fn list_by_object(resource: &str) -> Result<String, JsValue> {
    let resource = ResourceId::new(resource).map_err(to_js_err)?;
    with_engine(|e| {
        e.list_by_object(&resource, None, None)
            .map(|tuples| tuples_to_json(&tuples))
    })
}

#[wasm_bindgen]
pub fn list_by_subject(subject: &str) -> Result<String, JsValue> {
    let subject = SubjectId::new(subject).map_err(to_js_err)?;
    with_engine(|e| {
        e.list_by_subject(&subject, None, None)
            .map(|tuples| tuples_to_json(&tuples))
    })
}

fn tuples_to_json(tuples: &[RelationshipTuple]) -> String {
    let json: Vec<serde_json::Value> = tuples
        .iter()
        .map(|t| {
            serde_json::json!({
                "subject": t.subject.as_str(),
                "relation": t.relation.as_str(),
                "object": t.object.as_str(),
            })
        })
        .collect();
    serde_json::to_string(&json).unwrap_or_default()
}

#[wasm_bindgen]
pub fn export_json() -> Result<String, JsValue> {
    with_engine(|e| {
        let result = e.storage().query_tuples(
            &e.active_partition_id(),
            &TupleFilter::default(),
            &PaginationParams { cursor: None, limit: 100_000 },
            &ConsistencyMode::MinimizeLatency,
        ).map_err(|err| aegis_core::error::AegisError::Internal(err.to_string()))?;
        Ok(tuples_to_json(&result.tuples))
    })
}

#[wasm_bindgen]
pub fn import_json(json: &str) -> Result<String, JsValue> {
    let tuples: Vec<serde_json::Value> =
        serde_json::from_str(json).map_err(|e| JsValue::from_str(&format!("parse: {}", e)))?;
    for entry in &tuples {
        let subject = entry["subject"].as_str().unwrap_or("");
        let relation = entry["relation"].as_str().unwrap_or("");
        let object = entry["object"].as_str().unwrap_or("");
        write_relation(subject, relation, object)?;
    }
    Ok("ok".to_string())
}

#[wasm_bindgen]
pub fn explain_v2(
    subject: &str,
    permission: &str,
    resource: &str,
    consistency_opt: Option<String>,
) -> Result<String, JsValue> {
    let subject = SubjectId::new(subject).map_err(to_js_err)?;
    let resource = ResourceId::new(resource).map_err(to_js_err)?;
    let consistency = consistency_opt.as_deref().and_then(|s| match s {
        "minimize_latency" => Some(ConsistencyMode::MinimizeLatency),
        "maximize_consistency" => Some(ConsistencyMode::FullyConsistent),
        _ => None,
    });
    with_engine(|e| {
        e.explain_v2(&subject, permission, &resource, consistency)
            .map(|r| serde_json::to_string(&r).unwrap_or_default())
    })
}

#[wasm_bindgen]
pub fn who_can_access(
    permission: &str,
    resource: &str,
    page_offset: u64,
    page_limit: u64,
    include_paths: bool,
) -> Result<String, JsValue> {
    let resource = ResourceId::new(resource).map_err(to_js_err)?;
    let pagination = PaginationParams {
        cursor: Some(PaginationCursor {
            offset: page_offset,
            revision: Revision::ZERO,
        }),
        limit: page_limit,
    };
    with_engine(|e| {
        e.who_can_access(permission, &resource, &pagination, include_paths, 10, 5000)
            .map(|r| serde_json::to_string(&r).unwrap_or_default())
    })
}

#[wasm_bindgen]
pub fn access_diff(
    schema_before_json: &str,
    schema_after_json: &str,
    max_checks: Option<u64>,
) -> Result<String, JsValue> {
    let schema_before: Schema = serde_json::from_str(schema_before_json)
        .map_err(|e| JsValue::from_str(&format!("schema_before: {}", e)))?;
    let schema_after: Schema = serde_json::from_str(schema_after_json)
        .map_err(|e| JsValue::from_str(&format!("schema_after: {}", e)))?;
    with_engine(|e| {
        e.access_diff(&schema_before, &schema_after, None, max_checks)
            .map(|r| serde_json::to_string(&r).unwrap_or_default())
    })
}

#[wasm_bindgen]
pub fn list_policy_versions() -> Result<String, JsValue> {
    with_engine(|e| {
        e.list_policy_versions()
            .map(|versions| serde_json::to_string(&versions).unwrap_or_default())
    })
}

#[wasm_bindgen]
pub fn rollback_policy(version: u32) -> Result<String, JsValue> {
    with_engine(|e| e.rollback_policy(version).map(|_| "ok".to_string()))
}
