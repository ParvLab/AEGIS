use std::ffi::{CStr, CString};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};

use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::StorageBackend;
use aegis_core::types::*;

/// Opaque handle to the engine.
pub struct AegisEngine {
    inner: GraphEngine,
    closed: AtomicBool,
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

#[unsafe(no_mangle)]
pub extern "C" fn aegis_engine_create(
    db_path: *const libc::c_char,
    schema_yaml: *const libc::c_char,
) -> *mut AegisEngine {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<*mut AegisEngine, String> {
        let path = unsafe { CStr::from_ptr(db_path) }
            .to_str()
            .map_err(|e| e.to_string())?;
        let yaml = unsafe { CStr::from_ptr(schema_yaml) }
            .to_str()
            .map_err(|e| e.to_string())?;

        let config = SqliteConfig {
            path: path.to_string(),
            max_readers: 4,
            busy_timeout_ms: 5000,
            wal_mode: true,
            mmap_size: 0,
        };
        let mut storage =
            SqliteStorage::new(config).map_err(|e| e.to_string())?;
        storage
            .initialize()
            .map_err(|e| e.to_string())?;
        let schema = parse_schema(yaml).map_err(|e| e.to_string())?;
        let engine = GraphEngine::new(Box::new(storage), schema);

        Ok(Box::into_raw(Box::new(AegisEngine {
            inner: engine,
            closed: AtomicBool::new(false),
        })))
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
            }
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(|| -> Result<AegisCheckResult, *mut libc::c_char> {
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
    })) {
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
            }
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(|| -> Result<AegisWriteResult, *mut libc::c_char> {
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
    })) {
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
            }
        }
    };

    match panic::catch_unwind(AssertUnwindSafe(|| -> Result<AegisWriteResult, *mut libc::c_char> {
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

        let revision_token = eng
            .delete(&key)
            .map_err(|e| error_string(&e.to_string()))?;

        Ok(AegisWriteResult {
            revision: revision_token.revision.as_u64(),
            error: std::ptr::null_mut(),
        })
    })) {
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
pub extern "C" fn aegis_engine_health(
    engine: *mut AegisEngine,
) -> AegisHealthResult {
    let eng = match engine_from_ptr(engine) {
        Ok(e) => e,
        Err(err) => {
            return AegisHealthResult {
                healthy: false,
                revision: 0,
                schema_version: 0,
                error: err,
            }
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

// ── Error string free ──

#[unsafe(no_mangle)]
pub extern "C" fn aegis_free_string(s: *mut libc::c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

// ── Helpers ──

fn engine_from_ptr(ptr: *mut AegisEngine) -> Result<&'static GraphEngine, *mut libc::c_char> {
    if ptr.is_null() {
        Err(error_string("engine is null"))
    } else {
        let engine = unsafe { &*ptr };
        if engine.closed.load(Ordering::Relaxed) {
            Err(error_string("engine is closed"))
        } else {
            Ok(&engine.inner)
        }
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
