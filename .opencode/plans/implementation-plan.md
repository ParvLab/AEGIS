# Aegis Authorization Runtime — Implementation Plan

## Overview

End-to-end implementation plan organized by sprints. Each sprint builds on the previous, converging on the GA release.

### Legend
- ✅ **Done** — implemented, compiled, tested
- 🔧 **In Progress** — actively working
- ⬜ **Pending** — not started
- ❌ **Blocked** — blocked by prerequisite

---

## Sprint 0 — Bugfix & Stub Completion

Goal: Eliminate all `NotImplemented` stubs, fix upstream compilation errors, fix no-op implementations.

### Sprint 0.1 — PostgreSQL stubbed methods

- [x] `write_tuples_batch` — batch insert inside a transaction
- [x] `delete_subject` — soft-delete all tuples for a subject
- [x] `delete_object` — soft-delete all tuples for an object
- [x] `query_tuples` — dynamic SQL with subject/object/relation/metadata filters + pagination
- [x] `begin_transaction` — create `PostgresTransaction` wrapping a pooled connection
- [x] `PostgresTransaction` struct — full `StorageTransaction` impl (write, delete, savepoint, rollback_to_savepoint, release_savepoint, commit, rollback)
- [x] Fix `Cargo.toml`: tokio `rt-multi-thread`, tokio-postgres `with-chrono-0_4` + `with-serde_json-1`, deadpool-postgres `serde`
- [x] Fix `Manager::new` to pass `NoTls` (takes 2 args in 0.14)
- [x] Verify `cargo check --features postgres` compiles

### Sprint 0.2 — RocksDB stubbed methods

- [x] `query_tuples` — subject-type prefix scan, object-index filtered scan, metadata-key in-memory filtering, pagination cursor/limit
- [x] `begin_transaction` — `RocksDbTransaction` struct wrapping a `WriteBatch`, column family handles, node_id
- [x] `query_audit` — event CF iteration with revision range filtering + audit entry deserialization
- [x] `RocksDbTransaction` — full `StorageTransaction` impl via WriteBatch (write, delete, savepoint stubs, commit bumps revision + batch write, rollback drops batch)

### Sprint 0.3 — GDPR transfer ownership policy

- [x] Add `transfer_to_subject: Option<&SubjectId>` parameter to `delete_subject_with_policy`
- [x] Implement `"transfer"` policy: list subject tuples → write each with new subject → delete originals via transaction
- [x] Update error message to include `'transfer'` in valid policy list

### Sprint 0.4 — GDPR retention policy (real implementation)

- [x] Add `delete_events_before` / `delete_soft_deleted_tuples_before` to `StorageBackend` trait with default no-op impl
- [x] SQLite: `DELETE FROM _aegis_events WHERE timestamp < ?`
- [x] SQLite: `DELETE FROM _aegis_tuples WHERE revision_removed IS NOT NULL AND revision_removed <= (SELECT COALESCE(MAX(revision),0) FROM _aegis_events WHERE timestamp < ?)`
- [x] PostgreSQL: implemented via `_aegis_schema` table query
- [x] RocksDB: implemented via meta CF `schema_version` key
- [x] Wire `GdprManager` private methods to storage trait methods instead of no-ops

### Sprint 0.5 — FullyConsistent mode

- [x] In `SqliteStorage::query_tuples`: if `FullyConsistent` + WAL mode, run `PRAGMA wal_checkpoint(TRUNCATE)` before query

### Sprint 0.6 — Schema version tracking

- [x] Add `read_schema_version` / `write_schema_version` to `StorageBackend` trait with default no-op impl
- [x] SQLite: read/write from `_aegis_schema` table
- [x] PostgreSQL: read/write from `_aegis_schema` table
- [x] RocksDB: read/write `schema_version` key in meta CF
- [x] Fix `migration.rs::read_schema_version` to delegate to storage trait

### Sprint 0.7 — Regression verification

- [x] `cargo test -p aegis-core` — 167/167 pass
- [x] `cargo test -p aegis-test-utils` — 8/8 pass
- [x] `cargo check --features postgres` — compiles
- [x] `cargo check` (default) — compiles
- [x] Zero `NotImplemented` stubs in runtime code
- [x] Zero `todo!()` / `unimplemented!()` in storage backends

---

## Sprint 1 — V1 Complete

Goal: Expose all engine APIs, complete CLI, package for deployment.

### Sprint 1.1 — Expose missing engine APIs

- [x] `write_batch(&self, tuples: &[RelationshipTuple]) -> AegisResult<RevisionToken>`
- [x] `transaction(&self) -> AegisResult<Box<dyn StorageTransaction>>`
- [x] `list_by_object(&self, object: &ResourceId, relation: Option<&Relation>) -> AegisResult<Vec<RelationshipTuple>>`
- [x] `list_by_subject(&self, subject: &SubjectId, relation: Option<&Relation>) -> AegisResult<Vec<RelationshipTuple>>`
- [x] `query(&self, filter: &TupleFilter, pagination: &PaginationParams, consistency: Option<ConsistencyMode>) -> AegisResult<PaginatedTuples>`
- [x] `migrate(&self, target_version: u32) -> AegisResult<MigrationResult>`
- [x] `check_schema(&self, new_schema: &Schema) -> SchemaCompatibilityReport`
- [x] `delete_object(&self, object: &ResourceId) -> AegisResult<RevisionToken>`
- [x] NAPI-RS bindings: `list_by_subject`, `query`, `write_batch`, `migrate`, `check_schema`, `delete_object`
- [x] Verify `cargo test -p aegis-core` passes (167/167)

### Sprint 1.2 — CLI subcommands

- [x] `backup create <path>` — dump all tuples as JSON
- [x] `backup restore <path>` — restore tuples from backup
- [x] `export [--subject ...]` — export tuples in JSON
- [x] `import <file>` — import tuples from JSON file
- [x] `schema lint <path>` — validate schema file
- [x] `recover` — run event log compaction
- [x] Verify `cargo build -p aegis-cli` compiles

### Sprint 1.3 — SDK packaging & delivery

> **Note:** Aegis is an embedded library, not a standalone server. Users consume it as a package dependency (`cargo add`, `npm install`, `pip install`, `go get`). No Docker/K8s packaging is needed — the host application provides the runtime environment.

- [x] Verify all crates publish-ready: `aegis-core`, `aegis-cli`, `aegis-napi`, `aegis-test-utils`
- [x] Add `compact_events` to `StorageBackend` trait (default no-op, SQLite overrides)

---

## Sprint 2 — V2 (CRDT & Telemetry)

Goal: Multi-node replication, distributed tracing, rate limiting hardening.

### Sprint 2.1 — CRDT sync layer

- [ ] VersionVector — merge conflicts, causality tracking
- [ ] Replicator — pull/push delta exchange
- [ ] Delta exchange — serialize/deserialize tuple diffs
- [ ] HTTP transport — REST endpoints for sync
- [ ] Tests: two-node sync, conflict resolution

### Sprint 2.2 — OpenTelemetry integration

- [ ] Span creation for `check`, `write`, `delete`, `query_tuples`
- [ ] OTLP exporter configuration
- [ ] Decision cache hit/miss metrics
- [ ] Rate limiter metrics (tokens consumed, throttled requests)
- [ ] Health endpoint returns telemetry status

### Sprint 2.3 — Watch subscriptions

- [ ] `WatchSubscription` — client-side stream handle
- [ ] `WatchEvent` types — tuple_added, tuple_removed, heartbeat
- [ ] Polling-based watcher for backends that lack push notifications
- [ ] Tests: subscribe → write → receive event

---

## Sprint 3 — V3 (GDPR, Security, Hardening)

Goal: Production readiness — GDPR compliance, fail-closed hardening, performance optimization.

### Sprint 3.1 — GDPR compliance

- [ ] Right to erasure via policy system (cascade/transfer/fail)
- [ ] Data portability export (all subject data)
- [ ] Retention policy enforcement (scheduled)
- [ ] Audit log compaction (pair-matched add/remove removal)
- [ ] Tests: GDPR E2E scenarios

---

## Sprint 4 — Security & Production Hardening (parallel, weeks 12–14)

Goal: Production-grade security posture, fail-closed guarantees, operational readiness.

### Sprint 4.1 — Fail-closed hardening

- [ ] Storage connection loss → return `Err(AegisError::StorageConnection)` on all operations
- [ ] Schema validation error → reject write, fail-closed for `check`
- [ ] Rate limiter exhaustion → return `Err(AegisError::RateLimited)`
- [ ] Panic boundary — catch panics in async tasks, convert to `AegisError`
- [ ] Graceful degradation: if cache unavailable, fall through to storage (not fail)

### Sprint 4.2 — Input validation & DoS protection

- [ ] Max tuple size: reject tuples larger than configured limit (default 64KB)
- [ ] Max metadata pairs: reject metadata with > 16 entries
- [ ] Max identity length: already 256 chars, verify enforcement
- [ ] Max query depth: BFS traversal limit (default 50, configurable)
- [ ] Max pagination limit: cap at 10,000 per page
- [ ] Resource name validation: no path traversal, no null bytes

### Sprint 4.3 — Production hardening

- [ ] Graceful shutdown — drain in-flight requests, checkpoint WAL, close storage
- [ ] Signal handling — SIGTERM/SIGINT triggers graceful shutdown
- [ ] Health endpoint — liveness (storage reachable) + readiness (all subsystems ok)
- [ ] Memory limits — bound RocksDB block cache, SQLite mmap size
- [ ] File descriptor limits — detect ulimit and warn at startup
- [ ] Startup probe — verify storage access, schema loaded, rate limiter ready

### Sprint 4.4 — Security audit

- [ ] Secrets audit: verify no secrets in logs, error messages, metrics
- [ ] Dependency audit: `cargo audit` for known CVEs
- [ ] Penetration testing checklist:
  - [ ] Fuzz tuple/identity/resource inputs
  - [ ] Verify authorization boundary (no privilege escalation)
  - [ ] Verify deletion is soft (recoverable) unless explicit hard-delete
  - [ ] Verify audit log is append-only (not modifiable by writers)
- [ ] TLS for PostgreSQL/CRDT HTTP transport when configured
- [ ] API key / mTLS authentication for admin endpoints

### Sprint 4.5 — Performance & stress

- [ ] Traversal cache hit ratio > 90% on tree-shaped graphs
- [ ] Decision cache TTL-based eviction
- [ ] Connection pool tuning (SQLite WAL, PostgreSQL)
- [ ] Benchmark suite: `cargo bench` with baselines
- [ ] Memory profile: no leaks on long-running instances (24h soak)
- [ ] Throughput target: > 10,000 `check` ops/sec on single node

---

## Post-GA — SDKs & Ecosystem

- [ ] Go SDK — idiomatic Go client
- [ ] Python SDK — PEP 8, async support
- [ ] TypeScript SDK — NAPI bindings already exist, extend coverage
- [ ] Terraform provider — manage schemas as code
- [ ] Prometheus exporter — metrics endpoint
