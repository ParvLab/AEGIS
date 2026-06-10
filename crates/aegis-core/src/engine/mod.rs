pub mod cache;
pub mod crdt;
pub mod gdpr;
pub mod hooks;
#[cfg(feature = "hot-reload")]
pub mod hot_reload;
pub mod migration;
pub mod policy;
pub mod ratelimit;
pub mod traversal;
pub mod watch;

use chrono::Utc;
use crate::engine::cache::DecisionCache;
use crate::engine::migration::MigrationRunner;
use crate::engine::ratelimit::{RateLimitOp, TokenBucketRateLimiter};
use crate::engine::watch::{SharedWatchers, WatchEvent, WatchEventType, WatchFilter, WatchSubscription};
use crate::error::{AegisError, AegisResult};
use crate::storage::{StorageBackend, StorageTransaction, TupleFilter};
use crate::types::{
    CheckResult, ConsistencyMode, ExplainResult, ExplainTrace, FailClosedMode, HealthReport,
    MigrationResult, Relation, RelationshipTuple, ResourceId, Revision, RevisionToken, Schema,
    SubjectId, TupleKey, PaginatedTuples, PaginationParams,
};
use crate::types::schema::SchemaCompatibilityReport;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tracing::{error, field, info, span, Level};

/// The core authorization engine.
///
/// Combines a `StorageBackend` for tuple data with a `Schema` for policy definitions.
/// Provides the primary `check()` and `explain()` APIs.
pub struct GraphEngine {
    storage: Box<dyn StorageBackend>,
    schema: RwLock<Schema>,
    cache: Mutex<DecisionCache>,
    node_id: uuid::Uuid,
    hooks: hooks::SharedHookRegistry,
    fail_closed: FailClosedMode,
    closed: std::sync::atomic::AtomicBool,
    watchers: SharedWatchers,
    rate_limiter: TokenBucketRateLimiter,
}

impl GraphEngine {
    /// Create a new graph engine with the given storage and schema.
    pub fn new(storage: Box<dyn StorageBackend>, schema: Schema) -> Self {
        Self {
            storage,
            schema: RwLock::new(schema),
            cache: Mutex::new(DecisionCache::new(10_000)),
            node_id: uuid::Uuid::new_v4(),
            hooks: hooks::SharedHookRegistry::new(),
            fail_closed: FailClosedMode::DenyOnError,
            closed: std::sync::atomic::AtomicBool::new(false),
            watchers: Arc::new(Mutex::new(HashMap::new())),
            rate_limiter: TokenBucketRateLimiter::new(ratelimit::RateLimitConfig::default()),
        }
    }

    /// Access the hook registry to register callbacks.
    pub fn hooks(&self) -> &hooks::SharedHookRegistry {
        &self.hooks
    }

    /// Set a custom cache capacity.
    pub fn with_cache_capacity(mut self, capacity: usize) -> Self {
        self.cache = Mutex::new(DecisionCache::new(capacity));
        self
    }

    /// Subscribe to write/delete events with an optional filter.
    /// Returns a `WatchSubscription` that receives events until dropped.
    pub fn watch(&self, filter: WatchFilter) -> WatchSubscription {
        let id = uuid::Uuid::new_v4();
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher_tx = tx.clone();
        self.watchers.lock().unwrap().insert(id, (filter.clone(), watcher_tx));
        WatchSubscription::new(id, filter, rx, tx, Arc::clone(&self.watchers))
    }

    /// Subscribe to all write/delete events (no filter).
    pub fn watch_all(&self) -> WatchSubscription {
        self.watch(WatchFilter::default())
    }

    fn emit_watch_event(&self, event_type: WatchEventType, subject: &str, relation: &str, object: &str, revision: Revision) {
        let event = WatchEvent {
            event_type,
            subject: subject.to_string(),
            relation: relation.to_string(),
            object: object.to_string(),
            revision,
            timestamp: chrono::Utc::now(),
        };
        let mut watchers = self.watchers.lock().unwrap();
        watchers.retain(|_, (filter, tx)| {
            if !filter.matches(&event) {
                return true;
            }
            tx.send(event.clone()).is_ok()
        });
    }

    /// Set fail-closed mode (DenyOnError by default).
    pub fn with_fail_closed(mut self, mode: FailClosedMode) -> Self {
        self.fail_closed = mode;
        self
    }

    /// Access the underlying storage backend.
    pub fn storage(&self) -> &dyn StorageBackend {
        self.storage.as_ref()
    }

    /// Access the schema (read lock).
    pub fn schema(&self) -> std::sync::RwLockReadGuard<'_, Schema> {
        self.schema.read().unwrap()
    }

    /// Replace the schema (write lock). Invalidates cache on success.
    pub fn reload_schema(&self, new_schema: Schema) -> AegisResult<()> {
        let mut schema = self.schema.write().unwrap();
        *schema = new_schema;
        self.cache.lock().unwrap().clear();
        Ok(())
    }

    /// Health check: returns a report of engine health.
    pub fn health(&self) -> HealthReport {
        let revision = self.storage.current_revision().ok();
        let integrity = self.storage.integrity_check().ok();
        let cache = self.cache.lock().unwrap();
        let schema = self.schema.read().unwrap();
        HealthReport {
            healthy: revision.is_some() && integrity.as_ref().map(|i| i.passed).unwrap_or(false),
            revision: revision.unwrap_or(Revision::ZERO),
            schema_version: schema.schema_version,
            backend: self.storage.backend_type().to_string(),
            backend_healthy: integrity.as_ref().map(|i| i.passed).unwrap_or(false),
            cache_hit_rate: cache.hit_rate(),
            cache_entries: cache.len(),
            storage_integrity: integrity.as_ref().map(|i| i.passed).unwrap_or(false),
            error: None,
        }
    }

    /// Graceful shutdown: flush cache, checkpoint WAL, close connections.
    pub fn close(&self) -> AegisResult<()> {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        self.cache.lock().unwrap().clear();
        self.storage.close()
    }

    /// Check whether a subject has a permission on a resource.
    ///
    /// Returns `CheckResult { allowed: bool, revision: Revision }`.
    /// If `dry_run` is true, the decision is not cached and hooks are not triggered.
    pub fn check(
        &self,
        subject: &SubjectId,
        permission: &str,
        resource: &ResourceId,
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<CheckResult> {
        self.check_inner(subject, permission, resource, consistency, false)
    }

    /// Dry-run check: evaluates without caching or triggering hooks.
    pub fn check_dry_run(
        &self,
        subject: &SubjectId,
        permission: &str,
        resource: &ResourceId,
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<CheckResult> {
        self.check_inner(subject, permission, resource, consistency, true)
    }

    /// Internal check implementation with dry_run flag.
    fn check_inner(
        &self,
        subject: &SubjectId,
        permission: &str,
        resource: &ResourceId,
        consistency: Option<ConsistencyMode>,
        dry_run: bool,
    ) -> AegisResult<CheckResult> {
        let backend_str = self.storage.backend_type().to_string();
        let _span = span!(
            Level::INFO,
            "aegis.check",
            subject = subject.as_str(),
            permission = permission,
            resource = resource.as_str(),
            backend = &backend_str as &str,
        )
        .entered();

        // Rate limit check
        let rl_key = format!("check:{}", resource.as_str());
        if let Err(e) = self.rate_limiter.check(&rl_key, RateLimitOp::Check) {
            return Err(e);
        }

        let revision = match self.resolve_revision(consistency) {
            Ok(r) => r,
            Err(e) => {
                error!(error = field::display(&e), "revision resolution failed");
                return self.fail_closed_response(e);
            }
        };

        if !dry_run {
            let cache_span = span!(Level::DEBUG, crate::telemetry::spans::CACHE_LOOKUP);
            let _cache_guard = cache_span.enter();
            let mut cache = self.cache.lock().unwrap();
            if let Some(allowed) = cache.get(subject.as_str(), permission, resource.as_str(), revision) {
                info!(
                    allowed = allowed,
                    revision = field::display(&revision),
                    cache_hit = true,
                    "check cache hit"
                );
                return Ok(CheckResult { allowed, revision });
            }
        }

        // Resolve permission to relations
        let resource_type = resource_type_name(resource.as_str());
        let schema = self.schema.read().unwrap();
        let resolved = match policy::resolve_permission(&schema, &resource_type, permission) {
            Some(r) => r,
            None => {
                return Ok(CheckResult {
                    allowed: false,
                    revision,
                });
            }
        };
        drop(schema);

        // Try each relation - any match means allowed (union semantics)
        let mut allowed = false;
        for rel_name in &resolved.relations {
            let relation = match Relation::new(rel_name) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let max_depth = self.rate_limiter.max_traversal_depth();
            let max_visits = self.rate_limiter.max_traversal_visits();
            let result = match traversal::bfs_traversal_with_limits(
                self.storage.as_ref(),
                subject,
                &relation,
                resource,
                Some(revision),
                max_depth,
                max_visits,
            ) {
                Ok(r) => r,
                Err(e) => {
                    return self.fail_closed_response(e);
                }
            };

            if result.found {
                allowed = true;
                break;
            }
        }

        if !dry_run {
            // Cache the decision
            let mut cache = self.cache.lock().unwrap();
            cache.insert(subject.as_str(), permission, resource.as_str(), allowed, revision);

            self.hooks.trigger(&hooks::HookEvent::OnCheck {
                subject: subject.as_str().to_string(),
                permission: permission.to_string(),
                resource: resource.as_str().to_string(),
                allowed,
            });
        }

        info!(
            allowed = allowed,
            revision = field::display(&revision),
            cache_hit = false,
            "check decision"
        );

        Ok(CheckResult { allowed, revision })
    }

    /// Apply fail-closed policy: deny on error, or propagate error if allow-on-error.
    fn fail_closed_response(&self, error: AegisError) -> AegisResult<CheckResult> {
        match self.fail_closed {
            FailClosedMode::DenyOnError => Ok(CheckResult {
                allowed: false,
                revision: Revision::ZERO,
            }),
            FailClosedMode::AllowOnError => Err(error),
        }
    }

    /// Explain why a check returned its result, including the trace path.
    pub fn explain(
        &self,
        subject: &SubjectId,
        permission: &str,
        resource: &ResourceId,
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<ExplainResult> {
        let revision = self.resolve_revision(consistency)?;

        let resource_type = resource_type_name(resource.as_str());
        let schema = self.schema.read().unwrap();
        let resolved = match policy::resolve_permission(&schema, &resource_type, permission) {
            Some(r) => r,
            None => {
                return Ok(ExplainResult {
                    allowed: false,
                    revision,
                    trace: Vec::new(),
                    resolved_via: String::new(),
                    duration_ms: 0,
                });
            }
        };
        drop(schema);

        let start = std::time::Instant::now();

        let mut all_traces = Vec::new();
        let mut allowed = false;

        for rel_name in &resolved.relations {
            let relation = match Relation::new(rel_name) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let result = traversal::bfs_traversal(
                self.storage.as_ref(),
                subject,
                &relation,
                resource,
                Some(revision),
            )?;

            if result.found {
                allowed = true;
                let trace: Vec<ExplainTrace> = result
                    .trace
                    .iter()
                    .map(|s| ExplainTrace {
                        subject: s.subject.clone(),
                        relation: s.relation.clone(),
                        object: s.object.clone(),
                    })
                    .collect();
                all_traces = trace;
                break;
            }
        }

        let duration_ms = start.elapsed().as_micros() as u64 / 1000;

        let resolved_via = if allowed && !all_traces.is_empty() {
            let steps: Vec<String> = all_traces
                .iter()
                .map(|t| format!("{}#{}", t.subject, t.relation))
                .collect();
            format!("→ {}", steps.join(" → "))
        } else if allowed {
            format!("direct relation '{}'", permission)
        } else {
            "no path found".to_string()
        };

        Ok(ExplainResult {
            allowed,
            revision,
            trace: all_traces,
            resolved_via,
            duration_ms,
        })
    }

    /// Write a relationship tuple and return a revision token.
    pub fn write(&self, tuple: &crate::types::RelationshipTuple) -> AegisResult<RevisionToken> {
        let _span = span!(
            Level::INFO,
            "aegis.write",
            subject = tuple.subject.as_str(),
            relation = tuple.relation.as_str(),
            resource = tuple.object.as_str(),
        )
        .entered();

        // Rate limit check
        let rl_key = format!("write:{}", tuple.object.as_str());
        self.rate_limiter.check(&rl_key, RateLimitOp::Write)?;

        let revision = self.storage.write_tuple(tuple)?;

        info!(revision = field::display(&revision), "tuple written");

        self.emit_watch_event(
            WatchEventType::TupleAdded,
            tuple.subject.as_str(),
            tuple.relation.as_str(),
            tuple.object.as_str(),
            revision,
        );

        self.hooks.trigger(&hooks::HookEvent::OnWrite {
            tuple: tuple.clone(),
        });

        Ok(RevisionToken::new(revision, self.node_id))
    }

    /// Delete a tuple by key.
    pub fn delete(&self, key: &crate::types::TupleKey) -> AegisResult<RevisionToken> {
        let _span = span!(
            Level::INFO,
            "aegis.delete",
            subject = key.subject.as_str(),
            relation = key.relation.as_str(),
            resource = key.object.as_str(),
        )
        .entered();

        // Rate limit check
        let rl_key = format!("delete:{}", key.object.as_str());
        self.rate_limiter.check(&rl_key, RateLimitOp::Write)?;

        let revision = self.storage.delete_tuple(key)?;

        info!(revision = field::display(&revision), "tuple deleted");

        self.emit_watch_event(
            WatchEventType::TupleRemoved,
            key.subject.as_str(),
            key.relation.as_str(),
            key.object.as_str(),
            revision,
        );

        self.hooks.trigger(&hooks::HookEvent::OnDelete {
            key: key.clone(),
        });

        Ok(RevisionToken::new(revision, self.node_id))
    }

    /// Invalidate the decision cache.
    pub fn invalidate_cache(&self) {
        self.cache.lock().unwrap().clear();
    }

    /// Invalidate cache entries older than a revision.
    pub fn invalidate_cache_before(&self, revision: Revision) {
        self.cache.lock().unwrap().invalidate_before(revision);
    }

    /// Resolve the revision to use for a check operation.
    fn resolve_revision(&self, consistency: Option<ConsistencyMode>) -> AegisResult<Revision> {
        match consistency {
            Some(ConsistencyMode::AtRevision(rev)) => {
                let current = self.storage.current_revision()?;
                if rev > current {
                    return Err(AegisError::RevisionFromFuture(
                        rev.as_u64() as usize,
                    ));
                }
                Ok(rev)
            }
            _ => self.storage.current_revision(),
        }
    }

    /// Write a tuple in dry-run mode: validates against schema but does not persist.
    pub fn write_dry_run(
        &self,
        tuple: &crate::types::RelationshipTuple,
    ) -> AegisResult<RevisionToken> {
        let resource_type = resource_type_name(tuple.object.as_str());
        let schema = self.schema.read().unwrap();
        let type_def = match schema.types.get(&resource_type) {
            Some(t) => t,
            None => return Err(AegisError::UnknownSubjectType(resource_type)),
        };
        if !type_def.relations.contains_key(tuple.relation.as_str()) {
            return Err(AegisError::UnknownRelation {
                type_name: resource_type,
                relation: tuple.relation.to_string(),
            });
        }
        let revision = self.storage.current_revision()?;
        Ok(RevisionToken::new(revision, self.node_id))
    }

    /// Query the audit log for a given object within an optional revision range.
    pub fn query_audit(
        &self,
        object: &ResourceId,
        from_revision: Option<Revision>,
        to_revision: Option<Revision>,
        pagination: &crate::types::PaginationParams,
    ) -> AegisResult<Vec<crate::types::AuditEntry>> {
        self.storage.query_audit(object, from_revision, to_revision, pagination)
    }

    /// Export all tuples for a given subject (GDPR compliance).
    pub fn export_subject(&self, subject: &SubjectId) -> AegisResult<Vec<crate::types::RelationshipTuple>> {
        self.storage.list_by_subject(subject, None)
    }

    /// Delete subject with an ownership policy (GDPR compliance).
    ///
    /// Policies:
    /// - `"cascade"` — remove all tuples for the subject.
    /// - `"fail"` — error if the subject has tuples.
    /// - `"transfer"` — reassign all tuples to `transfer_to_subject`.
    pub fn delete_subject_with_policy(
        &self,
        subject: &SubjectId,
        policy: &str,
        transfer_to_subject: Option<&SubjectId>,
    ) -> AegisResult<RevisionToken> {
        match policy {
            "cascade" => {
                let revision = self.storage.delete_subject(subject)?;
                Ok(RevisionToken::new(revision, self.node_id))
            }
            "fail" => {
                let tuples = self.storage.list_by_subject(subject, None)?;
                if tuples.is_empty() {
                    let revision = self.storage.current_revision()?;
                    Ok(RevisionToken::new(revision, self.node_id))
                } else {
                    Err(AegisError::OperationNotPermitted(
                        "subject has active tuples; use cascade or transfer policy".into(),
                    ))
                }
            }
            "transfer" => {
                let target = transfer_to_subject.ok_or_else(|| {
                    AegisError::SchemaValidation(
                        "transfer policy requires a transfer_to_subject".into(),
                    )
                })?;
                let tuples = self.storage.list_by_subject(subject, None)?;
                if tuples.is_empty() {
                    return Ok(RevisionToken::new(
                        self.storage.current_revision()?,
                        self.node_id,
                    ));
                }
                let mut txn = self.storage.begin_transaction()?;
                for tuple in &tuples {
                    let new_tuple = RelationshipTuple {
                        subject: target.clone(),
                        relation: tuple.relation.clone(),
                        object: tuple.object.clone(),
                        created_at: Utc::now(),
                        metadata: tuple.metadata.clone(),
                    };
                    txn.write(&new_tuple)?;
                    txn.delete(&tuple.key())?;
                }
                let revision = txn.commit()?;
                Ok(RevisionToken::new(revision, self.node_id))
            }
            _ => Err(AegisError::SchemaValidation(format!(
                "unknown ownership policy: '{policy}'; expected 'cascade', 'transfer', or 'fail'"
            ))),
        }
    }

    /// Write multiple tuples atomically within a single transaction.
    pub fn write_batch(
        &self,
        tuples: &[RelationshipTuple],
    ) -> AegisResult<RevisionToken> {
        let _span = span!(Level::INFO, "aegis.write_batch", count = tuples.len()).entered();
        let rl_key = "write_batch";
        self.rate_limiter.check(rl_key, RateLimitOp::Write)?;
        let revision = self.storage.write_tuples_batch(tuples)?;
        for tuple in tuples {
            self.emit_watch_event(
                WatchEventType::TupleAdded,
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
                revision,
            );
        }
        info!(revision = field::display(&revision), "batch written");
        Ok(RevisionToken::new(revision, self.node_id))
    }

    /// Begin a storage transaction for atomic multi-operation writes.
    pub fn transaction(&self) -> AegisResult<Box<dyn StorageTransaction>> {
        self.storage.begin_transaction()
    }

    /// List all tuples for a given object, optionally filtered by relation.
    pub fn list_by_object(
        &self,
        object: &ResourceId,
        relation: Option<&Relation>,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        self.storage.list_by_object(object, relation)
    }

    /// List all tuples for a given subject, optionally filtered by relation.
    pub fn list_by_subject(
        &self,
        subject: &SubjectId,
        relation: Option<&Relation>,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        self.storage.list_by_subject(subject, relation)
    }

    /// Query tuples with filters and pagination.
    pub fn query(
        &self,
        filter: &TupleFilter,
        pagination: &PaginationParams,
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<PaginatedTuples> {
        let consistency = consistency.unwrap_or_default();
        self.storage
            .query_tuples(filter, pagination, &consistency)
    }

    /// Run schema migrations to reach the target version.
    pub fn migrate(&self, target_version: u32) -> AegisResult<MigrationResult> {
        let runner = MigrationRunner::new();
        let current = self.storage.read_schema_version()?;
        let result = runner.migrate(self.storage.as_ref(), current, target_version)?;
        self.storage.write_schema_version(target_version)?;
        Ok(result)
    }

    /// Check whether a new schema is compatible with the currently loaded schema.
    pub fn check_schema(&self, new_schema: &Schema) -> SchemaCompatibilityReport {
        let current = self.schema.read().unwrap();
        crate::engine::migration::check_compatibility(&current, new_schema)
    }

    /// Delete all tuples for a given resource.
    pub fn delete_object(&self, object: &ResourceId) -> AegisResult<RevisionToken> {
        let _span = span!(Level::INFO, "aegis.delete_object", resource = object.as_str()).entered();
        let rl_key = format!("delete_object:{}", object.as_str());
        self.rate_limiter.check(&rl_key, RateLimitOp::Write)?;
        let revision = self.storage.delete_object(object)?;
        info!(revision = field::display(&revision), "object deleted");
        Ok(RevisionToken::new(revision, self.node_id))
    }

    /// Access GDPR compliance operations.
    pub fn gdpr(&self) -> gdpr::GdprManager<'_> {
        gdpr::GdprManager::new(self)
    }

    /// Access the rate limiter for configuration.
    pub fn rate_limiter(&self) -> &TokenBucketRateLimiter {
        &self.rate_limiter
    }

    /// Replace the rate limiter with a new configuration.
    pub fn set_rate_limiter(&mut self, limiter: TokenBucketRateLimiter) {
        self.rate_limiter = limiter;
    }
}

/// Extract the type name from a resource ID (e.g., "repo:fluxbus" -> "repo").
fn resource_type_name(id: &str) -> String {
    id.split(':').next().unwrap_or(id).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::types::*;

    fn make_engine() -> GraphEngine {
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types: {
                let mut types = std::collections::HashMap::new();
                let mut relations = std::collections::HashMap::new();
                relations.insert(
                    "owner".to_string(),
                    crate::types::schema::RelationDef {
                        inherit_from: vec![],
                        description: None,
                    },
                );
                relations.insert(
                    "viewer".to_string(),
                    crate::types::schema::RelationDef {
                        inherit_from: vec![],
                        description: None,
                    },
                );
                let mut permissions = std::collections::HashMap::new();
                permissions.insert(
                    "read".to_string(),
                    crate::types::schema::PermissionDef {
                        union_of: vec!["viewer".to_string(), "owner".to_string()],
                        condition: None,
                        description: None,
                    },
                );
                permissions.insert(
                    "admin".to_string(),
                    crate::types::schema::PermissionDef {
                        union_of: vec!["owner".to_string()],
                        condition: None,
                        description: None,
                    },
                );
                types.insert(
                    "repo".to_string(),
                    crate::types::schema::TypeDef {
                        relations,
                        permissions,
                    },
                );
                types
            },
        };

        let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
        storage.initialize().unwrap();
        GraphEngine::new(Box::new(storage), schema)
    }

    #[test]
    fn test_check_direct_allowed() {
        let engine = make_engine();
        let subject = SubjectId::new("user:alice").unwrap();
        let resource = ResourceId::new("repo:fluxbus").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let result = engine
            .check(&subject, "read", &resource, None)
            .unwrap();
        assert!(result.allowed);
        assert!(result.revision.as_u64() > 0);
    }

    #[test]
    fn test_check_denied() {
        let engine = make_engine();
        let result = engine
            .check(
                &SubjectId::new("user:alice").unwrap(),
                "read",
                &ResourceId::new("repo:fluxbus").unwrap(),
                None,
            )
            .unwrap();
        assert!(!result.allowed);
    }

    #[test]
    fn test_check_admin_permission() {
        let engine = make_engine();
        let subject = SubjectId::new("user:admin").unwrap();
        let resource = ResourceId::new("repo:critical").unwrap();

        // Admin has owner, so admin permission should be allowed
        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let result = engine
            .check(&subject, "admin", &resource, None)
            .unwrap();
        assert!(result.allowed);

        // viewer should NOT have admin
        let viewer = SubjectId::new("user:viewer").unwrap();
        engine
            .write(&RelationshipTuple::new(
                viewer.clone(),
                Relation::new("viewer").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let result = engine
            .check(&viewer, "admin", &resource, None)
            .unwrap();
        assert!(!result.allowed);
    }

    #[test]
    fn test_explain_returns_trace() {
        let engine = make_engine();
        let subject = SubjectId::new("user:alice").unwrap();
        let resource = ResourceId::new("repo:fluxbus").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let explain = engine
            .explain(&subject, "read", &resource, None)
            .unwrap();
        assert!(explain.allowed);
        assert!(explain.revision.as_u64() > 0);
    }

    #[test]
    fn test_check_unknown_permission_denies() {
        let engine = make_engine();
        let result = engine
            .check(
                &SubjectId::new("user:alice").unwrap(),
                "nonexistent",
                &ResourceId::new("repo:fluxbus").unwrap(),
                None,
            )
            .unwrap();
        assert!(!result.allowed);
    }

    #[test]
    fn test_cache_invalidation() {
        let engine = make_engine();
        let subject = SubjectId::new("user:cached").unwrap();
        let resource = ResourceId::new("repo:cached").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        // First check populates cache
        let result = engine
            .check(&subject, "read", &resource, None)
            .unwrap();
        assert!(result.allowed);

        // Invalidate and verify still works (cache miss is fine)
        engine.invalidate_cache();
        let result = engine
            .check(&subject, "read", &resource, None)
            .unwrap();
        assert!(result.allowed);
    }

    #[test]
    fn test_resource_type_extraction() {
        assert_eq!(resource_type_name("repo:fluxbus"), "repo");
        assert_eq!(resource_type_name("workspace:acme"), "workspace");
        assert_eq!(resource_type_name("nocolon"), "nocolon");
    }
}
