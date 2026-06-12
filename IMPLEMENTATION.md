# Aegis — Implementation Roadmap

> Complete end-to-end build plan for the embedded ReBAC authorization runtime.
>
> **Total estimate:** ~40 weeks with 2-3 engineers
> **Phases:** 0 (Foundation) → 1 (Engine) → 2 (SDK) → 3 (Distributed)

## Current Status (June 2026)

**Sprints 0–7 Complete — 217 tests pass (211 unit + 6 integration), 0 failures**

| Sprint | Focus | Status | Key Deliverables |
|--------|-------|--------|-----------------|
| 0 | Security Hardening | ✅ | 18 vulnerabilities fixed |
| 1 | Engine Features | ✅ | ABAC, OTel, hot-reload, recover, lint, FullyConsistent, logger callback |
| 2 | Storage Backends | ✅ | MySQL, PG/RocksDB event compaction, required trait methods |
| 3 | NAPI/TS SDK | ✅ | 14 exports, JsWatchSubscription, JsTransaction |
| 4 | CLI & REPL | ✅ | 19 commands, REPL with tab completion + colors, --dry-run |
| 5 | Test Coverage | ✅ | 24 new tests across 12 categories |
| **6** | **Polish & Cleanup** | **✅** | **Dead code removal, deps cleanup, LRU cache, rate limiter GC, health fields, CI hardening, supply-chain docs** |
| 7 | Go & Python SDKs | ✅ | C FFI, Go CGo SDK, PyO3 bindings |
| 8 | Distributed Features | ❌ | Removed — embedded-only scope |

**Architecture:** Purely embedded — zero servers, ports, HTTP/gRPC listeners. SQLite + RocksDB as embedded storage backends (PG/MySQL available programmatically, not in CLI).

**Key decisions:** `std::thread::scope` for parallelism (not tokio), synchronous MPSC for watch events (zero threads), `AtomicBool` for parallel_eval toggle, `OnceLock<SdkMeterProvider>` for OTel.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Dependency Graph](#2-dependency-graph)
3. [Phase 0 — Foundation](#3-phase-0--foundation)
4. [Phase 1 — Engine Core](#4-phase-1--engine-core)
5. [Phase 2 — SDK & Integration](#5-phase-2--sdk--integration)
~~6. [Phase 3 — Distributed & Scale](#6-phase-3--distributed--scale) — Removed (embedded-only)~~
7. [Key Architectural Decisions](#7-key-architectural-decisions)
8. [Risk Assessment](#8-risk-assessment)
9. [Integration & E2E Test Index](#9-integration--e2e-test-index)

---

## 1. Architecture Overview

```
                    ┌─────────────────────────────────────┐
                    │       Application                    │
                    │   (Node/Go/Python/Rust)              │
                    └──────────────┬───────────────────────┘
                                   │
                    ┌──────────────▼───────────────────────┐
                    │      SDK Layer                        │
                    │  (NAPI / CGo / PyO3 / Pub)           │
                    └──────────────┬───────────────────────┘
                                   │
                    ┌──────────────▼───────────────────────┐
                    │      Aegis Runtime                    │
                    │                                       │
                    │  ┌─────────────────────────────────┐  │
                    │  │   Graph Engine                   │  │
                    │  │   ├─ Recursive Traversal         │  │
                    │  │   ├─ Cycle Detection             │  │
                    │  │   ├─ Parallel Sibling Eval       │  │
 │  │   │  (std::thread::scope)       │  │
                    │  │   └─ Policy Resolution           │  │
                    │  └───────────┬─────────────────────┘  │
                    │              │                         │
                    │  ┌───────────▼─────────────────────┐  │
                    │  │   Transaction Manager            │  │
                    │  │   + Revision Control             │  │
                    │  └───────────┬─────────────────────┘  │
                    │              │                         │
                    │  ┌───────────▼─────────────────────┐  │
                    │  │   Cache Layer                   │  │
                    │  │   (Decision + Traversal)         │  │
                    │  └─────────────────────────────────┘  │
                    │                                       │
                    │  ┌─────────────────────────────────┐  │
                    │  │   Connection Manager             │  │
                    │  │   ├─ Read Pool                   │  │
                    │  │   └─ Write Conn (serialized)     │  │
                    │  └───────────┬─────────────────────┘  │
                    └──────────────┼────────────────────────┘
                                   │
        ┌──────────────────────────┼──────────────────────────┐
        │          ┌───────────────▼───────────────┐          │
        │          │    Storage Adapter Layer       │          │
        │          │                                │          │
    ┌────▼────┐ ┌───────▼──────┐ ┌───────▼──────┐          │
    │ SQLite  │ │ RocksDB      │ │ PG/MySQL     │          │
    │ (WAL)   │ │ (LSM-tree)   │ │ (programmatic │          │
    └─────────┘ └──────────────┘ │  only, not    │          │
                                │  in CLI)      │          │
                                └───────────────┘          │
        │                                                    │
   ┌────▼────────────────────────────────────────────────┐   │
    │    ~~CRDT Sync Layer~~ — Removed (embedded-only)          │   │
    └─────────────────────────────────────────────────────┘   │
        │                                                    │
   ┌────▼────────────────────────────────────────────────┐   │
   │    Observability (OTel + Logger)                     │   │
   │    ├─ Spans for check/write/delete/explain            │   │
   │    ├─ Metrics (latency, count, cache, graph size)    │   │
   │    └─ Structured log callback                         │   │
   └─────────────────────────────────────────────────────┘   │
                                                             │
        ┌────────────────────────────────────────────────┐   │
        │    CLI + REPL                                   │   │
        │    ├─ aegis check / write / delete / explain    │   │
        │    ├─ aegis backup / export / import            │   │
        │    ├─ aegis schema lint                         │   │
        │    ├─ aegis health                              │   │
        │    └─ aegis repl (interactive shell)            │   │
        └────────────────────────────────────────────────┘   │
```

---

## 2. Dependency Graph

```
Phase 0 (Foundation)
  └── All phases depend on this

Phase 1.1 (SQLite + Connection Manager)
  └── 1.2 (Graph Engine)
       ├── 1.4 (Explain/Tracing)
       └── 1.6 (Cache)
            └── 1.7 (Write Operations)

1.3 (Transactions + Revisions)
  └── 1.5 (Schema Migration)
       └── 1.7 (Write Operations)

1.2 + 1.3 + 1.7
  └── Phase 2 (SDKs)
       ├── 2.1 (TypeScript SDK)
       ├── 2.2 (CLI)
       ├── 2.3 (REPL)
       └── 2.8 (Go/Rust/Python SDKs)

Phase 1 + 2 complete
  ├── Event Log, PG/MySQL, RocksDB, Watch Streams
  ├── OTel, GDPR
  └── Embedded engine stable
```

---

## 3. Phase 0 — Foundation (Weeks 1-4)

**Goal:** Project skeleton, tooling, CI, and core data structures.

### Status: ✅ COMPLETE

| Step | Component | Description | Status |
|------|-----------|-------------|--------|
| 0.1 | Rust crate scaffold | `cargo init` with workspace: `aegis-core` + `aegis-test-utils` | ✅ |
| 0.2 | Data model types | `SubjectId`, `ResourceId`, `Relation`, `RelationshipTuple`, `Revision`, `RevisionToken`, `Schema`, `TupleKey`, `TupleAction`, `ConsistencyMode`, pagination types | ✅ |
| 0.3 | Error hierarchy | `AegisError` enum (20+ variants across Storage, Schema, Validation, Consistency, RateLimit, Internal) + `AegisResult<T>` alias | ✅ |
| 0.4 | Schema parser + linter | YAML parser, orphan detection, circular inheritance check, undefined reference detection, compatibility checker | ✅ |
| 0.5 | Storage adapter trait | `StorageBackend` trait (15 methods: write, read, delete, list, query, transact, audit, integrity, close) + `StorageTransaction` + `TupleFilter` | ✅ |
| 0.6 | Build system | `Cargo.toml` workspace, `rust-toolchain.toml`, `rustfmt` + `clippy` config, `.gitignore` | ✅ |
| 0.7 | Test harness | `TestAegis` in-memory engine with write/check/delete/list/query/pagination; `load_fixture_yaml()`; 4 built-in fixtures | ✅ |
| 0.8 | CI pipeline | GitHub Actions: format check, clippy, test (debug + release), coverage, security audit | ✅ |

**Files created:** 22 source files across 2 crates

**Tests:** 217 total (211 unit + 6 integration) — all passing

---

## 4. Phase 1 — Engine Core (Weeks 5-12)

**Goal:** Full embedded authorization engine with SQLite, graph traversal, policies, and transactions.

### Sprint 1.1 — Storage Layer (Weeks 5-6)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 1.1.1 | SQLite adapter | Implement `StorageBackend` via `rusqlite` with WAL mode `PRAGMA` | Phase 0 (trait defined) |
| 1.1.2 | Connection manager | Read pool (`r2d2`) + single dedicated write connection; `busy_timeout=5000`; health checks | 1.1.1 |
| 1.1.3 | Revision counter | Atomic revision in `_aegis_meta` table; `next_revision()` as SQL function | 1.1.1 |
| 1.1.4 | Index implementation | B-tree indexes on `(subject, relation, object)`, `(object, relation)`, `(subject)` | 1.1.1 |
| 1.1.5 | Schema version table | `_aegis_schema(version, applied_at, checksum)`; read on init | 1.1.1 |

**Tests:** INT-001 through INT-005, PER-001 through PER-003

### Sprint 1.2 — Graph Engine (Weeks 6-7)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 1.2.1 | Recursive traversal | BFS/DFS engine with adjacency lookups via object/subject indexes | 1.1.4 |
| 1.2.2 | Cycle detection | `HashSet<(SubjectId, Relation)>` visited-node tracking; any revisit → branch returns deny | 1.2.1 |
| 1.2.3 | Parallel evaluation | `tokio::task::spawn` for sibling branches; `CancellationToken` for short-circuit on first allow | 1.2.1 |
| 1.2.4 | Policy resolution | Resolve `permission → [relations]` from schema; walk inheritance chain for each relation | Phase 0 (schema types) |
| 1.2.5 | `check()` implementation | lookup → resolve → traverse → decide; return `CheckResult { allowed, revision }` | 1.2.1–1.2.4 |

**Tests:** INT-001, INT-002, INT-011, INT-012, E2E-015

### Sprint 1.3 — Transactions & Revision Isolation (Weeks 7-8)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 1.3.1 | `begin_transaction()` | SQLite `BEGIN IMMEDIATE` → `StorageTransaction` implementation | 1.1.2 |
| 1.3.2 | Savepoints | `SAVEPOINT sp1` / `RELEASE sp1` / `ROLLBACK TO sp1` within transaction | 1.3.1 |
| 1.3.3 | Revision-based snapshots | Read against specific revision (stored in meta table); `SELECT ... WHERE revision <= target` | 1.1.3 |
| 1.3.4 | Consistency modes | `minimize_latency` (latest), `at_revision` (token-based), `fully_consistent` (flush + read) | 1.3.3 |
| 1.3.5 | Revision token encoding | `RevisionToken { revision: u64, node_id: Uuid, timestamp }` | Phase 0 (types) |

**Tests:** INT-010 through INT-014, INT-020 through INT-024

### Sprint 1.4 — Explain & Tracing (Week 8)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 1.4.1 | Trace path recording | Record visited nodes during traversal as `Vec<(SubjectId, Relation)>` | 1.2.1 |
| 1.4.2 | `explain()` API | `{ allowed, path: Vec<String>, resolvedVia: String, durationMs, revision, cacheHit }` | 1.4.1 |
| 1.4.3 | `resolvedVia` formatting | Format derivation chain: `"editor → member → owner"` (right-facing arrows) | 1.4.1 |

**Tests:** INT-007

### Sprint 1.5 — Schema Migration (Week 9)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 1.5.1 | Migration runner | Read `_aegis_schema` version; apply `up()` scripts in order; log each application | 1.1.5 |
| 1.5.2 | Migration scripts | V1→V2, V2→V3 as additive patterns (new types, new relations) | Phase 0 (schema types) |
| 1.5.3 | Rollback support | `down()` for each migration; `--to-version N` CLI flag | 1.5.1 |
| 1.5.4 | Compatibility check | `check_schema_compatibility(existing, new)` → `SchemaCompatibilityReport` | Phase 0 (validator) |
| 1.5.5 | Auto-migrate on init | `autoMigrate: true` → call migration runner during `initialize()` | 1.5.1 |

**Tests:** INT-030 through INT-037

### Sprint 1.6 — Cache Layer (Week 10)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 1.6.1 | Decision cache | `HashMap<(SubjectId, Relation, ResourceId, Revision), bool>`; TTL-based expiry; LRU eviction | 1.2.5 |
| 1.6.2 | Traversal cache | Cache `HashMap<(SubjectId, Relation), Vec<ResourceId>>` for intermediate path segments | 1.2.1 |
| 1.6.3 | Cache invalidation | On write → bump revision → compare `entry.revision < current_revision` → evict stale entries | 1.1.3 |
| 1.6.4 | Cache config | `maxEntries` (LRU cap), `decisionTtlMs` (TTL), `traversalCacheSize` | — |

**Tests:** INT-040 through INT-044

### Sprint 1.7 — Write Operations (Weeks 11-12)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 1.7.1 | Single `write()` | Validate → begin tx → upsert tuple → bump revision → commit → return token | 1.1.2, 1.3.1 |
| 1.7.2 | `writeBatch()` | Validate all → begin tx → upsert all → single revision bump → commit | 1.3.1 |
| 1.7.3 | `delete()` | Remove tuple by key → bump revision | 1.1.2, 1.3.1 |
| 1.7.4 | `deleteSubject()` | `DELETE FROM tuples WHERE subject = ?` → bump revision | 1.1.2 |
| 1.7.5 | `deleteObject()` | `DELETE FROM tuples WHERE object = ?` → bump revision | 1.1.2 |
| 1.7.6 | `list()` | Filter by subject/object/relation with optional relation filter | 1.1.4 |
| 1.7.7 | Paginated `query()` | Cursor-based: `SELECT ... WHERE id > cursor ORDER BY id LIMIT N` | 1.1.4 |
| 1.7.8 | Idempotent write | `INSERT OR REPLACE` for upsert; `DELETE` on non-existent = no-op | 1.1.1 |

**Tests:** INT-060 through INT-074

---

## 5. Phase 2 — SDK & Integration (Weeks 13-20)

**Goal:** Language bindings (TypeScript first), CLI tools, developer experience.

### Sprint 2.1 — TypeScript SDK (Weeks 13-15)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 2.1.1 | NAPI-RS binding | `napi-rs` async functions exposing all engine APIs | Phase 1 complete |
| 2.1.2 | TypeScript types | Full `.d.ts` matching the spec: `AegisConfig`, `WriteResult`, `CheckResult`, etc. | 2.1.1 |
| 2.1.3 | SDK wrapper class | `new Aegis(config)` → `initialize()` → all async methods | 2.1.2 |
| 2.1.4 | Async support | NAPI async work tasks; Promise-based API | 2.1.1 |
| 2.1.5 | Error mapping | `AegisError` enum variants → TypeScript `AegisError` class hierarchy | 2.1.1 |

**Tests:** E2E-001, SDK-001, SDK-002

### Sprint 2.2 — CLI Tools (Weeks 15-16)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 2.2.1 | CLI scaffold | `clap`-based binary with subcommands | Phase 1 complete |
| 2.2.2 | `aegis check` | `--subject --relation --object` → JSON/human output | 1.2.5 |
| 2.2.3 | `aegis backup` | `create` (dump to file) + `restore` (load from file) | 1.7.1 |
| 2.2.4 | `aegis export` / `import` | JSON format export/import of full graph | 1.7.6 |
| 2.2.5 | `aegis schema lint` | Load YAML schema file → run linter → diagnostics output | Phase 0 (schema linter) |
| 2.2.6 | `aegis health` | Connection status, schema version, revision, cache stats | 1.1.2 |

**Tests:** E2E-009, E2E-010, E2E-024, E2E-025

### Sprint 2.3 — REPL (Week 16)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 2.3.1 | REPL shell | `rustyline`-based interactive shell with history | Phase 1 complete |
| 2.3.2 | REPL commands | `write`, `check`, `delete`, `list`, `explain`, `watch`, `schema`, `health` | 1.2–1.7 |
| 2.3.3 | Tab completion | Auto-complete for commands + known subjects/relations/objects | 2.3.1 |
| 2.3.4 | Output formatting | Colored output; JSON mode (`--json` flag) | 2.3.1 |

### Sprint 2.4 — Test Helpers & Fixtures (Week 17)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 2.4.1 | `createTestAegis()` | In-memory SQLite instance + fixture loader | 1.1.1 |
| 2.4.2 | Fixture parser | YAML → schema + tuple list; `loadFixture()` method | Phase 0 (fixtures) |
| 2.4.3 | Snapshot assertion | `expect(check()).toMatchSnapshot()` via `insta` or custom | 1.2.5 |
| 2.4.4 | Test factories | `generateLargeFixture(n, m)` → `Vec<RelationshipTuple>` | Phase 0 (test-utils) |

### Sprint 2.5 — Error Handling & Health (Week 17)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 2.5.1 | `health()` API | Storage ping, schema version, revision, cache stats, connection pool stats | 1.1.2 |
| 2.5.2 | Storage integrity | `PRAGMA quick_check` (SQLite) on startup and periodic health checks | 1.1.1 |
| 2.5.3 | Graceful shutdown | `close()`: flush cache → checkpoint WAL → close connections | 1.1.2 |
| 2.5.4 | Fail-closed default | Storage error in hot path → return `deny`; configurable to fail-open | 1.2.5 |

**Tests:** INT-017, INT-018, ERR-001 through ERR-012

### Sprint 2.6 — Dry-Run & Audit (Week 18)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 2.6.1 | Dry-run check | `check({...}, {dryRun: true})` — evaluate without side effects or cache update | 1.2.5 |
| 2.6.2 | Dry-run write | `write({...}, {dryRun: true})` — validate schema + constraints, no persist | 1.7.1 |
| 2.6.3 | Audit log read | `audit({object, from, to})` → `Vec<AuditEntry>` | 1.1.1 |
| 2.6.4 | Event log (implicit) | Append-only `_aegis_events` table populated on every mutation | 1.7.1 |

**Tests:** INT-050 through INT-053, INT-090 through INT-093

### Sprint 2.7 — Webhook Hooks & Schema Hot-Reload (Week 19)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 2.7.1 | Hook callbacks | `onWrite(tuple)`, `onDelete(tuple)`, `onCheck(event)` registered in config | Phase 1 complete |
| 2.7.2 | Schema file watcher | `notify` crate for `FSEvent` on schema file | Phase 0 (schema) |
| 2.7.3 | Hot-reload logic | Detect change → validate compatibility → atomic swap → invalidate cache | 1.5.4 |

### Sprint 2.8 — Remaining SDKs (Week 20)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 2.8.1 | Go SDK | CGo bindings; `context.Context` support; idiomatic Go `Aegis` struct | Phase 1 complete |
| 2.8.2 | Rust SDK | Published `aegis` crate on `crates.io`; zero-overhead `pub use` API | Phase 1 complete |
| 2.8.3 | Python SDK | PyO3 bindings; `asyncio`-compatible async API | Phase 1 complete |

**Tests:** E2E-002, E2E-003, E2E-004, SDK-003 through SDK-009

---

## 6. ~~Phase 3 — Distributed & Scale~~ — Removed

❌ **Not applicable.** Aegis is and will remain an **embedded-only** authorization runtime. Distributed features (CRDT sync, edge replicas, distributed cache, multi-region consistency, gRPC servers, WAL-based sync) are explicitly out of scope. They would require external infrastructure (Redis, HTTP/gRPC servers, network coordination, CDC pipelines) that violates the embedded-first philosophy.

The existing event log, PostgreSQL/MySQL backends, RocksDB, watch/subscription streams, OpenTelemetry, and GDPR compliance have been moved into earlier phases or are part of the core embedded engine. |

**Tests:** E2E-024, E2E-052, E2E-053

### Sprint 3.8 — Consistency Tokens & Rate Limiting (Weeks 31-32)

| Step | Component | Description | Dependencies |
|------|-----------|-------------|--------------|
| 3.8.1 | Cross-node tokens | Token encodes `(revision, nodeId)`; validate token against local node | 1.3.5, 3.2.1 |
| 3.8.2 | Rate limiter | Token-bucket per tenant via `governor` crate | Phase 1 complete |
| 3.8.3 | Max traversal depth | Configurable depth limit; `deny` + trace when exceeded | 1.2.1 |
| 3.8.4 | Input validation hardening | Max length, character set, injection prevention (already at type level) | Phase 0 (types) |

**Tests:** ERR-007, SEC-001 through SEC-005

---

## 7. Key Architectural Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Storage engine** | SQLite WAL as primary; PG/RocksDB/IndexedDB as adapters | WAL allows concurrent reads + single writer; matches embedded-first philosophy |
| **Concurrency** | Single serialized writer + read pool via `r2d2` | No write-write conflicts; WAL enables non-blocking reads; simple locking model |
| **Consistency** | Revision-based snapshot isolation | Avoids TrueTime dependency; works in embedded context; 3 explicit modes |
| **Transaction** | SQLite ACID transactions (`BEGIN IMMEDIATE`) | Free rollback, savepoints, atomic batch writes; no distributed tx complexity |
| **Multi-node sync** | ❌ Not applicable — embedded-only | Each instance manages its own state; shared PG/MySQL for cross-instance data |
| **FFI** | NAPI-RS (Node), CGo (Go), PyO3 (Python) | Best-in-class FFI for each language ecosystem; wide community support |
| **Observability** | Optional OTel (no-op when absent) | Zero runtime cost when not configured; industry standard for metrics/tracing |
| **Cache** | Decision + Traversal, LRU + TTL, revision-invalidated | Simple, effective; no distributed cache coherence needed in embedded mode |
| **Error strategy** | Fail-closed by default | Authorization safety: deny on uncertainty; configurable to fail-open for non-critical paths |
| **Schema evolution** | Additive-only by default; compatibility check before apply | Safe zero-downtime migrations; breaking changes require explicit two-phase process |
| **ID format** | `type:identifier` (e.g. `user:123`, `repo:fluxbus`) | Simple, self-describing, no structured IDs needed; validated via regex |
| **Tenant isolation** | Namespace prefix scoping | No per-tenant infrastructure; natural graph partitioning via `tenant:id` prefix |

---

## 8. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| WAL checkpoint starvation under long reads | Medium | Medium | Watchdog monitor; abort stale read transactions after configurable timeout; `busy_timeout` |
| NAPI binding memory safety | Low | High | Heavy use of Rust's `#[napi]` safe patterns; `Valgrind` / `ASan` in CI; extensive integration tests |
| Go CGo overhead | Medium | Low | Minimize CGo call count; batch operations where possible; benchmark before optimization |
| Performance regression in deep traversals | Medium | Medium | `criterion` benchmark suite in CI; performance regression gate (`--bench -- --threshold 5%`) |
| Schema migration on live data | Low | High | Additive-only migrations by default; compatibility check before apply; rollback tested in staging |
| Concurrent access from multiple processes | Medium | Medium | SQLite file locking via WAL; document that multi-process requires PostgreSQL/RocksDB |
| Event log unbounded growth | Low | Medium | Configurable compaction + retention policy; archive to object storage |
| ~~Edge replica staleness (CRDT sync delay)~~ | ~~—~~ | ~~—~~ | ~~Removed (embedded-only)~~ |
| WASM size for IndexedDB backend | Medium | Low | Tree-shake unused backends; `wasm-opt` for binary size optimization |

---

## 9. Integration & E2E Test Index

Full test specifications are in [`aegis-test-plan.md`](./aegis-test-plan.md).

### Integration Tests (84 total)

| Category | Count | ID Range | Key Tests |
|----------|-------|----------|-----------|
| Basic Write + Check | 5 | INT-001–005 | Direct check, deny, empty graph, idempotent write, metadata |
| Transaction Semantics | 5 | INT-010–014 | Atomic writes, rollback, savepoints, empty tx, tx with reads |
| Revision & Consistency | 5 | INT-020–024 | Revision increments, read-your-writes, token staleness, consistent mode, latency mode |
| Schema & Migration | 8 | INT-030–037 | Invalid schema, circular types, auto-migration, version tracking, rollback, compatibility |
| Cache Behavior | 5 | INT-040–044 | Cache hit, invalidation, TTL, max size, cache miss |
| Dry-Run Mode | 4 | INT-050–053 | Dry-run check, write, validation failure, cache behavior |
| Deletion | 4 | INT-060–063 | Delete existing, non-existent, one of many, bulk subject |
| Query & List | 5 | INT-070–074 | List by object/subject/relation, pagination, cursor |
| Watch / Subscription | 5 | INT-080–084 | Subscribe, sinceRevision, unsubscribe, multiple, wildcard |
| Audit Log | 4 | INT-090–093 | All mutations, time range, object filter, entry structure |

### End-to-End Tests (29 total)

| Category | Count | ID Range | Key Tests |
|----------|-------|----------|-----------|
| Full SDK Lifecycle | 4 | E2E-001–004 | TypeScript, Go, Rust, Python |
| Multi-Language Interop | 3 | E2E-010–012 | Write in Go read in Node, write in Node read in Python, cross-language token |
| Persistence & Recovery | 6 | E2E-020–025 | SQLite restart, PG restart, backup/restore, export/import |
| Event Log Recovery | 3 | E2E-030–032 | Full recovery, PIT recovery, compaction |
| Middleware Integration | 4 | E2E-040–043 | Express allowed/forbidden/missing-auth, Hono |
| Deployment Modes | ~~4~~ → 1 | ~~E2E-050–053~~ → Embedded only | ❌ Distributed modes removed |

### Error, Stress, Persistence, Security, SDK, Benchmarks (70+ tests)

| Category | Count | ID Range |
|----------|-------|----------|
| Error Handling | 12 | ERR-001–012 |
| Concurrency & Stress | 10 | STR-001–010 |
| Persistence & Recovery | 7 | PER-001–007 |
| Multi-Tenancy Isolation | 6 | TEN-001–006 |
| Security & Boundary | 5 | SEC-001–005 |
| SDK Cross-Language | 9 | SDK-001–009 |
| Performance Benchmarks | 11 | BENCH-001–011 |

### Total: 217 implemented (211 unit + 6 integration), 0 failures — [tracked in `.opencode/plans/implementation-plan.md`]

---

## Appendix: Storage Decision Matrix

| Scenario | Recommended Backend | Config |
|----------|-------------------|--------|
| Local development | SQLite | `{ storage: "sqlite", path: "./aegis.db" }` |
| Unit / integration tests | In-memory SQLite | `createTestAegis()` |
| Single-server production | SQLite (WAL, PV mount) or PostgreSQL | See connection config examples |
| Multi-server production | PostgreSQL (shared) | `{ storage: "postgres", connectionString: "..." }` |
| High-write throughput | RocksDB | `{ storage: "rocksdb", path: "./aegis-data" }` |
| Browser / edge runtime | IndexedDB (WASM) | `{ storage: "indexeddb" }` |
| ~~Multi-region~~ | ~~Removed — embedded-only~~ | ❌ |

## Appendix: Version Compatibility

| Aegis Version | Protocol | Storage Schema | Migration Path |
|---------------|----------|---------------|----------------|
| V1 | rev-based | v1 (tuples + meta + schema) | — |
| V2 | rev-based | v2 (adds events + audit tables) | `migrate(v1 → v2)` |
| ~~V3~~ | ~~rev-based + CRDT~~ | ~~Removed — embedded-only~~ | ❌ |


---

*Document version 1.0 — Last updated: May 2026*
