# Aegis

**Embedded, relationship-based authorization runtime (ReBAC).**  
Single-process, zero external servers, multi-language.

[![CI — ubuntu](https://img.shields.io/github/actions/workflow/status/ParvLab/AEGIS/ci.yml?branch=main&label=ubuntu&logo=ubuntu)](https://github.com/ParvLab/AEGIS/actions)
[![CI — windows](https://img.shields.io/github/actions/workflow/status/ParvLab/AEGIS/ci.yml?branch=main&label=windows&logo=windows)](https://github.com/ParvLab/AEGIS/actions)
[![CI — macOS](https://img.shields.io/github/actions/workflow/status/ParvLab/AEGIS/ci.yml?branch=main&label=macOS&logo=apple)](https://github.com/ParvLab/AEGIS/actions)
[![Rust](https://img.shields.io/badge/rust_MSRV-1.96-dea584?logo=rust)](https://github.com/ParvLab/AEGIS)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
[![npm — @aegis-v/engine](https://img.shields.io/npm/v/@aegis-v/engine?label=%40aegis-v%2Fengine&logo=npm)](https://www.npmjs.com/package/@aegis-v/engine)
[![npm — @aegis-v/browser](https://img.shields.io/npm/v/@aegis-v/browser?label=%40aegis-v%2Fbrowser&logo=npm)](https://www.npmjs.com/package/@aegis-v/browser)
[![PyPI — aegis-auth](https://img.shields.io/pypi/v/aegis-auth?label=aegis-auth&logo=pypi)](https://pypi.org/project/aegis-auth/)
[![Go Reference](https://img.shields.io/badge/go-reference-00ADD8?logo=go)](https://pkg.go.dev/github.com/ParvLab/AEGIS/go)

</div>

---

- [Overview](#overview)
- [Architecture](#architecture)
- [Features](#features)
- [Language SDKs](#language-sdks)
- [Quick Start](#quick-start)
  - [Node.js](#nodejs)
  - [Python](#python)
  - [Go](#go)
  - [C / FFI](#c--ffi)
  - [Browser / WASM](#browser--wasm)
  - [CLI](#cli)
- [Storage Backends](#storage-backends)
- [Schema Definition](#schema-definition)
- [API Reference](#api-reference)
- [Configuration](#configuration)
- [Performance](#performance)
- [Building from Source](#building-from-source)
- [Testing](#testing)
- [Security](#security)
- [Project Status](#project-status)
- [Contributing](#contributing)
- [License](#license)

---

## Overview

Aegis is a **relationship-based access control (ReBAC)** engine inspired by Google's [Zanzibar](https://research.google/pubs/pub48190/) paper. Unlike external authorization services (e.g. OPA, AuthzForce, or cloud IAM), Aegis is designed to be **embedded directly into your application process**:

```
┌─────────────────────────────────────────────────────┐
│                   Your Application                   │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐          │
│  │  Node.js  │  │  Python  │  │    Go    │   ...     │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘          │
│       │              │             │                  │
│  ┌────┴──────────────┴─────────────┴────┐             │
│  │         Aegis Runtime (Rust)          │             │
│  │  ┌──────────┐  ┌──────────────────┐  │             │
│  │  │ ReBAC     │  │  Policy Lifecycle │  │             │
│  │  │ Engine    │  │  Draft→Validate→  │  │             │
│  │  │           │  │  Approve→Publish  │  │             │
│  │  ├──────────┤  ├──────────────────┤  │             │
│  │  │ Condition │  │  Audit Chain     │  │             │
│  │  │ Engine    │  │  (SHA-256)       │  │             │
│  │  ├──────────┤  ├──────────────────┤  │             │
│  │  │ Rate      │  │  Schedule /      │  │             │
│  │  │ Limiter   │  │  Enforcement     │  │             │
│  │  └──────────┘  └──────────────────┘  │             │
│  └──────────┬──────────────────────────┘             │
└─────────────┼────────────────────────────────────────┘
              │
     ┌────────┴────────┐
     │  Storage Layer   │
     │  SQLite  │  PG   │
     │  MySQL   │  RDB  │
     │  Memory  │ IDB   │
     └─────────────────┘
```

**Key design decisions:**

- **No servers, no sidecars** — embed directly. No network calls, no serialization overhead, no deployment complexity.
- **ReBAC-native** — relationships (`user:X is member of team:Y`) are the first-class primitive, not roles or attributes.
- **Pluggable storage** — choose SQLite (default, zero-config), PostgreSQL, MySQL, RocksDB, in-memory, or IndexedDB (browser).
- **Multi-language** — native bindings for Node.js (NAPI), Python (PyO3), Go (CGo), C (FFI), and WebAssembly (browser).
- **Audit integrity** — every mutation is hash-chained into a tamper-evident audit log using SHA-256.
- **Partitioned** — isolate authorization graphs per tenant, environment, or application within the same process.

---

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│                        SDK Layer                                │
│  ┌──────────┐  ┌──────────┐  ┌──────┐  ┌──────┐  ┌─────────┐  │
│  │ Node.js  │  │  Python  │  │  Go  │  │  C   │  │ Browser  │  │
│  │ (NAPI)   │  │ (PyO3)   │  │(CGo) │  │(FFI) │  │ (WASM)  │  │
│  └────┬─────┘  └────┬─────┘  └──┬───┘  └──┬───┘  └────┬────┘  │
└───────┼──────────────┼───────────┼──────────┼───────────┼───────┘
        │              │           │          │           │
┌───────┴──────────────┴───────────┴──────────┴───────────┴───────┐
│                      Aegis Runtime (Rust)                        │
│                                                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    GraphEngine                            │   │
│  │  ┌────────────┐  ┌──────────────┐  ┌──────────────────┐  │   │
│  │  │ Check Path │  │ Write Path   │  │ Explain / WhoCan  │  │   │
│  │  │  & Traverse │  │  & Revision  │  │ Access / Diff    │  │   │
│  │  └─────┬──────┘  └──────┬───────┘  └───────┬──────────┘  │   │
│  │        │                │                   │              │   │
│  │  ┌─────┴──────────────────┴──────────────────┴──────────┐  │   │
│  │  │              Subsystems                              │  │   │
│  │  │  ┌──────────┐ ┌───────────┐ ┌──────────────────┐   │  │   │
│  │  │  │ACL/RBAC  │ │ Condition │ │  Policy Lifecycle │   │  │   │
│  │  │  │Resolver  │ │ Evaluator │ │  Draft↔Published  │   │  │   │
│  │  │  ├──────────┤ ├───────────┤ ├──────────────────┤   │  │   │
│  │  │  │ Hierarchy │ │ Rate      │ │  Audit Chain     │   │  │   │
│  │  │  │ Resolver  │ │ Limiter   │ │  (SHA-256)       │   │  │   │
│  │  │  ├──────────┤ ├───────────┤ ├──────────────────┤   │  │   │
│  │  │  │ Decision │ │ Scheduler │ │  Enforcement     │   │  │   │
│  │  │  │ Cache    │ │ (Cron)    │ │  History         │   │  │   │
│  │  │  └──────────┘ └───────────┘ └──────────────────┘   │  │   │
│  │  └──────────────────────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │               Storage Adapter Layer                       │   │
│  │  ┌──────────┐ ┌──────────┐ ┌────────┐ ┌───────┐ ┌─────┐  │   │
│  │  │  SQLite  │ │PostgreSQL│ │  MySQL │ │RocksDB│ │Mem  │  │   │
│  │  │ (default)│ │ (opt)    │ │ (opt)  │ │ (opt) │ │     │  │   │
│  │  └──────────┘ └──────────┘ └────────┘ └───────┘ └─────┘  │   │
│  │  ┌────────────────────────────────────────────────────┐   │   │
│  │  │              IndexedDB (Browser)                    │   │   │
│  │  └────────────────────────────────────────────────────┘   │   │
│  └──────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────┘
```

### Core Engine Modules

| Module | File | Purpose |
|--------|------|---------|
| `GraphEngine` | `engine/mod.rs` | Main entry point — check, write, delete, explain, health |
| `ACL/RBAC` | `engine/acl.rs`, `engine/rbac.rs` | Direct tuple resolution and role hierarchy traversal |
| `Condition` | `engine/condition.rs` | ABAC-style attribute condition evaluation |
| `Hierarchy` | `engine/hierarchy.rs` | Subject-set resolution (`team:X#member`) |
| `Traversal` | `engine/traversal.rs` | Graph traversal for reachability analysis |
| `Cache` | `engine/cache.rs` | Decision and traversal LRU caches |
| `Partition` | `engine/partition.rs` | Multi-tenant partition management |
| `Rate Limiter` | `engine/ratelimit.rs` | Token bucket rate limiting per operation |
| `GDPR` | `engine/gdpr.rs` | Subject data export and right-to-erasure |
| `Watch` | `engine/watch.rs` | Event stream subscription |
| `Policy Lifecycle` | `engine/policy_lifecycle.rs` | Draft→validate→submit→approve→publish |
| `Scheduler` | `engine/scheduler.rs` | Cron-based recurring analysis |
| `Enforcement History` | `engine/enforcement_history.rs` | Sampled decision recording |
| `Migration` | `engine/migration.rs` | Schema version migration framework |

---

## Features

### Core Authorization

| Feature | Status | Description |
|---------|--------|-------------|
| Check (is-allowed) | ✅ | `engine.check("user:1", "read", "doc:42")` → boolean |
| Write Tuple | ✅ | `engine.write("user:1", "owner", "doc:42")` → revision |
| Delete Tuple | ✅ | `engine.delete("user:1", "owner", "doc:42")` |
| Explain | ✅ | Returns traversal path showing why access was granted/denied |
| List by Object | ✅ | `engine.list_by_object("doc:42")` → all tuples for an object |
| List by Subject | ✅ | `engine.list_by_subject("user:1")` → all tuples for a subject |
| Query | ✅ | Filtered tuple query with pagination |
| Subject-Set Resolution | ✅ | `team:eng#member` nested group resolution |
| Role Hierarchy | ✅ | `admin` inherits all `member` permissions |
| ABAC Conditions | ✅ | Attribute-based conditions on tuples (`attr eq value`, time windows) |
| Access Diff | ✅ | Semantic diff between two policy schemas |
| Who Can Access | ✅ | Reverse search — enumerate subjects that can reach a permission |
| Dry-Run Check | ✅ | `check()` without recording audit event |
| Dry-Run Write | ✅ | `write()` without committing |
| Partition Isolation | ✅ | Multi-tenant graph isolation within single process |

### Operational Intelligence (V7)

| Feature | Status | Description |
|---------|--------|-------------|
| Policy Lifecycle | ✅ | Draft→validate→submit→approve→reject→publish→archive workflow |
| Scheduled Analysis | ✅ | Cron-based recurring integrity and access analysis |
| Enforcement History | ✅ | Opt-in sampled recording of check decisions with trend analysis |
| Event Stream | ✅ | Watch/subscribe to policy, integrity, and analysis events |

### Audit & Compliance

| Feature | Status | Description |
|---------|--------|-------------|
| SHA-256 Audit Chain | ✅ | Every mutation is hash-chained — tamper-evident audit log |
| Audit Chain Verification | ✅ | `verify_audit_chain()` recomputes and validates every hash link |
| GDPR Export | ✅ | `export_subject()` — all data for a subject |
| Right to Erasure | ✅ | `delete_subject()` — cascade delete with policy options |
| Backup/Restore | ✅ | Full backup with schema snapshots, revision-safe restore |
| Integrity Reporting | ✅ | Comprehensive `integrity_report()` with cross-checks |

### Schema & Policy

| Feature | Status | Description |
|---------|--------|-------------|
| YAML Schema | ✅ | Declarative type/relation/permission definition |
| Schema Linter | ✅ | `schema-lint` — validate schema correctness |
| Schema Diff | ✅ | Semantic comparison between two schemas |
| Compatibility Check | ✅ | Detect breaking changes before deployment |
| Hot-Reload | ✅ | Watch schema file for changes and reload at runtime |
| Policy Versioning | ✅ | Snapshot and rollback policy schemas |

### Developer Tooling

| Feature | Status | Description |
|---------|--------|-------------|
| CLI | ✅ | Full command set + interactive REPL |
| Interactive REPL | ✅ | `aegis repl` — type-ahead with history |
| Multi-Language SDK | ✅ | Node.js, Python, Go, C, Browser/WASM |
| OpenTelemetry | ✅ | Metrics and tracing export |
| Fuzz Testing | ✅ | `cargo fuzz` — schema parser, tuple input |

---

## Language SDKs

| Language | Package | Directory | Bindings | Methods |
|----------|---------|-----------|----------|---------|
| Node.js | `@aegis-v/engine` | `crates/aegis-napi/` | NAPI-RS (native) | 40+ |
| Python | `aegis-auth` | `crates/aegis-pyo3/` | PyO3 (native) | 30+ |
| Go | `aegis-go` | `crates/aegis-go/` | CGo (C FFI) | 27 |
| C | `aegis_ffi.h` | `crates/aegis-ffi/` | C FFI (cdylib) | 27 |
| Browser | `@aegis-v/browser` | `packages/aegis-browser/` | WASM (wasm-pack) | 10 |
| Rust | `aegis-core` | `crates/aegis-core/` | Native (direct) | Full API |

---

## Quick Start

### Node.js

```bash
npm install @aegis-v/engine
```

```js
const { Engine } = require('@aegis-v/engine');

const engine = new Engine('aegis.db', `
types:
  user: {}
  repo:
    relations:
      owner: {}
      viewer: {}
    permissions:
      read:
        include: [owner, viewer]
`);

engine.write('user:alice', 'owner', 'repo:myapp');
const result = engine.check('user:alice', 'read', 'repo:myapp');
console.log(result.allowed); // true
```

### Python

```bash
pip install aegis-auth
```

```python
from aegis import Engine

engine = Engine("aegis.db", """
types:
  user: {}
  repo:
    relations:
      owner: {}
      viewer: {}
    permissions:
      read:
        include: [owner, viewer]
""")

engine.write("user:alice", "owner", "repo:myapp")
result = engine.check("user:alice", "read", "repo:myapp")
print(result.allowed)  # True
```

### Go

```bash
go get github.com/ParvLab/AEGIS-go
```

Requires `libaegis_ffi` shared library on the library path.

```go
package main

import (
    "fmt"
    "github.com/ParvLab/AEGIS-go"
)

func main() {
    engine, err := aegis.New(aegis.Config{
        DBPath:     "aegis.db",
        SchemaYAML: schema,
    })
    if err != nil { panic(err) }
    defer engine.Close()

    engine.Write("user:alice", "owner", "repo:myapp")
    result, _ := engine.Check("user:alice", "read", "repo:myapp")
    fmt.Println(result.Allowed) // true
}

const schema = `
types:
  user: {}
  repo:
    relations:
      owner: {}
      viewer: {}
    permissions:
      read:
        include: [owner, viewer]
`
```

### C / FFI

```c
#include "aegis_ffi.h"

int main() {
    aegis_handle *eng = aegis_create("aegis.db",
        "types:\n  user: {}\n  repo:\n    relations:\n      owner: {}\n      viewer: {}\n    permissions:\n      read:\n        include: [owner, viewer]\n");

    aegis_write_result wr = aegis_write(eng, "user:alice", "owner", "repo:myapp");

    aegis_check_result cr = aegis_check(eng, "user:alice", "read", "repo:myapp");
    printf("allowed = %d\n", cr.allowed); // 1

    aegis_destroy(eng);
    return 0;
}
```

Build: link against `libaegis_ffi.so` / `aegis_ffi.dll` / `libaegis_ffi.dylib`.

### Browser / WASM

```bash
npm install @aegis-v/browser
```

```typescript
import { createEngine } from '@aegis-v/browser';

const engine = await createEngine({
  schema: `
    types:
      user: {}
      repo:
        relations:
          owner: {}
          viewer: {}
        permissions:
          read:
            include: [owner, viewer]
  `,
  storage: 'indexeddb', // or 'memory'
});

await engine.write('user:alice', 'owner', 'repo:myapp');
const result = await engine.check('user:alice', 'read', 'repo:myapp');
console.log(result.allowed); // true
```

### CLI

```bash
# Install from source
cargo install --path crates/aegis-cli

# One-shot commands
aegis check user:alice read repo:myapp --schema schema.yml
aegis write user:alice owner repo:myapp --schema schema.yml
aegis explain user:alice read repo:myapp --schema schema.yml

# Interactive REPL
aegis repl --schema schema.yml
```

**REPL commands:**

| Command | Description |
|---------|-------------|
| `check <subject> <perm> <object>` | Check permission |
| `write <subject> <rel> <object>` | Write relationship |
| `delete <subject> <rel> <object>` | Delete relationship |
| `explain <subject> <perm> <object>` | Explain access decision |
| `who <perm> <object>` | Who can access (reverse search) |
| `list <object>` | List all tuples for an object |
| `history` | Show recent audit events |
| `health` | Show engine health |
| `backup <path>` | Create backup |
| `restore <path>` | Restore from backup |
| `help` | Show all commands |
| `exit` / `quit` | Exit REPL |

---

## Storage Backends

| Backend | Feature Flag | Type | Persistent | Concurrent | WAL | Browser | Use Case |
|---------|-------------|------|-----------|------------|-----|---------|----------|
| **SQLite** | `sqlite` (default) | Embedded SQL | ✅ | r2d2 pool | ✅ | ❌ | Default, single-server apps |
| **PostgreSQL** | `postgres` | External SQL | ✅ | deadpool | ✅ | ❌ | Multi-server, production HA |
| **MySQL** | `mysql` | External SQL | ✅ | mysql_async | ✅ | ❌ | Multi-server, MySQL shops |
| **RocksDB** | `rocksdb` | Embedded KV | ✅ | CF + prefix iter | ❌ | ❌ | High-throughput, embedded |
| **InMemory** | — | Memory | ❌ | Mutex | ❌ | ❌ | Testing, ephemeral workloads |
| **IndexedDB** | `wasm` | Browser JS | ✅ | IDB tx | ❌ | ✅ | Offline-first browser apps |

### Selecting a Backend

```rust
use aegis_core::storage::sqlite::SqliteStorage;
use aegis_core::storage::postgres::PostgresStorage;
use aegis_core::storage::mysql::MySqlStorage;
use aegis_core::storage::RocksDbStorage;
use aegis_core::storage::InMemoryStorage;

// SQLite (default)
let mut storage = SqliteStorage::new("aegis.db")?;

// PostgreSQL
let mut storage = PostgresStorage::new("host=localhost user=... dbname=aegis")?;

// RocksDB
let mut storage = RocksDbStorage::new("/data/aegis-rocks")?;

// InMemory
let storage = InMemoryStorage::new();
```

---

## Schema Definition

Aegis uses a YAML-based schema language to define types, relations, and permissions:

```yaml
types:
  user: {}

  team:
    relations:
      member: {}
      admin: {}
    permissions:
      view:
        include:
          - member
          - admin
      manage:
        include:
          - admin

  repo:
    relations:
      owner: {}
      maintainer: {}
      viewer: {}
    permissions:
      read:
        include:
          - owner
          - maintainer
          - viewer
      write:
        include:
          - maintainer
          - owner
      admin:
        include:
          - owner
```

### Schema Concepts

- **Types** — entities in the authorization domain (`user`, `team`, `repo`, `doc`)
- **Relations** — direct relationships between subjects and objects (`owner`, `member`, `viewer`)
- **Permissions** — computed access levels composed from relations (`read`, `write`, `admin`)
- **Inheritance** — permissions can include other permissions or relations
- **Subject Sets** — `team:eng#member` refers to all members of team `eng`
- **Conditions** — attribute-based conditions on tuples:
  ```yaml
  permissions:
    read:
      include:
        - owner
      condition: "role eq admin AND clearance gt 5"
    ```

---

## API Reference

### Core Operations

```rust
// Lifecycle
GraphEngine::new(storage, schema) -> Self
engine.initialize() -> Result<()>
engine.close() -> Result<()>

// Authorization
engine.check(subject, permission, resource) -> Result<CheckResult>
engine.write(subject, relation, resource) -> Result<WriteResult>
engine.delete(subject, relation, resource) -> Result<WriteResult>
engine.explain(subject, permission, resource) -> Result<ExplainResult>
engine.explain_v2(subject, permission, resource) -> Result<ExplainV2Result>

// Query & List
engine.list_by_object(object, filter, pagination) -> Result<PaginatedTuples>
engine.list_by_subject(subject, filter, pagination) -> Result<PaginatedTuples>
engine.query(filter, pagination) -> Result<PaginatedTuples>

// Analysis (V6)
engine.who_can_access(permission, resource) -> Result<Vec<SubjectId>>
engine.access_diff(old_schema, new_schema) -> Result<AccessDiffReport>
engine.integrity_report() -> Result<IntegrityReport>
engine.simulate_changes(tuples) -> Result<SimulationReport>
engine.reachable_subjects(object) -> Result<Vec<SubjectId>>
engine.find_orphaned_tuples() -> Result<Vec<TupleKey>>
engine.find_high_access_subjects(threshold) -> Result<Vec<SubjectAccessCount>>
engine.tenant_leakage_detection() -> Result<TenantLeakageReport>

// Policy Lifecycle (V7)
engine.create_policy_draft(name, description, schema) -> Result<PolicyDraft>
engine.update_policy_draft(id, schema) -> Result<PolicyDraft>
engine.validate_policy_draft(id) -> Result<ValidationReport>
engine.submit_for_review(id) -> Result<PolicyDraft>
engine.approve_policy_draft(id) -> Result<PolicyDraft>
engine.publish_policy_draft(id) -> Result<PublishResult>
engine.reject_policy_draft(id, reason) -> Result<PolicyDraft>
engine.archive_policy_draft(id) -> Result<()>
engine.list_policy_drafts(status_filter) -> Result<Vec<PolicyDraft>>

// Scheduled Analysis (V7)
engine.schedule_analysis(config) -> Result<AnalysisSchedule>
engine.clear_analysis_schedule(id) -> Result<()>
engine.list_analysis_schedules() -> Result<Vec<AnalysisSchedule>>
engine.list_analysis_runs(limit) -> Result<Vec<AnalysisRun>>
engine.run_analysis_now(schedule_id) -> Result<()>

// Enforcement History (V7)
engine.configure_enforcement(config) -> Result<()>
engine.get_enforcement_config() -> Result<EnforcementHistoryConfig>
engine.get_enforcement_trends(limit) -> Result<EnforcementTrends>

// Event Stream (V7)
engine.subscribe(filter) -> Result<WatchSubscription>

// Audit & Compliance
engine.audit_trail(object, from, to, limit) -> Result<Vec<AuditEntry>>
engine.export_subject(subject) -> Result<SubjectDataExport>
engine.delete_subject(subject, policy, transfer_to) -> Result<DeleteResult>
engine.verify_audit_chain(partition_id) -> Result<Option<String>>
engine.integrity_check() -> Result<Option<String>>

// Backup / Restore
engine.create_backup(path) -> Result<()>
engine.restore_backup(path) -> Result<()>
engine.export_json(writer, subject_filter) -> Result<()>
engine.import_json(reader) -> Result<()>

// Schema
engine.load_schema() -> Result<Schema>
engine.list_policy_versions() -> Result<Vec<PolicyVersion>>
engine.rollback_policy(version) -> Result<()>

// Partition Management
engine.create_partition(id) -> Result<()>
engine.delete_partition(id) -> Result<()>
engine.list_partitions() -> Result<Vec<PartitionId>>
engine.switch_partition(id) -> Result<()>

// Configuration
engine.set_actor_identity(identity) -> Option<String>
engine.set_rate_limiter(config) -> Result<()>
engine.set_hooks(hooks) -> Result<()>
engine.set_logger(log_fn) -> Result<()>
engine.set_fail_closed(mode) -> Result<()>
engine.set_telemetry(enabled) -> Result<()>
engine.set_api_key(key) -> Result<()>
engine.set_api_key_verified(verified) -> Result<()>
engine.set_integrity_check_interval(interval) -> Result<()>
engine.set_wal_checkpoint_threshold(threshold_mb) -> Result<()>
```

### Error Handling

Aegis uses a unified `AegisResult<T>` type alias with structured error variants:

```rust
pub enum AegisError {
    StorageConnection(String),
    StorageQuery(String),
    StorageNotInitialized,
    StorageExhausted,
    StorageCorruption(String),
    SchemaValidation(String),
    SchemaVersionMismatch { expected: u32, actual: u32 },
    SchemaMigration(String),
    SchemaNotFound(String),
    Validation(ValidationError),
    UnknownSubjectType(String),
    UnknownRelation { type_name: String, relation: String },
    UnknownPermission { type_name: String, permission: String },
    Consistency(String),
    CrossNodeToken,
    RevisionFromFuture(usize),
    PermissionDenied,
    OperationNotPermitted(String),
    RateLimitExceeded(String),
    Internal(String),
    EngineClosed,
    UnsupportedStorageOperation(String),
}
```

---

## Configuration

### Rate Limiting

```rust
use aegis_core::engine::ratelimit::{RateLimitConfig, RateLimitOp};

let config = RateLimitConfig {
    tokens_per_second: 100.0,
    bucket_size: 200,
    enabled_ops: vec![RateLimitOp::Check, RateLimitOp::Write],
};
engine.set_rate_limiter(config)?;
```

### Cache Configuration

```rust
// Decision cache: caches check() results (LRU, 10_000 entries, 30s TTL)
// Traversal cache: caches graph traversal results (LRU, 1_000 entries, 60s TTL)
```

### Consistency Modes

```rust
pub enum ConsistencyMode {
    MinimalLatency,  // Read from latest local revision
    BestEffort,      // Default — best available
    Strong,          // Wait for WAL commit
    Linearizable,    // Strict total order
}
```

### Fail-Closed Modes

```rust
pub enum FailClosedMode {
    #[default] DenyOnError,   // Deny on any internal error
    AllowOnError,              // Allow on internal error (use with caution!)
}
```

### Telemetry (OpenTelemetry)

```rust
// Enable with the `telemetry` feature flag
// aegis-core = { features = ["telemetry"] }

engine.set_telemetry(true)?;
```

Exports:
- `aegis.check.duration` — histogram of check latency
- `aegis.check.total` — counter of check decisions
- `aegis.write.total` — counter of write operations
- `aegis.storage.connections` — active connection gauge
- `aegis.graph.tuple_count` — total tuple gauge
- `aegis.graph.tenant_count` — active tenant gauge

---

## Performance

Approximate benchmarks on a modern x86_64 workstation (SQLite backend, decision cache warm):

| Operation | Latency | Throughput (single-threaded) |
|-----------|---------|------------------------------|
| Check (cache hit) | 0.1–0.5 µs | 2,000,000+ ops/sec |
| Check (cache miss, warm) | 1–10 µs | 100,000+ ops/sec |
| Check (cold, traversal) | 10–50 µs | 20,000+ ops/sec |
| Write Tuple | 5–20 µs | 50,000+ ops/sec |
| Delete Tuple | 5–20 µs | 50,000+ ops/sec |
| Explain Traversal | 10–100 µs | 10,000+ ops/sec |
| List by Object (100 tuples) | 50–200 µs | 5,000+ ops/sec |
| Audit Chain Verify (10K events) | 50–200 ms | — |
| Backup (10K tuples + 100K events) | 100–500 ms | — |

**Key factors:**
- WAL mode enables concurrent reads during writes
- Decision cache eliminates traversal for repeated checks
- Condition evaluation adds <5 µs per leaf condition
- RocksDB offers 2–5× throughput over SQLite for write-heavy workloads
- Cold start: first check after engine creation takes 50–200µs due to schema compilation

---

## Building from Source

### Prerequisites

- **Rust 1.96+** (MSRV 1.96.0, earlier versions may work but are untested)
- C toolchain (for native crate builds)
- **wasm-pack** (for browser/WASM builds)
- Optional: PostgreSQL/MySQL client libraries for those backends

### Build Commands

```bash
# Full workspace build
cargo build --workspace

# With specific backends
cargo build --workspace --features postgres
cargo build --workspace --features mysql
cargo build --workspace --features rocksdb
cargo build --workspace --features all

# Browser WASM build
cd packages/aegis-browser/rust
wasm-pack build --target web

# CLI build
cargo build -p aegis-cli

# Release build (optimized)
cargo build --release
```

### Feature Flags

| Flag | Enables | Default |
|------|---------|---------|
| `sqlite` | SQLite backend (via rusqlite + r2d2) | ✅ |
| `postgres` | PostgreSQL backend (via tokio-postgres + deadpool) | ❌ |
| `mysql` | MySQL backend (via mysql_async) | ❌ |
| `rocksdb` | RocksDB backend | ❌ |
| `hot-reload` | File-watch schema hot-reloading | ❌ |
| `telemetry` | OpenTelemetry metrics/tracing | ❌ |
| `wasm` | Browser/WASM async storage support | ❌ |
| `test-utils` | Test harness (implies sqlite) | ❌ |

---

## Testing

```bash
# Run all tests (default features)
cargo test --workspace

# Run with specific backend
cargo test --workspace --features postgres
cargo test --workspace --features mysql
cargo test --workspace --features rocksdb

# Run without default features (backends explicitly chosen)
cargo test --workspace --no-default-features --features sqlite

# WASM browser tests
cd crates/aegis-core
wasm-pack test --chrome --headless -- --no-default-features --features wasm

# Run only integration tests
cargo test --test v2_multi_model
cargo test --test v1_closure

# Stress / soak tests (run with --release for realistic results)
cargo test --test stress -- --release
cargo test --test soak -- --release

# Fuzz testing
cd crates/aegis-core
cargo fuzz run tuple_input    -- -max_total_time=120
cargo fuzz run schema_parser  -- -max_total_time=120

# Benchmarks
cargo bench --package aegis-core
```

### Test Suite Overview

| Suite | Type | Tests | Description |
|-------|------|-------|-------------|
| Unit tests | Inline `#[cfg(test)]` | 200+ | Per-module unit tests |
| `v1_closure` | Integration | 8 | CRUD lifecycle, traversal, tx, backup, migration, WAL, health |
| `v2_multi_model` | Integration | 25 | RBAC, ACL, ABAC, deny, expiry, hierarchy, subject-set, conditions |
| `stress` | Stress | 4 | Read-during-write, write-queue, large-graph, extended |
| `soak` | Soak | 2 | Memory leak, throughput targets |
| `fixture_based_test` | Integration | 1 | Fixture-driven integration |
| Go SDK test | E2E | 4 | Go binding health, check, write |
| `full_integration_cycle` | Integration | 1 | End-to-end cycle test |

---

## Security

### Vulnerability Reporting

Please report security vulnerabilities to **opensource@aegis-auth.dev** (PGP encrypted preferred) or via confidential GitHub issue. We acknowledge within 48 hours and aim for a fix within 5 business days.

See [SECURITY.md](SECURITY.md) for the full policy.

### Security Features

- **Fail-closed by default** — `DenyOnError` mode ensures any internal error results in a denial
- **Tamper-evident audit chain** — every mutation is SHA-256 hash-chained with previous event
- **Integrity verification** — `verify_audit_chain()` recomputes every hash link
- **Input validation** — all subject/resource/relation/partition strings validated against injection patterns
- **Metadata validation** — strict character whitelists and size limits on tuple metadata
- **Constant-time comparison** — API key verification uses `subtle::ConstantTimeEq`
- **Rate limiting** — token bucket prevents brute-force and DoS via check/write floods
- **SBOM** — generate via `cargo audit` / `cargo sbom` / `cargo-auditable`
- **`cargo deny`** — dependency license and advisory checking in CI

### CI Security Gates

- `cargo audit` — checks for known vulnerabilities in dependencies
- `cargo deny` — validates licenses, bans duplicate versions
- OpenSSF Scorecards — automated supply-chain security analysis

---

## Project Status

Aegis is currently at **V6 (Intelligence Layer) + V7 (Operational Intelligence)** development stage.

| Version | Focus | Status | Progress |
|---------|-------|--------|----------|
| V1 | Core Check/Write/Delete | ✅ Complete | 100% |
| V2 | Multi-model (RBAC, ACL, ABAC, hierarchy) | ✅ Complete | 100% |
| V3 | Storage backends + CLI | ✅ Complete | 100% |
| V4 | Enterprise (encryption, FIPS, hardening) | 🔄 In Progress | 15% |
| V5 | Browser / WASM support | 🔄 In Progress | 15% |
| V6 | Intelligence Layer (explain, who-can, diff, audit) | ✅ Complete | 100% |
| V7 | Operational Intelligence (lifecycle, scheduler, history, events) | ✅ Complete | 100% |

- **CI status**: All checks pass on Rust 1.96 (MSRV), stable, and nightly across Ubuntu, Windows, and macOS
- **Test count**: 250+ unit tests + 35+ integration tests + stress/soak tests + Go/WASM E2E tests
- **Documentation**: See [IMPLEMENTATION.md](IMPLEMENTATION.md), [AEGIS_IMPLEMENTATION_PLAN.md](AEGIS_IMPLEMENTATION_PLAN.md), [aegis-spec.md](aegis-spec.md)

### Roadmap (Never Build)

Aegis is intentionally **not** a distributed authorization service. The following are explicitly out of scope:

- ❌ WebSocket / SSE servers
- ❌ Message queues / durable delivery
- ❌ Distributed scheduling
- ❌ Dashboards, UIs, or visualization
- ❌ External infrastructure integrations (cloud IAM, LDAP, SCIM)
- ❌ Reverse proxies, sidecars, or gateways

Aegis **generates decisions and events**. Applications decide transport, visualization, alerting, and consumption.

---

## Contributing

We welcome contributions! Please see our guidelines:

1. **Fork** the repository
2. **Create a feature branch** (`git checkout -b feature/my-feature`)
3. **Make your changes** — please follow the existing code style
4. **Run tests** — `cargo test --workspace`
5. **Run clippy** — `cargo clippy --workspace --all-features -- -D warnings`
6. **Run formatter** — `cargo fmt --all -- --check`
7. **Submit a pull request**

### Code Style

- Rust edition 2024, MSRV 1.96.0
- Follow existing patterns — 2-space indent, no trailing whitespace
- Prefer `?` over `.unwrap()` / `.expect()` in production code
- Document public API surface with doc comments
- Add tests for new functionality

### Development Setup

```bash
git clone https://github.com/ParvLab/AEGIS.git
cd aegis
cargo build --workspace
cargo test --workspace
```

---

## License

Copyright 2026 Aegis Authors.

Licensed under the **Apache License, Version 2.0** (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

---

<div align="center">

**Aegis** — Embedded authorization. Zero servers. ReBAC-native.

[GitHub](https://github.com/ParvLab/AEGIS) · [Documentation](docs/architecture.md) · [Specification](aegis-spec.md) · [Implementation Plan](AEGIS_IMPLEMENTATION_PLAN.md) · [Test Plan](aegis-test-plan.md)

</div>
]]>