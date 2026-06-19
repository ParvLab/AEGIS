#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::{CStr, CString};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use aegis_core::engine::GraphEngine;
use aegis_core::engine::hooks;
use aegis_core::engine::ratelimit::{RateLimitConfig, TokenBucketRateLimiter};
use aegis_core::engine::watch::{WatchEventType, WatchFilter, WatchSubscription};
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::{StorageBackend, StorageTransaction};
use aegis_core::types::*;

/// Opaque handle to the engine.
pub struct AegisEngine {
    inner: GraphEngine,
}

// ── C-compatible result structs ──

#[repr(C)]
pub struct AegisWriteResult {
    pub revision: u64,
    pub error: *mut libc::c_char,
}

#[repr(C)]
pub struct AegisCheckResult {
    pub allowed: bool,
    pub revision: u64,
    pub error: *mut libc::c_char,
}

#[repr(C)]
pub struct AegisHealthResult {
    pub healthy: bool,
    pub revision: u64,
    pub schema_version: i32,
    pub error: *mut libc::c_char,
}

// ── Engine lifecycle ──

fn create_engine_base(
    path: &str,
    yaml: &str,
    config_json: *const libc::c_char,
) -> Result<*mut AegisEngine, String> {
    let mut cfg = SqliteConfig {
        path: path.to_string(),
        max_readers: 4,
        busy_timeout_ms: 5000,
        wal_mode: true,
        mmap_size: 0,
    };

    if !config_json.is_null() {
        let json_str = unsafe { CStr::from_ptr(config_json) }
            .to_str()
            .map_err(|e| e.to_string())?;
        #[derive(serde::Deserialize)]
        struct CfgOverride {
            max_readers: Option<u32>,
            busy_timeout_ms: Option<u32>,
            wal_mode: Option<bool>,
            mmap_size: Option<u64>,
        }
        if let Ok(overrides) = serde_json::from_str::<CfgOverride>(json_str) {
            if let Some(v) = overrides.max_readers {
                cfg.max_readers = v;
            }
            if let Some(v) = overrides.busy_timeout_ms {
                cfg.busy_timeout_ms = v;
            }
            if let Some(v) = overrides.wal_mode {
                cfg.wal_mode = v;
            }
            if let Some(v) = overrides.mmap_size {
                cfg.mmap_size = v;
            }
        }
    }

    let mut storage = SqliteStorage::new(cfg).map_err(|e| e.to_string())?;
    storage.initialize().map_err(|e| e.to_string())?;
    let schema = parse_schema(yaml).map_err(|e| e.to_string())?;
    let engine = GraphEngine::new(Box::new(storage), schema);
    Ok(Box::into_raw(Box::new(AegisEngine { inner: engine })))
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_create(
    db_path: *const libc::c_char,
    schema_yaml: *const libc::c_char,
) -> *mut AegisEngine {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<*mut AegisEngine, String> {
        if db_path.is_null() || schema_yaml.is_null() {
            return Err("null pointer argument".to_string());
        }
        let path = unsafe { CStr::from_ptr(db_path) }
            .to_str()
            .map_err(|e| e.to_string())?;
        let yaml = unsafe { CStr::from_ptr(schema_yaml) }
            .to_str()
            .map_err(|e| e.to_string())?;
        create_engine_base(path, yaml, std::ptr::null())
    }));

    match result {
        Ok(Ok(ptr)) => ptr,
        Ok(Err(e)) => {
            eprintln!("aegis_engine_create error: {}", e);
            std::ptr::null_mut()
        }
        Err(panic) => {
            eprintln!("aegis_engine_create panic: {:?}", panic);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_create_with_config(
    db_path: *const libc::c_char,
    schema_yaml: *const libc::c_char,
    config_json: *const libc::c_char,
) -> *mut AegisEngine {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<*mut AegisEngine, String> {
        if db_path.is_null() || schema_yaml.is_null() {
            return Err("null pointer argument".to_string());
        }
        let path = unsafe { CStr::from_ptr(db_path) }
            .to_str()
            .map_err(|e| e.to_string())?;
        let yaml = unsafe { CStr::from_ptr(schema_yaml) }
            .to_str()
            .map_err(|e| e.to_string())?;
        create_engine_base(path, yaml, config_json)
    }));

    match result {
        Ok(Ok(ptr)) => ptr,
        Ok(Err(e)) => {
            eprintln!("aegis_engine_create_with_config error: {}", e);
            std::ptr::null_mut()
        }
        Err(panic) => {
            eprintln!("aegis_engine_create_with_config panic: {:?}", panic);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_destroy(engine: *mut AegisEngine) {
    if !engine.is_null() {
        let _ = unsafe { Box::from_raw(engine) };
    }
}

// ── Core operations ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_check(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
    permission: *const libc::c_char,
    resource: *const libc::c_char,
) -> AegisCheckResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisCheckResult {
                allowed: false,
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisCheckResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let permission_str = c_str_to_str(permission)?;
            let resource_str = c_str_to_str(resource)?;

            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let resource_id =
                ResourceId::new(&resource_str).map_err(|e| error_string(&e.to_string()))?;

            let result = eng
                .check(&subject_id, &permission_str, &resource_id, None)
                .map_err(|e| error_string(&e.to_string()))?;

            Ok(AegisCheckResult {
                allowed: result.allowed,
                revision: result.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisCheckResult {
            allowed: false,
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisCheckResult {
                allowed: false,
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_write(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
    relation: *const libc::c_char,
    resource: *const libc::c_char,
) -> AegisWriteResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisWriteResult {
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisWriteResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let relation_str = c_str_to_str(relation)?;
            let resource_str = c_str_to_str(resource)?;

            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let relation_id =
                Relation::new(&relation_str).map_err(|e| error_string(&e.to_string()))?;
            let resource_id =
                ResourceId::new(&resource_str).map_err(|e| error_string(&e.to_string()))?;

            let tuple = RelationshipTuple::new(subject_id, relation_id, resource_id);

            let revision_token = eng
                .write(&tuple)
                .map_err(|e| error_string(&e.to_string()))?;

            Ok(AegisWriteResult {
                revision: revision_token.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisWriteResult {
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisWriteResult {
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_delete(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
    relation: *const libc::c_char,
    resource: *const libc::c_char,
) -> AegisWriteResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisWriteResult {
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisWriteResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let relation_str = c_str_to_str(relation)?;
            let resource_str = c_str_to_str(resource)?;

            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let relation_id =
                Relation::new(&relation_str).map_err(|e| error_string(&e.to_string()))?;
            let resource_id =
                ResourceId::new(&resource_str).map_err(|e| error_string(&e.to_string()))?;

            let key = TupleKey {
                subject: subject_id,
                relation: relation_id,
                object: resource_id,
            };

            let revision_token = eng.delete(&key).map_err(|e| error_string(&e.to_string()))?;

            Ok(AegisWriteResult {
                revision: revision_token.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisWriteResult {
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisWriteResult {
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_health(engine: *mut AegisEngine) -> AegisHealthResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisHealthResult {
                healthy: false,
                revision: 0,
                schema_version: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(|| -> AegisHealthResult {
        let report = eng.health();
        AegisHealthResult {
            healthy: report.healthy,
            revision: report.revision.as_u64(),
            schema_version: report.schema_version as i32,
            error: std::ptr::null_mut(),
        }
    })) {
        Ok(res) => res,
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisHealthResult {
                healthy: false,
                revision: 0,
                schema_version: 0,
                error: error_string(&msg),
            }
        }
    }
}

#[repr(C)]
pub struct AegisExplainResult {
    pub allowed: bool,
    pub revision: u64,
    pub trace_json: *mut libc::c_char,
    pub resolved_via: *mut libc::c_char,
    pub duration_ms: u64,
    pub error: *mut libc::c_char,
}

#[repr(C)]
pub struct AegisListResult {
    pub tuples_json: *mut libc::c_char,
    pub error: *mut libc::c_char,
}

#[repr(C)]
pub struct AegisExportResult {
    pub tuples_json: *mut libc::c_char,
    pub export_revision: u64,
    pub error: *mut libc::c_char,
}

#[repr(C)]
pub struct AegisAuditResult {
    pub entries_json: *mut libc::c_char,
    pub error: *mut libc::c_char,
}

#[repr(C)]
pub struct AegisQueryResult {
    pub tuples_json: *mut libc::c_char,
    pub next_cursor: u64,
    pub revision: u64,
    pub error: *mut libc::c_char,
}

// ── Explain ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_explain(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
    permission: *const libc::c_char,
    resource: *const libc::c_char,
) -> AegisExplainResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisExplainResult {
                allowed: false,
                revision: 0,
                trace_json: std::ptr::null_mut(),
                resolved_via: std::ptr::null_mut(),
                duration_ms: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisExplainResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let permission_str = c_str_to_str(permission)?;
            let resource_str = c_str_to_str(resource)?;
            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let resource_id =
                ResourceId::new(&resource_str).map_err(|e| error_string(&e.to_string()))?;
            let result = eng
                .explain(&subject_id, &permission_str, &resource_id, None)
                .map_err(|e| error_string(&e.to_string()))?;
            let trace_json =
                serde_json::to_string(&result.trace).map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisExplainResult {
                allowed: result.allowed,
                revision: result.revision.as_u64(),
                trace_json: error_string(&trace_json),
                resolved_via: error_string(&result.resolved_via),
                duration_ms: result.duration_ms as u64,
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisExplainResult {
            allowed: false,
            revision: 0,
            trace_json: std::ptr::null_mut(),
            resolved_via: std::ptr::null_mut(),
            duration_ms: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisExplainResult {
                allowed: false,
                revision: 0,
                trace_json: std::ptr::null_mut(),
                resolved_via: std::ptr::null_mut(),
                duration_ms: 0,
                error: error_string(&msg),
            }
        }
    }
}

// ── List by object ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_list_by_object(
    engine: *mut AegisEngine,
    object: *const libc::c_char,
    relation: *const libc::c_char,
) -> AegisListResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisListResult {
                tuples_json: std::ptr::null_mut(),
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisListResult, *mut libc::c_char> {
            let object_str = c_str_to_str(object)?;
            let object_id =
                ResourceId::new(&object_str).map_err(|e| error_string(&e.to_string()))?;
            let rel = if relation.is_null() {
                None
            } else {
                let rel_str = c_str_to_str(relation)?;
                Some(Relation::new(&rel_str).map_err(|e| error_string(&e.to_string()))?)
            };
            let tuples = eng
                .list_by_object(&object_id, rel.as_ref(), None)
                .map_err(|e| error_string(&e.to_string()))?;
            let json = serde_json::to_string(&tuples.iter().map(|t| {
            serde_json::json!({"subject": t.subject.as_str(), "relation": t.relation.as_str(), "object": t.object.as_str()})
        }).collect::<Vec<_>>()).map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisListResult {
                tuples_json: error_string(&json),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisListResult {
            tuples_json: std::ptr::null_mut(),
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisListResult {
                tuples_json: std::ptr::null_mut(),
                error: error_string(&msg),
            }
        }
    }
}

// ── List by subject ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_list_by_subject(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
    relation: *const libc::c_char,
) -> AegisListResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisListResult {
                tuples_json: std::ptr::null_mut(),
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisListResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let rel = if relation.is_null() {
                None
            } else {
                let rel_str = c_str_to_str(relation)?;
                Some(Relation::new(&rel_str).map_err(|e| error_string(&e.to_string()))?)
            };
            let tuples = eng
                .list_by_subject(&subject_id, rel.as_ref(), None)
                .map_err(|e| error_string(&e.to_string()))?;
            let json = serde_json::to_string(&tuples.iter().map(|t| {
            serde_json::json!({"subject": t.subject.as_str(), "relation": t.relation.as_str(), "object": t.object.as_str()})
        }).collect::<Vec<_>>()).map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisListResult {
                tuples_json: error_string(&json),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisListResult {
            tuples_json: std::ptr::null_mut(),
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisListResult {
                tuples_json: std::ptr::null_mut(),
                error: error_string(&msg),
            }
        }
    }
}

// ── Write batch ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_write_batch(
    engine: *mut AegisEngine,
    tuples_json: *const libc::c_char,
) -> AegisWriteResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisWriteResult {
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisWriteResult, *mut libc::c_char> {
            let json_str = c_str_to_str(tuples_json)?;
            let raw: Vec<serde_json::Value> =
                serde_json::from_str(&json_str).map_err(|e| error_string(&e.to_string()))?;
            let mut tuples = Vec::new();
            for item in raw {
                let subject = item["subject"]
                    .as_str()
                    .ok_or_else(|| error_string("missing subject"))?;
                let relation = item["relation"]
                    .as_str()
                    .ok_or_else(|| error_string("missing relation"))?;
                let object = item["object"]
                    .as_str()
                    .ok_or_else(|| error_string("missing object"))?;
                tuples.push(RelationshipTuple::new(
                    SubjectId::new(subject).map_err(|e| error_string(&e.to_string()))?,
                    Relation::new(relation).map_err(|e| error_string(&e.to_string()))?,
                    ResourceId::new(object).map_err(|e| error_string(&e.to_string()))?,
                ));
            }
            let rev = eng
                .write_batch(&tuples)
                .map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisWriteResult {
                revision: rev.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisWriteResult {
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisWriteResult {
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

// ── Migrate ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_migrate(
    engine: *mut AegisEngine,
    target_version: i32,
) -> *mut libc::c_char {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => return err,
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<*mut libc::c_char, *mut libc::c_char> {
            eng.migrate(target_version as u32)
                .map_err(|e| error_string(&e.to_string()))?;
            Ok(std::ptr::null_mut())
        },
    )) {
        Ok(Ok(ptr)) => ptr,
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during migration"),
    }
}

// ── Delete object ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_delete_object(
    engine: *mut AegisEngine,
    object: *const libc::c_char,
) -> AegisWriteResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisWriteResult {
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisWriteResult, *mut libc::c_char> {
            let object_str = c_str_to_str(object)?;
            let object_id =
                ResourceId::new(&object_str).map_err(|e| error_string(&e.to_string()))?;
            let rev = eng
                .delete_object(&object_id)
                .map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisWriteResult {
                revision: rev.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisWriteResult {
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisWriteResult {
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

// ── Check schema compatibility ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_check_schema(
    engine: *mut AegisEngine,
    schema_yaml: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => return err,
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<*mut libc::c_char, *mut libc::c_char> {
            let yaml_str = c_str_to_str(schema_yaml)?;
            let new_schema = parse_schema(&yaml_str).map_err(|e| error_string(&e.to_string()))?;
            let report = eng.check_schema(&new_schema);
            let json = serde_json::to_string(&serde_json::json!({
                "compatible": report.compatible,
                "warnings": report.warnings,
                "breaking": report.breaking,
            }))
            .map_err(|e| error_string(&e.to_string()))?;
            Ok(error_string(&json))
        },
    )) {
        Ok(Ok(ptr)) => ptr,
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during check_schema"),
    }
}

fn c_consistency(consistency: i32) -> Option<ConsistencyMode> {
    match consistency {
        -1 => None,
        0 => Some(ConsistencyMode::MinimizeLatency),
        1 => Some(ConsistencyMode::FullyConsistent),
        rev => Some(ConsistencyMode::AtRevision(Revision::from(rev as u64))),
    }
}

// ── Check dry run ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_check_dry_run(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
    permission: *const libc::c_char,
    resource: *const libc::c_char,
) -> AegisCheckResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisCheckResult {
                allowed: false,
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisCheckResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let permission_str = c_str_to_str(permission)?;
            let resource_str = c_str_to_str(resource)?;
            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let resource_id =
                ResourceId::new(&resource_str).map_err(|e| error_string(&e.to_string()))?;
            let result = eng
                .check_dry_run(&subject_id, &permission_str, &resource_id, None)
                .map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisCheckResult {
                allowed: result.allowed,
                revision: result.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisCheckResult {
            allowed: false,
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisCheckResult {
                allowed: false,
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

// ── Check with consistency ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_check_ex(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
    permission: *const libc::c_char,
    resource: *const libc::c_char,
    consistency: i32,
) -> AegisCheckResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisCheckResult {
                allowed: false,
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisCheckResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let permission_str = c_str_to_str(permission)?;
            let resource_str = c_str_to_str(resource)?;
            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let resource_id =
                ResourceId::new(&resource_str).map_err(|e| error_string(&e.to_string()))?;
            let cm = c_consistency(consistency);
            let result = eng
                .check(&subject_id, &permission_str, &resource_id, cm)
                .map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisCheckResult {
                allowed: result.allowed,
                revision: result.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisCheckResult {
            allowed: false,
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisCheckResult {
                allowed: false,
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

// ── Check with ABAC context ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_check_with_context(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
    permission: *const libc::c_char,
    resource: *const libc::c_char,
    context_json: *const libc::c_char,
    consistency: i32,
) -> AegisCheckResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisCheckResult {
                allowed: false,
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisCheckResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let permission_str = c_str_to_str(permission)?;
            let resource_str = c_str_to_str(resource)?;
            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let resource_id =
                ResourceId::new(&resource_str).map_err(|e| error_string(&e.to_string()))?;
            let cm = c_consistency(consistency);

            let ctx = if context_json.is_null() {
                Default::default()
            } else {
                let json_str = c_str_to_str(context_json)?;
                #[derive(serde::Deserialize)]
                struct CtxJson {
                    #[serde(default)]
                    subject_meta: std::collections::HashMap<String, String>,
                    #[serde(default)]
                    resource_meta: std::collections::HashMap<String, String>,
                    #[serde(default)]
                    env: std::collections::HashMap<String, String>,
                }
                let parsed: CtxJson =
                    serde_json::from_str(&json_str).map_err(|e| error_string(&e.to_string()))?;
                aegis_core::engine::condition::ConditionEvalContext {
                    subject_meta: parsed.subject_meta,
                    resource_meta: parsed.resource_meta,
                    env: parsed.env,
                }
            };

            let result = eng
                .check_with_context(&subject_id, &permission_str, &resource_id, cm, ctx)
                .map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisCheckResult {
                allowed: result.allowed,
                revision: result.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisCheckResult {
            allowed: false,
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisCheckResult {
                allowed: false,
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

// ── Write extended (with condition, metadata, valid_until) ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_write_ex(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
    relation: *const libc::c_char,
    resource: *const libc::c_char,
    condition: *const libc::c_char,
    metadata_json: *const libc::c_char,
    valid_until: *const libc::c_char,
) -> AegisWriteResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisWriteResult {
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisWriteResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let relation_str = c_str_to_str(relation)?;
            let resource_str = c_str_to_str(resource)?;
            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let relation_id =
                Relation::new(&relation_str).map_err(|e| error_string(&e.to_string()))?;
            let resource_id =
                ResourceId::new(&resource_str).map_err(|e| error_string(&e.to_string()))?;

            let condition_str = if condition.is_null() {
                None
            } else {
                Some(c_str_to_str(condition)?)
            };
            let metadata = if metadata_json.is_null() {
                None
            } else {
                let json_str = c_str_to_str(metadata_json)?;
                Some(
                    serde_json::from_str::<std::collections::HashMap<String, String>>(&json_str)
                        .map_err(|e| error_string(&e.to_string()))?,
                )
            };
            let valid_until_dt = if valid_until.is_null() {
                None
            } else {
                let s = c_str_to_str(valid_until)?;
                Some(
                    chrono::DateTime::parse_from_rfc3339(&s)
                        .map_err(|e| error_string(&e.to_string()))?
                        .with_timezone(&chrono::Utc),
                )
            };

            let tuple = RelationshipTuple {
                subject: subject_id,
                relation: relation_id,
                object: resource_id,
                created_at: chrono::Utc::now(),
                metadata,
                valid_until: valid_until_dt,
                condition: condition_str,
            };
            let rev = eng
                .write(&tuple)
                .map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisWriteResult {
                revision: rev.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisWriteResult {
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisWriteResult {
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

// ── Write dry run ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_write_dry_run(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
    relation: *const libc::c_char,
    resource: *const libc::c_char,
) -> AegisCheckResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisCheckResult {
                allowed: false,
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisCheckResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let relation_str = c_str_to_str(relation)?;
            let resource_str = c_str_to_str(resource)?;
            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let relation_id =
                Relation::new(&relation_str).map_err(|e| error_string(&e.to_string()))?;
            let resource_id =
                ResourceId::new(&resource_str).map_err(|e| error_string(&e.to_string()))?;
            let tuple = RelationshipTuple::new(subject_id, relation_id, resource_id);
            let rev = eng
                .write_dry_run(&tuple)
                .map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisCheckResult {
                allowed: false,
                revision: rev.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisCheckResult {
            allowed: false,
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisCheckResult {
                allowed: false,
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

// ── Export subject (GDPR) ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_export_subject(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
) -> AegisExportResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisExportResult {
                tuples_json: std::ptr::null_mut(),
                export_revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisExportResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let tuples = eng
                .export_subject(&subject_id)
                .map_err(|e| error_string(&e.to_string()))?;
            let rev = eng
                .storage()
                .current_revision(&PartitionId::default())
                .map_err(|e| error_string(&e.to_string()))?;
            let json = serde_json::to_string(&tuples.iter().map(|t| {
            serde_json::json!({"subject": t.subject.as_str(), "relation": t.relation.as_str(), "object": t.object.as_str()})
        }).collect::<Vec<_>>()).map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisExportResult {
                tuples_json: error_string(&json),
                export_revision: rev.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisExportResult {
            tuples_json: std::ptr::null_mut(),
            export_revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisExportResult {
                tuples_json: std::ptr::null_mut(),
                export_revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

// ── Delete subject with policy (GDPR) ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_delete_subject_with_policy(
    engine: *mut AegisEngine,
    subject: *const libc::c_char,
    policy: *const libc::c_char,
    transfer_to_subject: *const libc::c_char,
) -> AegisWriteResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisWriteResult {
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisWriteResult, *mut libc::c_char> {
            let subject_str = c_str_to_str(subject)?;
            let policy_str = c_str_to_str(policy)?;
            let subject_id =
                SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?;
            let transfer = if transfer_to_subject.is_null() {
                None
            } else {
                let t_str = c_str_to_str(transfer_to_subject)?;
                Some(SubjectId::new(&t_str).map_err(|e| error_string(&e.to_string()))?)
            };
            let rev = eng
                .delete_subject_with_policy(&subject_id, &policy_str, transfer.as_ref())
                .map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisWriteResult {
                revision: rev.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisWriteResult {
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisWriteResult {
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

// ── Query audit ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_query_audit(
    engine: *mut AegisEngine,
    object: *const libc::c_char,
    from_revision: i64,
    to_revision: i64,
    limit: u64,
) -> AegisAuditResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisAuditResult {
                entries_json: std::ptr::null_mut(),
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisAuditResult, *mut libc::c_char> {
            let object_str = c_str_to_str(object)?;
            let object_id =
                ResourceId::new(&object_str).map_err(|e| error_string(&e.to_string()))?;
            let from = if from_revision < 0 {
                None
            } else {
                Some(Revision::from(from_revision as u64))
            };
            let to = if to_revision < 0 {
                None
            } else {
                Some(Revision::from(to_revision as u64))
            };
            let pp = PaginationParams {
                limit,
                cursor: None,
            };
            let entries = eng
                .query_audit(&object_id, from, to, &pp)
                .map_err(|e| error_string(&e.to_string()))?;
            let json = serde_json::to_string(
                &entries
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "revision": e.revision.as_u64(),
                            "action": format!("{:?}", e.action).to_lowercase(),
                            "subject": e.subject,
                            "relation": e.relation,
                            "object": e.object,
                            "timestamp": e.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                            "identity": e.identity,
                        })
                    })
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisAuditResult {
                entries_json: error_string(&json),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisAuditResult {
            entries_json: std::ptr::null_mut(),
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisAuditResult {
                entries_json: std::ptr::null_mut(),
                error: error_string(&msg),
            }
        }
    }
}

// ── List by relation ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_list_by_relation(
    engine: *mut AegisEngine,
    object: *const libc::c_char,
    relation: *const libc::c_char,
) -> AegisListResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisListResult {
                tuples_json: std::ptr::null_mut(),
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisListResult, *mut libc::c_char> {
            let object_str = c_str_to_str(object)?;
            let relation_str = c_str_to_str(relation)?;
            let object_id =
                ResourceId::new(&object_str).map_err(|e| error_string(&e.to_string()))?;
            let relation_id =
                Relation::new(&relation_str).map_err(|e| error_string(&e.to_string()))?;
            let tuples = eng
                .list_by_relation(&object_id, &relation_id)
                .map_err(|e| error_string(&e.to_string()))?;
            let json = serde_json::to_string(&tuples.iter().map(|t| {
            serde_json::json!({"subject": t.subject.as_str(), "relation": t.relation.as_str(), "object": t.object.as_str()})
        }).collect::<Vec<_>>()).map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisListResult {
                tuples_json: error_string(&json),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisListResult {
            tuples_json: std::ptr::null_mut(),
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisListResult {
                tuples_json: std::ptr::null_mut(),
                error: error_string(&msg),
            }
        }
    }
}

// ── Query ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_query(
    engine: *mut AegisEngine,
    filter_json: *const libc::c_char,
    limit: u64,
    cursor_offset: u64,
) -> AegisQueryResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisQueryResult {
                tuples_json: std::ptr::null_mut(),
                next_cursor: 0,
                revision: 0,
                error: err,
            };
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisQueryResult, *mut libc::c_char> {
            let json_str = c_str_to_str(filter_json)?;
            #[derive(serde::Deserialize)]
            struct FilterJson {
                subject_type: Option<String>,
                relation: Option<String>,
                object_type: Option<String>,
                metadata_key: Option<String>,
                metadata_value: Option<String>,
            }
            let f: FilterJson =
                serde_json::from_str(&json_str).map_err(|e| error_string(&e.to_string()))?;
            let relation = match f.relation {
                Some(r) => Some(Relation::new(&r).map_err(|e| error_string(&e.to_string()))?),
                None => None,
            };
            let tf = aegis_core::storage::TupleFilter {
                subject_type: f.subject_type,
                relation,
                object_type: f.object_type,
                metadata_key: f.metadata_key,
                metadata_value: f.metadata_value,
                ..Default::default()
            };
            let current_rev = eng
                .storage()
                .current_revision(&PartitionId::default())
                .map_err(|e| error_string(&e.to_string()))?;
            let pp = PaginationParams {
                limit,
                cursor: if cursor_offset > 0 {
                    Some(aegis_core::types::PaginationCursor {
                        offset: cursor_offset,
                        revision: current_rev,
                    })
                } else {
                    None
                },
            };
            let result = eng
                .query(&tf, &pp, None)
                .map_err(|e| error_string(&e.to_string()))?;
            let json = serde_json::to_string(&result.tuples.iter().map(|t| {
            serde_json::json!({"subject": t.subject.as_str(), "relation": t.relation.as_str(), "object": t.object.as_str()})
        }).collect::<Vec<_>>()).map_err(|e| error_string(&e.to_string()))?;
            Ok(AegisQueryResult {
                tuples_json: error_string(&json),
                next_cursor: result.next_cursor.map(|c| c.offset).unwrap_or(0),
                revision: result.revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    )) {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisQueryResult {
            tuples_json: std::ptr::null_mut(),
            next_cursor: 0,
            revision: 0,
            error: err,
        },
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            AegisQueryResult {
                tuples_json: std::ptr::null_mut(),
                next_cursor: 0,
                revision: 0,
                error: error_string(&msg),
            }
        }
    }
}

// ── Reload schema ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_reload_schema(
    engine: *mut AegisEngine,
    schema_yaml: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => return err,
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<*mut libc::c_char, *mut libc::c_char> {
            let yaml_str = c_str_to_str(schema_yaml)?;
            let new_schema = parse_schema(&yaml_str).map_err(|e| error_string(&e.to_string()))?;
            eng.reload_schema(new_schema)
                .map_err(|e| error_string(&e.to_string()))?;
            Ok(std::ptr::null_mut())
        },
    )) {
        Ok(Ok(ptr)) => ptr,
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during reload_schema"),
    }
}

// ── Close ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_close(engine: *mut AegisEngine) -> *mut libc::c_char {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => return err,
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<*mut libc::c_char, *mut libc::c_char> {
            eng.close().map_err(|e| error_string(&e.to_string()))?;
            Ok(std::ptr::null_mut())
        },
    )) {
        Ok(Ok(ptr)) => ptr,
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during close"),
    }
}

// ── Is closed ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_is_closed(engine: *const AegisEngine) -> bool {
    if engine.is_null() {
        return true;
    }
    let eng = unsafe { &*engine };
    eng.inner.is_closed()
}

// ── Rate limiter ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_set_rate_limiter(
    engine: *mut AegisEngine,
    config_json: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => return err,
    };

    let result = panic::catch_unwind(AssertUnwindSafe(
        || -> Result<*mut libc::c_char, *mut libc::c_char> {
            let json_str = c_str_to_str(config_json)?;
            #[derive(serde::Deserialize)]
            struct RlConfigJson {
                checks_per_second: Option<u32>,
                check_burst: Option<u32>,
                writes_per_second: Option<u32>,
                write_burst: Option<u32>,
                max_traversal_depth: Option<usize>,
                max_traversal_visits: Option<usize>,
                max_keys: Option<usize>,
            }
            let parsed: RlConfigJson =
                serde_json::from_str(&json_str).map_err(|e| error_string(&e.to_string()))?;

            let mut cfg = RateLimitConfig::default();
            if let Some(v) = parsed.checks_per_second {
                cfg.checks_per_second = v;
            }
            if let Some(v) = parsed.check_burst {
                cfg.check_burst = v;
            }
            if let Some(v) = parsed.writes_per_second {
                cfg.writes_per_second = v;
            }
            if let Some(v) = parsed.write_burst {
                cfg.write_burst = v;
            }
            if let Some(v) = parsed.max_traversal_depth {
                cfg.max_traversal_depth = v;
            }
            if let Some(v) = parsed.max_traversal_visits {
                cfg.max_traversal_visits = v;
            }
            if let Some(v) = parsed.max_keys {
                cfg.max_keys = v;
            }

            eng.set_rate_limiter(TokenBucketRateLimiter::new(cfg));
            Ok(std::ptr::null_mut())
        },
    ));

    match result {
        Ok(Ok(ptr)) => ptr,
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during set_rate_limiter"),
    }
}

// ── Actor identity ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_set_actor(
    engine: *mut AegisEngine,
    actor: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => return err,
    };

    match panic::catch_unwind(AssertUnwindSafe(
        || -> Result<*mut libc::c_char, *mut libc::c_char> {
            if actor.is_null() {
                eng.set_actor(None);
            } else {
                let s = c_str_to_str(actor)?;
                eng.set_actor(Some(&s));
            }
            Ok(std::ptr::null_mut())
        },
    )) {
        Ok(Ok(ptr)) => ptr,
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during set_actor"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_active_actor(engine: *const AegisEngine) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(err) => return err,
    };

    match eng.active_actor() {
        Some(s) => error_string(&s),
        None => std::ptr::null_mut(),
    }
}

// ── Logger ──

pub type AegisLogFn = unsafe extern "C" fn(
    level: i32,
    target: *const libc::c_char,
    msg: *const libc::c_char,
    user_data: *mut libc::c_void,
);

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_set_logger(
    engine: *mut AegisEngine,
    callback: Option<AegisLogFn>,
    user_data: *mut libc::c_void,
) {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(_) => return,
    };

    match callback {
        Some(cb) => {
            let ud = user_data as usize;
            let wrapped: hooks::LoggerFn =
                Box::new(move |level: hooks::LogLevel, target: &str, msg: &str| {
                    let level_i32 = match level {
                        hooks::LogLevel::Error => 0,
                        hooks::LogLevel::Warn => 1,
                        hooks::LogLevel::Info => 2,
                        hooks::LogLevel::Debug => 3,
                        hooks::LogLevel::Trace => 4,
                    };
                    let c_target = CString::new(target).unwrap_or_default();
                    let c_msg = CString::new(msg).unwrap_or_default();
                    unsafe {
                        cb(
                            level_i32,
                            c_target.as_ptr(),
                            c_msg.as_ptr(),
                            ud as *mut libc::c_void,
                        );
                    }
                });
            eng.set_logger(wrapped);
        }
        None => {
            eng.set_logger(|_: hooks::LogLevel, _: &str, _: &str| {});
        }
    }
}

// ── Watch ──

pub struct AegisWatchSubscription {
    inner: Option<WatchSubscription>,
}

#[repr(C)]
pub struct AegisWatchEvent {
    pub event_type: i32,
    pub subject: *mut libc::c_char,
    pub relation: *mut libc::c_char,
    pub object: *mut libc::c_char,
    pub revision: u64,
    pub timestamp: *mut libc::c_char,
    pub payload: *mut libc::c_char,
    pub error: *mut libc::c_char,
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_watch(
    engine: *mut AegisEngine,
    subject_type: *const libc::c_char,
    relation: *const libc::c_char,
    object_type: *const libc::c_char,
) -> *mut AegisWatchSubscription {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(_) => return std::ptr::null_mut(),
    };

    let result = panic::catch_unwind(AssertUnwindSafe(|| -> *mut AegisWatchSubscription {
        let filter = WatchFilter {
            subjects: if subject_type.is_null() {
                None
            } else {
                Some(vec![
                    unsafe { CStr::from_ptr(subject_type) }
                        .to_str()
                        .unwrap_or_default()
                        .to_string(),
                ])
            },
            relations: if relation.is_null() {
                None
            } else {
                Some(vec![
                    unsafe { CStr::from_ptr(relation) }
                        .to_str()
                        .unwrap_or_default()
                        .to_string(),
                ])
            },
            objects: if object_type.is_null() {
                None
            } else {
                Some(vec![
                    unsafe { CStr::from_ptr(object_type) }
                        .to_str()
                        .unwrap_or_default()
                        .to_string(),
                ])
            },
            event_types: None,
        };
        let sub = eng.watch(filter);
        Box::into_raw(Box::new(AegisWatchSubscription { inner: Some(sub) }))
    }));

    match result {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_watch_poll(sub: *mut AegisWatchSubscription) -> *mut AegisWatchEvent {
    if sub.is_null() {
        return std::ptr::null_mut();
    }
    let sub = unsafe { &*sub };

    let event = match &sub.inner {
        Some(s) => match s.try_recv() {
            Ok(evt) => evt,
            Err(_) => return std::ptr::null_mut(),
        },
        None => return std::ptr::null_mut(),
    };

    let evt_type = match event.event_type {
        WatchEventType::TupleAdded => 0,
        WatchEventType::TupleRemoved => 1,
        WatchEventType::PolicyVersionCreated => 2,
        WatchEventType::PolicyRolledBack => 3,
        WatchEventType::IntegrityFinding => 4,
        WatchEventType::AnalysisCompleted => 5,
        WatchEventType::RateLimitWarning => 6,
    };

    let payload = event
        .payload
        .as_ref()
        .map(|v| error_string(&v.to_string()))
        .unwrap_or(std::ptr::null_mut());

    #[allow(clippy::let_and_return)]
    let ptr = Box::into_raw(Box::new(AegisWatchEvent {
        event_type: evt_type,
        subject: error_string(&event.subject),
        relation: error_string(&event.relation),
        object: error_string(&event.object),
        revision: event.revision.as_u64(),
        timestamp: error_string(&event.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()),
        payload,
        error: std::ptr::null_mut(),
    }));
    ptr
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_watch_free(sub: *mut AegisWatchSubscription) {
    if !sub.is_null() {
        let _ = unsafe { Box::from_raw(sub) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_watch_event_free(evt: *mut AegisWatchEvent) {
    if !evt.is_null() {
        let evt = unsafe { Box::from_raw(evt) };
        if !evt.subject.is_null() {
            unsafe {
                let _ = CString::from_raw(evt.subject);
            }
        }
        if !evt.relation.is_null() {
            unsafe {
                let _ = CString::from_raw(evt.relation);
            }
        }
        if !evt.object.is_null() {
            unsafe {
                let _ = CString::from_raw(evt.object);
            }
        }
        if !evt.timestamp.is_null() {
            unsafe {
                let _ = CString::from_raw(evt.timestamp);
            }
        }
        if !evt.payload.is_null() {
            unsafe {
                let _ = CString::from_raw(evt.payload);
            }
        }
        if !evt.error.is_null() {
            unsafe {
                let _ = CString::from_raw(evt.error);
            }
        }
    }
}

// ── Transaction ──

pub struct AegisTransaction {
    inner: Mutex<Option<Box<dyn StorageTransaction>>>,
    consumed: AtomicBool,
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_transaction_begin(
    engine: *mut AegisEngine,
) -> *mut AegisTransaction {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(_) => return std::ptr::null_mut(),
    };

    let result = panic::catch_unwind(AssertUnwindSafe(
        || -> Result<*mut AegisTransaction, *mut libc::c_char> {
            let txn = eng
                .transaction()
                .map_err(|e| error_string(&e.to_string()))?;
            Ok(Box::into_raw(Box::new(AegisTransaction {
                inner: Mutex::new(Some(txn)),
                consumed: AtomicBool::new(false),
            })))
        },
    ));

    match result {
        Ok(Ok(ptr)) => ptr,
        Ok(Err(err)) => {
            eprintln!("aegis_engine_transaction_begin error");
            let _ = unsafe { CString::from_raw(err) };
            std::ptr::null_mut()
        }
        Err(_) => {
            eprintln!("aegis_engine_transaction_begin panic");
            std::ptr::null_mut()
        }
    }
}

fn txn_check_open(txn: &AegisTransaction) -> Result<(), *mut libc::c_char> {
    if txn.consumed.load(Ordering::Relaxed) {
        Err(error_string("transaction already committed or rolled back"))
    } else {
        Ok(())
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_transaction_write(
    txn: *mut AegisTransaction,
    subject: *const libc::c_char,
    relation: *const libc::c_char,
    resource: *const libc::c_char,
) -> *mut libc::c_char {
    if txn.is_null() {
        return error_string("transaction is null");
    }
    let txn = unsafe { &*txn };
    if let Err(err) = txn_check_open(txn) {
        return err;
    }

    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), *mut libc::c_char> {
        let subject_str = c_str_to_str(subject)?;
        let relation_str = c_str_to_str(relation)?;
        let resource_str = c_str_to_str(resource)?;
        let tuple = RelationshipTuple::new(
            SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?,
            Relation::new(&relation_str).map_err(|e| error_string(&e.to_string()))?,
            ResourceId::new(&resource_str).map_err(|e| error_string(&e.to_string()))?,
        );
        let mut guard = txn.inner.lock().map_err(|e| error_string(&e.to_string()))?;
        let inner = guard
            .as_mut()
            .ok_or_else(|| error_string("transaction not initialized"))?;
        inner
            .write(&PartitionId::default(), &tuple)
            .map_err(|e| error_string(&e.to_string()))?;
        Ok(())
    }));

    match result {
        Ok(Ok(())) => std::ptr::null_mut(),
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during transaction_write"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_transaction_delete(
    txn: *mut AegisTransaction,
    subject: *const libc::c_char,
    relation: *const libc::c_char,
    resource: *const libc::c_char,
) -> *mut libc::c_char {
    if txn.is_null() {
        return error_string("transaction is null");
    }
    let txn = unsafe { &*txn };
    if let Err(err) = txn_check_open(txn) {
        return err;
    }

    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), *mut libc::c_char> {
        let subject_str = c_str_to_str(subject)?;
        let relation_str = c_str_to_str(relation)?;
        let resource_str = c_str_to_str(resource)?;
        let key = TupleKey {
            subject: SubjectId::new(&subject_str).map_err(|e| error_string(&e.to_string()))?,
            relation: Relation::new(&relation_str).map_err(|e| error_string(&e.to_string()))?,
            object: ResourceId::new(&resource_str).map_err(|e| error_string(&e.to_string()))?,
        };
        let mut guard = txn.inner.lock().map_err(|e| error_string(&e.to_string()))?;
        let inner = guard
            .as_mut()
            .ok_or_else(|| error_string("transaction not initialized"))?;
        inner
            .delete(&PartitionId::default(), &key)
            .map_err(|e| error_string(&e.to_string()))?;
        Ok(())
    }));

    match result {
        Ok(Ok(())) => std::ptr::null_mut(),
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during transaction_delete"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_transaction_savepoint(
    txn: *mut AegisTransaction,
    name: *const libc::c_char,
) -> *mut libc::c_char {
    if txn.is_null() {
        return error_string("transaction is null");
    }
    let txn = unsafe { &*txn };
    if let Err(err) = txn_check_open(txn) {
        return err;
    }

    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), *mut libc::c_char> {
        let name_str = c_str_to_str(name)?;
        let guard = txn.inner.lock().map_err(|e| error_string(&e.to_string()))?;
        let inner = guard
            .as_ref()
            .ok_or_else(|| error_string("transaction not initialized"))?;
        inner
            .savepoint(&name_str)
            .map_err(|e| error_string(&e.to_string()))?;
        Ok(())
    }));

    match result {
        Ok(Ok(())) => std::ptr::null_mut(),
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during transaction_savepoint"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_transaction_rollback_to_savepoint(
    txn: *mut AegisTransaction,
    name: *const libc::c_char,
) -> *mut libc::c_char {
    if txn.is_null() {
        return error_string("transaction is null");
    }
    let txn = unsafe { &*txn };
    if let Err(err) = txn_check_open(txn) {
        return err;
    }

    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), *mut libc::c_char> {
        let name_str = c_str_to_str(name)?;
        let guard = txn.inner.lock().map_err(|e| error_string(&e.to_string()))?;
        let inner = guard
            .as_ref()
            .ok_or_else(|| error_string("transaction not initialized"))?;
        inner
            .rollback_to_savepoint(&name_str)
            .map_err(|e| error_string(&e.to_string()))?;
        Ok(())
    }));

    match result {
        Ok(Ok(())) => std::ptr::null_mut(),
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during transaction_rollback_to_savepoint"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_transaction_release_savepoint(
    txn: *mut AegisTransaction,
    name: *const libc::c_char,
) -> *mut libc::c_char {
    if txn.is_null() {
        return error_string("transaction is null");
    }
    let txn = unsafe { &*txn };
    if let Err(err) = txn_check_open(txn) {
        return err;
    }

    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), *mut libc::c_char> {
        let name_str = c_str_to_str(name)?;
        let guard = txn.inner.lock().map_err(|e| error_string(&e.to_string()))?;
        let inner = guard
            .as_ref()
            .ok_or_else(|| error_string("transaction not initialized"))?;
        inner
            .release_savepoint(&name_str)
            .map_err(|e| error_string(&e.to_string()))?;
        Ok(())
    }));

    match result {
        Ok(Ok(())) => std::ptr::null_mut(),
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during transaction_release_savepoint"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_transaction_commit(txn: *mut AegisTransaction) -> AegisWriteResult {
    if txn.is_null() {
        return AegisWriteResult {
            revision: 0,
            error: error_string("transaction is null"),
        };
    }
    let txn = unsafe { &*txn };
    if let Err(err) = txn_check_open(txn) {
        return AegisWriteResult {
            revision: 0,
            error: err,
        };
    }

    let result = panic::catch_unwind(AssertUnwindSafe(
        || -> Result<AegisWriteResult, *mut libc::c_char> {
            let mut guard = txn.inner.lock().map_err(|e| error_string(&e.to_string()))?;
            let inner = guard
                .take()
                .ok_or_else(|| error_string("transaction not initialized"))?;
            let revision = inner.commit().map_err(|e| error_string(&e.to_string()))?;
            txn.consumed.store(true, Ordering::Relaxed);
            Ok(AegisWriteResult {
                revision: revision.as_u64(),
                error: std::ptr::null_mut(),
            })
        },
    ));

    match result {
        Ok(Ok(res)) => res,
        Ok(Err(err)) => AegisWriteResult {
            revision: 0,
            error: err,
        },
        Err(_) => AegisWriteResult {
            revision: 0,
            error: error_string("panic during transaction_commit"),
        },
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_transaction_rollback(txn: *mut AegisTransaction) -> *mut libc::c_char {
    if txn.is_null() {
        return error_string("transaction is null");
    }
    let txn = unsafe { &*txn };
    if let Err(err) = txn_check_open(txn) {
        return err;
    }

    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), *mut libc::c_char> {
        let mut guard = txn.inner.lock().map_err(|e| error_string(&e.to_string()))?;
        let inner = guard
            .take()
            .ok_or_else(|| error_string("transaction not initialized"))?;
        inner.rollback().map_err(|e| error_string(&e.to_string()))?;
        txn.consumed.store(true, Ordering::Relaxed);
        Ok(())
    }));

    match result {
        Ok(Ok(())) => std::ptr::null_mut(),
        Ok(Err(err)) => err,
        Err(_) => error_string("panic during transaction_rollback"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_transaction_free(txn: *mut AegisTransaction) {
    if !txn.is_null() {
        let txn = unsafe { Box::from_raw(txn) };
        if !txn.consumed.load(Ordering::Relaxed) && let Ok(mut guard) = txn.inner.lock() && let Some(inner) = guard.take() {
            let _ = inner.rollback();
        }
    }
}

// ── Error string free ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_free_string(s: *mut libc::c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

// ── V6 Analysis APIs (return JSON strings) ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_explain_v2(
    engine: *const AegisEngine,
    subject: *const libc::c_char,
    permission: *const libc::c_char,
    resource: *const libc::c_char,
    consistency: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let subject_str = match c_str_to_str(subject) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let permission_str = match c_str_to_str(permission) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let resource_str = match c_str_to_str(resource) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let cm: Option<ConsistencyMode> = if consistency.is_null() {
        None
    } else {
        match c_str_to_str(consistency) {
            Ok(s) => match s.as_str() {
                "minimize_latency" => Some(ConsistencyMode::MinimizeLatency),
                "fully_consistent" => Some(ConsistencyMode::FullyConsistent),
                _ => None,
            },
            Err(_) => None,
        }
    };
    let subject_id = match SubjectId::new(&subject_str) {
        Ok(s) => s,
        Err(e) => return error_string(&e.to_string()),
    };
    let resource_id = match ResourceId::new(&resource_str) {
        Ok(r) => r,
        Err(e) => return error_string(&e.to_string()),
    };
    match eng.explain_v2(&subject_id, &permission_str, &resource_id, cm) {
        Ok(result) => {
            let json = serde_json::to_string(&result).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_who_can_access(
    engine: *const AegisEngine,
    permission: *const libc::c_char,
    resource: *const libc::c_char,
    page_offset: u64,
    page_limit: u64,
    include_paths: bool,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let permission_str = match c_str_to_str(permission) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let resource_str = match c_str_to_str(resource) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let resource_id = match ResourceId::new(&resource_str) {
        Ok(r) => r,
        Err(e) => return error_string(&e.to_string()),
    };
    let pagination = PaginationParams {
        limit: page_limit,
        cursor: Some(PaginationCursor {
            offset: page_offset,
            revision: Revision::from(0),
        }),
    };
    match eng.who_can_access(
        &permission_str,
        &resource_id,
        &pagination,
        include_paths,
        10,
        5000,
    ) {
        Ok(result) => {
            let json = serde_json::to_string(&result).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_access_diff(
    engine: *const AegisEngine,
    schema_before_json: *const libc::c_char,
    schema_after_json: *const libc::c_char,
    max_checks: i64,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let before_str = match c_str_to_str(schema_before_json) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let after_str = match c_str_to_str(schema_after_json) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let schema_before: Schema = match serde_json::from_str(&before_str) {
        Ok(s) => s,
        Err(e) => return error_string(&e.to_string()),
    };
    let schema_after: Schema = match serde_json::from_str(&after_str) {
        Ok(s) => s,
        Err(e) => return error_string(&e.to_string()),
    };
    let mc = if max_checks > 0 {
        Some(max_checks as u64)
    } else {
        None
    };
    match eng.access_diff(&schema_before, &schema_after, None, mc) {
        Ok(result) => {
            let json = serde_json::to_string(&result).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_list_policy_versions(
    engine: *const AegisEngine,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    match eng.list_policy_versions() {
        Ok(result) => {
            let json = serde_json::to_string(&result).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_rollback_policy(
    engine: *const AegisEngine,
    version: u32,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    match eng.rollback_policy(version) {
        Ok(_) => CString::new("ok").unwrap_or_default().into_raw(),
        Err(e) => error_string(&e.to_string()),
    }
}

// ── V7 Policy Lifecycle ─────────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_create_policy_draft(
    engine: *const AegisEngine,
    name: *const libc::c_char,
    description: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let name = match c_str_to_str(name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let description = match c_str_to_str(description) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match eng.create_policy_draft(&name, &description) {
        Ok(draft) => {
            let json = serde_json::to_string(&draft).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_update_policy_draft(
    engine: *const AegisEngine,
    id: *const libc::c_char,
    schema_json: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let id = match c_str_to_str(id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let uid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(e) => return error_string(&format!("invalid id: {}", e)),
    };
    let schema_json = match c_str_to_str(schema_json) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let schema: Schema = match serde_json::from_str(&schema_json) {
        Ok(s) => s,
        Err(e) => return error_string(&format!("invalid schema: {}", e)),
    };
    match eng.update_policy_draft(uid, schema) {
        Ok(draft) => {
            let json = serde_json::to_string(&draft).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_validate_policy_draft(
    engine: *const AegisEngine,
    id: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let id = match c_str_to_str(id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let uid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(e) => return error_string(&format!("invalid id: {}", e)),
    };
    match eng.validate_policy_draft(uid) {
        Ok(report) => {
            let json = serde_json::to_string(&report).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_submit_policy_draft_for_review(
    engine: *const AegisEngine,
    id: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let id = match c_str_to_str(id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let uid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(e) => return error_string(&format!("invalid id: {}", e)),
    };
    match eng.submit_policy_draft_for_review(uid) {
        Ok(draft) => {
            let json = serde_json::to_string(&draft).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_approve_policy_draft(
    engine: *const AegisEngine,
    id: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let id = match c_str_to_str(id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let uid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(e) => return error_string(&format!("invalid id: {}", e)),
    };
    match eng.approve_policy_draft(uid) {
        Ok(draft) => {
            let json = serde_json::to_string(&draft).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_reject_policy_draft(
    engine: *const AegisEngine,
    id: *const libc::c_char,
    rejection_reason: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let id = match c_str_to_str(id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let uid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(e) => return error_string(&format!("invalid id: {}", e)),
    };
    let reason = match c_str_to_str(rejection_reason) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match eng.reject_policy_draft(uid, &reason) {
        Ok(draft) => {
            let json = serde_json::to_string(&draft).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_publish_policy_draft(
    engine: *const AegisEngine,
    id: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let id = match c_str_to_str(id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let uid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(e) => return error_string(&format!("invalid id: {}", e)),
    };
    match eng.publish_policy_draft(uid) {
        Ok(result) => {
            let json = serde_json::to_string(&result).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_archive_policy_draft(
    engine: *const AegisEngine,
    id: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let id = match c_str_to_str(id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let uid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(e) => return error_string(&format!("invalid id: {}", e)),
    };
    match eng.archive_policy_draft(uid) {
        Ok(draft) => {
            let json = serde_json::to_string(&draft).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_list_policy_drafts(
    engine: *const AegisEngine,
    filter_status: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let status = if filter_status.is_null() {
        None
    } else {
        let s = match c_str_to_str(filter_status) {
            Ok(s) => s,
            Err(e) => return e,
        };
        Some(match s.to_lowercase().as_str() {
            "drafting" => aegis_core::engine::policy_lifecycle::DraftStatus::Drafting,
            "underreview" => aegis_core::engine::policy_lifecycle::DraftStatus::UnderReview,
            "approved" => aegis_core::engine::policy_lifecycle::DraftStatus::Approved,
            "rejected" => aegis_core::engine::policy_lifecycle::DraftStatus::Rejected,
            "published" => aegis_core::engine::policy_lifecycle::DraftStatus::Published,
            "archived" => aegis_core::engine::policy_lifecycle::DraftStatus::Archived,
            _ => return error_string(&format!("unknown status: {}", s)),
        })
    };
    match eng.list_policy_drafts(status) {
        Ok(drafts) => {
            let json = serde_json::to_string(&drafts).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

// ── V7 Scheduler ────────────────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_create_analysis_schedule(
    engine: *const AegisEngine,
    name: *const libc::c_char,
    interval_seconds: u64,
    queries_json: *const libc::c_char,
    compare_schema_json: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let name = match c_str_to_str(name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let queries_json = match c_str_to_str(queries_json) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let queries: Vec<aegis_core::types::analysis::CheckQuery> =
        match serde_json::from_str(&queries_json) {
            Ok(q) => q,
            Err(e) => return error_string(&format!("invalid queries: {}", e)),
        };
    let compare_schema = if compare_schema_json.is_null() {
        None
    } else {
        let s = match c_str_to_str(compare_schema_json) {
            Ok(s) => s,
            Err(e) => return e,
        };
        match serde_json::from_str(&s) {
            Ok(schema) => Some(schema),
            Err(e) => return error_string(&format!("invalid compare schema: {}", e)),
        }
    };
    match eng.create_analysis_schedule(&name, interval_seconds, queries, compare_schema) {
        Ok(schedule) => {
            let json = serde_json::to_string(&schedule).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_list_analysis_schedules(
    engine: *const AegisEngine,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    match eng.list_analysis_schedules() {
        Ok(schedules) => {
            let json = serde_json::to_string(&schedules).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_delete_analysis_schedule(
    engine: *const AegisEngine,
    id: *const libc::c_char,
) -> i32 {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    let id = match c_str_to_str(id) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let uid = match uuid::Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return 0,
    };
    match eng.delete_analysis_schedule(uid) {
        Ok(true) => 1,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_run_analysis_now(
    engine: *const AegisEngine,
    schedule_id: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let uid = if schedule_id.is_null() {
        None
    } else {
        let id = match c_str_to_str(schedule_id) {
            Ok(s) => s,
            Err(e) => return e,
        };
        match uuid::Uuid::parse_str(&id) {
            Ok(u) => Some(u),
            Err(e) => return error_string(&format!("invalid id: {}", e)),
        }
    };
    match eng.run_analysis_now(uid) {
        Ok(runs) => {
            let json = serde_json::to_string(&runs).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_get_analysis_runs(
    engine: *const AegisEngine,
    limit: u64,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    match eng.get_analysis_runs(limit as usize) {
        Ok(runs) => {
            let json = serde_json::to_string(&runs).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

// ── V7 Enforcement History ──────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_set_enforcement_history_config(
    engine: *const AegisEngine,
    config_json: *const libc::c_char,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    let config_json = match c_str_to_str(config_json) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let config: aegis_core::engine::enforcement_history::EnforcementHistoryConfig =
        match serde_json::from_str(&config_json) {
            Ok(c) => c,
            Err(e) => return error_string(&format!("invalid config: {}", e)),
        };
    match eng.set_enforcement_history_config(config) {
        Ok(_) => CString::new("ok").unwrap_or_default().into_raw(),
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_get_enforcement_history_config(
    engine: *const AegisEngine,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    match eng.get_enforcement_history_config() {
        Ok(config) => {
            let json = serde_json::to_string(&config).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_enforcement_trends(
    engine: *const AegisEngine,
    limit: u64,
) -> *mut libc::c_char {
    let eng = match engine_from_const_ptr(engine) {
        Ok(e) => e,
        Err(e) => return e,
    };
    match eng.enforcement_trends(limit as usize) {
        Ok(trends) => {
            let json = serde_json::to_string(&trends).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => error_string(&e.to_string()),
    }
}

// ── Helpers ──

fn engine_from_ptr(ptr: *mut AegisEngine) -> Result<&'static GraphEngine, *mut libc::c_char> {
    if ptr.is_null() {
        Err(error_string("engine is null"))
    } else {
        let engine = unsafe { &*ptr };
        Ok(&engine.inner)
    }
}

fn engine_from_const_ptr(
    ptr: *const AegisEngine,
) -> Result<&'static GraphEngine, *mut libc::c_char> {
    if ptr.is_null() {
        Err(error_string("engine is null"))
    } else {
        let engine = unsafe { &*ptr };
        Ok(&engine.inner)
    }
}

fn error_string(msg: &str) -> *mut libc::c_char {
    CString::new(msg).unwrap_or_default().into_raw()
}

fn c_str_to_str(ptr: *const libc::c_char) -> Result<String, *mut libc::c_char> {
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(|s| s.to_string())
        .map_err(|e| error_string(&e.to_string()))
}
