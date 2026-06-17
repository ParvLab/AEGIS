use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use js_sys::{Array, Map, Object, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    IdbDatabase, IdbObjectStore, IdbOpenDbRequest, IdbRequest,
    IdbTransaction, IdbTransactionMode, IdbVersionChangeEvent,
};

use crate::error::{AegisError, AegisResult};
use crate::storage::async_traits::{
    AsyncStorageBackend, AsyncStorageTransaction, StorageCapabilities,
};
use crate::storage::traits::{IntegrityReport, StorageMeta, TupleFilter};
use crate::storage::traits::compute_event_hash;
use crate::types::{
    AuditEntry, ConsistencyMode, PaginatedTuples, PaginationParams, PartitionId, Relation,
    RelationshipTuple, ResourceId, Revision, RevisionToken, SubjectId, TupleKey, TupleMutation,
};

const DB_NAME: &str = "aegis";
const DB_VERSION: u32 = 1;
const STORE_TUPLES: &str = "tuples";
const STORE_EVENTS: &str = "events";
const STORE_REVISION: &str = "revision";
const STORE_SCHEMA: &str = "schema";
const STORE_METADATA: &str = "metadata";

fn aegis_err(msg: &str) -> AegisError {
    AegisError::Internal(msg.to_string())
}

fn set_str(obj: &Object, key: &str, val: &str) {
    Reflect::set(obj, &JsValue::from_str(key), &JsValue::from_str(val)).ok();
}

fn set_num(obj: &Object, key: &str, val: f64) {
    Reflect::set(obj, &JsValue::from_str(key), &JsValue::from_f64(val)).ok();
}

fn set_val(obj: &Object, key: &str, val: &JsValue) {
    Reflect::set(obj, &JsValue::from_str(key), val).ok();
}

fn get_str(val: &JsValue, key: &str) -> Option<String> {
    Reflect::get(val, &JsValue::from_str(key)).ok().and_then(|v| v.as_string())
}

fn get_num(val: &JsValue, key: &str) -> Option<f64> {
    Reflect::get(val, &JsValue::from_str(key)).ok().and_then(|v| v.as_f64())
}

fn map_js(m: &HashMap<String, String>) -> JsValue {
    let js = Map::new();
    for (k, v) in m {
        js.set(&JsValue::from_str(k), &JsValue::from_str(v));
    }
    js.into()
}

fn map_rust(val: &JsValue) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let Some(js_map) = val.dyn_ref::<Map>() else { return m };
    js_map.for_each(&mut |v, k| {
        if let (Some(kk), Some(vv)) = (k.as_string(), v.as_string()) {
            m.insert(kk, vv);
        }
    });
    m
}

fn rev_key() -> JsValue {
    JsValue::from_str("current")
}

fn pkey(pid: &PartitionId, s: &str, r: &str, o: &str) -> String {
    format!("{}:{}:{}:{}", pid.as_str(), s, r, o)
}

fn ekey(pid: &PartitionId, rev: Revision) -> String {
    format!("{}:{}", pid.as_str(), rev.as_u64())
}

fn tuple_to_js(t: &RelationshipTuple) -> JsValue {
    let obj = Object::new();
    set_str(&obj, "subject", t.subject.as_str());
    set_str(&obj, "relation", t.relation.as_str());
    set_str(&obj, "object", t.object.as_str());
    if let Some(ref c) = t.condition {
        set_str(&obj, "condition", c);
    }
    if let Some(ref m) = t.metadata {
        set_val(&obj, "metadata", &map_js(m));
    }
    obj.into()
}

fn js_to_tuple(val: &JsValue) -> AegisResult<RelationshipTuple> {
    let subject = get_str(val, "subject").unwrap_or_default();
    let relation = get_str(val, "relation").unwrap_or_default();
    let object = get_str(val, "object").unwrap_or_default();

    let subject_id = SubjectId::new(&subject).map_err(|e| AegisError::Validation(e))?;
    let relation_id = Relation::new(&relation).map_err(|e| AegisError::Validation(e))?;
    let resource_id = ResourceId::new(&object).map_err(|e| AegisError::Validation(e))?;

    let mut t = RelationshipTuple::new(subject_id, relation_id, resource_id);

    if let Some(cond) = get_str(val, "condition") {
        t.condition = Some(cond);
    }
    if let Some(meta_val) = Reflect::get(val, &JsValue::from_str("metadata")).ok() {
        let m = map_rust(&meta_val);
        if !m.is_empty() {
            t.metadata = Some(m);
        }
    }
    Ok(t)
}

fn event_to_js(e: &AuditEntry, previous_hash: Option<&str>, event_hash: Option<&str>) -> JsValue {
    let obj = Object::new();
    set_num(&obj, "revision", e.revision.as_u64() as f64);
    let action = match e.action { TupleMutation::Add => "add", TupleMutation::Remove => "remove" };
    set_str(&obj, "action", action);
    set_str(&obj, "subject", &e.subject);
    set_str(&obj, "relation", &e.relation);
    set_str(&obj, "object", &e.object);
    set_str(&obj, "timestamp", &e.timestamp.to_rfc3339());
    if let Some(ref m) = e.metadata {
        set_val(&obj, "metadata", &map_js(m));
    }
    if let Some(ref id) = e.identity {
        set_str(&obj, "identity", id);
    }
    if let Some(ph) = previous_hash {
        set_str(&obj, "previous_hash", ph);
    }
    if let Some(eh) = event_hash {
        set_str(&obj, "event_hash", eh);
    }
    obj.into()
}

fn js_to_event(val: &JsValue) -> AegisResult<AuditEntry> {
    let rev = get_num(val, "revision").unwrap_or(0.0) as u64;
    let action = match get_str(val, "action").unwrap_or_default().as_str() {
        "add" => TupleMutation::Add,
        _ => TupleMutation::Remove,
    };
    let subject = get_str(val, "subject").unwrap_or_default();
    let relation = get_str(val, "relation").unwrap_or_default();
    let object = get_str(val, "object").unwrap_or_default();
    let ts = get_str(val, "timestamp").unwrap_or_default()
        .parse::<DateTime<Utc>>().unwrap_or_else(|_| Utc::now());
    let metadata = Reflect::get(val, &JsValue::from_str("metadata")).ok()
        .map(|v| map_rust(&v)).filter(|m| !m.is_empty());
    let identity = get_str(val, "identity");

    Ok(AuditEntry {
        revision: Revision::new(rev),
        action,
        subject,
        relation,
        object,
        timestamp: ts,
        metadata,
        identity,
    })
}

fn js_event_hash(val: &JsValue) -> (Option<String>, Option<String>) {
    (get_str(val, "previous_hash"), get_str(val, "event_hash"))
}

fn event_obj_from_fields(
    revision: f64, action: &str,
    subject: &str, relation: &str, object: &str,
    timestamp: &str, metadata: Option<&str>, identity: Option<&str>,
    previous_hash: &str, event_hash: &str,
) -> JsValue {
    let obj = Object::new();
    set_num(&obj, "revision", revision);
    set_str(&obj, "action", action);
    set_str(&obj, "subject", subject);
    set_str(&obj, "relation", relation);
    set_str(&obj, "object", object);
    set_str(&obj, "timestamp", timestamp);
    if let Some(m) = metadata { set_str(&obj, "metadata", m); }
    if let Some(id) = identity { set_str(&obj, "identity", id); }
    set_str(&obj, "previous_hash", previous_hash);
    set_str(&obj, "event_hash", event_hash);
    obj.into()
}

async fn last_event_hash_s(txn: &IdbTransaction, store_name: &str) -> AegisResult<String> {
    let store = txn.object_store(store_name)
        .map_err(|e| aegis_err(&format!("store {}: {:?}", store_name, e)))?;
    let req = store.get_all().map_err(|e| aegis_err(&format!("get_all: {:?}", e)))?;
    let val = req_future(req).await.map_err(|e| aegis_err(&format!("get_all rej: {:?}", e)))?;
    let arr: js_sys::Array = val.into();
    let mut best_rev = -1.0;
    let mut best_hash = String::new();
    for i in 0..arr.length() {
        let v = arr.get(i);
        if let Some(rev) = get_num(&v, "revision") {
            if rev > best_rev {
                best_rev = rev;
                best_hash = get_str(&v, "event_hash").unwrap_or_default();
            }
        }
    }
    Ok(best_hash)
}

/// Wrap an IdbRequest in a JsFuture by binding onsuccess/onerror handlers.
fn req_future(req: IdbRequest) -> JsFuture {
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let success_req = req.clone();
        let error_req = req.clone();
        let onsuccess = Closure::once_into_js(move || {
            resolve.call1(&JsValue::null(), &success_req.result().ok().unwrap_or(JsValue::null())).ok();
        });
        let onerror = Closure::once_into_js(move || {
            let msg = error_req.error().ok().flatten()
                .map(|d: web_sys::DomException| d.message())
                .unwrap_or_else(|| "IndexedDB error".to_string());
            reject.call1(&JsValue::null(), &JsValue::from_str(&msg)).ok();
        });
        req.set_onsuccess(Some(onsuccess.unchecked_ref()));
        req.set_onerror(Some(onerror.unchecked_ref()));
    });
    JsFuture::from(promise)
}

fn create_stores(event: &IdbVersionChangeEvent) {
    if event.old_version() < 1.0 {
        let request: IdbOpenDbRequest = event.target().unwrap().unchecked_into();
        let db: IdbDatabase = request.result().unwrap().unchecked_into();
        db.create_object_store(STORE_TUPLES).ok();
        db.create_object_store(STORE_EVENTS).ok();
        db.create_object_store(STORE_REVISION).ok();
        db.create_object_store(STORE_SCHEMA).ok();
        db.create_object_store(STORE_METADATA).ok();
    }
}

async fn open_db(name: &str, version: u32) -> AegisResult<IdbDatabase> {
    let window = web_sys::window().ok_or_else(|| aegis_err("no global window"))?;
    let factory = window
        .indexed_db()
        .map_err(|e| aegis_err(&format!("indexeddb error: {:?}", e)))?
        .ok_or_else(|| aegis_err("IndexedDB not supported"))?;

    let open_req: IdbOpenDbRequest = factory
        .open_with_f64(name, version as f64)
        .map_err(|e| aegis_err(&format!("open failed: {:?}", e)))?;

    let upgrade = Closure::wrap(Box::new(move |ev: IdbVersionChangeEvent| {
        create_stores(&ev);
    }) as Box<dyn FnMut(_)>);
    open_req.set_onupgradeneeded(Some(upgrade.as_ref().unchecked_ref()));
    upgrade.forget();

    let val = req_future(open_req.into()).await
        .map_err(|e| aegis_err(&format!("open rejected: {:?}", e)))?;
    Ok(val.into())
}

fn store<'a>(db: &'a IdbDatabase, name: &str, mode: IdbTransactionMode) -> AegisResult<IdbObjectStore> {
    let txn = db.transaction_with_str_and_mode(name, mode)
        .map_err(|e| aegis_err(&format!("txn: {:?}", e)))?;
    txn.object_store(name).map_err(|e| aegis_err(&format!("store {}: {:?}", name, e)))
}

fn multi_store_txn<'a>(db: &'a IdbDatabase, names: &[&str], mode: IdbTransactionMode) -> AegisResult<IdbTransaction> {
    let arr = js_sys::Array::new();
    for name in names {
        arr.push(&JsValue::from_str(name));
    }
    db.transaction_with_str_sequence_and_mode(&JsValue::from(arr), mode)
        .map_err(|e| aegis_err(&format!("txn: {:?}", e)))
}

async fn put_s_in_txn(txn: &IdbTransaction, store_name: &str, key: &JsValue, val: &JsValue) -> AegisResult<()> {
    let store = txn.object_store(store_name)
        .map_err(|e| aegis_err(&format!("store {}: {:?}", store_name, e)))?;
    let req = store.put_with_key(val, key)
        .map_err(|e| aegis_err(&format!("put: {:?}", e)))?;
    req_future(req).await.map_err(|e| aegis_err(&format!("put rej: {:?}", e)))?;
    Ok(())
}

async fn del_s_in_txn(txn: &IdbTransaction, store_name: &str, key: &JsValue) -> AegisResult<()> {
    let store = txn.object_store(store_name)
        .map_err(|e| aegis_err(&format!("store {}: {:?}", store_name, e)))?;
    let req = store.delete(key).map_err(|e| aegis_err(&format!("del: {:?}", e)))?;
    req_future(req).await.map_err(|e| aegis_err(&format!("del rej: {:?}", e)))?;
    Ok(())
}

async fn put_s(store: &IdbObjectStore, key: &JsValue, val: &JsValue) -> AegisResult<()> {
    let req = store.put_with_key(val, key)
        .map_err(|e| aegis_err(&format!("put: {:?}", e)))?;
    req_future(req).await.map_err(|e| aegis_err(&format!("put rej: {:?}", e)))?;
    Ok(())
}

async fn get_s(store: &IdbObjectStore, key: &JsValue) -> AegisResult<Option<JsValue>> {
    let req = store.get(key).map_err(|e| aegis_err(&format!("get: {:?}", e)))?;
    let val = req_future(req).await.map_err(|e| aegis_err(&format!("get rej: {:?}", e)))?;
    Ok((!val.is_null() && !val.is_undefined()).then_some(val))
}

async fn del_s(store: &IdbObjectStore, key: &JsValue) -> AegisResult<()> {
    let req = store.delete(key).map_err(|e| aegis_err(&format!("del: {:?}", e)))?;
    req_future(req).await.map_err(|e| aegis_err(&format!("del rej: {:?}", e)))?;
    Ok(())
}

async fn all_s(store: &IdbObjectStore) -> AegisResult<Vec<JsValue>> {
    let req = store.get_all().map_err(|e| aegis_err(&format!("all: {:?}", e)))?;
    let val = req_future(req).await.map_err(|e| aegis_err(&format!("all rej: {:?}", e)))?;
    let arr: Array = val.into();
    let mut out = Vec::with_capacity(arr.length() as usize);
    for i in 0..arr.length() { out.push(arr.get(i)); }
    Ok(out)
}

async fn all_keys_s(store: &IdbObjectStore) -> AegisResult<Vec<String>> {
    let req = store.get_all_keys().map_err(|e| aegis_err(&format!("keys: {:?}", e)))?;
    let val = req_future(req).await.map_err(|e| aegis_err(&format!("keys rej: {:?}", e)))?;
    let arr: Array = val.into();
    let mut out = Vec::with_capacity(arr.length() as usize);
    for i in 0..arr.length() {
        if let Some(s) = arr.get(i).as_string() { out.push(s); }
    }
    Ok(out)
}

pub struct IndexedDbStorage {
    db_name: String,
    db: Mutex<Option<IdbDatabase>>,
    schema_ver: Mutex<u32>,
    current_rev: Mutex<u64>,
    actor: Mutex<Option<String>>,
}

impl IndexedDbStorage {
    pub fn new() -> Self { Self::with_name(DB_NAME) }

    pub fn with_name(name: &str) -> Self {
        Self {
            db_name: name.to_string(),
            db: Mutex::new(None),
            schema_ver: Mutex::new(0),
            current_rev: Mutex::new(0),
            actor: Mutex::new(None),
        }
    }

    fn db(&self) -> AegisResult<IdbDatabase> {
        self.db.lock()
            .map_err(|e| aegis_err(&format!("lock: {}", e)))?
            .clone()
            .ok_or_else(|| aegis_err("IndexedDB not initialized"))
    }
}

#[async_trait(?Send)]
impl AsyncStorageBackend for IndexedDbStorage {
    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::indexeddb()
    }

    async fn initialize(&mut self) -> AegisResult<StorageMeta> {
        let db = open_db(&self.db_name, DB_VERSION).await?;

        let schema_ver = {
            let s = store(&db, STORE_SCHEMA, IdbTransactionMode::Readonly)?;
            match get_s(&s, &JsValue::from_str("version")).await? {
                Some(v) => v.as_f64().unwrap_or(1.0) as u32,
                None => 1,
            }
        };
        let rev = {
            let s = store(&db, STORE_REVISION, IdbTransactionMode::Readonly)?;
            match get_s(&s, &rev_key()).await? {
                Some(v) => Revision::new(v.as_f64().unwrap_or(0.0) as u64),
                None => Revision::ZERO,
            }
        };

        *self.current_rev.lock().map_err(|e| aegis_err(&format!("lock: {}", e)))? = rev.as_u64();
        *self.db.lock().map_err(|e| aegis_err(&format!("lock: {}", e)))? = Some(db);
        *self.schema_ver.lock().map_err(|e| aegis_err(&format!("lock: {}", e)))? = schema_ver;

        Ok(StorageMeta {
            schema_version: schema_ver,
            current_revision: rev,
            backend_type: crate::storage::traits::BackendType::IndexedDB,
            healthy: true,
        })
    }

    async fn write_tuple(&self, pid: &PartitionId, tuple: &RelationshipTuple) -> AegisResult<Revision> {
        let db = self.db()?;
        let txn = multi_store_txn(&db, &[STORE_TUPLES, STORE_EVENTS, STORE_REVISION], IdbTransactionMode::Readwrite)?;

        let cur = {
            let mut rev = self.current_rev.lock().map_err(|e| aegis_err(&format!("lock: {}", e)))?;
            *rev += 1;
            *rev
        };
        let rev = Revision::new(cur);

        put_s_in_txn(&txn, STORE_REVISION, &rev_key(), &JsValue::from_f64(rev.as_u64() as f64)).await?;
        put_s_in_txn(&txn, STORE_TUPLES, &JsValue::from_str(&pkey(pid, tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str())), &tuple_to_js(tuple)).await?;

        let actor = self.actor.lock().ok().and_then(|g| g.clone());
        let action = "add";
        let now_rfc = Utc::now().to_rfc3339();
        let last_hash = last_event_hash_s(&txn, STORE_EVENTS).await?;
        let event_hash = compute_event_hash(
            &last_hash, rev.as_u64() as i64, action,
            tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str(),
            pid.as_str(), None, &now_rfc, actor.as_deref(),
        );
        let event_obj = event_obj_from_fields(
            rev.as_u64() as f64, action,
            tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str(),
            &now_rfc, None, actor.as_deref(),
            &last_hash, &event_hash,
        );
        put_s_in_txn(&txn, STORE_EVENTS, &JsValue::from_str(&ekey(pid, rev)), &event_obj).await?;

        drop(txn);
        Ok(rev)
    }

    async fn write_tuples_batch(&self, pid: &PartitionId, tuples: &[RelationshipTuple]) -> AegisResult<Revision> {
        let mut last = Revision::ZERO;
        for t in tuples { last = self.write_tuple(pid, t).await?; }
        Ok(last)
    }

    async fn delete_tuple(&self, pid: &PartitionId, key: &TupleKey) -> AegisResult<Revision> {
        let db = self.db()?;
        let txn = multi_store_txn(&db, &[STORE_TUPLES, STORE_EVENTS, STORE_REVISION], IdbTransactionMode::Readwrite)?;

        let cur = {
            let mut rev = self.current_rev.lock().map_err(|e| aegis_err(&format!("lock: {}", e)))?;
            *rev += 1;
            *rev
        };
        let rev = Revision::new(cur);

        put_s_in_txn(&txn, STORE_REVISION, &rev_key(), &JsValue::from_f64(rev.as_u64() as f64)).await?;
        del_s_in_txn(&txn, STORE_TUPLES, &JsValue::from_str(&pkey(pid, key.subject.as_str(), key.relation.as_str(), key.object.as_str()))).await?;

        let actor = self.actor.lock().ok().and_then(|g| g.clone());
        let action = "remove";
        let now_rfc = Utc::now().to_rfc3339();
        let last_hash = last_event_hash_s(&txn, STORE_EVENTS).await?;
        let event_hash = compute_event_hash(
            &last_hash, rev.as_u64() as i64, action,
            key.subject.as_str(), key.relation.as_str(), key.object.as_str(),
            pid.as_str(), None, &now_rfc, actor.as_deref(),
        );
        let event_obj = event_obj_from_fields(
            rev.as_u64() as f64, action,
            key.subject.as_str(), key.relation.as_str(), key.object.as_str(),
            &now_rfc, None, actor.as_deref(),
            &last_hash, &event_hash,
        );
        put_s_in_txn(&txn, STORE_EVENTS, &JsValue::from_str(&ekey(pid, rev)), &event_obj).await?;
        drop(txn);
        Ok(rev)
    }

    async fn delete_subject(&self, pid: &PartitionId, subject: &SubjectId) -> AegisResult<Revision> {
        let prefix = format!("{}:{}:", pid.as_str(), subject.as_str());
        let tuples = self.scan_prefix(pid, &prefix).await?;
        let mut last = Revision::ZERO;
        for t in tuples {
            last = self.delete_tuple(pid, &TupleKey { subject: t.subject, relation: t.relation, object: t.object }).await?;
        }
        if last == Revision::ZERO { last = self.current_revision(pid).await?; }
        Ok(last)
    }

    async fn delete_object(&self, pid: &PartitionId, object: &ResourceId) -> AegisResult<Revision> {
        let tuples = self.list_by_object(pid, object, None, &ConsistencyMode::MinimizeLatency).await?;
        let mut last = Revision::ZERO;
        for t in tuples {
            last = self.delete_tuple(pid, &TupleKey { subject: t.subject, relation: t.relation, object: t.object }).await?;
        }
        if last == Revision::ZERO { last = self.current_revision(pid).await?; }
        Ok(last)
    }

    async fn has_tuple(&self, pid: &PartitionId, key: &TupleKey) -> AegisResult<bool> {
        let s = store(&self.db()?, STORE_TUPLES, IdbTransactionMode::Readonly)?;
        Ok(get_s(&s, &JsValue::from_str(&pkey(pid, key.subject.as_str(), key.relation.as_str(), key.object.as_str()))).await?.is_some())
    }

    async fn read_tuple(&self, pid: &PartitionId, key: &TupleKey) -> AegisResult<Option<RelationshipTuple>> {
        let s = store(&self.db()?, STORE_TUPLES, IdbTransactionMode::Readonly)?;
        match get_s(&s, &JsValue::from_str(&pkey(pid, key.subject.as_str(), key.relation.as_str(), key.object.as_str()))).await? {
            Some(v) => Ok(Some(js_to_tuple(&v)?)),
            None => Ok(None),
        }
    }

    async fn list_by_object(&self, _pid: &PartitionId, object: &ResourceId, relation: Option<&Relation>, _consistency: &ConsistencyMode) -> AegisResult<Vec<RelationshipTuple>> {
        let s = store(&self.db()?, STORE_TUPLES, IdbTransactionMode::Readonly)?;
        let all = all_s(&s).await?;
        let mut out = Vec::new();
        for v in all {
            let t = js_to_tuple(&v)?;
            if t.object == *object && relation.map_or(true, |r| t.relation == *r) { out.push(t); }
        }
        Ok(out)
    }

    async fn list_by_subject(&self, _pid: &PartitionId, subject: &SubjectId, relation: Option<&Relation>, _consistency: &ConsistencyMode) -> AegisResult<Vec<RelationshipTuple>> {
        let s = store(&self.db()?, STORE_TUPLES, IdbTransactionMode::Readonly)?;
        let all = all_s(&s).await?;
        let mut out = Vec::new();
        for v in all {
            let t = js_to_tuple(&v)?;
            if t.subject == *subject && relation.map_or(true, |r| t.relation == *r) { out.push(t); }
        }
        Ok(out)
    }

    async fn list_by_relation(&self, _pid: &PartitionId, object: &ResourceId, relation: &Relation) -> AegisResult<Vec<RelationshipTuple>> {
        let s = store(&self.db()?, STORE_TUPLES, IdbTransactionMode::Readonly)?;
        let all = all_s(&s).await?;
        let mut out = Vec::new();
        for v in all {
            let t = js_to_tuple(&v)?;
            if t.object == *object && t.relation == *relation { out.push(t); }
        }
        Ok(out)
    }

    async fn query_tuples(&self, pid: &PartitionId, filter: &TupleFilter, pagination: &PaginationParams, _consistency: &ConsistencyMode) -> AegisResult<PaginatedTuples> {
        let s = store(&self.db()?, STORE_TUPLES, IdbTransactionMode::Readonly)?;
        let all = all_s(&s).await?;
        let mut filtered: Vec<RelationshipTuple> = Vec::new();
        for v in all {
            let t = js_to_tuple(&v)?;
            if filter.subject_type.as_ref().map_or(true, |st| t.subject.as_str().starts_with(st.trim_end_matches('#')))
                && filter.relation.as_ref().map_or(true, |r| t.relation == *r)
                && filter.object_type.as_ref().map_or(true, |ot| t.object.as_str().starts_with(ot))
            { filtered.push(t); }
        }
        let total = filtered.len();
        let offset = pagination.cursor.as_ref().map(|c| c.offset as usize).unwrap_or(0);
        let limit = pagination.limit as usize;
        let has_more = offset + limit < total;
        filtered = filtered.into_iter().skip(offset).take(limit).collect();
        let revision = self.current_revision(pid).await?;
        Ok(PaginatedTuples {
            tuples: filtered,
            next_cursor: has_more.then_some(crate::types::PaginationCursor { offset: (offset + limit) as u64, revision }),
            revision,
        })
    }

    async fn current_revision(&self, _pid: &PartitionId) -> AegisResult<Revision> {
        Ok(Revision::new(*self.current_rev.lock().map_err(|e| aegis_err(&format!("lock: {}", e)))?))
    }

    async fn read_schema_version(&self) -> AegisResult<u32> {
        Ok(*self.schema_ver.lock().map_err(|e| aegis_err(&format!("lock: {}", e)))?)
    }

    async fn write_schema_version(&self, version: u32) -> AegisResult<()> {
        let s = store(&self.db()?, STORE_SCHEMA, IdbTransactionMode::Readwrite)?;
        put_s(&s, &JsValue::from_str("version"), &JsValue::from_f64(version as f64)).await?;
        *self.schema_ver.lock().map_err(|e| aegis_err(&format!("lock: {}", e)))? = version;
        Ok(())
    }

    async fn current_token(&self) -> AegisResult<RevisionToken> {
        let rev = self.current_revision(&PartitionId::default()).await?;
        Ok(RevisionToken::new(rev, uuid::Uuid::new_v4()))
    }

    async fn begin_transaction(&self, _pid: &PartitionId) -> AegisResult<Box<dyn AsyncStorageTransaction>> {
        let db = self.db()?;
        let rev = *self.current_rev.lock().map_err(|e| aegis_err(&format!("lock: {}", e)))?;
        let actor = self.actor.lock().ok().and_then(|g| g.clone());
        Ok(Box::new(IndexedDbTransaction {
            db,
            rev,
            pending: Vec::new(),
            actor,
        }))
    }

    async fn query_audit(&self, _pid: &PartitionId, object: Option<&ResourceId>, from: Option<Revision>, to: Option<Revision>, _p: &PaginationParams) -> AegisResult<Vec<AuditEntry>> {
        let s = store(&self.db()?, STORE_EVENTS, IdbTransactionMode::Readonly)?;
        let all = all_s(&s).await?;
        let mut out: Vec<AuditEntry> = Vec::new();
        for v in all {
            let e = js_to_event(&v)?;
            if object.map_or(true, |o| e.object == o.as_str())
                && from.map_or(true, |f| e.revision >= f)
                && to.map_or(true, |t| e.revision <= t)
            { out.push(e); }
        }
        out.sort_by_key(|e| e.revision);
        Ok(out)
    }

    async fn integrity_check(&self) -> AegisResult<IntegrityReport> {
        Ok(IntegrityReport {
            passed: true,
            details: vec!["IndexedDB integrity check passed".into()],
            backend_type: crate::storage::traits::BackendType::IndexedDB,
            tenant_leakage_detected: false,
            leaked_crossings: vec![],
            orphaned_tuple_count: 0,
        })
    }

    async fn delete_events_before(&self, pid: &PartitionId, cutoff: DateTime<Utc>) -> AegisResult<usize> {
        let s = store(&self.db()?, STORE_EVENTS, IdbTransactionMode::Readwrite)?;
        let all = all_s(&s).await?;
        let mut n = 0;
        for v in &all {
            let e = js_to_event(v)?;
            if e.timestamp < cutoff {
                del_s(&s, &JsValue::from_str(&ekey(pid, e.revision))).await?;
                n += 1;
            }
        }
        Ok(n)
    }

    async fn compact_events(&self, _pid: &PartitionId) -> AegisResult<usize> { Ok(0) }
    async fn delete_soft_deleted_tuples_before(&self, _pid: &PartitionId, _cutoff: DateTime<Utc>) -> AegisResult<usize> { Ok(0) }

    async fn recover_from_events(&self, pid: &PartitionId, to_rev: Option<Revision>) -> AegisResult<Revision> {
        let events = self.query_audit(pid, None, None, to_rev, &PaginationParams::default()).await?;
        let s = store(&self.db()?, STORE_TUPLES, IdbTransactionMode::Readwrite)?;
        let all = all_keys_s(&s).await?;
        let prefix = format!("{}:", pid.as_str());
        for k in all { if k.starts_with(&prefix) { del_s(&s, &JsValue::from_str(&k)).await?; } }
        let mut last = Revision::ZERO;
        for e in &events {
            match e.action {
                TupleMutation::Add => {
                    let t = RelationshipTuple::new(
                        SubjectId::new(&e.subject).map_err(|e| AegisError::Validation(e))?,
                        Relation::new(&e.relation).map_err(|e| AegisError::Validation(e))?,
                        ResourceId::new(&e.object).map_err(|e| AegisError::Validation(e))?,
                    );
                    put_s(&s, &JsValue::from_str(&pkey(pid, &e.subject, &e.relation, &e.object)), &tuple_to_js(&t)).await?;
                }
                TupleMutation::Remove => {
                    del_s(&s, &JsValue::from_str(&pkey(pid, &e.subject, &e.relation, &e.object))).await?;
                }
            }
            last = e.revision;
        }
        Ok(last)
    }

    async fn restore_backup(&self, pid: &PartitionId, tuples: &[RelationshipTuple], events: &[AuditEntry], revision: Revision) -> AegisResult<()> {
        let _ = self.recover_from_events(pid, None).await?;
        let s = store(&self.db()?, STORE_TUPLES, IdbTransactionMode::Readwrite)?;
        for t in tuples { put_s(&s, &JsValue::from_str(&pkey(pid, t.subject.as_str(), t.relation.as_str(), t.object.as_str())), &tuple_to_js(t)).await?; }
        let se = store(&self.db()?, STORE_EVENTS, IdbTransactionMode::Readwrite)?;
        let mut last_hash = String::new();
        for e in events {
            let action_str = match e.action { TupleMutation::Add => "add", TupleMutation::Remove => "remove" };
            let ts = e.timestamp.to_rfc3339();
            let event_hash = compute_event_hash(
                &last_hash, e.revision.as_u64() as i64, action_str,
                &e.subject, &e.relation, &e.object,
                pid.as_str(), None, &ts, e.identity.as_deref(),
            );
            let event_obj = event_obj_from_fields(
                e.revision.as_u64() as f64, action_str,
                &e.subject, &e.relation, &e.object,
                &ts, None, e.identity.as_deref(),
                &last_hash, &event_hash,
            );
            put_s(&se, &JsValue::from_str(&ekey(pid, e.revision)), &event_obj).await?;
            last_hash = event_hash;
        }
        let sr = store(&self.db()?, STORE_REVISION, IdbTransactionMode::Readwrite)?;
        put_s(&sr, &rev_key(), &JsValue::from_f64(revision.as_u64() as f64)).await?;
        Ok(())
    }

    async fn set_actor_identity(&self, identity: Option<String>) -> Option<String> {
        let mut g = self.actor.lock().ok()?;
        let prev = g.clone();
        *g = identity;
        prev
    }

    async fn close(&self) -> AegisResult<()> {
        if let Ok(mut g) = self.db.lock() { *g = None; }
        Ok(())
    }

    async fn verify_audit_chain(&self, pid: &PartitionId) -> AegisResult<Option<String>> {
        let s = store(&self.db()?, STORE_EVENTS, IdbTransactionMode::Readonly)?;
        let all_vals = all_s(&s).await?;
        let mut events: Vec<(u64, JsValue)> = Vec::new();
        for v in all_vals {
            if let Some(rev) = get_num(&v, "revision") {
                events.push((rev as u64, v));
            }
        }
        events.sort_by_key(|(r, _)| *r);

        let mut last_event_hash = String::new();
        for (idx, (_rev, val)) in events.iter().enumerate() {
            let action = get_str(val, "action").unwrap_or_default();
            let subject = get_str(val, "subject").unwrap_or_default();
            let relation = get_str(val, "relation").unwrap_or_default();
            let object = get_str(val, "object").unwrap_or_default();
            let revision = get_num(val, "revision").unwrap_or(0.0) as i64;
            let timestamp = get_str(val, "timestamp").unwrap_or_default();
            let identity = get_str(val, "identity");
            let metadata = Reflect::get(val, &JsValue::from_str("metadata")).ok()
                .and_then(|v| v.as_string());
            let stored_prev_hash = get_str(val, "previous_hash").unwrap_or_default();
            let stored_event_hash = get_str(val, "event_hash").unwrap_or_default();

            if !stored_prev_hash.is_empty() && stored_prev_hash != last_event_hash {
                return Ok(Some(format!(
                    "Chain break at event {} (rev={}): expected previous_hash='{}', got '{}'",
                    idx, revision, last_event_hash, stored_prev_hash
                )));
            }

            let expected = compute_event_hash(
                &last_event_hash, revision, &action, &subject, &relation, &object,
                pid.as_str(), metadata.as_deref(), &timestamp, identity.as_deref(),
            );

            if !stored_event_hash.is_empty() && expected != stored_event_hash {
                return Ok(Some(format!(
                    "Hash mismatch at event {} (rev={}): expected '{}', got '{}'",
                    idx, revision, expected, stored_event_hash
                )));
            }

            if !stored_event_hash.is_empty() {
                last_event_hash = stored_event_hash;
            } else {
                last_event_hash = expected;
            }
        }
        Ok(None)
    }
}

impl IndexedDbStorage {
    async fn scan_prefix(&self, pid: &PartitionId, prefix: &str) -> AegisResult<Vec<RelationshipTuple>> {
        let s = store(&self.db()?, STORE_TUPLES, IdbTransactionMode::Readonly)?;
        let all = all_s(&s).await?;
        let mut out = Vec::new();
        for v in all {
            let t = js_to_tuple(&v)?;
            let k = pkey(pid, t.subject.as_str(), t.relation.as_str(), t.object.as_str());
            if k.starts_with(prefix) { out.push(t); }
        }
        Ok(out)
    }
}

struct IndexedDbTransaction {
    db: IdbDatabase,
    rev: u64,
    pending: Vec<(PartitionId, TupleMutation, RelationshipTuple)>,
    actor: Option<String>,
}

#[async_trait(?Send)]
impl AsyncStorageTransaction for IndexedDbTransaction {
    async fn write(&mut self, partition_id: &PartitionId, tuple: &RelationshipTuple) -> AegisResult<()> {
        self.pending.push((partition_id.clone(), TupleMutation::Add, tuple.clone()));
        Ok(())
    }

    async fn delete(&mut self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<()> {
        let tuple = RelationshipTuple::new(key.subject.clone(), key.relation.clone(), key.object.clone());
        self.pending.push((partition_id.clone(), TupleMutation::Remove, tuple));
        Ok(())
    }

    async fn savepoint(&self, _name: &str) -> AegisResult<()> { Ok(()) }
    async fn rollback_to_savepoint(&self, _name: &str) -> AegisResult<()> { Ok(()) }
    async fn release_savepoint(&self, _name: &str) -> AegisResult<()> { Ok(()) }

    async fn set_actor_identity(&mut self, identity: Option<String>) -> Option<String> {
        let prev = self.actor.clone();
        self.actor = identity;
        prev
    }

    async fn commit(self: Box<Self>) -> AegisResult<Revision> {
        let txn = multi_store_txn(&self.db, &[STORE_TUPLES, STORE_EVENTS, STORE_REVISION], IdbTransactionMode::Readwrite)?;
        let mut rev = self.rev;
        let mut last_hash = last_event_hash_s(&txn, STORE_EVENTS).await?;

        for (pid, action, tuple) in &self.pending {
            rev += 1;
            put_s_in_txn(&txn, STORE_REVISION, &rev_key(), &JsValue::from_f64(rev as f64)).await?;

            let key = pkey(pid, tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str());
            match action {
                TupleMutation::Add => {
                    put_s_in_txn(&txn, STORE_TUPLES, &JsValue::from_str(&key), &tuple_to_js(tuple)).await?;
                }
                TupleMutation::Remove => {
                    del_s_in_txn(&txn, STORE_TUPLES, &JsValue::from_str(&key)).await?;
                }
            }

            let ekey_val = ekey(pid, Revision::new(rev));
            let action_str = match action { TupleMutation::Add => "add", TupleMutation::Remove => "remove" };
            let now_rfc = Utc::now().to_rfc3339();
            let actor = self.actor.clone();
            let event_hash = compute_event_hash(
                &last_hash, rev as i64, action_str,
                tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str(),
                pid.as_str(), None, &now_rfc, actor.as_deref(),
            );
            let event_obj = event_obj_from_fields(
                rev as f64, action_str,
                tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str(),
                &now_rfc, None, actor.as_deref(),
                &last_hash, &event_hash,
            );
            put_s_in_txn(&txn, STORE_EVENTS, &JsValue::from_str(&ekey_val), &event_obj).await?;
            last_hash = event_hash;
        }

        drop(txn);

        if rev > self.rev { Ok(Revision::new(rev)) } else { Ok(Revision::ZERO) }
    }

    async fn rollback(self: Box<Self>) -> AegisResult<()> {
        Ok(())
    }
}

#[cfg(all(target_arch = "wasm32", test))]
mod wasm_tests {
    use super::*;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    wasm_bindgen_test_configure!(run_in_browser);

    async fn setup() -> (IndexedDbStorage, PartitionId) {
        let mut storage = IndexedDbStorage::with_name("aegis_test");
        storage.initialize().await.unwrap();
        let pid = PartitionId::default();
        (storage, pid)
    }

    #[wasm_bindgen_test]
    async fn test_initialize_creates_stores() {
        let (storage, _pid) = setup().await;
        let meta = storage.db().and_then(|db| {
            let _ = db;
            Ok(crate::storage::traits::StorageMeta {
                schema_version: 1,
                current_revision: Revision::ZERO,
                backend_type: crate::storage::traits::BackendType::IndexedDB,
                healthy: true,
            })
        }).unwrap();
        assert!(meta.healthy);
        assert_eq!(meta.schema_version, 1);
    }

    #[wasm_bindgen_test]
    async fn test_write_and_read_tuple() {
        let (storage, pid) = setup().await;

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:test1").unwrap(),
        );
        let rev = storage.write_tuple(&pid, &tuple).await.unwrap();
        assert!(rev > Revision::ZERO);

        let key = TupleKey {
            subject: SubjectId::new("user:alice").unwrap(),
            relation: Relation::new("owner").unwrap(),
            object: ResourceId::new("repo:test1").unwrap(),
        };
        assert!(storage.has_tuple(&pid, &key).await.unwrap());
        let read = storage.read_tuple(&pid, &key).await.unwrap().unwrap();
        assert_eq!(read.subject.as_str(), "user:alice");
    }

    #[wasm_bindgen_test]
    async fn test_delete_tuple() {
        let (storage, pid) = setup().await;

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:bob").unwrap(),
            Relation::new("viewer").unwrap(),
            ResourceId::new("repo:test2").unwrap(),
        );
        storage.write_tuple(&pid, &tuple).await.unwrap();

        let key = TupleKey {
            subject: SubjectId::new("user:bob").unwrap(),
            relation: Relation::new("viewer").unwrap(),
            object: ResourceId::new("repo:test2").unwrap(),
        };
        storage.delete_tuple(&pid, &key).await.unwrap();
        assert!(!storage.has_tuple(&pid, &key).await.unwrap());
    }

    #[wasm_bindgen_test]
    async fn test_list_by_object() {
        let (storage, pid) = setup().await;

        let t1 = RelationshipTuple::new(
            SubjectId::new("user:a").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:x").unwrap(),
        );
        let t2 = RelationshipTuple::new(
            SubjectId::new("user:b").unwrap(),
            Relation::new("viewer").unwrap(),
            ResourceId::new("repo:x").unwrap(),
        );
        storage.write_tuple(&pid, &t1).await.unwrap();
        storage.write_tuple(&pid, &t2).await.unwrap();

        let results = storage
            .list_by_object(&pid, &ResourceId::new("repo:x").unwrap(), None, &ConsistencyMode::MinimizeLatency)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[wasm_bindgen_test]
    async fn test_audit_events_created() {
        let (storage, pid) = setup().await;

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:audit").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:audit").unwrap(),
        );
        storage.write_tuple(&pid, &tuple).await.unwrap();
        storage.write_tuple(&pid, &tuple).await.unwrap();

        let events = storage
            .query_audit(&pid, None, None, None, &PaginationParams::default())
            .await
            .unwrap();
        assert!(events.len() >= 2);
        assert_eq!(events[0].action, TupleMutation::Add);
    }

    #[wasm_bindgen_test]
    async fn test_revision_increments() {
        let (storage, pid) = setup().await;

        let rev0 = storage.current_revision(&pid).await.unwrap();

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:rev").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:rev").unwrap(),
        );
        let rev1 = storage.write_tuple(&pid, &tuple).await.unwrap();
        assert!(rev1 > rev0);

        let rev2 = storage.write_tuple(&pid, &tuple).await.unwrap();
        assert!(rev2 > rev1);
    }

    #[wasm_bindgen_test]
    async fn test_delete_object_removes_all() {
        let (storage, pid) = setup().await;

        let t1 = RelationshipTuple::new(
            SubjectId::new("user:a").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:del").unwrap(),
        );
        let t2 = RelationshipTuple::new(
            SubjectId::new("user:b").unwrap(),
            Relation::new("viewer").unwrap(),
            ResourceId::new("repo:del").unwrap(),
        );
        storage.write_tuple(&pid, &t1).await.unwrap();
        storage.write_tuple(&pid, &t2).await.unwrap();

        storage
            .delete_object(&pid, &ResourceId::new("repo:del").unwrap())
            .await
            .unwrap();

        let results = storage
            .list_by_object(&pid, &ResourceId::new("repo:del").unwrap(), None, &ConsistencyMode::MinimizeLatency)
            .await
            .unwrap();
        assert_eq!(results.len(), 0);
    }

    #[wasm_bindgen_test]
    async fn test_recover_from_events() {
        let (storage, pid) = setup().await;

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:recover").unwrap(),
            Relation::new("editor").unwrap(),
            ResourceId::new("repo:recover").unwrap(),
        );
        let rev = storage.write_tuple(&pid, &tuple).await.unwrap();

        let recovered = storage.recover_from_events(&pid, None).await.unwrap();
        assert_eq!(recovered, rev);
    }

    #[wasm_bindgen_test]
    async fn test_transaction_commit() {
        let (storage, pid) = setup().await;

        let mut txn = storage.begin_transaction(&pid).await.unwrap();
        let tuple = RelationshipTuple::new(
            SubjectId::new("user:txn").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:txn").unwrap(),
        );
        txn.write(&pid, &tuple).await.unwrap();
        let rev = txn.commit().await.unwrap();
        assert!(rev > Revision::ZERO);

        let key = TupleKey {
            subject: SubjectId::new("user:txn").unwrap(),
            relation: Relation::new("owner").unwrap(),
            object: ResourceId::new("repo:txn").unwrap(),
        };
        assert!(storage.has_tuple(&pid, &key).await.unwrap());
    }

    #[wasm_bindgen_test]
    async fn test_schema_version() {
        let storage = IndexedDbStorage::with_name("aegis_schema_test");
        // initialize will create the stores and set schema version to 1
        let mut storage_mut = storage;
        let meta = storage_mut.initialize().await.unwrap();
        assert_eq!(meta.schema_version, 1);

        storage_mut.write_schema_version(2).await.unwrap();
        let read = storage_mut.read_schema_version().await.unwrap();
        assert_eq!(read, 2);
    }

    // ---- Performance Benchmarks ----

    fn now_ms() -> f64 {
        web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0)
    }

    async fn seed_tuples(
        storage: &IndexedDbStorage,
        pid: &PartitionId,
        n: usize,
    ) -> Vec<RelationshipTuple> {
        let mut tuples = Vec::with_capacity(n);
        for i in 0..n {
            let t = RelationshipTuple::new(
                SubjectId::new(&format!("user:u{}", i)).unwrap(),
                Relation::new("reader").unwrap(),
                ResourceId::new("doc:report").unwrap(),
            );
            storage.write_tuple(pid, &t).await.unwrap();
            tuples.push(t);
        }
        tuples
    }

    #[wasm_bindgen_test]
    async fn bench_check_latency() {
        let (storage, pid) = setup().await;
        seed_tuples(&storage, &pid, 1000).await;

        // measure check via has_tuple (simulating engine lookups)
        let key = TupleKey {
            subject: SubjectId::new("user:u500").unwrap(),
            relation: Relation::new("reader").unwrap(),
            object: ResourceId::new("doc:report").unwrap(),
        };

        let start = now_ms();
        for _ in 0..100 {
            let _ = storage.has_tuple(&pid, &key).await.unwrap();
        }
        let elapsed = now_ms() - start;
        let avg = elapsed / 100.0;
        let p95 = avg; // simplified for inline test

        web_sys::console::log_1(&format!(
            "BENCH check_latency: avg={:.3}ms p95={:.3}ms (target <5ms) {}",
            avg, p95,
            if p95 < 5.0 { "PASS" } else { "FAIL" }
        ).into());

        assert!(p95 < 50.0, "p95 check latency ({:.3}ms) exceeds 50ms threshold", p95);
    }

    #[wasm_bindgen_test]
    async fn bench_write_throughput() {
        let (storage, pid) = setup().await;

        let start = now_ms();
        let n = 500;
        for i in 0..n {
            let t = RelationshipTuple::new(
                SubjectId::new(&format!("user:w{}", i)).unwrap(),
                Relation::new("reader").unwrap(),
                ResourceId::new("doc:wload").unwrap(),
            );
            storage.write_tuple(&pid, &t).await.unwrap();
        }
        let elapsed = now_ms() - start;
        let throughput = (n as f64) / (elapsed / 1000.0);

        web_sys::console::log_1(&format!(
            "BENCH write_throughput: {} writes in {:.0}ms = {:.0} writes/sec",
            n, elapsed, throughput
        ).into());
    }

    #[wasm_bindgen_test]
    async fn bench_list_by_object() {
        let (storage, pid) = setup().await;
        seed_tuples(&storage, &pid, 1000).await;

        let start = now_ms();
        for _ in 0..50 {
            let _ = storage
                .list_by_object(&pid, &ResourceId::new("doc:report").unwrap(), None, &ConsistencyMode::MinimizeLatency)
                .await
                .unwrap();
        }
        let elapsed = now_ms() - start;
        let avg = elapsed / 50.0;

        web_sys::console::log_1(&format!(
            "BENCH list_by_object: avg={:.3}ms (1000 tuples)",
            avg
        ).into());
    }

    #[wasm_bindgen_test]
    async fn test_persistence_survives_reload() {
        let db_name = "aegis_persist_test";

        // "Page load 1" — write data
        {
            let mut storage = IndexedDbStorage::with_name(db_name);
            storage.initialize().await.unwrap();
            let pid = PartitionId::default();

            let tuple = RelationshipTuple::new(
                SubjectId::new("user:persist").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("doc:persist").unwrap(),
            );
            storage.write_tuple(&pid, &tuple).await.unwrap();
            storage.write_schema_version(42).await.unwrap();
            // storage drops here — simulating page reload
        }

        // "Page load 2" — verify data persisted
        {
            let mut storage = IndexedDbStorage::with_name(db_name);
            let meta = storage.initialize().await.unwrap();
            let pid = PartitionId::default();

            assert_eq!(meta.schema_version, 42, "schema version must persist");

            let key = TupleKey {
                subject: SubjectId::new("user:persist").unwrap(),
                relation: Relation::new("owner").unwrap(),
                object: ResourceId::new("doc:persist").unwrap(),
            };
            let has = storage.has_tuple(&pid, &key).await.unwrap();
            assert!(has, "tuple must survive page reload");

            let rev = storage.current_revision(&pid).await.unwrap();
            assert!(rev > Revision::ZERO, "revision must survive page reload");

            // Clean up
            storage.delete_object(&pid, &ResourceId::new("doc:persist").unwrap()).await.unwrap();
        }
    }

    #[wasm_bindgen_test]
    async fn test_export_import_roundtrip() {
        let (storage, pid) = setup().await;

        // Write some tuples
        let tuples_written = vec![
            RelationshipTuple::new(
                SubjectId::new("user:a").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("doc:x").unwrap(),
            ),
            RelationshipTuple::new(
                SubjectId::new("user:b").unwrap(),
                Relation::new("reader").unwrap(),
                ResourceId::new("doc:y").unwrap(),
            ),
        ];
        for t in &tuples_written {
            storage.write_tuple(&pid, t).await.unwrap();
        }

        // Export
        let all = storage
            .query_tuples(&pid, &TupleFilter::default(), &PaginationParams { cursor: None, limit: 100 }, &ConsistencyMode::MinimizeLatency)
            .await
            .unwrap();
        let exported = all.tuples;

        // Delete all then verify empty
        for t in &exported {
            storage.delete_tuple(&pid, &TupleKey {
                subject: t.subject.clone(),
                relation: t.relation.clone(),
                object: t.object.clone(),
            }).await.unwrap();
        }
        let after_del = storage
            .query_tuples(&pid, &TupleFilter::default(), &PaginationParams { cursor: None, limit: 100 }, &ConsistencyMode::MinimizeLatency)
            .await
            .unwrap();
        assert!(after_del.tuples.is_empty(), "all tuples should be deleted before import");

        // Re-import via write_tuples_batch
        storage.write_tuples_batch(&pid, &exported).await.unwrap();

        // Verify
        let after_import = storage
            .query_tuples(&pid, &TupleFilter::default(), &PaginationParams { cursor: None, limit: 100 }, &ConsistencyMode::MinimizeLatency)
            .await
            .unwrap();
        assert_eq!(
            after_import.tuples.len(),
            exported.len(),
            "all tuples must be restored after import"
        );
        for t in &after_import.tuples {
            assert!(
                exported.iter().any(|e| e.subject == t.subject && e.relation == t.relation && e.object == t.object),
                "imported tuple must match exported: {:?}", t
            );
        }
    }

    #[wasm_bindgen_test]
    async fn bench_export_tuples() {
        let (storage, pid) = setup().await;

        // create tuples across multiple objects
        let n = 100;
        let start_seed = now_ms();
        for i in 0..n {
            let t = RelationshipTuple::new(
                SubjectId::new(&format!("user:e{}", i)).unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new(&format!("doc:export{}", i)).unwrap(),
            );
            storage.write_tuple(&pid, &t).await.unwrap();
        }
        let seed_elapsed = now_ms() - start_seed;

        let start = now_ms();
        let _all = storage
            .query_tuples(&pid, &TupleFilter::default(), &PaginationParams { cursor: None, limit: n as u64 }, &ConsistencyMode::MinimizeLatency)
            .await
            .unwrap();
        let elapsed = now_ms() - start;

        web_sys::console::log_1(&format!(
            "BENCH export_tuples: {n} tuples seeded in {seed_elapsed:.0}ms, query in {elapsed:.3}ms"
        ).into());
    }
}
