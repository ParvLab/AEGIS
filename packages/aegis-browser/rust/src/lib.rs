use std::sync::Mutex;
use aegis_core::engine::GraphEngine;
use aegis_core::storage::{InMemoryStorage, StorageBackend, TupleFilter};
use aegis_core::types::{
    ConsistencyMode, PaginationCursor, PaginationParams, Relation, RelationshipTuple, ResourceId,
    Revision, Schema, SubjectId, TupleKey,
};
use aegis_core::engine::policy_lifecycle::DraftStatus;
use aegis_core::engine::watch::WatchEventType;
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

// === Policy Lifecycle (9) ===
#[wasm_bindgen]
pub fn create_policy_draft(name: &str, description: &str) -> Result<String, JsValue> {
    with_engine(|e| e.create_policy_draft(name, description)
        .map(|d| serde_json::to_string(&d).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn update_policy_draft(id: &str, schema_json: &str) -> Result<String, JsValue> {
    let uid = uuid::Uuid::parse_str(id).map_err(to_js_err)?;
    let schema: aegis_core::types::Schema = serde_json::from_str(schema_json).map_err(to_js_err)?;
    with_engine(|e| e.update_policy_draft(uid, schema)
        .map(|d| serde_json::to_string(&d).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn validate_policy_draft(id: &str) -> Result<String, JsValue> {
    let uid = uuid::Uuid::parse_str(id).map_err(to_js_err)?;
    with_engine(|e| e.validate_policy_draft(uid)
        .map(|r| serde_json::to_string(&r).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn submit_policy_draft_for_review(id: &str) -> Result<String, JsValue> {
    let uid = uuid::Uuid::parse_str(id).map_err(to_js_err)?;
    with_engine(|e| e.submit_policy_draft_for_review(uid)
        .map(|d| serde_json::to_string(&d).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn approve_policy_draft(id: &str) -> Result<String, JsValue> {
    let uid = uuid::Uuid::parse_str(id).map_err(to_js_err)?;
    with_engine(|e| e.approve_policy_draft(uid)
        .map(|d| serde_json::to_string(&d).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn reject_policy_draft(id: &str, reason: &str) -> Result<String, JsValue> {
    let uid = uuid::Uuid::parse_str(id).map_err(to_js_err)?;
    with_engine(|e| e.reject_policy_draft(uid, reason)
        .map(|d| serde_json::to_string(&d).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn publish_policy_draft(id: &str) -> Result<String, JsValue> {
    let uid = uuid::Uuid::parse_str(id).map_err(to_js_err)?;
    with_engine(|e| e.publish_policy_draft(uid)
        .map(|r| serde_json::to_string(&r).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn archive_policy_draft(id: &str) -> Result<String, JsValue> {
    let uid = uuid::Uuid::parse_str(id).map_err(to_js_err)?;
    with_engine(|e| e.archive_policy_draft(uid)
        .map(|d| serde_json::to_string(&d).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn list_policy_drafts(filter_status: Option<String>) -> Result<String, JsValue> {
    let filter = match filter_status {
        Some(s) => {
            let status: DraftStatus = serde_json::from_str(&format!("\"{}\"", s)).map_err(to_js_err)?;
            Some(status)
        }
        None => None,
    };
    with_engine(|e| e.list_policy_drafts(filter)
        .map(|d| serde_json::to_string(&d).unwrap_or_default()))
}

// === Scheduler (5) ===
#[wasm_bindgen]
pub fn create_analysis_schedule(config_json: &str) -> Result<String, JsValue> {
    let config: aegis_core::engine::scheduler::AnalysisScheduleConfig = serde_json::from_str(config_json).map_err(to_js_err)?;
    with_engine(|e| e.create_analysis_schedule(&config.name, config.interval_seconds, config.queries, config.compare_schema)
        .map(|s| serde_json::to_string(&s).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn list_analysis_schedules() -> Result<String, JsValue> {
    with_engine(|e| e.list_analysis_schedules()
        .map(|s| serde_json::to_string(&s).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn delete_analysis_schedule(id: &str) -> Result<String, JsValue> {
    let uid = uuid::Uuid::parse_str(id).map_err(to_js_err)?;
    with_engine(|e| e.delete_analysis_schedule(uid).map(|_| "ok".to_string()))
}

#[wasm_bindgen]
pub fn run_analysis_now(schedule_id: Option<String>) -> Result<String, JsValue> {
    let uid = match schedule_id {
        Some(id) => Some(uuid::Uuid::parse_str(&id).map_err(to_js_err)?),
        None => None,
    };
    with_engine(|e| e.run_analysis_now(uid)
        .map(|r| serde_json::to_string(&r).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn get_analysis_runs(limit: Option<usize>) -> Result<String, JsValue> {
    with_engine(|e| e.get_analysis_runs(limit.unwrap_or(100))
        .map(|r| serde_json::to_string(&r).unwrap_or_default()))
}

// === Enforcement History (3) ===
#[wasm_bindgen]
pub fn set_enforcement_history_config(config_json: &str) -> Result<String, JsValue> {
    let config: aegis_core::engine::enforcement_history::EnforcementHistoryConfig = serde_json::from_str(config_json).map_err(to_js_err)?;
    with_engine(|e| e.set_enforcement_history_config(config).map(|_| "ok".to_string()))
}

#[wasm_bindgen]
pub fn get_enforcement_history_config() -> Result<String, JsValue> {
    with_engine(|e| e.get_enforcement_history_config()
        .map(|c| serde_json::to_string(&c).unwrap_or_default()))
}

#[wasm_bindgen]
pub fn enforcement_trends(limit: Option<usize>) -> Result<String, JsValue> {
    with_engine(|e| e.enforcement_trends(limit.unwrap_or(100))
        .map(|t| serde_json::to_string(&t).unwrap_or_default()))
}

// === Subscribe (1) ===
#[wasm_bindgen]
pub fn subscribe(event_types_json: &str) -> Result<String, JsValue> {
    let type_strings: Vec<String> = serde_json::from_str(event_types_json).map_err(to_js_err)?;
    let types: Vec<WatchEventType> = type_strings.iter()
        .map(|s| match s.to_lowercase().as_str() {
            "tupleadded" => Ok(WatchEventType::TupleAdded),
            "tupleremoved" => Ok(WatchEventType::TupleRemoved),
            "policyversioncreated" => Ok(WatchEventType::PolicyVersionCreated),
            "policyrolledback" => Ok(WatchEventType::PolicyRolledBack),
            "integrityfinding" => Ok(WatchEventType::IntegrityFinding),
            "analysiscompleted" => Ok(WatchEventType::AnalysisCompleted),
            "ratelimitwarning" => Ok(WatchEventType::RateLimitWarning),
            _ => Err(JsValue::from_str(&format!("unknown event type: {}", s))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    with_engine(|e| {
        let sub = e.subscribe(types);
        Ok(serde_json::json!({"subscription_id": sub.id().to_string()}).to_string())
    })
}
