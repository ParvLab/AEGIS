use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use aegis_core::engine::watch::{WatchEvent, WatchFilter, WatchSubscription};
use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::{StorageBackend, StorageTransaction, TupleFilter};
use aegis_core::types::{
    AuditEntry, PaginationParams, Relation, RelationshipTuple, ResourceId, SubjectId, TupleKey,
};
use napi_derive::napi;

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

fn validate_tuple(
    subject: &str,
    relation: &str,
    resource: &str,
) -> napi::Result<RelationshipTuple> {
    let subject_id = SubjectId::new(subject).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let relation_id =
        Relation::new(relation).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let resource_id =
        ResourceId::new(resource).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(RelationshipTuple::new(subject_id, relation_id, resource_id))
}

// ── Structs ──────────────────────────────────────────────────────────────────────────

#[napi(object)]
pub struct InitializeResultNAP {
    pub schema_version: i32,
    pub revision: i64,
    pub healthy: bool,
}

#[napi(object)]
pub struct CheckResultNAP {
    pub allowed: bool,
    pub revision: i64,
}

#[napi(object)]
pub struct WriteResultNAP {
    pub revision: i64,
    pub node_id: String,
    pub timestamp: String,
}

#[napi(object)]
pub struct TupleNAP {
    pub subject: String,
    pub relation: String,
    pub object: String,
}

#[napi(object)]
pub struct ExplainTraceNAP {
    pub subject: String,
    pub relation: String,
    pub object: String,
}

#[napi(object)]
pub struct ExplainResultNAP {
    pub allowed: bool,
    pub revision: i64,
    pub trace: Vec<ExplainTraceNAP>,
    pub resolved_via: String,
    pub duration_ms: i64,
}

#[napi(object)]
pub struct HealthReportNAP {
    pub healthy: bool,
    pub error: Option<String>,
    pub revision: i64,
    pub schema_version: i32,
    pub backend: String,
    pub backend_healthy: bool,
    pub telemetry_healthy: bool,
    pub cache_hit_rate: f64,
    pub cache_entries: i32,
    pub storage_integrity: bool,
    pub total_checks: f64,
    pub allowed_checks: f64,
    pub denied_checks: f64,
    pub error_checks: f64,
    pub cache_size: f64,
    pub cache_hit_ratio: f64,
    // Sprint 6.4 fields
    pub integrity_status: String,
    pub uptime_ms: f64,
    pub storage_version: Option<String>,
    pub connections: ConnectionStatsNAP,
    pub wal_size_mb: Option<f64>,
}

#[napi(object)]
pub struct ConnectionStatsNAP {
    pub read_active: i32,
    pub read_idle: i32,
    pub write_busy: bool,
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

#[napi(object)]
pub struct SchemaCheckReportNAP {
    pub compatible: bool,
    pub warnings: Vec<String>,
    pub breaking: Vec<String>,
}

#[napi(object)]
pub struct AuditEntryNAP {
    pub revision: i64,
    pub action: String,
    pub subject: String,
    pub relation: String,
    pub object: String,
    pub timestamp: String,
}

#[napi(object)]
pub struct WatchEventNAP {
    pub event_type: String,
    pub subject: String,
    pub relation: String,
    pub object: String,
    pub revision: i64,
    pub timestamp: String,
}

#[napi(object)]
pub struct ExportSubjectResultNAP {
    pub subject: String,
    pub active_tuples: Vec<TupleNAP>,
    pub audit_entries: Vec<AuditEntryNAP>,
    pub export_revision: i64,
    pub exported_at: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────────────

fn revision_token_to_nap(token: &aegis_core::types::RevisionToken) -> WriteResultNAP {
    WriteResultNAP {
        revision: token.revision.as_u64() as i64,
        node_id: token.node_id.to_string(),
        timestamp: token
            .timestamp
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
    }
}

fn audit_entry_to_nap(entry: &AuditEntry) -> AuditEntryNAP {
    AuditEntryNAP {
        revision: entry.revision.as_u64() as i64,
        action: format!("{:?}", entry.action).to_lowercase(),
        subject: entry.subject.clone(),
        relation: entry.relation.clone(),
        object: entry.object.clone(),
        timestamp: entry
            .timestamp
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
    }
}

fn tuple_to_nap(tuple: &RelationshipTuple) -> TupleNAP {
    TupleNAP {
        subject: tuple.subject.as_str().to_string(),
        relation: tuple.relation.as_str().to_string(),
        object: tuple.object.as_str().to_string(),
    }
}

fn health_report_to_nap(report: &aegis_core::types::HealthReport) -> HealthReportNAP {
    HealthReportNAP {
        healthy: report.healthy,
        error: report.error.clone(),
        revision: report.revision.as_u64() as i64,
        schema_version: report.schema_version as i32,
        backend: report.backend.clone(),
        backend_healthy: report.backend_healthy,
        telemetry_healthy: report.telemetry_healthy,
        cache_hit_rate: report.cache_hit_rate,
        cache_entries: report.cache_entries as i32,
        storage_integrity: report.storage_integrity,
        total_checks: report.total_checks as f64,
        allowed_checks: report.allowed_checks as f64,
        denied_checks: report.denied_checks as f64,
        error_checks: report.error_checks as f64,
        cache_size: report.cache_size as f64,
        cache_hit_ratio: report.cache_hit_ratio,
        integrity_status: report.integrity_status.clone(),
        uptime_ms: report.uptime_ms as f64,
        storage_version: report.storage_version.clone(),
        connections: ConnectionStatsNAP {
            read_active: report.connections.read_active as i32,
            read_idle: report.connections.read_idle as i32,
            write_busy: report.connections.write_busy,
        },
        wal_size_mb: report.wal_size_mb,
    }
}

fn watch_event_to_nap(event: &WatchEvent) -> WatchEventNAP {
    WatchEventNAP {
        event_type: format!("{:?}", event.event_type),
        subject: event.subject.clone(),
        relation: event.relation.clone(),
        object: event.object.clone(),
        revision: event.revision.as_u64() as i64,
        timestamp: event
            .timestamp
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
    }
}

// ── Main engine class ────────────────────────────────────────────────────────────────

#[napi]
pub struct JsAegis {
    engine: Arc<GraphEngine>,
    closed: AtomicBool,
}

#[napi]
impl JsAegis {
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    fn check_open(&self) -> napi::Result<()> {
        if self.closed.load(Ordering::Relaxed) {
            Err(napi::Error::from_reason("engine is closed"))
        } else {
            Ok(())
        }
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────────────

#[napi]
pub fn initialize(path: String, schema_yaml: String) -> napi::Result<JsAegis> {
    catch_engine_panic(move || {
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
        Ok(JsAegis {
            engine: Arc::new(engine),
            closed: AtomicBool::new(false),
        })
    })
}

// ── Core operations ───────────────────────────────────────────────────────────────────

#[napi]
impl JsAegis {
    #[napi]
    pub fn initialize_result(&self) -> napi::Result<InitializeResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let health = self.engine.health();
            Ok(InitializeResultNAP {
                schema_version: health.schema_version as i32,
                revision: health.revision.as_u64() as i64,
                healthy: health.healthy,
            })
        })
    }

    #[napi]
    pub fn check(
        &self,
        subject: String,
        permission: String,
        resource: String,
    ) -> napi::Result<CheckResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let subject_id = SubjectId::new(&subject)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let resource_id = ResourceId::new(&resource)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let result = self
                .engine
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
        &self,
        subject: String,
        relation: String,
        resource: String,
    ) -> napi::Result<WriteResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let tuple = validate_tuple(&subject, &relation, &resource)?;
            let result = self
                .engine
                .write(&tuple)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(revision_token_to_nap(&result))
        })
    }

    #[napi]
    pub fn delete(
        &self,
        subject: String,
        relation: String,
        resource: String,
    ) -> napi::Result<WriteResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let tuple = validate_tuple(&subject, &relation, &resource)?;
            let key = TupleKey {
                subject: tuple.subject,
                relation: tuple.relation,
                object: tuple.object,
            };
            let result = self
                .engine
                .delete(&key)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(revision_token_to_nap(&result))
        })
    }

    #[napi]
    pub fn list_by_object(
        &self,
        object: String,
        relation: Option<String>,
    ) -> napi::Result<Vec<TupleNAP>> {
        self.check_open()?;
        catch_engine_panic(|| {
            let object_id = ResourceId::new(&object)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let relation_opt = relation
                .as_deref()
                .map(Relation::new)
                .transpose()
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let tuples = self
                .engine
                .list_by_object(&object_id, relation_opt.as_ref(), None)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(tuples.iter().map(tuple_to_nap).collect())
        })
    }

    #[napi]
    pub fn explain(
        &self,
        subject: String,
        permission: String,
        resource: String,
    ) -> napi::Result<ExplainResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let subject_id = SubjectId::new(&subject)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let resource_id = ResourceId::new(&resource)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let result = self
                .engine
                .explain(&subject_id, &permission, &resource_id, None)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(ExplainResultNAP {
                allowed: result.allowed,
                revision: result.revision.as_u64() as i64,
                trace: result
                    .trace
                    .iter()
                    .map(|t| ExplainTraceNAP {
                        subject: t.subject.clone(),
                        relation: t.relation.clone(),
                        object: t.object.clone(),
                    })
                    .collect(),
                resolved_via: result.resolved_via,
                duration_ms: result.duration_ms as i64,
            })
        })
    }

    #[napi]
    pub fn health(&self) -> napi::Result<HealthReportNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let report = self.engine.health();
            Ok(health_report_to_nap(&report))
        })
    }

    #[napi]
    pub fn check_dry_run(
        &self,
        subject: String,
        permission: String,
        resource: String,
    ) -> napi::Result<CheckResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let subject_id = SubjectId::new(&subject)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let resource_id = ResourceId::new(&resource)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let result = self
                .engine
                .check_dry_run(&subject_id, &permission, &resource_id, None)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(CheckResultNAP {
                allowed: result.allowed,
                revision: result.revision.as_u64() as i64,
            })
        })
    }

    #[napi]
    pub fn list_by_subject(
        &self,
        subject: String,
        relation: Option<String>,
    ) -> napi::Result<Vec<TupleNAP>> {
        self.check_open()?;
        catch_engine_panic(|| {
            let subject_id = SubjectId::new(&subject)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let relation_opt = relation
                .as_deref()
                .map(Relation::new)
                .transpose()
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let tuples = self
                .engine
                .list_by_subject(&subject_id, relation_opt.as_ref(), None)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(tuples.iter().map(tuple_to_nap).collect())
        })
    }

    #[napi]
    pub fn query(
        &self,
        filter: QueryFilterNAP,
        pagination: PaginationNAP,
    ) -> napi::Result<PaginatedTuplesNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let relation = match filter.relation {
                Some(r) => Some(
                    Relation::new(&r).map_err(|e| napi::Error::from_reason(e.to_string()))?,
                ),
                None => None,
            };
            let tf = TupleFilter {
                subject_type: filter.subject_type,
                relation,
                object_type: filter.object_type,
                metadata_key: filter.metadata_key,
                metadata_value: filter.metadata_value,
            };
            let current_rev = self
                .engine
                .storage()
                .current_revision()
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let pp = PaginationParams {
                limit: pagination.limit as u64,
                cursor: pagination.cursor_offset.map(|o| {
                    aegis_core::types::PaginationCursor {
                        offset: o as u64,
                        revision: current_rev,
                    }
                }),
            };
            let result = self
                .engine
                .query(&tf, &pp, None)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(PaginatedTuplesNAP {
                tuples: result.tuples.iter().map(tuple_to_nap).collect(),
                next_cursor: result.next_cursor.map(|c| c.offset as f64),
                revision: result.revision.as_u64() as f64,
            })
        })
    }

    #[napi]
    pub fn write_batch(&self, tuples: Vec<TupleNAP>) -> napi::Result<WriteResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
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
            let result = self
                .engine
                .write_batch(&rel_tuples)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(revision_token_to_nap(&result))
        })
    }

    #[napi]
    pub fn migrate(&self, target_version: i32) -> napi::Result<()> {
        self.check_open()?;
        catch_engine_panic(|| {
            self.engine
                .migrate(target_version as u32)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(())
        })
    }

    #[napi]
    pub fn check_schema(&self, schema_yaml: String) -> napi::Result<SchemaCheckReportNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let new_schema = parse_schema(&schema_yaml)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let report = self.engine.check_schema(&new_schema);
            Ok(SchemaCheckReportNAP {
                compatible: report.compatible,
                warnings: report.warnings,
                breaking: report.breaking,
            })
        })
    }

    #[napi]
    pub fn delete_object(&self, object: String) -> napi::Result<WriteResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let object_id = ResourceId::new(&object)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let result = self
                .engine
                .delete_object(&object_id)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(revision_token_to_nap(&result))
        })
    }
}

// ── Sprint 3: New exports ─────────────────────────────────────────────────────────────

#[napi]
impl JsAegis {
    // S3.2 — write_dry_run
    #[napi]
    pub fn write_dry_run(
        &self,
        subject: String,
        relation: String,
        resource: String,
    ) -> napi::Result<CheckResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let tuple = validate_tuple(&subject, &relation, &resource)?;
            let result = self
                .engine
                .write_dry_run(&tuple)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(CheckResultNAP {
                allowed: false,
                revision: result.revision.as_u64() as i64,
            })
        })
    }

    // S3.3 — export_subject (GDPR)
    #[napi]
    pub fn export_subject(
        &self,
        subject: String,
    ) -> napi::Result<ExportSubjectResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let subject_id = SubjectId::new(&subject)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let tuples = self
                .engine
                .export_subject(&subject_id)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let revision = self
                .engine
                .storage()
                .current_revision()
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(ExportSubjectResultNAP {
                subject: subject.clone(),
                active_tuples: tuples.iter().map(tuple_to_nap).collect(),
                audit_entries: Vec::new(),
                export_revision: revision.as_u64() as i64,
                exported_at: chrono::Utc::now()
                    .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                    .to_string(),
            })
        })
    }

    // S3.4 — delete_subject_with_policy
    #[napi]
    pub fn delete_subject_with_policy(
        &self,
        subject: String,
        policy: String,
        transfer_to_subject: Option<String>,
    ) -> napi::Result<WriteResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let subject_id = SubjectId::new(&subject)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let transfer = match transfer_to_subject {
                Some(s) => Some(
                    SubjectId::new(&s)
                        .map_err(|e| napi::Error::from_reason(e.to_string()))?,
                ),
                None => None,
            };
            let result = self
                .engine
                .delete_subject_with_policy(&subject_id, &policy, transfer.as_ref())
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(revision_token_to_nap(&result))
        })
    }

    // S3.7 — query_audit
    #[napi]
    pub fn query_audit(
        &self,
        object: String,
        from_revision: Option<i64>,
        to_revision: Option<i64>,
        limit: f64,
    ) -> napi::Result<Vec<AuditEntryNAP>> {
        self.check_open()?;
        catch_engine_panic(|| {
            let object_id = ResourceId::new(&object)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            let from = from_revision
                .map(|r| aegis_core::types::Revision::from(r as u64));
            let to =
                to_revision.map(|r| aegis_core::types::Revision::from(r as u64));
            let pp = PaginationParams {
                limit: limit as u64,
                cursor: None,
            };
            let entries = self
                .engine
                .query_audit(&object_id, from, to, &pp)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(entries.iter().map(audit_entry_to_nap).collect())
        })
    }

    // S3.8 — close
    #[napi]
    pub fn close(&self) -> napi::Result<()> {
        if self.closed.swap(true, Ordering::Relaxed) {
            return Ok(()); // idempotent
        }
        catch_engine_panic(|| {
            self.engine
                .close()
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    // S3.9 — reload_schema
    #[napi]
    pub fn reload_schema(&self, schema_yaml: String) -> napi::Result<()> {
        self.check_open()?;
        catch_engine_panic(|| {
            let new_schema = parse_schema(&schema_yaml)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            self.engine
                .reload_schema(new_schema)
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    // S3.5 — watch (returns a subscription)
    #[napi]
    pub fn watch(
        &self,
        subject_type: Option<String>,
        relation: Option<String>,
        object_type: Option<String>,
    ) -> napi::Result<JsWatchSubscription> {
        self.check_open()?;
        catch_engine_panic(|| {
            let filter = WatchFilter {
                subjects: subject_type.map(|s| vec![s]),
                relations: relation.map(|r| vec![r]),
                objects: object_type.map(|o| vec![o]),
                event_types: None,
            };
            let subscription = self.engine.watch(filter);
            Ok(JsWatchSubscription {
                inner: Some(subscription),
            })
        })
    }

    // S3.6 — transaction
    #[napi]
    pub fn transaction(&self) -> napi::Result<JsTransaction> {
        self.check_open()?;
        catch_engine_panic(|| {
            let txn = self
                .engine
                .transaction()
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(JsTransaction {
                inner: Mutex::new(Some(txn)),
                consumed: AtomicBool::new(false),
            })
        })
    }
}

// ── Watch subscription class ──────────────────────────────────────────────────────────

#[napi]
pub struct JsWatchSubscription {
    inner: Option<WatchSubscription>,
}

#[napi]
impl JsWatchSubscription {
    #[napi]
    pub fn poll(&self) -> Option<WatchEventNAP> {
        match &self.inner {
            Some(sub) => match sub.try_recv() {
                Ok(event) => Some(watch_event_to_nap(&event)),
                Err(_) => None,
            },
            None => None,
        }
    }

    #[napi]
    pub fn unsubscribe(&mut self) {
        self.inner.take();
    }
}

// ── Transaction class ─────────────────────────────────────────────────────────────────

#[napi]
pub struct JsTransaction {
    inner: Mutex<Option<Box<dyn StorageTransaction>>>,
    consumed: AtomicBool,
}

#[napi]
impl JsTransaction {
    fn check_open(&self) -> napi::Result<()> {
        if self.consumed.load(Ordering::Relaxed) {
            Err(napi::Error::from_reason(
                "transaction already committed or rolled back",
            ))
        } else {
            Ok(())
        }
    }

    #[napi]
    pub fn write(&self, subject: String, relation: String, resource: String) -> napi::Result<()> {
        self.check_open()?;
        catch_engine_panic(|| {
            let tuple = validate_tuple(&subject, &relation, &resource)?;
            let mut guard = self
                .inner
                .lock()
                .map_err(|e| napi::Error::from_reason(format!("lock error: {}", e)))?;
            let txn = guard
                .as_mut()
                .ok_or_else(|| napi::Error::from_reason("transaction not initialized"))?;
            txn.write(&tuple)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(())
        })
    }

    #[napi]
    pub fn delete(&self, subject: String, relation: String, resource: String) -> napi::Result<()> {
        self.check_open()?;
        catch_engine_panic(|| {
            let tuple = validate_tuple(&subject, &relation, &resource)?;
            let key = TupleKey {
                subject: tuple.subject,
                relation: tuple.relation,
                object: tuple.object,
            };
            let mut guard = self
                .inner
                .lock()
                .map_err(|e| napi::Error::from_reason(format!("lock error: {}", e)))?;
            let txn = guard
                .as_mut()
                .ok_or_else(|| napi::Error::from_reason("transaction not initialized"))?;
            txn.delete(&key)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(())
        })
    }

    #[napi]
    pub fn savepoint(&self, name: String) -> napi::Result<()> {
        self.check_open()?;
        catch_engine_panic(|| {
            let guard = self
                .inner
                .lock()
                .map_err(|e| napi::Error::from_reason(format!("lock error: {}", e)))?;
            let txn = guard
                .as_ref()
                .ok_or_else(|| napi::Error::from_reason("transaction not initialized"))?;
            txn.savepoint(&name)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(())
        })
    }

    #[napi]
    pub fn rollback_to_savepoint(&self, name: String) -> napi::Result<()> {
        self.check_open()?;
        catch_engine_panic(|| {
            let guard = self
                .inner
                .lock()
                .map_err(|e| napi::Error::from_reason(format!("lock error: {}", e)))?;
            let txn = guard
                .as_ref()
                .ok_or_else(|| napi::Error::from_reason("transaction not initialized"))?;
            txn.rollback_to_savepoint(&name)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(())
        })
    }

    #[napi]
    pub fn release_savepoint(&self, name: String) -> napi::Result<()> {
        self.check_open()?;
        catch_engine_panic(|| {
            let guard = self
                .inner
                .lock()
                .map_err(|e| napi::Error::from_reason(format!("lock error: {}", e)))?;
            let txn = guard
                .as_ref()
                .ok_or_else(|| napi::Error::from_reason("transaction not initialized"))?;
            txn.release_savepoint(&name)
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(())
        })
    }

    #[napi]
    pub fn commit(&self) -> napi::Result<WriteResultNAP> {
        self.check_open()?;
        catch_engine_panic(|| {
            let mut guard = self
                .inner
                .lock()
                .map_err(|e| napi::Error::from_reason(format!("lock error: {}", e)))?;
            let txn = guard
                .take()
                .ok_or_else(|| napi::Error::from_reason("transaction not initialized"))?;
            let revision = txn
                .commit()
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            self.consumed.store(true, Ordering::Relaxed);
            Ok(WriteResultNAP {
                revision: revision.as_u64() as i64,
                node_id: String::new(),
                timestamp: chrono::Utc::now()
                    .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                    .to_string(),
            })
        })
    }

    #[napi]
    pub fn rollback(&self) -> napi::Result<()> {
        self.check_open()?;
        catch_engine_panic(|| {
            let mut guard = self
                .inner
                .lock()
                .map_err(|e| napi::Error::from_reason(format!("lock error: {}", e)))?;
            let txn = guard
                .take()
                .ok_or_else(|| napi::Error::from_reason("transaction not initialized"))?;
            txn.rollback()
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            self.consumed.store(true, Ordering::Relaxed);
            Ok(())
        })
    }
}
