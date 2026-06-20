# Aegis Complete End-to-End Implementation Plan

## Purpose

This document defines the execution plan for Aegis as a complete embedded authorization runtime.

It is intended to be:

- strategic enough to define the product end state,
- concrete enough to execute quarter by quarter,
- trackable enough to measure progress milestone by milestone,
- strict enough to preserve the embedded-only product boundary.

This plan covers:

- product definition,
- non-negotiable architecture constraints,
- version roadmap,
- milestone-based execution,
- dependency and critical-path analysis,
- workstreams,
- risks,
- testing strategy,
- performance targets,
- upgrade and compatibility policy,
- release gates,
- completion criteria.

## Overall Progress Dashboard

Update this section continuously during execution.

Legend:

- `NS` = not started
- `IP` = in progress
- `BL` = blocked
- `DN` = done

| Version | Status | Overall | M1 | M2 | M3 | M4 | Target Window |
|---|---:|---|---|---|---|---|---:|
| V1 Embedded Core Foundation | DN | 100% | DN | DN | DN | DN | Q1-Q2 |
| V2 Multi-Model Authorization | DN | 100% | DN | DN | DN | DN | Q3 |
| V3 SDK and Developer Platform Completion | NS | 0% | NS | NS | NS | NS | Q4-Q5 |
| V4 Enterprise Reliability and Governance | DN | 100% | DN | DN | DN | DN | Q6-Q7 |
| V5 Authorization Everywhere — Browser, Offline, Worker | IP | 15% | DN | NS | NS | NS | Q8 |
| V6 Policy Intelligence and Ecosystem | NS | 0% | NS | NS | NS | NS | Q10-Q11 |

## Program Tracking Model

Track progress at three levels.

### 1. Version Level

Each version has:

- a goal,
- four milestones,
- exit criteria,
- blocked conditions,
- dependency rules.

### 2. Milestone Level

Each milestone must include:

- scope,
- owner,
- target weeks,
- status,
- objective evidence of completion.

Recommended status template for execution updates:

```text
Milestone: V1-M2
Status: IP
Owner: <name>
Start: <date>
Target Complete: <date>
Percent Complete: <n>%
Blocked By: <none or dependency>
Evidence:
- <completed item>
- <completed item>
Remaining:
- <remaining item>
- <remaining item>
```

### 3. Workstream Level

Each workstream should maintain its own checklist and map tasks back to version milestones.

## Product Definition

Aegis is an embedded authorization runtime that runs inside an application process instead of as a separately deployed authorization service.

Core principles:

- authorization should behave like a library, not a network service,
- permission checks should be local function calls, not HTTP or gRPC requests,
- the authorization graph should remain durable even when embedded,
- ReBAC should be the native graph model,
- RBAC, ACL, ownership, hierarchy, tenancy, and ABAC should be supported through one runtime,
- Aegis itself must never require a standalone Aegis server.

## Non-Negotiable Architectural Boundaries

Aegis must remain:

- embedded,
- in-process,
- library-first,
- SDK-first,
- CLI-assisted.

Aegis must not become:

- a separately deployed auth service,
- a long-running authorization daemon,
- an HTTP API server for permission checks,
- a gRPC server for permission checks,
- a port-binding runtime requirement,
- a mandatory control plane.

Acceptable external dependencies:

- SQLite file for embedded persistent storage,
- RocksDB directory for embedded persistent storage,
- PostgreSQL as optional external shared storage,
- MySQL as optional external shared storage,
- OpenTelemetry exporters when explicitly enabled.

Clarifications:

- PostgreSQL and MySQL are storage adapters only.
- They do not turn Aegis into a service.
- The Aegis runtime still executes inside the host process.
- If a host application exposes HTTP or gRPC, that host surface is not part of Aegis itself.

## Authorization Model Strategy

Aegis should be a multi-model authorization engine with ReBAC as the foundation.

Model strategy:

- ReBAC: first-class and native,
- RBAC: implemented as a graph pattern over relationships,
- ACL: represented as direct tuples,
- ownership: represented as direct tuples and hierarchy,
- hierarchical authorization: represented through recursive traversal,
- tenant-scoped authorization: represented through explicit partitioning and traversal enforcement,
- ABAC: represented through conditions over subject, resource, relation, request, and environment context,
- time-bounded grants: implemented through ABAC conditions or tuple metadata,
- explicit deny rules: optional advanced feature with clearly defined precedence.

## Planning Assumptions

This timeline assumes:

- 2 to 4 engineers full time,
- 1 docs-capable engineer part time,
- 1 QA or SDET equivalent from mid-cycle onward,
- CI, release automation, package publishing, and test infrastructure are part of the effort.

Time units:

- 1 quarter = 12 weeks,
- 1 half = 24 weeks,
- 1 active engineering year = 48 weeks.

## Team Model and Capacity Assumptions

Recommended baseline staffing:

- Engineer A: engine, storage, transactions,
- Engineer B: schema, evaluation, explainability,
- Engineer C: SDKs, CLI, packaging,
- Engineer D: enterprise, browser, tooling, or shared overflow,
- QA/SDET: integration, matrix, recovery, conformance,
- docs owner: user docs, reference docs, migration guides.

Recommended allocation by phase:

| Version | Engine/Storage | Schema/Auth | SDK/CLI | QA/Infra | Docs |
|---|---:|---:|---:|---:|---:|
| V1 | 1.5 | 0.75 | 0.5 | 0.25 | 0.25 |
| V2 | 1.0 | 1.25 | 0.5 | 0.25 | 0.25 |
| V3 | 0.75 | 0.25 | 1.5 | 0.5 | 0.25 |
| V4 | 1.0 | 0.5 | 0.5 | 0.75 | 0.25 |
| V5 | 1.25 | 0.25 | 1.0 | 0.5 | 0.25 |
| V6 | 0.75 | 1.0 | 0.75 | 0.5 | 0.25 |

## Version Strategy

Aegis should be delivered through these versions:

- V1: Embedded Core Foundation,
- V2: Multi-Model Authorization,
- V3: SDK and Developer Platform Completion,
- V4: Enterprise Reliability and Governance,
- V5: Authorization Everywhere — Browser, Offline, and Worker Runtime,
- V6: Advanced Policy Intelligence and Ecosystem.

Each version contains:

- goal,
- timeline,
- dependency rules,
- milestone plan,
- deliverables,
- exit criteria.

## Dependency Graph and Critical Path

### Major Dependencies

- V1 is the foundation for everything else.
- V2 depends on V1 engine lifecycle, schema system, traversal semantics, and stable data contracts.
- V3 depends on a stable V1 API surface and partially on V2 if SDKs must expose RBAC, ABAC, and tenant-aware helpers.
- V4 depends on V1 durability and V3 packaging stability.
- V5 depends on V4 sync StorageBackend trait stability, the new AsyncStorageBackend trait, and stable schema/evaluation semantics from V1-V2.
- V6 depends on V2 explainability and V4 audit/compliance foundations.

### Critical Path

The minimum path to a credible production Aegis release is:

1. V1 core engine lifecycle,
2. V1 SQLite durability and recovery,
3. V1 schema and migration stability,
4. V1 CLI and validation coverage,
5. V2 tenant enforcement and ABAC grammar,
6. V3 SDK parity and packaging,
7. V4 backup/restore fidelity and compliance hardening.

V5 and V6 are valuable but not on the critical path for initial embedded platform maturity.

### Parallelization Guidance

The following can run in parallel once the required contracts exist:

- test infrastructure can start in week 1 of V1,
- CLI work can begin after core query/mutation contracts are stable,
- Node/Python/Go SDK wrappers can start once Rust API and FFI contracts are frozen,
- security automation can begin during late V1 and continue through V4,
- browser feasibility spikes can start before full V5 if the Rust core is stable enough.

### Dependency Rules Per Workstream

| Workstream | Hard Dependency | Can Start Earlier? |
|---|---|---|
| Core runtime | none | no |
| Authorization models | schema + traversal core | partially |
| Storage and durability | core runtime | no |
| Transactions and consistency | storage semantics | partially |
| Schema and policy | parser contract | no |
| Explainability and auditability | evaluation engine | partially |
| Performance and caching | baseline implementation | partially |
| SDKs and embeddings | stable Rust API / FFI | partially |
| CLI and REPL | stable query/mutation surface | partially |
| Security and compliance | storage + audit foundations | partially |
| Testing and verification | none | yes |
| Documentation and examples | baseline API semantics | yes |
| Packaging and release engineering | publishable artifacts | partially |
| Browser and edge | stable core and schema semantics | partially |
| Governance and enterprise operations | audit and policy diff foundations | partially |

## Master Workstreams

The implementation plan is organized into these workstreams:

1. Core runtime
2. Authorization models
3. Storage and durability
4. Transactions and consistency
5. Schema and policy system
6. Explainability and auditability
7. Performance and caching
8. SDKs and embeddings
9. CLI and REPL
10. Security and compliance
11. Testing and verification
12. Documentation and examples
13. Packaging and release engineering
14. Browser and edge
15. Governance and enterprise operations

## Version Plans

## V1: Embedded Core Foundation

### Goal

Deliver a complete embedded authorization engine that runs inside an application process, persists data durably, evaluates permissions locally, and exposes stable Rust, CLI, and basic cross-language functionality.

### Timeline

- Duration: 20 weeks
- Suggested window: Quarter 1 to Quarter 2

### Dependencies

- none; this is the base version.

### V1 Scope

#### Core runtime

- stable `GraphEngine` lifecycle,
- initialization and shutdown guarantees,
- clear engine configuration object,
- fail-closed by default,
- stable in-process Rust API,
- closed-state enforcement across all public methods.

#### Data model

- subject ID validation,
- resource ID validation,
- relation validation,
- relationship tuple validation,
- metadata validation rules,
- tuple key helpers,
- revision and revision token types,
- pagination types.

#### Storage

- production-grade SQLite backend,
- WAL mode support,
- busy timeout configuration,
- soft delete semantics,
- revision tracking,
- schema version tracking,
- audit events table,
- recovery from event log,
- backup and export foundation,
- graceful close and checkpoint behavior.

#### Query and mutation APIs

- `write()`,
- `write_batch()`,
- `delete()`,
- `delete_subject()`,
- `delete_object()`,
- `list_by_subject()`,
- `list_by_object()`,
- `list_by_relation()`,
- `query()`,
- `query_audit()`,
- `health()`,
- `close()`.

#### Authorization evaluation

- direct tuple checks,
- recursive traversal,
- cycle prevention,
- traversal depth limit,
- traversal visit limit,
- direct relation checks,
- permission resolution through schema,
- explain result path generation.

#### Consistency and transactions

- current revision tracking,
- `AtRevision` support,
- `FullyConsistent` support,
- transaction trait and implementation,
- savepoints,
- atomic write batch semantics.

#### Schema and policy

- schema parser,
- schema validator,
- compatibility checker,
- permissions-to-relations resolution,
- migration framework,
- schema reload API.

#### Observability

- local metrics counters,
- health report shape,
- structured log callback,
- optional tracing integration.

#### CLI and REPL

- `check`,
- `write`,
- `delete`,
- `list`,
- `query`,
- `explain`,
- `audit`,
- `health`,
- `backup create`,
- `backup restore`,
- `export`,
- `import`,
- `recover`,
- `schema lint`,
- `repl`.

#### Compliance and audit

- audit log reads,
- subject export,
- subject deletion policies,
- event compaction,
- retention policy API foundation.

### V1 Milestones

#### V1-M1: Core engine and SQLite foundation

- Target weeks: 1-5
- Status: NS
- Owner: unassigned

Scope:

- finalize `GraphEngine` lifecycle,
- add config-driven initialization,
- land SQLite WAL and revision semantics,
- complete tuple validation,
- complete direct tuple `check`, `write`, and `delete` flows.

Completion evidence:

- CRUD contract tests pass on SQLite,
- engine close-state tests pass,
- cold start and clean shutdown verified,
- revision increments verified for single and batch writes.

Blocked if:

- engine lifecycle remains ambiguous,
- storage contract differs across constructors,
- close-state behavior remains inconsistent.

#### V1-M2: Schema, traversal, and query surface

- Target weeks: 6-10
- Status: NS
- Owner: unassigned

Scope:

- finalize schema parser and validator,
- implement permission resolution through schema,
- complete recursive traversal with cycle prevention,
- add list/query APIs,
- add explain path generation.

Completion evidence:

- schema fixtures parse successfully,
- traversal tests cover depth, cycles, and visit limits,
- query/list contract tests pass,
- explain output verified against golden fixtures.

Blocked if:

- canonical schema syntax is unresolved,
- traversal rules are not deterministic,
- query semantics differ by backend.

#### V1-M3: Transactions, CLI, and restore foundations

- Target weeks: 11-15
- Status: NS
- Owner: unassigned

Scope:

- complete transaction and savepoint behavior,
- standardize `AtRevision` and `FullyConsistent` semantics,
- implement CLI command surface,
- add export/import and backup/restore foundation,
- add `health` and `audit` flows.

Completion evidence:

- transaction rollback/savepoint tests pass,
- CLI E2E tests pass for documented commands,
- export/import round-trip passes,
- health report structure is stable and documented.

Blocked if:

- consistency semantics remain backend-specific without documentation,
- CLI contract diverges from docs,
- restore format is still tuple-only in practice.

#### V1-M4: Hardening, docs, and release readiness

- Target weeks: 16-20
- Status: NS
- Owner: unassigned

Scope:

- finalize built-in migration catalog,
- close documentation/schema divergence,
- add release checklist automation,
- complete Rust API docs,
- prove durability and recovery behavior.

Completion evidence:

- migration tests and rollback tests pass,
- durability survives restart and recovery scenarios,
- CLI and Rust docs are complete,
- no Aegis listener/server code introduced.

Blocked if:

- migration numbering or policy is undefined,
- recovery from event log is not reliable,
- embedded-only architecture boundary is ambiguous in release docs.

### V1 Deliverables

- stable Rust crate,
- SQLite-backed embedded engine,
- CLI and REPL,
- test helper crate,
- core docs set,
- release security checklist integration.

### V1 Exit Criteria

- all Rust APIs documented,
- all core tests green in CI,
- SQLite restart durability proven,
- recovery from event log proven,
- CLI documented and exercised by E2E tests,
- no Aegis listener/server/API daemon introduced.

## V2: Multi-Model Authorization

### Goal

Expand Aegis from a relationship engine into a complete multi-model authorization runtime while preserving the embedded-first design.

### Timeline

- Duration: 16 weeks
- Suggested window: Quarter 3

### Dependencies

- V1 lifecycle, traversal, schema, and query contracts complete.

### V2 Scope

#### ReBAC completion

- stronger recursive path semantics,
- better path pruning,
- subject-set and indirect relation modeling improvements,
- resource hierarchy traversal utilities,
- namespace-scoped graph traversal.

#### RBAC layer

- role object patterns,
- built-in role helper APIs,
- role assignment APIs,
- role permission mapping APIs,
- role hierarchy support,
- scoped roles per tenant/resource/workspace,
- examples for translating classic RBAC to graph tuples.

#### ACL layer

- first-class direct grant patterns,
- simplified ACL authoring helpers,
- ACL import/export representation.

#### ABAC layer

- stable condition grammar,
- subject metadata conditions,
- resource metadata conditions,
- environment conditions,
- numeric comparisons,
- boolean comparisons,
- enum membership,
- presence and absence checks,
- request-scoped context evaluation,
- conditional relationship grants.

#### Time and context features

- expiration-aware tuples,
- `valid_until` (persisted in SQLite, filtered during BFS traversal),
- ~~`valid_from`~~ (not implemented),
- ABAC time conditions (Before, After, DayOfWeek),
- environment-aware conditions,
- deployment environment scoping.

#### Deny semantics

- explicit deny rule design,
- deny precedence rules,
- explain output for deny reasons,
- safe composition of allow and deny.

#### Tenant and namespace isolation

- ~~built-in tenant scoping abstraction~~ (tenant_id moved to app layer),
- ~~explicit tenant parameter support~~,
- ~~cross-tenant traversal prevention~~,
- namespace-based scoping via subject/resource ID prefix (e.g. `tenant1:user:alice`),
- namespace-aware list/query filters (`namespace` field on `TupleFilter`).

### V2 Milestones

#### V2-M1: Canonical model contracts

- Target weeks: 1-4
- Status: DN
- Owner: unassigned

Scope:

- finalize RBAC, ACL, and ABAC representation contracts,
- define canonical condition grammar,
- ~~define tenant scoping contract~~ (tenant_id moved to app layer),
- decide deny semantics precedence.

Completion evidence:

- design spec approved,
- schema grammar fixtures added,
- deny precedence test cases defined,
- ~~tenant isolation rules documented~~.

#### V2-M2: Runtime implementation of RBAC, ACL, and tenancy

- Target weeks: 5-8
- Status: DN
- Owner: unassigned

Scope:

- implement role helpers,
- implement direct ACL helpers,
- ~~enforce tenant-aware validation~~ (tenant_id is app-layer concern),
- ~~enforce cross-tenant traversal prevention~~,
- add scoped role support.

Completion evidence:

- RBAC helper API tests green,
- ACL helper tests green,
- ~~cross-tenant traversal deny tests green~~,
- ~~tenant-aware query filters verified~~.

#### V2-M3: ABAC, time conditions, and deny behavior

- Target weeks: 9-12
- Status: DN
- Owner: unassigned

Scope:

- implement stable ABAC evaluator,
- add request/environment/time conditions,
- implement explicit deny evaluation,
- expand explain output for condition and deny reasoning.

Completion evidence:

- ABAC grammar fixtures pass,
- conditional evaluation tests green,
- deny precedence tests green,
- explain output covers allow and deny branches.

#### V2-M4: Multi-model hardening and documentation

- Target weeks: 13-16
- Status: DN
- Owner: unassigned

Scope:

- align all examples and docs with actual syntax,
- add performance tests for richer traversal,
- ~~validate role/tenant ergonomics in SDK contracts~~ (tenant_id removed),
- close documentation claims that exceed implementation.

Completion evidence:

- ReBAC/RBAC/ACL/ABAC docs complete,
- example matrix verified,
- ~~runtime tenant enforcement proven~~,
- multi-model parity signoff complete.

### V2 Deliverables

- multi-model auth spec,
- condition grammar reference,
- role helper APIs,
- deny semantics design and implementation,
- passive tuple expiry via `valid_until`.

### V2 Exit Criteria

- ReBAC, RBAC, ACL, and ABAC documented and tested,
- namespace scoping available for multi-tenant environments,
- explain output covers both allow and deny decisions,
- schema and examples aligned with actual syntax.

## V3: SDK and Developer Platform Completion

### Goal

Make Aegis feel complete from the application developer point of view across Rust, Node.js, Python, and Go.

### Timeline

- Duration: 20 weeks
- Suggested window: Quarter 4 to Quarter 5

### Dependencies

- V1 stable API surface,
- V2 model semantics frozen enough for public SDK exposure.

### V3 Scope

#### Rust SDK completion

- public crate docs,
- full examples,
- ergonomic builder/config APIs,
- error taxonomy documentation,
- middleware examples for Rust web frameworks.

#### Node.js / TypeScript SDK

- publishable package structure,
- `package.json`,
- NAPI native module packaging,
- TypeScript wrapper class,
- `.d.ts` coverage,
- Promise-based APIs,
- transaction wrapper,
- watch subscription wrapper,
- config object initialization,
- error mapping to JS classes,
- ESM and CJS compatibility,
- prebuild strategy.

#### Python SDK

- publishable wheel/sdist strategy,
- Pythonic class API,
- async wrapper support where documented,
- context manager support,
- rich exceptions,
- type hints,
- FastAPI/Django/Flask examples.

#### Go SDK

- idiomatic Go API,
- real `context.Context` support,
- cancellation-aware operations where feasible,
- error wrapping and sentinel helpers,
- transaction wrapper,
- watch subscription support,
- `net/http`, Gin, Echo, Fiber examples.

#### C FFI completion

- full lifecycle coverage,
- check/write/delete/list/query/explain/health,
- transaction entry points,
- memory ownership rules,
- header stability,
- C integration examples.

#### Developer experience

- fixture loading APIs,
- snapshot-friendly testing helpers,
- local dev bootstrap commands,
- rich REPL help,
- schema authoring templates,
- example apps in multiple languages.

#### Packaging and distribution

- crates.io publication pipeline,
- npm publication pipeline,
- PyPI publication pipeline,
- Go module release tagging,
- release artifact signing,
- cross-platform release matrix.

### V3 Milestones

#### V3-M1: Public API contracts and packaging skeletons

- Target weeks: 1-5
- Status: NS
- Owner: unassigned

Scope:

- freeze core FFI and SDK API contracts,
- add package skeletons for npm and PyPI,
- define Go package layout,
- define error mapping contract across languages.

Completion evidence:

- SDK parity matrix baseline created,
- publishable package manifests exist,
- C header ownership rules documented,
- compatibility review completed.

#### V3-M2: Node and Python completion

- Target weeks: 6-10
- Status: NS
- Owner: unassigned

Scope:

- complete TypeScript package and wrappers,
- complete Python API and type hints,
- decide and implement Python async story,
- add framework integration examples.

Completion evidence:

- npm package installs and smoke tests pass,
- Python wheel installs and smoke tests pass,
- Node and Python E2E fixtures green,
- docs reflect real sync/async behavior.

#### V3-M3: Go and C FFI completion

- Target weeks: 11-15
- Status: NS
- Owner: unassigned

Scope:

- implement real Go context semantics,
- complete Go transaction and watch wrappers,
- complete missing C FFI surface,
- add ABI compatibility tests.

Completion evidence:

- Go cancellation tests pass,
- C ABI contract tests pass,
- Go examples compile and run,
- core operations available through all target bindings.

#### V3-M4: Cross-language release hardening

- Target weeks: 16-20
- Status: NS
- Owner: unassigned

Scope:

- finalize publish workflows,
- add cross-platform prebuild/release matrix,
- complete public docs per language,
- add language smoke tests to release gates.

Completion evidence:

- crates/npm/PyPI/Go release flows verified,
- install matrix passes on supported targets,
- API docs versioned and published,
- SDK parity matrix signed off.

### V3 Deliverables

- Rust, Node, Python, Go SDK docs,
- reference examples by language,
- published packages and release workflows,
- integration guides for common app frameworks.

### V3 Exit Criteria

- SDK parity matrix signed off,
- cross-language E2E fixtures green,
- package install and smoke tests green for supported targets,
- public API docs complete and versioned.

## V4: Enterprise Reliability and Governance

### Goal

Make Aegis suitable for high-confidence production use in regulated and operationally mature environments.

### Timeline

- Duration: 20 weeks
- Suggested window: Quarter 6 to Quarter 7

### Dependencies

- V1 durability,
- V3 package and API stability.

### V4 Scope

#### Durability and recovery

- full-fidelity backup format,
- verified restore format,
- schema-plus-event-plus-tuple restore,
- point-in-time recovery,
- crash recovery verification suite,
- corruption detection and recovery guidance.

#### Audit integrity

- tamper-evident audit chains,
- optional hash-chained audit log,
- signed export packages,
- provenance metadata for mutations,
- actor identity propagation.

#### Security hardening

- secrets redaction,
- safer API key handling,
- cryptographic hashing for sensitive values where needed,
- secure configuration guidance,
- TLS guidance for PostgreSQL/MySQL clients,
- secure-by-default production presets.

#### Compliance

- retention policies,
- export portability format contract,
- erasure and transfer semantics,
- audit retention strategy,
- data residency and tenancy guidance.

#### Governance tooling

- policy review guidance,
- schema lint hardening,
- policy diff and impact analysis foundations,
- audit report generation,
- access review export tools.

#### Operations

- better health diagnostics,
- integrity checks scheduling,
- WAL growth guidance and controls,
- performance guardrails,
- capacity planning docs.

### V4 Milestones

#### V4-M1: Backup, restore, and recovery contract

- Target weeks: 1-5
- Status: NS
- Owner: unassigned

Scope:

- define backup format contract,
- implement full-fidelity restore semantics,
- add crash-recovery verification suite,
- document compatibility and retention rules.

Completion evidence:

- backup format documented,
- restore includes schema, tuples, metadata, revision, and audit state,
- crash recovery tests green,
- cross-version restore fixtures added.

#### V4-M2: Audit integrity and compliance primitives

- Target weeks: 6-10
- Status: NS
- Owner: unassigned

Scope:

- implement tamper-evident audit chain option,
- add actor provenance metadata,
- implement retention and erasure semantics,
- add signed export packaging if required.

Completion evidence:

- audit integrity tests green,
- provenance surfaces documented,
- compliance semantics verified with fixtures,
- export integrity checks pass.

#### V4-M3: Security automation and production hardening

- Target weeks: 11-15
- Status: NS
- Owner: unassigned

Scope:

- codify security checklist as release/test gates,
- add secrets-in-logs tests,
- add fuzzing/security test tier,
- define secure production presets.

Completion evidence:

- security gates enforced in CI,
- redaction tests pass,
- fuzzing or mutation-security tier established,
- hardening guide complete.

#### V4-M4: Governance tooling and enterprise readiness

- Target weeks: 16-20
- Status: NS
- Owner: unassigned

Scope:

- finalize access review exports,
- improve lint and policy diff tooling,
- complete enterprise operations guide,
- finish capacity planning and health diagnostics docs.

Completion evidence:

- governance workflows documented,
- enterprise guide complete,
- health diagnostics stable,
- production readiness signoff complete.

### V4 Deliverables

- enterprise operations guide,
- security guide,
- compliance and data lifecycle guide,
- production hardening presets.

### V4 Exit Criteria

- restore tested from real backups,
- audit retention and integrity verified,
- production checklist automated in CI or release process where possible,
- security review completed.

## V5: Authorization Everywhere — Browser, Offline, and Worker Runtime

### Goal

Extend Aegis into browser, offline, and worker environments so that the exact same authorization engine runs on the server, in the browser, and offline — with the same schema, same traversal, and same permission model.

```
V4 = Enterprise Embedded Authorization
V5 = Authorization Everywhere

Server  Browser  Desktop  PWA  Offline  Workers  Edge*
    └──────┴────────┴──────┴─────┴──────┴──────┘
           Same engine. Same schema. Same checks.
                                          * Edge KV/D1 deferred to V6
```

### Timeline

- Duration: 14 weeks
- Suggested window: Quarter 8

### Dependencies

- stable Rust core (V1),
- stable schema and evaluation semantics (V1-V2),
- V4 sync `StorageBackend` trait stability (V4),
- new `AsyncStorageBackend` trait introduced in V5-M2 Phase A.

### Architecture

```
StorageBackend (sync)          AsyncStorageBackend (async)
├── SqliteStorage              ├── InMemoryAsyncStorage
├── PostgresStorage            └── IndexedDbStorage
├── MysqlStorage
├── RocksDbStorage
└── InMemoryStorage

GraphEngine
├── check() / write() / delete()     ← sync (server backends)
├── async_check() / async_write()    ← async (browser backends)
│
├── Schema       ── same ──┐
├── Cache        ── same ──┤
├── Traversal    ── same ──┤
├── Partitions   ── same ──┤
└── Audit        ── same ──┘
```

### V5 Scope

#### WASM runtime

- core engine compilation for WebAssembly (M1 — done),
- minimal feature profile for browser runtimes (M1 — done),
- deterministic memory and size constraints (M1 — done),
- raw WASM bundle: 792 KB (target < 2.5 MB). ✓

#### AsyncStorageBackend trait (NEW — Phase A of M2)

- async trait mirroring sync `StorageBackend` methods,
- only compiled for `cfg(target_arch = "wasm32")`,
- `InMemoryAsyncStorage` wrapper for testing,
- `GraphEngine.async_check()`, `async_write()`, `async_delete()` APIs,
- `StorageCapabilities` struct for feature detection.

#### IndexedDB storage adapter (Phase B of M2)

- browser persistence backend via web-sys bindings,
- 5 IndexedDB stores: `tuples`, `events`, `revision`, `schema`, `metadata`,
- revision tracking and query flows,
- audit/event storage,
- export/import support,
- schema version + content persisted in `schema` store.

#### Web Worker support (M3)

- `AegisEngine` runs in a Web Worker by default,
- communicates via `postMessage` bridge,
- main thread never blocked by authorization checks.

#### Browser SDK (M3)

- JS/TypeScript package (`@aegis-v/browser`),
- `AegisEngine.create(schema, config)` factory,
- `check()`, `write()`, `delete()`, `listByObject()`, `listBySubject()`,
- `exportToJson()`, `importFromJson()`,
- `partition(id)` — same V4 partition semantics for browser.

#### Offline-first flows (M3)

- portable export/import between browser and server,
- offline demo app: single HTML page, IndexedDB-backed, works without network,
- V5 does NOT include automatic conflict resolution — documented explicitly as out of scope.

### Out of Scope for V5

The following are deferred to V6 or V5.1:

- ❌ Automatic conflict resolution / CRDT sync for offline edits
- ❌ Cloudflare Workers / D1 / KV / Durable Objects → V6
- ❌ Deno / Bun → V6
- ❌ React bindings (`usePermission()`, `<AegisProvider />`) → V5.1
- ❌ Service Worker PWA scaffolding → V5.1

### V5 Milestones

#### V5-M1: WASM feasibility and boundary definition (COMPLETE)

- Target weeks: — (done)
- Status: DN
- Owner: —

Completion evidence (all met):

| Item | Status |
|---|---|
| `sqlite` feature flag: `rusqlite`/`r2d2` optional | ✅ |
| `SqliteStorage` gated behind `#[cfg(feature = "sqlite")]` | ✅ |
| `InMemoryStorage` backend (HashMap-based, 7 tests) | ✅ |
| `BackendType::InMemory` variant | ✅ |
| `libc` target-gated, not compiled on wasm32 | ✅ |
| `wasm` feature flag for minimal WASM builds | ✅ |
| Core compiles for `wasm32-unknown-unknown` | ✅ |
| Raw WASM bundle: 792 KB (target < 2.5 MB) | ✅ |
| WASM demo example (`examples/wasm-demo/`) | ✅ |
| Unsupported features documented | ❌ |
| Runtime contract document | ❌ |

#### V5-M2: AsyncStorageBackend + IndexedDB (6 weeks)

- Target weeks: 1-6
- Status: NS
- Owner: unassigned

**Phase A — AsyncStorageBackend trait (weeks 1-2)**

Scope:

- define `AsyncStorageBackend` trait with async equivalents of all `StorageBackend` methods,
- add `StorageCapabilities` struct (`persistent`, `transactional`, `audit_supported`, `backup_supported`, `export_import_supported`, `async_only`),
- implement `InMemoryAsyncStorage` for testing,
- add `async_check()`, `async_write()`, `async_delete()` methods to `GraphEngine`,
- share traversal, schema, cache, partition, and audit logic with sync path.

**Phase B — IndexedDbStorage (weeks 3-6)**

Scope:

- IndexedDB store layout:

  | Store | Key | Value |
  |---|---|---|
  | `tuples` | `[partition, subject, relation, object]` | tuple JSON |
  | `events` | auto-increment | full audit entry |
  | `revision` | `partition` | `{current_revision}` |
  | `schema` | `partition` | `{version, hash, yaml_content}` |
  | `metadata` | `key` | `{value}` |

- `IndexedDbStorage` struct implementing `AsyncStorageBackend`:
  - async open/create database,
  - full CRUD + list + query + pagination operations,
  - audit write + query with revision range,
  - `recover_from_events()` via replay,
  - `restore_backup()` clear + batch insert,
  - `verify_audit_chain()` hash chain replay,
  - `close()`.
- `IndexedDbTransaction` implementing async transaction semantics.
- Tests via `wasm-bindgen-test` in headless browser (Chrome, Firefox).
- CI: `wasm-pack test --chrome --headless`.

Completion evidence:

- CRUD and query contract tests green in browser harness,
- revision semantics verified (monotonic per partition),
- persistence survives page reload,
- export/import local round-trip works,
- schema version persisted across sessions.

#### V5-M3: Browser SDK + Web Worker + offline-first (6 weeks)

- Target weeks: 7-12
- Status: NS
- Owner: unassigned

Scope:

- **Web Worker**: `AegisEngine` runs in a Worker by default; main-thread API uses `postMessage` bridge. Worker support is M3, not deferred to M4.
- **JS/TS SDK** (`packages/aegis-browser/`):

  ```ts
  const engine = await AegisEngine.create(schema, { dbName: "myapp" });
  await engine.check("user:alice", "read", "repo:hello");
  await engine.write("user:alice", "owner", "repo:hello");
  await engine.delete("user:alice", "owner", "repo:hello");
  await engine.listByObject("repo:hello");
  await engine.listBySubject("user:alice");
  await engine.exportToJson();
  await engine.importFromJson(json);
  engine.partition("acme"); // same V4 partition semantics
  ```

- TypeScript type definitions: `CheckResult`, `Tuple`, `AuditEntry`, `AegisConfig`.
- Size optimization: `wasm-opt -Oz`, tree-shaking unused exports, target < 350 KB gzip.
- CI: `wasm-pack build`, `wasm-pack test`, bundle size gate.

- **Export/import portability**:
  - `exportToJson()` → dump all tuples + events + revision + schema as JSON,
  - `importFromJson()` → clear + restore (wraps `restore_backup`),
  - explicit documentation: no automatic conflict resolution (V5 scope boundary).

- **Offline demo app**:
  - Single HTML page (no framework dependency),
  - IndexedDB-backed,
  - Write tuples, run checks, export while fully offline,
  - Re-import on server on reconnect.

Completion evidence:

- SDK install and smoke tests green (npm package),
- offline demo works without network,
- import/export portability validated (server→browser and browser→server),
- Worker isolation verified (main thread not blocked during checks),
- bundle size gate passes,
- browser docs complete.

#### V5-M4: Release readiness (2 weeks)

- Target weeks: 13-14
- Status: NS
- Owner: unassigned

Scope:

- **Performance benchmarks**:
  - p95 check latency < 5 ms on 10k tuple graph in browser,
  - export 100k tuples under 5 seconds,
  - import 100k tuples under 10 seconds,
  - idle memory < 50 MB, steady-state < 200 MB,
  - cold start to first check < 250 ms.
- **Support matrix**:

  | Platform | Status |
  |---|---|
  | Chrome (latest) | ✅ |
  | Firefox (latest) | ✅ |
  | Safari (latest) | ✅ |
  | Edge (latest) | ✅ |
  | Web Worker | ✅ |
  | Cloudflare Workers | ❌ V6 |
  | Deno | ❌ V6 |
  | Bun | ❌ V6 |

- **Docs**:
  - WASM architecture spec (`docs/wasm-architecture.md`),
  - Browser getting-started guide (`docs/browser-getting-started.md`),
  - Schema migration on browser guide,
  - Support matrix (rendered from `AEGIS_IMPLEMENTATION_PLAN.md`).
- **Release**:
  - npm publish (`@aegis-v/browser`),
  - cargo publish (`aegis-core` with `wasm` + `indexeddb` features),
  - V5 changelog,
  - All release gates signed off (engineering, product, security, architecture, performance, compatibility).

Completion evidence:

- edge examples pass,
- runtime size targets met or adjudicated,
- support matrix published,
- V5 release gates signed off.

### V5 Deliverables

- `AsyncStorageBackend` trait + `StorageCapabilities` (M2),
- `IndexedDbStorage` implementation (M2),
- `@aegis-v/browser` npm package (M3),
- Web Worker integration (M3),
- offline-first demo app (M3),
- WASM/IndexedDB architecture spec (M4),
- browser docs + support matrix (M4).

### V5 Exit Criteria

- browser demo app works fully offline (IndexedDB + Worker),
- export/import between server and browser validated with schema persistence,
- runtime size and performance targets met (< 350 KB gzip, p95 < 5 ms on 10k graph),
- Web Worker isolation verified (main thread not blocked),
- `AsyncStorageBackend` trait stable and documented for V6 edge backends.

## V6: Advanced Policy Intelligence and Ecosystem

### Goal

Complete Aegis as a mature authorization platform with advanced analysis, ecosystem tooling, and long-term governance features.

### Timeline

- Duration: 24 weeks
- Suggested window: Quarter 10 to Quarter 11

### Dependencies

- V2 explainability,
- V4 audit integrity,
- stable cross-language platform from V3.

### V6 Scope

#### Policy intelligence

- policy simulation,
- dry-run impact analysis,
- access diff reports,
- who-can-access queries,
- why-not explanations,
- bulk permission analysis.

#### Graph analysis

- reachability analysis,
- orphaned relationship analysis,
- over-broad access detection,
- tenant leakage detection,
- dormant role detection.

#### Admin and review tooling

- optional admin UI package or reference app,
- audit browsing reference UI,
- access review workflow exports,
- role review tooling.

#### Ecosystem examples

- SaaS reference app,
- collaborative editor reference app,
- multi-tenant platform reference app,
- edge/browser reference app.

#### Language and platform expansion

- additional FFI consumers if needed,
- JVM and .NET evaluation,
- additional package templates and examples.

### V6 Milestones

#### V6-M1: Analysis API foundations

- Target weeks: 1-6
- Status: NS
- Owner: unassigned

Scope:

- define analysis API contracts,
- implement who-can-access and access diff foundations,
- add why-not explanation model,
- define bulk analysis limits and safeguards.

Completion evidence:

- analysis spec approved,
- baseline analysis fixtures pass,
- API shape documented,
- cost guardrails defined.

#### V6-M2: Policy simulation and graph diagnostics

- Target weeks: 7-12
- Status: NS
- Owner: unassigned

Scope:

- implement dry-run simulation,
- implement reachability and orphan detection,
- implement leakage and over-broad access checks,
- add reviewable reports.

Completion evidence:

- simulation tests green,
- graph diagnostics fixtures green,
- report outputs stable,
- analysis correctness reviewed.

#### V6-M3: Admin/reference tooling and examples

- Target weeks: 13-18
- Status: NS
- Owner: unassigned

Scope:

- build reference admin tooling,
- add audit browse reference UI,
- ship example app suite,
- add access review workflows.

Completion evidence:

- tooling demos function end to end,
- example suite published,
- review exports validated,
- ecosystem docs expanded.

#### V6-M4: Ecosystem hardening and expansion

- Target weeks: 19-24
- Status: NS
- Owner: unassigned

Scope:

- refine analysis API stability,
- evaluate JVM/.NET or other bindings,
- finalize long-term governance docs,
- complete ecosystem integration guides.

Completion evidence:

- access analysis APIs stable,
- example and docs suite complete,
- platform expansion decision documented,
- final ecosystem signoff completed.

### V6 Deliverables

- policy intelligence APIs,
- admin reference tooling,
- example application suite,
- ecosystem integration guides.

### V6 Exit Criteria

- access analysis APIs stable,
- example suite published,
- multi-language ecosystem docs complete.

## Cross-Version Gap Register

This section converts the current audit into tracked work.

### Gap 1: Documentation and Schema Syntax Divergence

Problem:

- several docs and examples use shorthand YAML arrays for relations and permissions,
- the current parser expects explicit `inherit_from` and `union_of` structures.

Required work:

- decide canonical schema format,
- either implement shorthand parsing or remove shorthand docs,
- update examples and fixtures,
- add parser compatibility tests.

Target:

- V1-M2 through V1-M4.

### Gap 2: Embedded Boundary Messaging Inconsistency

Problem:

- some docs use endpoint/server/distributed language that conflicts with the embedded-only position.

Required work:

- rewrite docs to make the embedded boundary explicit,
- clarify PostgreSQL/MySQL as storage adapters only,
- remove ambiguous health-endpoint wording unless clearly host-app-specific,
- remove or mark service-style language as out of scope.

Target:

- V1-M4.

### Gap 3: TypeScript SDK Packaging Incomplete

Problem:

- native NAPI bindings exist, but package/distribution structure is incomplete.

Required work:

- add package structure,
- add TS wrapper,
- add typings,
- add publishing workflow,
- add install/test matrix.

Target:

- V3-M1 through V3-M4.

### Gap 4: Python SDK Async Claim Exceeds Actual Surface

Problem:

- Python docs imply async capability, but the exposed surface is synchronous.

Required work:

- decide sync-only or async wrapper,
- implement the chosen API honestly,
- add matching docs and tests.

Target:

- V3-M2.

### Gap 5: Go Context Support Needs Real Semantics

Problem:

- `context.Context` exists in signatures but is not meaningfully honored for cancellation.

Required work:

- define cancellable operations,
- implement context checks around boundary calls where feasible,
- document behavior honestly.

Target:

- V3-M3.

### Gap 6: C FFI Surface Is Incomplete

Problem:

- the current FFI does not expose the full engine surface.

Required work:

- add list/query/explain/health/audit/transaction/export APIs,
- document memory safety and ownership,
- add ABI compatibility tests.

Target:

- V3-M1 through V3-M3.

### Gap 7: Backup Restore Fidelity

Problem:

- restore should rebuild complete state and history where promised.

Required work:

- define backup format contract,
- restore schema, tuples, metadata, revision, event log, and audit history consistently,
- add compatibility tests between versions.

Target:

- V4-M1.

### Gap 8: Tenant Isolation Needs First-Class Enforcement

Problem:

- tenant isolation is partly conceptual and naming-based.

Required work:

- add explicit tenant scoping model,
- enforce tenant boundaries in traversal and query paths,
- add cross-tenant deny tests.

Target:

- V2-M1 through V2-M4.

### Gap 9: IndexedDB Backend Missing

Problem:

- documented but not implemented.

Required work:

- add AsyncStorageBackend trait and StorageCapabilities,
- implement IndexedDbStorage using AsyncStorageBackend,
- add browser SDK and examples.

Target:

- V5-M2 through V5-M4.

### Gap 10: Migration Framework Needs Real Built-In Steps

Problem:

- migration infrastructure exists, but the registered migration catalog must be completed.

Required work:

- define migration numbering,
- add built-in migrations,
- add compatibility policy,
- add rollback tests.

Target:

- V1-M3 through V1-M4.

### Gap 11: Security Hardening Automation

Problem:

- security checklist exists but needs codified enforcement.

Required work:

- turn checklist items into tests and release gates,
- add fuzzing to CI tiers where feasible,
- add secrets-in-logs tests,
- review hashing/security choices.

Target:

- V4-M3.

### Gap 12: Advanced Explainability

Problem:

- explainability can be richer for allow and deny reasoning.

Required work:

- add failed-branch reason output,
- add deny explanation format,
- add condition evaluation explanation,
- add depth-limit and cycle annotations.

Target:

- V2-M3 through V6-M2.

### Gap 13: Missing Milestone and Tracking Structure

Problem:

- the original plan described versions but did not support reliable execution tracking.

Required work:

- add milestone structure per version,
- add top-level progress dashboard,
- define milestone evidence requirements,
- define status model and update cadence.

Target:

- complete in this document revision.

### Gap 14: Missing Dependency and Critical-Path Analysis

Problem:

- the original plan implied order but did not clearly define blockers or parallel work.

Required work:

- add dependency graph,
- define critical path,
- mark non-critical but parallelizable streams.

Target:

- complete in this document revision.

### Gap 15: Missing Concrete Performance and Testing Targets

Problem:

- the original plan did not define measurable performance or testing standards.

Required work:

- add explicit benchmark targets,
- define test layers,
- define backend and SDK matrices,
- add boundary regression rules.

Target:

- complete in this document revision; enforced from V1 onward.

### Gap 16: Missing Upgrade, Error, and Compatibility Policy

Problem:

- the original plan did not define cross-version upgrade rules, semver policy, or cross-language error contracts.

Required work:

- add migration/upgrade policy,
- add error taxonomy,
- add semver and deprecation rules,
- define language mapping expectations.

Target:

- complete in this document revision; enforced starting in V1 and V3.

## Detailed Work Breakdown by Workstream

### Workstream 1: Core Runtime

Tasks:

- finalize engine constructor patterns,
- add config struct parity across SDKs,
- enforce closed-state checks everywhere,
- standardize error returns,
- add API stability tests.

Success criteria:

- predictable lifecycle across SDKs,
- zero hidden panics on normal invalid input.

Primary versions:

- V1, V3.

### Workstream 2: Authorization Models

Tasks:

- model RBAC helpers,
- expand ABAC grammar,
- add deny semantics,
- add time-based grants,
- add resource hierarchy helpers.

Success criteria:

- one engine can represent ReBAC, RBAC, ACL, ownership, tenancy, and ABAC.

Primary versions:

- V2, V6.

### Workstream 3: Storage and Durability

Tasks:

- stabilize SQLite contract,
- equalize behavior across backends,
- define backup format,
- ensure revision semantics across storage adapters,
- improve retention and compaction behavior.

Success criteria:

- storage parity matrix documented and tested.

Primary versions:

- V1, V4, V5.

### Workstream 4: Transactions and Consistency

Tasks:

- verify `AtRevision` semantics end to end,
- standardize fully-consistent semantics by backend,
- ensure transaction rollback correctness,
- add savepoint conformance tests.

Success criteria:

- snapshot guarantees well-defined by backend.

Primary versions:

- V1, V4.

### Workstream 5: Schema and Policy System

Tasks:

- finalize grammar,
- finalize schema structure,
- add shorthand support or remove shorthand docs,
- add policy lint levels,
- add impact diff tooling.

Success criteria:

- schema examples always match parser behavior.

Primary versions:

- V1, V2, V4.

### Workstream 6: Explainability and Auditability

Tasks:

- expand explain output,
- add denial reasoning,
- add condition reasoning,
- improve audit integrity.

Success criteria:

- every decision can be explained in developer-readable and human-readable form.

Primary versions:

- V1, V2, V4, V6.

### Workstream 7: Performance and Caching

Tasks:

- benchmark traversal patterns,
- tune caches,
- add cache observability,
- add cache invalidation conformance tests,
- optimize sibling traversal and pruning.

Success criteria:

- performance targets documented and continuously measured.

Primary versions:

- V1 through V6.

### Workstream 8: SDKs and Embeddings

Tasks:

- complete Node package,
- complete Python API,
- complete Go API,
- complete C FFI,
- keep Rust SDK first-class.

Success criteria:

- cross-language parity for core operations.

Primary versions:

- V1, V3, V5.

### Workstream 9: CLI and REPL

Tasks:

- stabilize command set,
- improve UX and help text,
- add scriptable JSON output consistency,
- add import/export format validation.

Success criteria:

- CLI usable for local admin/debugging without becoming a service.

Primary versions:

- V1, V4.

### Workstream 10: Security and Compliance

Tasks:

- security tests,
- logging redaction,
- hardening defaults,
- GDPR semantics,
- audit retention.

Success criteria:

- security checklist enforced by code and process.

Primary versions:

- V1, V4.

### Workstream 11: Testing and Verification

Tasks:

- expand integration coverage,
- add cross-backend contract tests,
- add SDK E2E tests,
- add backup/recovery tests,
- add no-network-boundary regression checks.

Success criteria:

- test suite maps directly to promised behavior.

Primary versions:

- V1 through V6.

### Workstream 12: Documentation and Examples

Tasks:

- add architecture guide,
- add storage guide,
- add schema guide,
- add examples in all SDK languages,
- add migration and upgrade guides.

Success criteria:

- users can adopt Aegis without reading internal roadmap docs.

Primary versions:

- V1 through V6.

### Workstream 13: Packaging and Release Engineering

Tasks:

- version policy,
- release automation,
- artifact signing,
- multi-platform build matrix,
- package publishing verification.

Success criteria:

- reliable releases across Rust, npm, PyPI, and Go module flows.

Primary versions:

- V3, V4, V5.

### Workstream 14: Browser Runtime

Tasks:

- WASM feasibility and feature gating (M1 — done),
- AsyncStorageBackend trait (M2 Phase A),
- IndexedDB backend (M2 Phase B),
- Web Worker integration (M3),
- browser SDK (M3),
- offline-first demo app (M3),
- release readiness, benchmarks, docs (M4).

Out of scope (deferred to V6):

- Cloudflare Workers / D1 / KV / Durable Objects,
- Deno / Bun,
- automatic conflict resolution,
- React bindings.

Success criteria:

- Aegis works in offline-first and browser-embedded settings with the exact same engine and schema as the server runtime.

Primary versions:

- V5.

### Workstream 15: Governance and Enterprise Operations

Tasks:

- policy review workflow,
- access review exports,
- admin reference tooling,
- compliance documentation.

Success criteria:

- enterprise teams can review and govern policies confidently.

Primary versions:

- V4, V6.

## Risk Register

Review this register at the start and end of every milestone.

| ID | Risk | Likelihood | Impact | Mitigation | Trigger Version |
|---|---|---|---|---|---|
| R1 | Schema syntax remains ambiguous between docs and parser | High | High | Decide one canonical format early; gate examples against parser tests | V1 |
| R2 | SQLite durability and recovery semantics fail under crash scenarios | Medium | High | Build restart/recovery tests early; treat durability as a release blocker | V1 |
| R3 | Traversal semantics become too expensive on large graphs | Medium | High | Add visit/depth limits, pruning, benchmarks, and explainable cutoffs | V1-V2 |
| R4 | ABAC grammar grows too complex and hard to maintain | Medium | Medium | Phase grammar delivery; lock a minimal stable subset before expansion | V2 |
| R5 | Tenant isolation remains naming-convention based | Medium | High | Add explicit tenant model and deny tests as a V2 blocker | V2 |
| R6 | SDK packaging drifts from Rust feature reality | High | Medium | Maintain parity matrix and release smoke tests per language | V3 |
| R7 | Python async claims exceed actual implementation | High | Medium | Decide sync-only vs async wrapper before docs expansion | V3 |
| R8 | Go `context.Context` remains superficial | Medium | Medium | Define cancellable boundaries and test cancellation explicitly | V3 |
| R9 | C ABI becomes unstable across releases | Medium | High | Freeze header policy and add ABI compatibility tests | V3 |
| R10 | Backup restore format is incomplete or incompatible | Medium | High | Version backup format and add cross-version fixtures | V4 |
| R11 | Security checklist stays manual and unenforced | Medium | High | Convert checklist items into CI/release gates | V4 |
| R12 | WASM bundle size or runtime limits make browser story impractical | Low | Medium | Feasibility spike complete (M1). Raw WASM: 792 KB (target < 2.5 MB). Risk mitigated. | V5 |
| R13 | IndexedDB behavior diverges from SQLite semantics | Medium | Medium | Add browser contract tests and document intentional differences | V5 |
| R15 | AsyncStorageBackend and sync StorageBackend diverge over time | Medium | High | Share traversal/cache/schema/partition logic between sync and async paths; enforce common test suite | V5 |
| R14 | Analysis APIs in V6 become computationally expensive | Medium | Medium | Add query cost limits, pagination, and report scoping | V6 |

## Testing and Verification Strategy

Testing must prove every shipped claim.

### Test Layers

#### Unit tests

Purpose:

- parser behavior,
- tuple validation,
- relation and permission resolution,
- condition evaluation,
- error mapping,
- small algorithmic invariants.

Target share:

- approximately 55% to 65% of total automated tests.

#### Integration tests

Purpose:

- engine lifecycle,
- storage adapters,
- transactions,
- migration behavior,
- recovery semantics,
- backup/restore,
- query semantics.

Target share:

- approximately 20% to 30% of total automated tests.

#### End-to-end tests

Purpose:

- CLI flows,
- SDK parity flows,
- language install/smoke tests,
- cross-language fixture execution,
- example apps.

Target share:

- approximately 10% to 15% of total automated tests.

#### Special test tiers

Purpose:

- fuzzing,
- corruption and crash recovery,
- performance benchmarks,
- memory leak checks where practical,
- ABI compatibility,
- network-boundary regression checks.

### Required Test Matrices

#### Backend matrix

- SQLite,
- RocksDB when implemented,
- PostgreSQL adapter,
- MySQL adapter,
- IndexedDB in V5.

#### Language matrix

- Rust,
- Node.js,
- Python,
- Go,
- C FFI.

#### Scenario matrix

- direct tuple allow/deny,
- recursive traversal allow/deny,
- cycle detection,
- depth and visit cutoffs,
- schema reload,
- migration upgrade,
- export/import,
- backup/restore,
- audit query,
- tenant isolation,
- ABAC condition evaluation,
- deny precedence,
- explain path correctness.

### Boundary Regression Tests

CI must fail if Aegis introduces embedded-boundary violations such as:

- listener creation,
- HTTP server startup,
- gRPC server startup,
- required port binding,
- mandatory control-plane dependency.

Recommended checks:

- source scans for server/listener patterns,
- dependency review for server frameworks introduced into core crates,
- architectural review checklist on release branches.

### Release Verification Commands

Minimum expected release verification includes:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features`
- `cargo test --workspace --all-features`
- language-specific install and smoke tests
- backup/restore compatibility suite
- cross-language parity suite

## Performance Targets

These targets are starting objectives and may be revised with measured evidence.

### Core evaluation targets

- local `check()` p50 under 0.5 ms on SQLite with warm cache and under 100k tuples,
- local `check()` p99 under 2 ms on SQLite with warm cache and under 100k tuples,
- local `check()` p99 under 10 ms on SQLite and under 1M tuples for bounded traversals,
- explain path generation p99 under 20 ms for bounded traversals under 1M tuples.

### Mutation targets

- single `write()` p99 under 5 ms on SQLite,
- single `delete()` p99 under 5 ms on SQLite,
- `write_batch()` of 1k tuples under 250 ms,
- export throughput above 10k tuples/sec on local disk.

### Startup and shutdown targets

- engine cold start under 50 ms for local SQLite with small schema,
- graceful close and checkpoint under 100 ms for normal state,
- recovery startup from clean journal state under 250 ms for moderate local datasets.

### Resource targets

- idle memory under 50 MB for a typical embedded runtime,
- steady-state memory under 200 MB for moderate local workloads,
- browser/WASM bundle target under 350 KB gzip (raw 792 KB as of M1),
- browser runtime initialization under 250 ms on target hardware for the minimal profile,
- browser p95 check latency under 5 ms on a 10k tuple graph (latency > throughput as the primary metric),
- browser export of 100k tuples under 5 seconds,
- browser import of 100k tuples under 10 seconds.

### Performance governance rules

- no optimization work should precede baseline benchmarks,
- all benchmark harnesses must be repeatable in CI or scheduled perf environments,
- regressions above 10% on release-critical paths require explicit adjudication.

## Upgrade, Migration, and Compatibility Policy

### Storage and schema versioning

- every persisted format must have an explicit version,
- schema format must expose compatibility rules,
- backup format must be versioned independently if needed,
- migration metadata must record source and target versions.

### Upgrade policy

- upgrades must be forward-only unless explicitly documented otherwise,
- every minor/major storage or schema change must include an automated migration path,
- supported upgrade range should be at least N-2 for storage and backup import where feasible,
- upgrade guides must ship with every breaking release.

### Downgrade policy

- downgrade is not guaranteed,
- if unsupported, documentation must say so explicitly,
- downgrade-safe export/import may be supported only for documented subsets.

### Compatibility tests

Required:

- old backup import into new engine,
- old schema parse/compatibility validation,
- migration replay from supported prior versions,
- restore correctness after version transitions.

## Error Model and API Compatibility Contract

### Error taxonomy

All SDKs should map into a shared conceptual taxonomy:

- `ValidationError`
- `SchemaError`
- `StorageError`
- `ConsistencyError`
- `PermissionDenied`
- `NotFound` where semantically useful
- `NotSupported`
- `Timeout` where applicable
- `InternalError`

### Error contract rules

- permission denial must not be conflated with internal failure,
- invalid input must not surface as internal error,
- backend-specific failures may carry vendor detail but must retain a stable top-level category,
- SDKs must expose both machine-readable category and human-readable message.

### Cross-language mapping expectation

- Rust: enums/struct errors,
- Node.js: typed `Error` subclasses or category-bearing objects,
- Python: typed exceptions,
- Go: wrapped errors with category helpers or sentinels,
- C: stable error codes plus message accessors.

### API versioning policy

- semantic versioning is required,
- MAJOR: breaking API, schema, storage, or behavior contract changes,
- MINOR: additive features and non-breaking behavior,
- PATCH: bug fixes and security fixes without contract breakage.

### Deprecation policy

- deprecations must be documented,
- removed APIs should receive at least one minor version of notice when practical,
- SDK docs must mark deprecated APIs clearly,
- migration guidance must accompany any planned removal.

## Documentation and Operational Tracking Requirements

The plan only works if execution updates are maintained.

### Required update cadence

- update milestone status at least weekly during active implementation,
- update version overall percent complete at least biweekly,
- update risk register when any risk changes likelihood or impact,
- update blockers immediately when a critical-path item slips.

### Minimum evidence for marking a milestone done

Do not mark a milestone `DN` unless:

- code is merged,
- tests covering the milestone scope are green,
- docs for the shipped surface are updated,
- unresolved blockers are either closed or explicitly moved to the next milestone.

### Suggested tracking fields

Maintain these fields per milestone during execution:

- owner,
- start date,
- target complete date,
- actual complete date,
- percent complete,
- blockers,
- evidence links,
- carryover items.

## Timeline Summary

| Version | Duration | Focus |
|---|---:|---|
| V1 | 20 weeks | Embedded core foundation |
| V2 | 16 weeks | Multi-model authorization |
| V3 | 20 weeks | SDK and developer platform completion |
| V4 | 20 weeks | Reliability, security, governance |
| V5 | 14 weeks | Browser, offline, worker runtime |
| V6 | 24 weeks | Advanced policy intelligence and ecosystem |

Total projected duration:

- 114 weeks,
- approximately 2 years for full completion.

Accelerated option:

- 16 to 18 months with 4+ engineers and dedicated QA/docs support.

## Release Gates Per Version

Each version release requires these gates.

### Engineering gates

- formatting clean,
- linting clean,
- tests green,
- release tests green,
- cross-platform packaging verified.

### Product gates

- docs complete for released APIs,
- examples included,
- upgrade notes written,
- storage behavior documented.

### Security gates

- `cargo audit` clean or explicitly adjudicated,
- `cargo deny` clean or explicitly adjudicated,
- no sensitive logs introduced,
- security checklist reviewed.

### Architecture gates

- no Aegis network service introduced,
- no port-binding behavior introduced,
- no mandatory external control plane introduced,
- embedded-only boundary preserved.

### Performance gates

- benchmark suite executed,
- critical-path regressions reviewed,
- startup, evaluation, and mutation targets either met or explicitly adjudicated.

### Compatibility gates

- migration tests pass for supported prior versions,
- SDK parity matrix updated,
- language package install/smoke tests pass,
- docs match actual released syntax and behavior.

## Definition of Completion

Aegis is considered complete end to end when all of the following are true:

- the embedded runtime is stable and documented,
- ReBAC, RBAC, ACL, ownership, tenancy, and ABAC are first-class and tested,
- SQLite, RocksDB, PostgreSQL, MySQL, and IndexedDB stories are clearly implemented or explicitly version-scoped,
- AsyncStorageBackend trait exists for browser and future edge backends, with sync and async GraphEngine APIs sharing all traversal/schema/cache logic,
- Rust, Node, Go, Python, and C interfaces are complete for core engine operations,
- backup, restore, recovery, audit, and retention are production-grade,
- documentation is coherent and aligned with code,
- the no-service embedded boundary is preserved throughout,
- performance and reliability targets are continuously verified,
- package publishing and versioning are stable,
- example apps and ecosystem guidance are complete,
- milestone and version progress can be tracked objectively from the plan itself.

## Immediate Next Action Plan

The practical execution sequence should be:

1. Finalize embedded-only boundary language across all docs and specs.
2. Finalize canonical schema syntax and update docs, examples, and test fixtures.
3. Execute V1-M1 and V1-M2 to stabilize the engine, storage, traversal, and schema contracts.
4. Execute V1-M3 and V1-M4 to close transactions, CLI, restore fidelity, migrations, and release readiness.
5. Execute V2 with tenant enforcement, RBAC/ACL helpers, ABAC grammar, and deny semantics.
6. Execute V3 with SDK parity, packaging, release workflows, and cross-language smoke tests.
7. Execute V4 with backup/restore hardening, audit integrity, and security automation.
8. Execute V5 with WASM feasibility (M1 — done), AsyncStorageBackend trait + IndexedDB (M2), browser SDK + Web Worker + offline-first (M3), release readiness (M4).
9. Execute V6 with simulation, diagnostics, reference tooling, and ecosystem expansion.

## Final Direction

Aegis should remain:

- embedded,
- fast,
- durable,
- explainable,
- multi-model,
- cross-language.

Aegis should not drift into:

- managed auth infrastructure,
- distributed auth control plane,
- network service dependency,
- ops-heavy deployment model.

The differentiator is the combination of:

- ReBAC-native core,
- broader authorization model coverage,
- embedded execution,
- real durability,
- developer-first ergonomics.

That is the long-term implementation target for Aegis.
