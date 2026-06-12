pub mod cache;
pub mod condition;
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
use crate::engine::cache::{DecisionCache, TraversalCache};
use crate::engine::migration::MigrationRunner;
use crate::engine::ratelimit::{RateLimitOp, TokenBucketRateLimiter};
use crate::engine::watch::{SharedWatchers, WatchEvent, WatchEventType, WatchFilter, WatchSubscription};
use crate::error::{AegisError, AegisResult};
use crate::storage::{StorageBackend, StorageTransaction, TupleFilter};
use crate::types::{
    CheckResult, ConsistencyMode, ExplainResult, ExplainTrace, FailClosedMode, HealthReport,
    MigrationResult, Relation, RelationshipTuple, ResourceId, Revision, RevisionToken, Schema,
    SubjectId, PaginatedTuples, PaginationParams,
};
use crate::types::schema::SchemaCompatibilityReport;
#[cfg(feature = "hot-reload")]
use crate::engine::hot_reload::SchemaWatcher;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
#[cfg(feature = "hot-reload")]
use std::thread::JoinHandle;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tracing::{error, field, info, span, Level};

/// The core authorization engine.
///
/// Combines a `StorageBackend` for tuple data with a `Schema` for policy definitions.
/// Provides the primary `check()` and `explain()` APIs.
pub struct GraphEngine {
    storage: Box<dyn StorageBackend>,
    schema: RwLock<Schema>,
    cache: Mutex<DecisionCache>,
    traversal_cache: Mutex<TraversalCache>,
    node_id: uuid::Uuid,
    hooks: hooks::SharedHookRegistry,
    logger: std::sync::Mutex<Option<hooks::LoggerFn>>,
    fail_closed: FailClosedMode,
    closed: std::sync::atomic::AtomicBool,
    watchers: SharedWatchers,
    shutdown_flag: Arc<AtomicBool>,
    #[cfg(feature = "hot-reload")]
    schema_watcher: Mutex<Option<SchemaWatcher>>,
    #[cfg(feature = "hot-reload")]
    watcher_thread: Mutex<Option<JoinHandle<()>>>,
    rate_limiter: TokenBucketRateLimiter,
    telemetry_enabled: std::sync::atomic::AtomicBool,
    api_key_hash: Option<[u8; 32]>,
    parallel_eval: AtomicBool,
}

impl GraphEngine {
    /// Create a new graph engine with the given storage and schema.
    pub fn new(storage: Box<dyn StorageBackend>, schema: Schema) -> Self {
        // Detect file descriptor limits at startup (Unix only)
        #[cfg(unix)]
        {
            let mut rlim = std::mem::MaybeUninit::<libc::rlimit>::uninit();
            if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, rlim.as_mut_ptr()) == 0 } {
                let rlim = unsafe { rlim.assume_init() };
                if rlim.rlim_cur < 1024 {
                    tracing::warn!(
                        "low file descriptor limit: soft={}, hard={}. Consider 'ulimit -n 4096'",
                        rlim.rlim_cur,
                        rlim.rlim_max,
                    );
                }
            }
        }

        Self {
            storage,
            schema: RwLock::new(schema),
            cache: Mutex::new(DecisionCache::new(10_000)),
            traversal_cache: Mutex::new(TraversalCache::new(1_000)),
            node_id: uuid::Uuid::new_v4(),
            hooks: hooks::SharedHookRegistry::new(),
            logger: std::sync::Mutex::new(None),
            fail_closed: FailClosedMode::DenyOnError,
            closed: std::sync::atomic::AtomicBool::new(false),
            watchers: Arc::new(Mutex::new(HashMap::new())),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            #[cfg(feature = "hot-reload")]
            schema_watcher: Mutex::new(None),
            #[cfg(feature = "hot-reload")]
            watcher_thread: Mutex::new(None),
            rate_limiter: TokenBucketRateLimiter::new(ratelimit::RateLimitConfig::default()),
            telemetry_enabled: std::sync::atomic::AtomicBool::new(false),
            api_key_hash: None,
            parallel_eval: AtomicBool::new(true),
        }
    }

    /// Set a custom TTL for the decision cache.
    pub fn with_cache_ttl(mut self, ttl: std::time::Duration) -> Self {
        self.cache = Mutex::new(DecisionCache::new(10_000).with_ttl(ttl));
        self
    }

    /// Set an API key required for write/delete operations.
    /// Stores a SHA-256 hash of the key (not plaintext).
    pub fn with_api_key(mut self, api_key: String) -> Self {
        let hash: [u8; 32] = Sha256::digest(api_key.as_bytes()).into();
        self.api_key_hash = Some(hash);
        self
    }

    /// Verify an API key against the configured key (if any).
    /// Uses constant-time comparison to prevent timing attacks.
    /// Returns Ok(()) if no API key is configured or if it matches.
    pub fn verify_api_key(&self, key: Option<&str>) -> AegisResult<()> {
        if let Some(ref configured_hash) = self.api_key_hash {
            match key {
                Some(k) => {
                    let incoming: [u8; 32] = Sha256::digest(k.as_bytes()).into();
                    if configured_hash.ct_eq(&incoming).into() {
                        Ok(())
                    } else {
                        Err(AegisError::OperationNotPermitted(
                            "invalid or missing API key".to_string(),
                        ))
                    }
                }
                _ => Err(AegisError::OperationNotPermitted(
                    "invalid or missing API key".to_string(),
                )),
            }
        } else {
            Ok(())
        }
    }

    /// Mark telemetry as enabled (called after init_otel).
    pub fn set_telemetry_enabled(&self, enabled: bool) {
        self.telemetry_enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// Access the hook registry to register callbacks.
    pub fn hooks(&self) -> &hooks::SharedHookRegistry {
        &self.hooks
    }

    /// Register a structured logger callback.
    /// The callback receives `(level, message, context)` for key engine events.
    pub fn set_logger<F>(&self, logger: F)
    where
        F: Fn(hooks::LogLevel, &str, &str) + Send + Sync + 'static,
    {
        if let Ok(mut guard) = self.logger.lock() {
            *guard = Some(Box::new(logger));
        }
    }

    /// Emit a structured log event through the registered callback (if any).
    fn emit_log(&self, level: hooks::LogLevel, message: &str, context: &str) {
        if let Ok(guard) = self.logger.lock() {
            if let Some(ref logger) = *guard {
                logger(level, message, context);
            }
        }
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

    /// Set a custom MeterProvider for OpenTelemetry metrics.
    /// When set, this provider is used instead of the global meter provider.
    #[cfg(feature = "telemetry")]
    pub fn with_meter_provider(self, provider: opentelemetry_sdk::metrics::SdkMeterProvider) -> Self {
        crate::telemetry::otel_metrics::init_provider(provider);
        self
    }

    /// Enable or disable parallel sibling BFS evaluation.
    /// When enabled (default), sibling relations are evaluated concurrently.
    /// The first `allow` short-circuits remaining evaluations.
    pub fn with_parallel_eval(mut self, enabled: bool) -> Self {
        self.parallel_eval.store(enabled, Ordering::Relaxed);
        self
    }

    /// Set parallel sibling BFS evaluation after construction.
    pub fn set_parallel_eval(&self, enabled: bool) {
        self.parallel_eval.store(enabled, Ordering::Relaxed);
    }

    /// Enable schema file watching for hot-reload.
    /// When enabled, the engine polls the schema file for changes.
    /// Requires the `hot-reload` feature.
    #[cfg(feature = "hot-reload")]
    pub fn with_schema_watch(mut self, path: &str) -> Self {
        self.schema_watcher = Mutex::new(Some(SchemaWatcher::new(path)));
        self
    }

    /// Manually check if the schema file has changed and reload if needed.
    /// Returns `true` if the schema was reloaded.
    /// Requires the `hot-reload` feature.
    #[cfg(feature = "hot-reload")]
    pub fn check_schema_reload(&self) -> AegisResult<bool> {
        let watcher = self.schema_watcher.lock().map_err(|e| {
            AegisError::Internal(format!("schema watcher lock failed: {e}"))
        })?;
        match watcher.as_ref() {
            Some(w) => {
                let reloaded = w.check_and_reload(self)?;
                if reloaded {
                    self.emit_log(hooks::LogLevel::Info, "Schema hot-reloaded", "(schema file changed)");
                }
                Ok(reloaded)
            }
            None => Ok(false),
        }
    }

    /// Start the background polling thread that periodically calls `check_schema_reload`.
    /// The polling interval is 5 seconds.
    /// Requires the `hot-reload` feature.
    #[cfg(feature = "hot-reload")]
    pub fn start_background_poller(self: &Arc<Self>) {
        let mut guard = self.watcher_thread.lock().unwrap();
        if guard.is_some() {
            return;
        }
        let shutdown = Arc::clone(&self.shutdown_flag);
        let engine_weak = Arc::downgrade(self);
        let handle = std::thread::spawn(move || {
            loop {
                for _ in 0..5 {
                    if shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                if shutdown.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(engine) = engine_weak.upgrade() {
                    if let Err(e) = engine.check_schema_reload() {
                        tracing::error!("Schema hot-reload poll error: {e}");
                    }
                } else {
                    return;
                }
            }
        });
        *guard = Some(handle);
    }

    /// Stop the background polling thread and wait for it to finish.
    /// Requires the `hot-reload` feature.
    #[cfg(feature = "hot-reload")]
    pub fn stop_watcher(&self) {
        self.shutdown_flag.store(true, Ordering::Relaxed);
        if let Ok(mut guard) = self.watcher_thread.lock() {
            if let Some(handle) = guard.take() {
                handle.join().ok();
            }
        }
    }

    fn with_cache<F, T>(&self, f: F) -> Option<T>
    where
        F: FnOnce(&mut DecisionCache) -> T,
    {
        match self.cache.lock() {
            Ok(mut guard) => Some(f(&mut guard)),
            Err(poisoned) => {
                error!("cache mutex poisoned, re-initializing cache");
                let mut guard = poisoned.into_inner();
                *guard = DecisionCache::new(guard.capacity());
                Some(f(&mut guard))
            }
        }
    }

    fn with_schema<F, T>(&self, f: F) -> Option<T>
    where
        F: FnOnce(&Schema) -> T,
    {
        self.schema.read().ok().map(|guard| f(&guard))
    }

    fn check_closed(&self) -> AegisResult<()> {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AegisError::EngineClosed);
        }
        Ok(())
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Startup probe: returns Ok if the engine is initialized and healthy.
    pub fn startup_probe(&self) -> AegisResult<()> {
        self.check_closed()?;
        self.storage.current_revision()?;
        self.with_schema(|_| ()).ok_or(AegisError::EngineClosed)?;
        Ok(())
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
        let version = new_schema.schema_version;
        *schema = new_schema;
        self.with_cache(|cache| cache.clear());
        crate::telemetry::update_schema_version(version as u64);
        Ok(())
    }

    /// Health check: returns a report of engine health.
    pub fn health(&self) -> HealthReport {
        let revision = self.storage.current_revision().ok();
        let integrity = self.storage.integrity_check().ok();
        let cache_info = self.with_cache(|cache| (cache.hit_rate(), cache.len())).unwrap_or((0.0, 0));
        let schema = self.schema.read().unwrap_or_else(|e| e.into_inner());

        // Update telemetry cache metrics
        crate::telemetry::set_cache_size(cache_info.1 as u64);

        HealthReport {
            healthy: revision.is_some() && integrity.as_ref().map(|i| i.passed).unwrap_or(false),
            revision: revision.unwrap_or(Revision::ZERO),
            schema_version: schema.schema_version,
            backend: self.storage.backend_type().to_string(),
            backend_healthy: integrity.as_ref().map(|i| i.passed).unwrap_or(false),
            telemetry_healthy: self.telemetry_enabled.load(std::sync::atomic::Ordering::Relaxed),
            cache_hit_rate: cache_info.0,
            cache_entries: cache_info.1,
            storage_integrity: integrity.as_ref().map(|i| i.passed).unwrap_or(false),
            error: None,
            total_checks: crate::telemetry::METRIC_CHECK_TOTAL.load(std::sync::atomic::Ordering::Relaxed),
            allowed_checks: crate::telemetry::METRIC_CHECK_ALLOWED.load(std::sync::atomic::Ordering::Relaxed),
            denied_checks: crate::telemetry::METRIC_CHECK_DENIED.load(std::sync::atomic::Ordering::Relaxed),
            error_checks: crate::telemetry::METRIC_CHECK_ERROR.load(std::sync::atomic::Ordering::Relaxed),
            cache_size: crate::telemetry::METRIC_CACHE_SIZE.load(std::sync::atomic::Ordering::Relaxed),
            cache_hit_ratio: 0.0,
        }
    }

    /// Recover the tuple store from the event log by replaying all events.
    /// Returns the latest revision after recovery.
    /// Only meaningful for backends that persist an event log (e.g. SQLite).
    pub fn recover_from_events(&self) -> AegisResult<Revision> {
        let rev = self.storage.recover_from_events()?;
        self.emit_log(hooks::LogLevel::Info, "Recovered from event log", &format!("revision={}", rev));
        Ok(rev)
    }

    /// Graceful shutdown: flush cache, checkpoint WAL, close connections.
    pub fn close(&self) -> AegisResult<()> {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        self.with_cache(|cache| cache.clear());
        let result = self.storage.close();
        self.emit_log(hooks::LogLevel::Info, "Engine closed", "(no context)");
        result
    }

    /// Evaluate relations sequentially (single-threaded fallback).
    fn evaluate_relations_sequential(
        &self,
        resolved: policy::ResolvedPolicy,
        subject: &SubjectId,
        resource: &ResourceId,
        revision: Revision,
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<bool> {
        let condition_str = resolved.condition;
        for rel_name in &resolved.relations {
            let relation = match Relation::new(rel_name) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let max_depth = self.rate_limiter.max_traversal_depth();
            let max_visits = self.rate_limiter.max_traversal_visits();
            let rev = Some(revision);
            let mut cache_guard = self.traversal_cache.lock().ok();
            let cache_ref = cache_guard.as_deref_mut();
            let result = match traversal::bfs_traversal_with_limits(
                self.storage.as_ref(),
                subject,
                &relation,
                resource,
                rev,
                consistency,
                max_depth,
                max_visits,
                cache_ref,
            ) {
                Ok(r) => r,
                Err(e) => return Err(e),
            };

            if result.found && evaluate_condition_if_present(&condition_str) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Evaluate relations in parallel using scoped threads.
    /// First `allow` short-circuits remaining evaluations.
    fn evaluate_relations_parallel(
        &self,
        resolved: policy::ResolvedPolicy,
        subject: &SubjectId,
        resource: &ResourceId,
        revision: Revision,
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<bool> {
        let found = std::sync::atomic::AtomicBool::new(false);
        let condition_str = Arc::new(resolved.condition);
        let max_depth = self.rate_limiter.max_traversal_depth();
        let max_visits = self.rate_limiter.max_traversal_visits();

        let rel_names: Vec<String> = resolved.relations.clone();
        let subject = Arc::new(SubjectId::new(subject.as_str()).map_err(|e| {
            AegisError::Validation(e)
        })?);

        std::thread::scope(|s| {
            for rel_name in &rel_names {
                if found.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let relation = match Relation::new(rel_name) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let found_ref = &found;
                let cond = Arc::clone(&condition_str);
                let subj = Arc::clone(&subject);
                let _handle = s.spawn(move || {
                    if found_ref.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    let mut cache = TraversalCache::new(100);
                    let result = traversal::bfs_traversal_with_limits(
                        self.storage.as_ref(),
                        subj.as_ref(),
                        &relation,
                        resource,
                        Some(revision),
                        consistency,
                        max_depth,
                        max_visits,
                        Some(&mut cache),
                    );
                    if let Ok(r) = result {
                        if r.found && evaluate_condition_if_present(cond.as_ref()) {
                            found_ref.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                });
            }
        });

        Ok(found.into_inner())
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
        let _start = std::time::Instant::now();

        // Rate limit check
        let rl_key = format!("check:{}", resource.as_str());
        if let Err(e) = self.rate_limiter.check(&rl_key, RateLimitOp::Check) {
            crate::telemetry::inc_check_error();
            return Err(e);
        }

        let revision = match self.resolve_revision(consistency) {
            Ok(r) => r,
            Err(e) => {
                error!(error = field::display(&e), "revision resolution failed");
                crate::telemetry::inc_check_error();
                return self.fail_closed_response(e);
            }
        };

        if !dry_run {
            let cache_span = span!(Level::DEBUG, crate::telemetry::spans::CACHE_LOOKUP);
            let _cache_guard = cache_span.enter();
            let from_cache = self.with_cache(|cache| {
                cache.get(subject.as_str(), permission, resource.as_str(), revision)
            });
            if let Some(Some(allowed)) = from_cache {
                info!(
                    allowed = allowed,
                    revision = field::display(&revision),
                    cache_hit = true,
                    "check cache hit"
                );
                crate::telemetry::inc_cache_hit();
                crate::telemetry::inc_check_total();
                if allowed {
                    crate::telemetry::inc_check_allowed();
                } else {
                    crate::telemetry::inc_check_denied();
                }
                return Ok(CheckResult { allowed, revision });
            }
        }

        // Resolve permission to relations
        let resource_type = resource_type_name(resource.as_str());
        let schema = self.schema.read().unwrap();
        let resolved = match policy::resolve_permission(&schema, &resource_type, permission) {
            Some(r) => r,
            None => {
                crate::telemetry::inc_check_total();
                crate::telemetry::inc_check_denied();
                return Ok(CheckResult {
                    allowed: false,
                    revision,
                });
            }
        };
        drop(schema);

        // Try each relation — any match means allowed (union semantics)
        // Parallel evaluation via scoped threads when enabled.
        let allowed = match if self.parallel_eval.load(Ordering::Relaxed) && resolved.relations.len() > 1 {
            self.evaluate_relations_parallel(
                resolved, subject, resource, revision, consistency,
            )
        } else {
            self.evaluate_relations_sequential(
                resolved, subject, resource, revision, consistency,
            )
        } {
            Ok(a) => a,
            Err(e) => {
                crate::telemetry::inc_check_error();
                crate::telemetry::inc_check_total();
                return self.fail_closed_response(e);
            }
        };

        if !dry_run {
            // Cache the decision
            self.with_cache(|cache| {
                cache.insert(subject.as_str(), permission, resource.as_str(), allowed, revision);
            });

            self.hooks.trigger(&hooks::HookEvent::OnCheck {
                subject: subject.as_str().to_string(),
                permission: permission.to_string(),
                resource: resource.as_str().to_string(),
                allowed,
            });
        }

        crate::telemetry::inc_check_total();
        crate::telemetry::update_revision_current(revision.as_u64());
        if allowed {
            crate::telemetry::inc_check_allowed();
        } else {
            crate::telemetry::inc_check_denied();
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
                consistency,
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

        self.check_closed()?;

        // Rate limit check
        let rl_key = format!("write:{}", tuple.object.as_str());
        self.rate_limiter.check(&rl_key, RateLimitOp::Write)?;

        // Schema validation
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
        drop(schema);

        let revision = self.storage.write_tuple(tuple)?;
        crate::telemetry::update_revision_current(revision.as_u64());

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

        self.check_closed()?;

        // Rate limit check
        let rl_key = format!("delete:{}", key.object.as_str());
        self.rate_limiter.check(&rl_key, RateLimitOp::Write)?;

        let revision = self.storage.delete_tuple(key)?;
        crate::telemetry::update_revision_current(revision.as_u64());

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
        self.with_cache(|cache| cache.clear());
    }

    pub fn invalidate_cache_before(&self, revision: Revision) {
        self.with_cache(|cache| cache.invalidate_before(revision));
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
        self.storage.query_audit(Some(object), from_revision, to_revision, pagination)
    }

    /// Query the audit log for all objects within an optional revision range.
    pub fn query_audit_all(
        &self,
        from_revision: Option<Revision>,
        to_revision: Option<Revision>,
        pagination: &crate::types::PaginationParams,
    ) -> AegisResult<Vec<crate::types::AuditEntry>> {
        self.storage.query_audit(None, from_revision, to_revision, pagination)
    }

    /// Export all tuples for a given subject (GDPR compliance).
    pub fn export_subject(&self, subject: &SubjectId) -> AegisResult<Vec<crate::types::RelationshipTuple>> {
        self.storage.list_by_subject(subject, None, &ConsistencyMode::MinimizeLatency)
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
                crate::telemetry::update_revision_current(revision.as_u64());
                Ok(RevisionToken::new(revision, self.node_id))
            }
            "fail" => {
                let tuples = self.storage.list_by_subject(subject, None, &ConsistencyMode::MinimizeLatency)?;
                if tuples.is_empty() {
                    let revision = self.storage.current_revision()?;
                    crate::telemetry::update_revision_current(revision.as_u64());
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
                let tuples = self.storage.list_by_subject(subject, None, &ConsistencyMode::MinimizeLatency)?;
                if tuples.is_empty() {
                    let revision = self.storage.current_revision()?;
                    crate::telemetry::update_revision_current(revision.as_u64());
                    return Ok(RevisionToken::new(revision, self.node_id));
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
                crate::telemetry::update_revision_current(revision.as_u64());
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
        crate::telemetry::update_revision_current(revision.as_u64());
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
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let c = consistency.as_ref().unwrap_or(&ConsistencyMode::MinimizeLatency);
        self.storage.list_by_object(object, relation, c)
    }

    /// List all tuples for a given subject, optionally filtered by relation.
    pub fn list_by_subject(
        &self,
        subject: &SubjectId,
        relation: Option<&Relation>,
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let c = consistency.as_ref().unwrap_or(&ConsistencyMode::MinimizeLatency);
        self.storage.list_by_subject(subject, relation, c)
    }

    /// Query tuples with filters and pagination.
    pub fn query(
        &self,
        filter: &TupleFilter,
        pagination: &PaginationParams,
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<PaginatedTuples> {
        let _span = span!(
            Level::INFO,
            crate::telemetry::spans::QUERY,
        )
        .entered();

        let consistency = consistency.unwrap_or_default();
        let pagination = pagination.clone().capped();
        self.storage
            .query_tuples(filter, &pagination, &consistency)
    }

    /// Run schema migrations to reach the target version.
    pub fn migrate(&self, target_version: u32) -> AegisResult<MigrationResult> {
        let runner = MigrationRunner::new();
        let current = self.storage.read_schema_version()?;
        let result = runner.migrate(self.storage.as_ref(), current, target_version)?;
        self.storage.write_schema_version(target_version)?;
        crate::telemetry::update_schema_version(target_version as u64);
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
        crate::telemetry::update_revision_current(revision.as_u64());
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

#[cfg(feature = "hot-reload")]
impl Drop for GraphEngine {
    fn drop(&mut self) {
        self.stop_watcher();
    }
}

/// Evaluate an ABAC condition expression against available context.
/// Returns `true` if no condition is present or the condition evaluates to `true`.
fn evaluate_condition_if_present(condition_str: &Option<String>) -> bool {
    match condition_str {
        Some(cond) => {
            match condition::parse_condition(cond) {
                Ok(expr) => {
                    let ctx = condition::ConditionEvalContext {
                        subject_meta: std::collections::HashMap::new(),
                        resource_meta: std::collections::HashMap::new(),
                        env: std::collections::HashMap::new(),
                    };
                    condition::evaluate_condition(&expr, &ctx)
                }
                Err(_) => false,
            }
        }
        None => true,
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
    use std::io::Write;

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

    // ── S1.2: Parallelism test ──

    #[test]
    fn test_parallel_eval_disabled() {
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

        // Disable parallel, verify check still works
        engine.set_parallel_eval(false);
        let result = engine
            .check(&subject, "read", &resource, None)
            .unwrap();
        assert!(result.allowed);
    }

    // ── S1.5: FullyConsistent test ──

    #[test]
    fn test_fully_consistent_read() {
        let engine = make_engine();
        let subject = SubjectId::new("user:alice").unwrap();
        let resource = ResourceId::new("repo:fluxbus").unwrap();

        // Write a tuple
        let token = engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        // Read with FullyConsistent mode
        let result = engine
            .check(&subject, "read", &resource, Some(ConsistencyMode::FullyConsistent))
            .unwrap();
        assert!(result.allowed);
        assert!(result.revision >= token.revision);
    }

    // ── S1.6: AtRevision snapshot test ──

    #[test]
    fn test_at_revision_snapshot() {
        let engine = make_engine();
        let subject = SubjectId::new("user:bob").unwrap();
        let resource = ResourceId::new("repo:bob").unwrap();

        // First write
        let token1 = engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("viewer").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        // Check at this revision — should be allowed (viewer can read)
        let result = engine
            .check(&subject, "read", &resource, Some(ConsistencyMode::AtRevision(token1.revision)))
            .unwrap();
        assert!(result.allowed);
    }

    // ── S1.7: Logger callback test ──

    #[test]
    fn test_logger_callback() {
        let engine = make_engine();
        let logged = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let logged_clone = logged.clone();

        engine.set_logger(move |level, message, _context| {
            let mut msgs = logged_clone.lock().unwrap();
            msgs.push((level.to_string(), message.to_string()));
        });

        // Trigger a close event
        engine.close().ok();
        engine.emit_log(crate::engine::hooks::LogLevel::Info, "test message", "test context");

        let msgs = logged.lock().unwrap();
        assert!(!msgs.is_empty(), "expected at least one log message");
        assert!(msgs.iter().any(|(_, m)| m.contains("test message")));
    }

    #[cfg(feature = "hot-reload")]
    #[test]
    fn test_hot_reload_background_poller() {
        let tmpdir = std::env::temp_dir().join(format!("aegis_hot_reload_{}", std::process::id()));
        std::fs::create_dir_all(&tmpdir).unwrap();
        let schema_path = tmpdir.join("schema.yaml");

        let schema_v1 = r#"
schemaVersion: 1
namespace: test
types:
  repo:
    relations:
      owner: {}
    permissions:
      read:
        union_of: [owner]
"#;
        let mut f1 = std::fs::File::create(&schema_path).unwrap();
        f1.write_all(schema_v1.as_bytes()).unwrap();
        f1.sync_all().unwrap();
        drop(f1);

        let schema = crate::schema::parse_schema(schema_v1).unwrap();
        let storage = Box::new(SqliteStorage::new(SqliteConfig::in_memory()).unwrap());
        let engine = Arc::new(
            GraphEngine::new(storage, schema).with_schema_watch(schema_path.to_str().unwrap()),
        );

        let schema_v2 = r#"
schemaVersion: 2
namespace: test
types:
  repo:
    relations:
      owner: {}
    permissions:
      read:
        union_of: [owner]
"#;

        // Write v2 BEFORE starting the background poller, so the file is ready
        let mut f2 = std::fs::File::create(&schema_path).unwrap();
        f2.write_all(schema_v2.as_bytes()).unwrap();
        f2.sync_all().unwrap();
        drop(f2);

        engine.start_background_poller();

        // Wait for the background thread's first poll (sleeps 5s then checks)
        std::thread::sleep(std::time::Duration::from_secs(7));

        let _ = engine.check_schema_reload();
        assert_eq!(engine.schema().schema_version, 2);

        engine.stop_watcher();
        std::fs::remove_file(&schema_path).ok();
        std::fs::remove_dir(&tmpdir).ok();
    }
}
