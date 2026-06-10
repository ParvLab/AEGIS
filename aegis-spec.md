# Aegis — Embedded Relationship-Based Authorization Runtime
### Complete Technical Specification & Build Reference

---

## Table of Contents

1. [Project Summary](#1-project-summary)
2. [Core Philosophy](#2-core-philosophy)
3. [What Aegis Is (and Is Not)](#3-what-aegis-is-and-is-not)
4. [Tech Stack](#4-tech-stack)
5. [System Architecture](#5-system-architecture)
6. [Core Concepts & Data Model](#6-core-concepts--data-model)
7. [Authorization Models](#7-authorization-models)
8. [Permission Evaluation Engine](#8-permission-evaluation-engine)
9. [Consistency Model & Transaction Semantics](#9-consistency-model--transaction-semantics)
10. [Storage & Persistence Layer](#10-storage--persistence-layer)
11. [Durability, Backup & Disaster Recovery](#11-durability-backup--disaster-recovery)
12. [SDK Reference](#12-sdk-reference)
13. [Architecture Patterns](#13-architecture-patterns)
14. [Security Model](#14-security-model)
15. [Scalability Strategy](#15-scalability-strategy)
16. [Multi-Tenancy](#16-multi-tenancy)
17. [Observability & Tracing](#17-observability--tracing)
18. [Developer Experience](#18-developer-experience)
19. [Development Roadmap](#19-development-roadmap)
20. [Real-World Use Cases](#20-real-world-use-cases)
21. [Integration & E2E Test Plan](#21-integration--e2e-test-plan)

---

## 1. Project Summary

**Aegis** is an embedded, distributed, relationship-based authorization runtime (ReBAC). It is inspired by production-grade systems like Google Zanzibar and SpiceDB, but built with a fundamentally different philosophy:

> *Authorization should feel like a library, not infrastructure.*

Most authorization systems force developers to maintain a separate auth service, a dedicated graph database, and an operational cluster. Aegis eliminates that overhead by embedding the entire authorization engine directly inside the application process — similar to how SQLite embeds a relational database without requiring a server.

Aegis is appropriate for:

- SaaS platforms with workspaces, teams, and role hierarchies

- Collaborative editors with per-document sharing models
- Developer platforms managing API keys, environments, and service access
- Multi-tenant systems requiring strict graph-level tenant isolation
- Local-first and edge applications requiring offline authorization
- Enterprise systems requiring audit trails and explainable access decisions

---

## 2. Core Philosophy

### Traditional Authorization Architecture

```
Application
    ↓
Authorization Service  ← separate deployment
    ↓
Authorization Database ← separate infrastructure
```

**Problems with this approach:**
- Additional deployment complexity and cost
- Network latency on every permission check
- Distributed system failure modes
- Operational burden (scaling, monitoring, patching)
- Poor developer experience during local development

### Aegis Architecture

```
Application
 ├── Business Logic
 ├── Main Database
 └── Embedded Aegis Runtime  ← co-located, in-process
      ├── Graph Engine (Rust core)
      ├── Policy Evaluator
      ├── Connection Manager
      └── Storage Adapter
```

Aegis runs inside the application process itself. Permission checks are local function calls, not HTTP requests.

### Design Principles

| Principle | Description |
|---|---|
| **Embedded First** | Authorization works without any separate infrastructure |
| **Explainability** | Every permission decision is traceable with a human-readable path |
| **Relationship-Centric** | Relationships are the first-class primitive, not roles or attributes |
| **Durability** | Authorization data must persist across restarts, crashes, and migrations |
| **Local Evaluation** | Checks are fast because they run in-process, not over the network |
| **Concurrent Safety** | Safe for multi-threaded access with clear locking semantics |
| **Progressive Scalability** | Start embedded; scale to distributed without changing the API |

---

## 3. What Aegis Is (and Is Not)

### What Aegis Manages

- Relationships between identities and resources
- Permission evaluation through graph traversal
- Policy definitions and inheritance rules
- Authorization traces and access explanations
- Multi-tenant permission graphs


### What Aegis Does NOT Manage

- User authentication (passwords, sessions, OAuth, JWT)
- Identity management (sign-up, email verification)
- Login flows or SSO

Aegis is designed to integrate with existing authentication providers:

- Clerk
- Auth0
- Firebase Auth
- Supabase Auth
- Custom identity systems

The contract is: **auth provider gives Aegis an identity; Aegis decides what that identity can do.**

---

## 4. Tech Stack

### Core Runtime

| Layer | Technology | Rationale |
|---|---|---|
| Core Engine | **Rust** | Memory safety, zero-cost abstractions, high-throughput graph traversal |
| Graph Traversal | Custom recursive engine in Rust | Parallel evaluation support, no GC pauses |
| Policy Engine | YAML schema + Rust evaluator | Human-readable policy definitions, compiled evaluation |
| IPC / Embedding | Rust FFI + NAPI (for Node) / PyO3 (for Python) / CGo (for Go) | Native embedding into host language runtimes |

### Storage Layer

| Backend | Use Case | Notes |
|---|---|---|
| **SQLite** | Local dev, small/medium apps, local-first | Persistent on-disk; single-file portability; WAL mode for concurrent access |
| **PostgreSQL** | Production SaaS, multi-instance deployments | Shared across app servers; full SQL capabilities |
| **MySQL** | Existing MySQL ecosystems | Compatible alternative to Postgres |
| **RocksDB** | High-throughput embedded deployments | LSM-tree storage; excellent write performance |
| **IndexedDB** | Browser / edge runtimes | In-browser local-first authorization |

> **Critical Rule:** Never use in-memory-only storage in production. Runtime state is ephemeral; authorization data must be durable.

### Connection Management

| Backend | Read Connections | Write Connections | Pooling |
|---|---|---|---|
| SQLite (WAL) | Multiple concurrent readers | Single serialized writer | Configurable pool size |
| PostgreSQL | Connection pool | Connection pool (single writer recommended) | Native pooling via pg-pool / r2d2 |
| MySQL | Connection pool | Connection pool | Native pooling |
| RocksDB | Single-process single-instance | Single-process single-instance | Not applicable |

### SDKs

| Language | Package | Notes |
|---|---|---|
| **TypeScript / Node.js** | `@aegis/core` | Primary SDK; NAPI bindings to Rust core |
| **Go** | `github.com/aegis-auth/aegis-go` | CGo bindings; idiomatic Go API |
| **Rust** | `aegis` (crate) | Direct library usage; zero-overhead |
| **Python** | `aegis-auth` (PyPI) | PyO3 bindings; async-compatible |

### Infrastructure / Tooling

| Tool | Purpose |
|---|---|
| WAL (Write-Ahead Log) | Storage-level concurrency (SQLite); replication stream (future) |
| CRDT | Future multi-region graph consistency |

---

## 5. System Architecture

### Component Overview

```
┌──────────────────────────────────────────────────────────────────┐
│                      Application Process                          │
│                                                                   │
│  ┌─────────────┐    ┌─────────────────────────────────────┐      │
│  │  App Logic  │───▶│          Aegis Runtime               │      │
│  └─────────────┘    │                                       │      │
│                      │  ┌─────────────────────────────────┐ │      │
│  ┌─────────────┐    │  │   Graph Traversal Engine (Rust) │ │      │
│  │   App DB    │    │  │   - Recursive traversal          │ │      │
│  └─────────────┘    │  │   - Parallel sibling evaluation  │ │      │
│                      │  │   - Cycle detection              │ │      │
│                      │  └─────────────────────────────────┘ │      │
│                      │  ┌─────────────────────────────────┐ │      │
│                      │  │   Policy Evaluator              │ │      │
│                      │  │   - Schema resolution            │ │      │
│                      │  │   - Inheritance rules            │ │      │
│                      │  │   - Hot-reload support           │ │      │
│                      │  └─────────────────────────────────┘ │      │
│                      │  ┌─────────────────────────────────┐ │      │
│                      │  │   Permission Tracer             │ │      │
│                      │  │   - Evaluation path recording    │ │      │
│                      │  │   - Human-readable explanations  │ │      │
│                      │  └─────────────────────────────────┘ │      │
│                      │  ┌─────────────────────────────────┐ │      │
│                      │  │   Connection Manager            │ │      │
│                      │  │   - Read connection pool         │ │      │
│                      │  │   - Write connection (serialized)│ │      │
│                      │  │   - Health monitoring            │ │      │
│                      │  └─────────────────────────────────┘ │      │
│                      │  ┌─────────────────────────────────┐ │      │
│                      │  │   Cache Layer                   │ │      │
│                      │  │   - Decision cache (TTL+revision)│ │      │
│                      │  │   - Traversal cache              │ │      │
│                      │  │   - LRU eviction                 │ │      │
│                      │  └─────────────────────────────────┘ │      │
│                      │  ┌─────────────────────────────────┐ │      │
│                      │  │   Storage Adapter               │ │      │
│                      │  └────────────┬────────────────────┘ │      │
│                      └───────────────┼──────────────────────┘      │
└──────────────────────────────────────┼─────────────────────────────┘
                                        │
                          ┌─────────────▼─────────────────────┐
                          │       Persistent Storage            │
                          │  (SQLite / PostgreSQL / etc)        │
                          └───────────────────────────────────┘
```

### Runtime Components

**1. Graph Engine (Rust Core)**
- Recursive relationship traversal
- Parallel sub-graph evaluation (Rust async tasks)
- Permission decision computation
- Relationship tuple management
- Cycle detection with visited-node tracking

**2. Policy Engine**
- YAML-defined permission schemas
- Inheritance rule resolution
- Relationship type validation
- Schema compilation and hot-reload
- Schema version compatibility checks

**3. Permission Tracer**
- Records evaluation path per check
- Produces human-readable decision explanations
- Supports access audit history
- Exports traces as structured data (JSON / OTel)

**4. Connection Manager**
- Manages read-connection pool (multiple concurrent readers)
- Maintains single serialized write connection
- WAL checkpoint scheduling
- Connection health monitoring and auto-recovery
- `PRAGMA busy_timeout` configuration (SQLite)

**5. Cache Layer**
- Decision cache: caches final allow/deny results keyed by (subject, relation, object)
- Traversal cache: caches intermediate graph path segments
- Both caches invalidated on revision bump
- Configurable TTL, max-size, LRU eviction policy

**6. Storage Adapter**
- Pluggable backend interface
- Manages relationship tuples on disk
- Handles indexing for subject/object/relation lookups
- Schema version tracking
- WAL-based replication (future)

**7. Event Log (future)**
- Append-only relationship event stream
- Enables graph reconstruction, rollback, and sync
- CRDT delta sync layer

---

## 6. Core Concepts & Data Model

### Identity

An identity is any principal that can hold a relationship. Identities are typed strings.

```
user:123
team:engineering
service:billing
apikey:abc123
bot:deploy-agent
```

### Resource

A resource is any protected object within the system.

```
workspace:core
repo:fluxbus
document:design-spec
project:aegis
environment:production
memory:workspace-alpha
```

### Relationship Tuple

The atomic unit of the authorization graph. Every access rule is derived from tuples.

```
subject   relation   object
─────────────────────────────
user:123  editor     repo:fluxbus
team:eng  owner      workspace:core
```

The full tuple structure in storage:

```typescript
interface RelationshipTuple {
  subject:   string                // e.g. "user:123"
  relation:  string                // e.g. "editor"
  object:    string                // e.g. "repo:fluxbus"
  createdAt: Date                  // immutable timestamp of creation
  metadata?: Record<string, string> // optional key-value annotations
}
```

### Revision

Every mutation to the graph produces a monotonically increasing revision number. This revision serves as a consistency token and enables snapshot-isolated reads.

```
Revision 1:   ADD user:123 editor repo:fluxbus
Revision 2:   ADD team:eng owner workspace:core
Revision 3:   REMOVE user:456 viewer repo:core
```

### Schema / Policy Definition

Schemas define which relations are valid and how permissions are derived.

```yaml
schemaVersion: 1

namespace: acme

types:
  repo:
    relations:
      owner:
        - user
        - team#member        # team members inherit owner relation

      editor:
        - owner              # owners are always editors
        - collaborator

      viewer:
        - editor             # editors are always viewers
        - public

    permissions:
      read:
        - viewer
        - editor
        - owner

      write:
        - editor
        - owner

      delete:
        - owner
```

### Indexes

Three core indexes maintained for fast lookup:

| Index | Query Pattern | Complexity |
|---|---|---|
| Subject Index | "All objects this subject has a relation to" | O(1) |
| Object Index | "All subjects with a relation to this object" | O(1) |
| Relation Index | "All tuples with this relation type" | O(1) |

---

## 7. Authorization Models

| Model | Status | Notes |
|---|---|---|
| **ReBAC** (Relationship-Based) | Supported | Primary model; contextual and hierarchical |
| **RBAC** (Role-Based) | Supported | Implemented as a relationship graph pattern |
| **Hierarchical Permissions** | Supported | Via recursive tuple traversal |
| **Contextual Roles** | Supported | Role is scoped to a resource, not global |
| **Multi-Tenant Authorization** | Supported | Via tenant-scoped graph namespacing |
| **Recursive Relationships** | Supported | Deep graph traversal with cycle detection |
| **ABAC** (Attribute-Based) | Planned (V3) | Policy conditions on relationship metadata |

### RBAC via ReBAC Pattern

Traditional RBAC maps roles globally. Aegis models roles as a relationship layer:

```
RBAC (traditional):
  user:123 → role:admin → [all admin permissions globally]

ReBAC as RBAC (Aegis):
  role:admin  grants    permission:delete-workspace
  user:123    member    role:admin        ← scoped to tenant:alpha
  role:admin  member    tenant:alpha      ← role is a resource, not global

Access is contextual to each resource, not a global role.
```

### ReBAC (Native)

```
ReBAC (Aegis):
  user:123 → editor → repo:fluxbus
  user:123 → viewer → repo:other

Access is contextual to each resource.
```

---

## 8. Permission Evaluation Engine

### Evaluation Flow

When a permission check is issued:

```
check(user:123, "edit", repo:fluxbus)
```

The engine:

1. Looks up direct tuples for `(user:123, *, repo:fluxbus)` in the current snapshot
2. Resolves the schema to find what relations satisfy `edit`
3. Recursively traverses inherited relationships
4. Executes policy rules at each graph node
5. Checks decision cache for a cached result at the current revision
6. Returns `allow` or `deny` with a resolution path

### Example Traversal

```
user:123
  ↓ member
team:engineering
  ↓ owner
workspace:core
  ↓ contains
repo:fluxbus

→ Decision: ALLOW (edit)
```

### Cycle Detection

The traversal engine tracks visited nodes in a hash set to prevent infinite loops in circular relationship graphs. If a cycle is detected, that branch returns `deny` and evaluation continues on remaining branches.

### Parallel Evaluation

Sibling branches of the graph are evaluated concurrently using Rust async tasks. The first `allow` response short-circuits remaining branches via cooperative cancellation (structured concurrency).

### Concurrency Model

| Aspect | Policy |
|---|---|
| Read concurrency | Unlimited concurrent readers (WAL mode) |
| Write concurrency | Single serialized writer at any time |
| Lock granularity | Database-level (SQLite); row-level (PostgreSQL) |
| Busy handling | `busy_timeout` with exponential backoff (configurable) |
| Deadlock prevention | Single writer eliminates write-write deadlocks; read-locks are shared |
| Async safety | All public APIs are Send + Sync safe in Rust; thread-safe across languages |

### Check API

```typescript
const result = await auth.check({
  subject:  "user:123",
  relation: "edit",
  object:   "repo:fluxbus",
  context?: { tenant: "alpha" }  // optional scoping
})
// result: { allowed: boolean, revision: number }
```

### Explain API

```typescript
const trace = await auth.explain({
  subject:  "user:123",
  relation: "edit",
  object:   "repo:fluxbus"
})

// Response:
{
  allowed: true,
  path: [
    "user:123",
    "team:engineering",
    "workspace:core",
    "repo:fluxbus"
  ],
  resolvedVia: "editor → member → owner",
  evaluatedAt: "2025-01-01T00:00:00Z",
  durationMs: 0.8,
  revision: 42
}
```

The `resolvedVia` field uses right-facing arrows (`→`) to show the derivation chain from the user toward the resource.

---

## 9. Consistency Model & Transaction Semantics

### Revision-Based Snapshot Isolation

Aegis provides **revision-based snapshot isolation** inspired by Google Zanzibar. Every graph write produces a new, globally unique, monotonically increasing revision number. Reads are implicitly against the latest available snapshot, with explicit controls:

| Consistency Mode | Description | Use Case |
|---|---|---|
| `minimize_latency` (default) | Reads from latest available local snapshot. May be slightly stale in multi-instance deployments. | Hot-path permission checks |
| `at_revision(token)` | Reads from a snapshot at least as fresh as the given revision token. Guarantees read-your-writes. | After a relationship write, check reflects it |
| `fully_consistent` | Reads the absolute latest committed state. Highest latency in distributed setups. | Audit, compliance, administrative checks |

### Revision Token

```typescript
interface RevisionToken {
  revision: number    // monotonic counter
  nodeId: string      // originating node identifier
  timestamp: Date     // wall-clock at write time
}
```

Every write returns a `RevisionToken`. Passing this token to a subsequent read guarantees the read reflects that write:

```typescript
const { token } = await auth.write({
  subject: "user:123", relation: "editor", object: "repo:fluxbus"
})
// token: { revision: 42, nodeId: "node-a", timestamp: ... }

// Later — guaranteed to see the write:
const { allowed } = await auth.check({
  subject: "user:123", relation: "edit", object: "repo:fluxbus",
  consistency: { atRevision: token }
})
```

### Transaction Support

```typescript
// Single atomic transaction
const result = await auth.transaction(async (tx) => {
  await tx.write({ subject: "user:123", relation: "editor", object: "repo:a" })
  await tx.write({ subject: "user:123", relation: "viewer", object: "repo:b" })
  await tx.write({ subject: "team:eng", relation: "owner", object: "workspace:core" })

  // All-or-nothing: if any write fails, all are rolled back
})

// Transaction with savepoint (nested)
await auth.transaction(async (tx) => {
  await tx.write({ subject: "user:123", relation: "editor", object: "repo:a" })
  await tx.savepoint(async (sp) => {
    await sp.write({ subject: "user:123", relation: "viewer", object: "repo:b" })
    // If this block fails, only the savepoint is rolled back, not the outer tx
  })
})
```

### Write Semantics

| Operation | Behavior |
|---|---|
| Write existing tuple | Idempotent (upsert) — metadata updated if provided |
| Write new tuple | Inserted at current revision |
| Delete existing tuple | Removed at current revision; recorded in event log |
| Delete non-existent tuple | No-op (not an error) |
| Concurrent writes | Serialized by single-writer connection; no conflicts possible |

### Read-Your-Writes Guarantee

Within the same process, all reads after a write implicitly see the latest revision (session causality). This is enforced by the revision counter in the Connection Manager. Across processes (multi-instance), the explicit `atRevision` token must be used.

---

## 10. Storage & Persistence Layer

### Critical Rule

> **Embedded does not mean ephemeral.**

Aegis runtime state (in-memory caches, computed traversals) is temporary. Authorization data (relationship tuples, schemas, event logs) is always persisted to disk-backed storage.

This is identical to SQLite: it is embedded, but data lives on disk and survives process restarts.

### Storage Separation

| Layer | Nature | Survives Restart? |
|---|---|---|
| Runtime cache | In-memory | No |
| Computed traversals | In-memory | No |
| Decision cache | In-memory | No |
| Relationship tuples | Persistent DB | Yes |
| Schema/policy definitions | Persistent DB | Yes |
| Event log | Persistent append-only | Yes |
| Snapshots | Persistent file | Yes |

### Connection Configuration

```typescript
// SQLite (embedded, local)
const auth = new Aegis({
  storage: "sqlite",
  path: "./aegis.db",
  pool: {
    maxReaders: 4,              // read connection pool size
    busyTimeoutMs: 5000,        // PRAGMA busy_timeout
    walMode: true,              // enable WAL journal mode
    synchronous: "normal"       // PRAGMA synchronous
  }
})

// PostgreSQL (shared, multi-instance)
const auth = new Aegis({
  storage: "postgres",
  connectionString: process.env.AEGIS_DB_URL,
  pool: {
    maxConnections: 10,
    idleTimeoutMs: 30000
  }
})

// RocksDB (high-throughput embedded)
const auth = new Aegis({
  storage: "rocksdb",
  path: "./aegis-data"
})
```

### WAL Mode (SQLite)

WAL (Write-Ahead Logging) is **required** for SQLite in any multi-threaded or concurrent-read scenario:

- Multiple readers can read concurrently while a writer is active
- Writers are still serialized (one at a time), but readers never block
- WAL file size monitored by the Connection Manager; automatic checkpoint on close
- `PRAGMA synchronous = NORMAL` for safe crash recovery with WAL

> **Warning:** WAL mode is incompatible with network filesystems. Always use a local filesystem for SQLite WAL.

### Schema Versioning & Migration

The schema version is stored in the database and validated on startup.

```typescript
// Automatic migration (default)
const auth = new Aegis({
  storage: "sqlite",
  path: "./aegis.db",
  schema: "./schema.yaml",
  migrations: {
    autoMigrate: true,            // apply pending migrations on open
    targetVersion: undefined      // latest if not specified
  }
})

// Manual migration
await auth.migrate({ targetVersion: 3 })
// Returns: { fromVersion: 2, toVersion: 3, appliedMigrations: ["v2-to-v3"] }

// Schema compatibility check (no changes applied)
const report = await auth.checkSchema({ schema: "./new-schema.yaml" })
// report: { compatible: true, warnings: [], breaking: [] }
```

#### Migration Rules

| Rule | Description |
|---|---|
| **Additive only** | New types/relations/permissions can be added at any time |
| **Remove with care** | Removing a relation requires verifying no tuples use it |
| **Rename is two-step** | Add new name → migrate data → remove old name |
| **Version tracking** | Schema version field in DB; migration scripts numbered |
| **Rollback** | Each migration has a reverse script for rollback (tested) |

---

## 11. Durability, Backup & Disaster Recovery

Authorization data is mission-critical. Loss of the permission graph results in either universal access (catastrophic) or universal lockout (equally catastrophic). The following strategies are mandatory for production.

### Backup

```bash
# CLI backup
aegis backup create --output ./backups/aegis-$(date +%Y%m%d).backup

# Backup includes:
# - all relationship tuples
# - schema definitions
# - policy configurations
# - event log (if enabled)
# - metadata
# - current revision token
```

Automated backup recommendations:
- Daily snapshots at minimum
- Backup to external object storage (S3, GCS, R2)
- Test restore procedures quarterly
- Retain at least 30 days of backups

### Restore

```bash
aegis backup restore --from ./backups/aegis-20250101.backup
```

Restore validates:
- Backup integrity checksum
- Schema compatibility with current runtime
- Revision continuity with event log

### Graph Export / Import

```bash
# Export full graph to portable format
aegis export --format json --output graph.json

# Import to new instance
aegis import --from graph.json
```

Use cases: migrations, compliance, portability, seeding new environments.

### Event Log (Append-Only)

The most powerful durability primitive in Aegis. Instead of storing only current graph state, the event log stores every mutation as an ordered event:

```
[rev:1]  ADD    user:123  editor  repo:fluxbus
[rev:2]  ADD    team:eng  owner   workspace:core
[rev:3]  REMOVE user:456  viewer  repo:core

```

Benefits of event log architecture:

| Capability | Description |
|---|---|
| **Replayability** | Reconstruct any historical graph state at any revision |
| **Auditability** | Complete history of every permission change |
| **Recovery** | Replay events after corruption or accidental wipe |
| **Debugging** | Understand exactly when and why access changed |
| **Rollback** | Replay events up to a point in time |
| **Sync** | Replicate to edge nodes or secondary instances |

### Event Log Compaction

Over time, the event log grows unbounded. Compaction merges redundant events:

```
Before compaction:
  [rev:1]  ADD  user:123  editor  repo:a
  [rev:5]  REM  user:123  editor  repo:a
  [rev:8]  ADD  user:123  editor  repo:a

After compaction (same effect, fewer events):
  [rev:8]  ADD  user:123  editor  repo:a
```

Compaction is:
- Optional and configurable (retention window, size threshold)
- Background process, does not block reads/writes
- Reversible from backup if needed

### Event-Sourced Architecture (Recommended for Production)

```
Write Path:
  auth.write(tuple)
    → assign revision number
    → append to event log
    → apply to current graph state

Recovery Path:
  aegis recover --from-events
    → replay all events in order
    → reconstruct current graph state
    → return final revision number

Point-in-Time Recovery:
  aegis recover --to-revision 42
    → replay events up to revision 42
    → return graph state at that point
```

### Multi-Server Deployments

```
Load Balancer
 ├── Server A (Aegis embedded)
 ├── Server B (Aegis embedded)
 └── Server C (Aegis embedded)
         ↓
   Central PostgreSQL
   (shared persistent storage)
```

If one server dies, others continue serving from the same shared graph. No permission data is lost. Revision tokens remain valid across instances as long as they share storage.

### GDPR Compliance

| API | Purpose |
|---|---|
| `auth.deleteSubject("user:123")` | Removes all tuples referencing a user as subject |
| `auth.exportSubject("user:123")` | Exports all relationships for a user as structured JSON |
| `auth.deleteObject("workspace:old")` | Removes all tuples for a resource |

Deletion policy for ownership cascading:

```typescript
await auth.deleteSubject("user:123", {
  ownershipPolicy: "transfer",    // "fail" | "transfer" | "cascade"
  transferToSubject: "user:456"   // new owner if ownershipPolicy is "transfer"
})
```

All deletion operations are recorded in the audit log with responsible identity and timestamp.

---

## 12. SDK Reference

### Installation

```bash
# TypeScript / Node.js
npm install @aegis/core

# Python
pip install aegis-auth

# Go
go get github.com/aegis-auth/aegis-go

# Rust
cargo add aegis
```

### Initialization

```typescript
import { Aegis } from "@aegis/core"

const auth = new Aegis({
  storage: "sqlite",
  path: "./aegis.db",
  schema: "./aegis.schema.yaml",
  pool: { maxReaders: 4, busyTimeoutMs: 5000 },
  cache: { decisionTtlMs: 30000, maxEntries: 10000 },
  consistency: { defaultMode: "minimize_latency" }
})

await auth.initialize()
// - Validates schema
// - Applies pending migrations
// - Opens storage connections
// - Verifies database integrity
// - Returns { schemaVersion: 3, revision: 42, healthy: true }
```

### Write Relationship

```typescript
const result = await auth.write({
  subject:  "user:123",
  relation: "editor",
  object:   "repo:fluxbus",
  metadata: { grantedBy: "admin:456" }   // optional
})
// result: { revision: 43, token: { revision: 43, nodeId: "...", timestamp: ... } }
```

### Atomic Transaction

```typescript
const result = await auth.transaction(async (tx) => {
  await tx.write({ subject: "user:123", relation: "editor", object: "repo:a" })
  await tx.write({ subject: "user:123", relation: "viewer", object: "repo:b" })
  // All writes commit atomically; any failure rolls back all
})
// result: { revision: 44, token: ... }
```

### Check Permission

```typescript
const { allowed, revision } = await auth.check({
  subject: "user:123",
  relation: "edit",
  object: "repo:fluxbus",
  consistency: { mode: "minimize_latency" }   // optional
})

if (!allowed) throw new ForbiddenError()

// With revision token for read-your-writes:
const result = await auth.check({
  subject: "user:123",
  relation: "edit",
  object: "repo:fluxbus",
  consistency: { atRevision: previousWriteToken }
})
```

### Delete Relationship

```typescript
const result = await auth.delete({
  subject:  "user:123",
  relation: "editor",
  object:   "repo:fluxbus"
})
// result: { revision: 45, token: ... }
```

### Bulk Delete Subject

```typescript
// Remove all traces of a user (GDPR / account deletion)
await auth.deleteSubject("user:123", {
  ownershipPolicy: "transfer",
  transferToSubject: "user:456"
})
```

### List Relationships

```typescript
// All relationships on an object
const tuples = await auth.list({ object: "repo:fluxbus" })

// All objects a subject has a relation to
const tuples = await auth.list({ subject: "user:123" })

// Filtered by relation
const tuples = await auth.list({
  object:   "workspace:core",
  relation: "member"
})

// With pagination
const page = await auth.query({
  filter: { subjectType: "user", relation: "editor" },
  pagination: { limit: 100, cursor: page.nextCursor }
})
```

### Explain Permission (Trace)

```typescript
const trace = await auth.explain({
  subject: "user:123",
  relation: "edit",
  object: "repo:fluxbus"
})

console.log(trace.path)
// ["user:123", "team:engineering", "workspace:core", "repo:fluxbus"]

console.log(trace.resolvedVia)
// "editor → member → owner"
```

### Bulk Write

```typescript
await auth.writeBatch([
  { subject: "user:123", relation: "editor", object: "repo:a" },
  { subject: "user:123", relation: "viewer", object: "repo:b" },
  { subject: "team:eng", relation: "owner",  object: "workspace:core" },
])
// Atomic — all succeed or all roll back
```

### Dry-Run Mode

Evaluate permissions without any side effects:

```typescript
// Check without modifying state
const { allowed, path } = await auth.check({
  subject: "user:123", relation: "edit", object: "repo:fluxbus"
}, { dryRun: true })

// Validate a write without persisting
const { valid, warnings } = await auth.write({
  subject: "user:123", relation: "editor", object: "repo:fluxbus"
}, { dryRun: true })
// valid: true, warnings: []
```

### Health Check

```typescript
const health = await auth.health()
// {
//   status: "healthy",             // "healthy" | "degraded" | "unhealthy"
//   storageConnected: true,
//   schemaVersion: 3,
//   currentRevision: 42,
//   cacheHitRatio: 0.87,
//   cacheSize: 2341,
//   readConnections: { active: 2, idle: 2, total: 4 },
//   walSizeMb: 12.4,               // SQLite only
//   uptimeMs: 3840000
// }
```

### Watch / Subscription

```typescript
const subscription = auth.watch({
  object: "repo:fluxbus",
  sinceRevision: 42
})

subscription.on("change", (event) => {
  // event: { type: "add" | "remove", subject, relation, object, revision, timestamp }
  console.log(`Revision ${event.revision}: ${event.subject} ${event.relation} ${event.object}`)
})

// Later:
subscription.unsubscribe()
```

### Audit Log

```typescript
const history = await auth.audit({
  object:  "repo:fluxbus",
  from:    "2025-01-01",
  to:      "2025-02-01"
})

// Returns ordered list of all relationship mutations on this resource
// Each entry: { revision, action: "add"|"remove", subject, relation, object, timestamp, metadata }
```

### Export User Data (GDPR)

```typescript
const data = await auth.exportSubject("user:123")
// {
//   subject: "user:123",
//   exportedAt: "2025-01-01T00:00:00Z",
//   relationships: [
//     { subject: "user:123", relation: "editor", object: "repo:fluxbus", createdAt: ... }
//   ],
//   formatVersion: 1
// }
```

### Middleware (Express / Hono example)

```typescript
export function requirePermission(relation: string, getResource: (req: Request) => string) {
  return async (req: Request, res: Response, next: NextFunction) => {
    const { allowed } = await auth.check({
      subject: `user:${req.user.id}`,
      relation,
      object: getResource(req)
    })
    if (!allowed) return res.status(403).json({ error: "Forbidden" })
    next()
  }
}

// Usage
router.put(
  "/repos/:id",
  requirePermission("edit", (req) => `repo:${req.params.id}`),
  updateRepoHandler
)
```

---

## 13. Architecture Patterns

### Pattern 1: Embedded (Default)

The application and Aegis run in the same process. Aegis is added as a library dependency (`cargo add`, `npm install`, `pip install`). Storage is either local (SQLite/RocksDB) or external (PostgreSQL).

```
Application Process
 ├── App Logic
 ├── Aegis Runtime (in-process)
 │    ├── Graph Engine
 │    ├── Connection Manager
 │    └── Cache Layer
 └── Storage: SQLite (local) or PostgreSQL (shared)
```

**Best for:** startups, SaaS products, developer tools, local-first apps.

### Mode 2: Distributed / Cluster

Multiple application instances share a central Aegis-aware storage layer. Optional Aegis coordination layer for distributed caching and watch streams.

```
Applications (N instances)
       ↓
 Aegis Cluster (coordination layer)
       ↓
 Distributed Storage (PostgreSQL / CockroachDB)
```

**Best for:** enterprises, large-scale SaaS, multi-region systems.

### Mode 3: Hybrid Edge

Central graph with edge replicas for low-latency offline evaluation.

```
Central Graph (authoritative)
       ↓
  Edge Replicas (read-only, synced via CRDT)
       ↓
  Local Evaluation (near-user, offline-capable)
```

**Best for:** CDN-native apps, edge runtimes, IoT, mobile-first platforms.

---

## 14. Security Model

### Server-Authoritative Design

Clients never directly write to the authorization graph. All mutations are:
1. Received by the application server
2. Validated against existing permissions (can this caller modify this relationship?)
3. Written to Aegis only if authorized

### Principle of Least Privilege

Permission schemas should define the minimum relations required. Wildcard grants should require explicit policy justification.

### Tenant Isolation

Each tenant's graph is scoped under a tenant namespace. Cross-tenant graph traversal is not possible by default.

```
tenant:alpha → workspace:core (alpha)
tenant:beta  → workspace:core (beta)

user:123 (alpha) cannot access any resource under tenant:beta
```

### Read-Only Edge Replicas

Edge replicas support local read evaluations but cannot accept write operations. All mutations route to the authoritative server.

### Input Validation & Constraints

```typescript
// Subject/resource naming rules:
// - Must match: /^[a-zA-Z0-9_:-]+$/
// - Max length: 256 characters
// - No SQL injection characters

// Metadata constraints:
// - Max 16 key-value pairs
// - Key max length: 64 characters
// - Value max length: 512 characters
// - Keys must match: /^[a-zA-Z0-9_-]+$/
```

Violations throw `AegisValidationError` before any database operation.

### Rate Limiting & Abuse Prevention

| Limit | Default | Configurable |
|---|---|---|
| Max write rate (per tenant) | 1000/s | Yes |
| Max traversal depth | 32 | Yes |
| Max relationships per resource | ∞ | Yes (per namespace) |
| Max query result size | 1000 | Yes |

### Auditability

Every write to the graph is:
- Revision-numbered
- Timestamped
- Associated with the requesting identity
- Stored in the event log
- Available for inspection via the audit API

---

## 15. Scalability Strategy

### Phase 1 — Embedded (Millions of Relationships)

- Single embedded runtime
- SQLite (WAL mode) or PostgreSQL backend
- In-process graph traversal
- Local index-based lookups
- Decision cache with TTL + revision-based invalidation
- Connection manager with read pool

### Phase 2 — Distributed Cache (Hundreds of Millions)

- Distributed caching layer (Redis / shared in-process cache)
- Parallel graph traversal across CPU cores
- Shared PostgreSQL with read replicas
- Graph indexing optimizations (composite indexes)
- Watch streams for cross-instance cache invalidation

### Phase 3 — Billion-Scale

- Graph sharding by tenant or resource namespace
- Distributed recursive evaluation with work dispatch
- Consistency tokens for snapshot-consistent reads across shards
- CRDT-based replication streams via WAL
- Partial graph sync for edge nodes

### Performance Optimizations

| Technique | Description |
|---|---|
| Subject Index | O(1) lookup of all objects for a subject |
| Object Index | O(1) lookup of all subjects for an object |
| Decision Cache | Cache resolved permission decisions keyed by (subject, relation, object, revision) |
| Traversal Cache | Cache intermediate graph path segments |
| Parallel Evaluation | Concurrent sibling branch evaluation in Rust async |
| Short-circuit | First `allow` in OR branches terminates remaining checks via structured cancellation |
| Cycle Detection | Prevents infinite loops in circular graphs |

### CRDT-Based Graph Sync (V3+)

Multi-node deployments use an **OR-Set CRDT** (Observed-Removed Set) over relationship tuples for convergent synchronization:

- Each tuple `(subject, relation, object)` is a unique element in the set
- Deletes are tracked via tombstones (add-wins semantics)
- Merge is commutative, associative, and idempotent
- Delta-state sync: only transmit differences between nodes
- SQLite remains authoritative transactional store; CRDT layer syncs between instances

```
Node A (primary)
  │
  ├── CRDT delta ──→ Node B (edge)
  │
  └── CRDT delta ──→ Node C (edge)

All nodes converge to the same state given the same operations.
```

---

## 16. Multi-Tenancy

### Graph Scoping

Tenants are represented as graph namespaces. All resources and identities under a tenant are prefixed or scoped to prevent cross-tenant access.

```
tenant:alpha
 ├── workspace:core (alpha)
 ├── team:engineering (alpha)
 └── user:123 (alpha member)

tenant:beta
 ├── workspace:core (beta)
 └── user:456 (beta member)
```

### Relationship Isolation

```typescript
await auth.write({
  subject:  "user:123",
  relation: "member",
  object:   "tenant:alpha"
})

// Check is tenant-scoped
await auth.check({
  subject: "user:123",
  relation: "read",
  object:   "workspace:core",
  context:  { tenant: "alpha" }    // limits traversal to tenant namespace
})
```

### Admin Isolation

Tenant admins have no power over other tenants. Super-admins are a separate identity class with explicit policies. Cross-tenant relationships require a super-admin grant.

---

## 17. Observability & Tracing

### Permission Tracing

Every check can return a full explanation of how the decision was reached.

```typescript
const trace = await auth.explain({
  subject: "user:123",
  relation: "edit",
  object: "repo:fluxbus"
})

// Returns:
{
  allowed:     true,
  path:        ["user:123", "team:engineering", "workspace:core", "repo:fluxbus"],
  resolvedVia: "editor → member → owner",
  evaluatedAt: "2025-01-01T00:00:00Z",
  durationMs:  0.8,
  revision:    42,
  cacheHit:    false
}
```

### Metrics

Expose the following metrics (Prometheus-compatible via OpenTelemetry):

| Metric | Type | Description |
|---|---|---|
| `aegis.check.total` | Counter | Total permission checks (tagged by status: allowed/denied/error) |
| `aegis.check.duration_ms` | Histogram | Evaluation latency (buckets: 0.1, 0.5, 1, 5, 10, 50, 100) |
| `aegis.graph.tuple_count` | Gauge | Total relationship tuples in graph |
| `aegis.graph.tenant_count` | Gauge | Active tenants |
| `aegis.cache.hit_ratio` | Gauge | Decision cache efficiency |
| `aegis.cache.size` | Gauge | Current cache entry count |
| `aegis.storage.connections.active` | Gauge | Active read connections |
| `aegis.storage.wal_size_mb` | Gauge | WAL file size (SQLite only) |
| `aegis.schema.version` | Gauge | Current schema version |
| `aegis.revision.current` | Gauge | Current graph revision |

### OpenTelemetry Integration

Aegis accepts an optional OpenTelemetry `TracerProvider` and `MeterProvider` on initialization:

```typescript
const auth = new Aegis({
  storage: "sqlite",
  path: "./aegis.db",
  openTelemetry: {
    tracerProvider: otel.trace.getTracerProvider(),
    meterProvider:  otel.metrics.getMeterProvider(),
    serviceName:    "my-app",
    attributes:     { env: "production" }
  }
})
```

When not configured, a **no-op** implementation is used (zero overhead, no allocations).

Span coverage:
- `aegis.check` — permission check evaluation
- `aegis.write` — relationship write
- `aegis.delete` — relationship deletion
- `aegis.read` — relationship list/query
- `aegis.explain` — permission trace
- `aegis.migrate` — schema migration

### Structured Logging

Aegis accepts an optional logger callback:

```typescript
const auth = new Aegis({
  storage: "sqlite",
  path: "./aegis.db",
  logger: (level, message, context) => {
    console.log(JSON.stringify({ level, message, ...context }))
  }
})
```

Log levels: `error`, `warn`, `info`, `debug`

Key log events:
- Schema migration applied (version from → to)
- Cache eviction (entries removed)
- Write conflict detected
- Storage connection recovered after failure
- WAL checkpoint completed

### Health Check API

```typescript
const health = await auth.health()
// {
//   status: "healthy" | "degraded" | "unhealthy",
//   storage: { connected: true, type: "sqlite", version: "3.45.0" },
//   schema: { version: 3, valid: true },
//   revision: { current: 42, persisted: 42 },
//   cache: { hitRatio: 0.87, size: 2341, maxSize: 10000 },
//   connections: { readActive: 2, readIdle: 2, writeBusy: false },
//   walSizeMb: 12.4,
//   uptimeMs: 3840000,
//   lastIntegrityCheck: "2025-01-01T00:00:00Z",
//   integrityStatus: "ok"
// }
```

### Audit Log API

```typescript
const history = await auth.audit({
  object:  "repo:fluxbus",
  from:    "2025-01-01",
  to:      "2025-02-01"
})

// Returns ordered list of all relationship mutations on this resource
```

### Audit Log Retention

```typescript
const auth = new Aegis({
  storage: "sqlite",
  path: "./aegis.db",
  audit: {
    retentionDays: 90,            // auto-purge entries older than 90 days
    compactionSchedule: "daily",  // background compaction frequency
    archivePath: "/mnt/archive"   // optional: archive before purge
  }
})
```

---

## 18. Developer Experience

### REPL (Interactive Shell)

Aegis ships with a command-line REPL for interactive debugging and policy exploration:

```bash
npx @aegis/repl --storage sqlite --path ./aegis.db
```

```
Aegis REPL v0.1.0
> write user:123 editor repo:fluxbus
  ✓ revision: 42

> check user:123 edit repo:fluxbus
  ✓ ALLOWED (0.4ms)
  path: user:123 → team:eng → workspace:core → repo:fluxbus

> explain user:123 edit repo:fluxbus
  allowed: true
  resolvedVia: editor → member → owner

> watch repo:fluxbus
  [revision:43] ADD user:456 viewer repo:fluxbus
  [revision:44] REM user:456 viewer repo:fluxbus

> help
  Available commands: write, check, delete, list, explain, watch, schema, health
```

### Policy Linting

```bash
# Validate schema for common issues
aegis schema lint ./schema.yaml

# Check for:
# - Orphan relations (defined but never referenced)
# - Circular relation chains
# - Overly broad permissions (wildcards)
# - Unused types/namespaces
# - Missing documentation
```

### Test Helpers & Fixtures

```typescript
import { createTestAegis } from "@aegis/testing"

// In-memory SQLite for fast test isolation
const auth = await createTestAegis()

// Load fixture
await auth.loadFixture("./test/fixtures/basic-team.yaml")

// make assertions
expect(await auth.check({
  subject: "user:123", relation: "edit", object: "repo:fluxbus"
})).toMatchObject({ allowed: true })
```

Fixture format:

```yaml
# test/fixtures/basic-team.yaml
schema: ./schema.yaml
tuples:
  - subject: "user:123"
    relation: "member"
    object: "team:eng"
  - subject: "team:eng"
    relation: "owner"
    object: "workspace:core"
  - subject: "workspace:core"
    relation: "contains"
    object: "repo:fluxbus"
```

### Schema Hot-Reload

```typescript
const auth = new Aegis({
  storage: "sqlite",
  path: "./aegis.db",
  schema: {
    path: "./schema.yaml",
    watch: true               // watch for file changes
  }
})

// On schema file change:
// 1. Validate new schema against existing data
// 2. If compatible, atomically swap schema
// 3. Log the change
// 4. Invalidated cached decisions that depend on changed relations
```

### Webhook / Event Hooks

```typescript
const auth = new Aegis({
  storage: "sqlite",
  path: "./aegis.db",
  hooks: {
    onWrite: (tuple) => {
      // Invalidate external cache, update search index, etc.
    },
    onDelete: (tuple) => {
      // Clean up related external state
    },
    onCheck: ({ subject, relation, object, allowed }) => {
      // Analytics, audit
    }
  }
})
```

---

## 19. Development Roadmap

### V1 — Foundation

- Embedded Rust runtime with SQLite (WAL mode) backend
- TypeScript SDK (NAPI bindings)
- ReBAC support with recursive graph traversal
- Policy engine (YAML schema) with version tracking
- Permission tracing / explain API
- Revision-based snapshot isolation
- Connection manager with read pool + serialized writer
- Decision cache with TTL + revision-based invalidation
- Multi-tenancy (namespace scoping)
- Transaction support (atomic multi-tuple writes)
- Dry-run mode for checks and writes
- Error handling hierarchy (AegisError types)
- Health check API
- Backup / export / import CLI
- REPL interactive debugger
- Policy linting CLI
- Test helpers / fixtures
- Input validation & constraints

### V2 — Distributed Foundation

- Event log (append-only relationship mutations)
- PostgreSQL backend
- Watch streams (subscribe to graph changes)
- Graph synchronization between instances
- Edge read replicas (read-only, synced)
- Distributed decision cache
- OpenTelemetry integration
- Structured logging
- Audit log retention & archival
- GDPR compliance APIs (deleteSubject, exportSubject, cascading ownership)
- Webhook / event hooks
- Schema hot-reload
- Go SDK + Python SDK
- MySQL backend

### V3 — Scale

- CRDT-based graph replication
- Distributed traversal dispatch
- Multi-region consistency tokens
- Partial graph sync for edge nodes
- Consistency-level controls per check (minimize_latency, at_revision, fully_consistent)
- ABAC (attribute-based conditions on relationship metadata)
- WAL-based sync between instances
- RocksDB backend
- Rate limiting & abuse prevention

---

## 20. Real-World Use Cases

### SaaS Platform

```
user:alice  owner    workspace:acme
user:bob    editor   repo:api-server
team:eng    member   workspace:acme
team:eng    owner    repo:api-server

→ bob can edit repo:api-server (direct)
→ alice can delete repo:api-server (via owner → workspace → repo)
→ new eng team member auto-inherits repo:api-server access
```

### Collaborative Document Editor

```
user:alice  owner    document:q4-report
user:bob    editor   document:q4-report
team:sales  viewer   document:q4-report

→ granular per-document access
→ team-level inheritance
→ no global roles needed
```

### Developer Platform

```
user:alice   owner    environment:production
service:ci   deploy   environment:staging
apikey:abc   read     environment:production

→ CI can deploy to staging but not production
→ API key read-only access scoped to production
```

---

## 21. Integration & E2E Test Plan

### Integration Tests

Integration tests validate the embedded runtime's internal components and their interactions.

| Test ID | Area | Scenario | Expected Result |
|---|---|---|---|
| INT-001 | Write + Check | Write a tuple, then check the corresponding permission | `check()` returns `{ allowed: true }` |
| INT-002 | Write + Check (deny) | Check a permission for a relation that was never granted | `check()` returns `{ allowed: false }` |
| INT-003 | Transaction atomicity | Write 3 tuples in a transaction; fail one; verify none persisted | Only the revision before the transaction is visible |
| INT-004 | Transaction success | Write 3 tuples in a transaction; commit; verify all visible | All 3 tuples queryable, single revision bump |
| INT-005 | Revision increment | Each write bumps revision by exactly 1 | Revision counter is strictly monotonically increasing |
| INT-006 | Read-your-writes | Write a tuple, check with its revision token | Check reflects the write |
| INT-007 | Consistency modes | Compare `minimize_latency`, `at_revision`, and `fully_consistent` modes | All return the same result at the same revision |
| INT-008 | Schema validation | Load an invalid schema (undefined relation reference) | Validation error thrown |
| INT-009 | Schema migration | Apply a migration from v1 to v2; verify schema version updated | `schemaVersion` in health check returns 2 |
| INT-010 | Migration rollback | Apply migration v2, then rollback to v1; verify tuples preserved | Tuples survive the rollback |
| INT-011 | Cycle detection | Create a circular relationship graph | Engine detects cycle, returns `deny` for affected branches |
| INT-012 | Parallel evaluation | Create a graph with 10 sibling branches, check permission | All branches evaluated, result returned within timeout |
| INT-013 | Decision cache hit | Check same permission twice | Second call returns from cache (lower latency) |
| INT-014 | Cache invalidation | Check permission, write a tuple affecting it, check again | Second check reflects new state (cache invalidated) |
| INT-015 | Dry-run check | Call `check()` with `{ dryRun: true }` | Returns decision without incrementing revision |
| INT-016 | Dry-run write | Call `write()` with `{ dryRun: true }` | Validates write without persisting; revision unchanged |
| INT-017 | Health check healthy | Normal operation | `health()` returns `{ status: "healthy" }` |
| INT-018 | Health check degraded | Close write connection | `health()` returns `{ status: "degraded" }` |
| INT-019 | Input validation | Write with subject containing SQL injection characters | `AegisValidationError` thrown |
| INT-020 | Metadata constraints | Write with 17 metadata key-value pairs (exceeds limit of 16) | `AegisValidationError` thrown |
| INT-021 | Concurrent reads | Open 10 concurrent read connections; read from all | All reads succeed concurrently |
| INT-022 | Serialized writes | Issue 20 concurrent writes | All writes execute sequentially in order; revision count is +20 |
| INT-023 | Delete relationship | Write a tuple, delete it, verify absence | `check()` returns `{ allowed: false }` |
| INT-024 | Delete non-existent | Delete a tuple that does not exist | No-op; no revision bump |
| INT-025 | Export user data (GDPR) | Export all relationships for a user | Returns complete list with correct format |
| INT-026 | Delete subject (GDPR) | Delete all relationships for a user | User has no remaining relationships |

### E2E Tests

End-to-end tests validate the full system across SDK boundaries, language runtimes, and deployment configurations.

| Test ID | Scenario | Steps | Expected Result |
|---|---|---|---|
| E2E-001 | Full lifecycle (TypeScript SDK) | Initialize → write tuple → check → explain → list → health → close | All APIs respond correctly, clean shutdown |
| E2E-002 | Full lifecycle (Go SDK) | Same operations via Go SDK | Same results as TypeScript |
| E2E-003 | Full lifecycle (Rust SDK) | Same operations via Rust SDK | Same results as TypeScript |
| E2E-004 | SQLite backend persistence | Write tuples, restart process, verify tuples survive | All tuples present after restart |
| E2E-005 | PostgreSQL backend persistence | Write tuples, restart process, verify tuples survive | All tuples present after restart |
| E2E-006 | Multi-tenancy isolation | Write same resource name in two tenants, cross-tenant check | Cross-tenant check returns denied |
| E2E-007 | Backup + restore | Create backup, delete graph, restore from backup | Graph state matches pre-delete state |
| E2E-010 | Export + import | Export graph to JSON, import into new instance | New instance has same graph state |
| E2E-011 | Event log reconstruction | Enable event log, write 100 tuples, recover from events | Graph state at final revision matches original |
| E2E-012 | Point-in-time recovery | Write 10 tuples, record revision at step 5, recover to revision 5 | Graph state matches what existed at revision 5 |
| E2E-013 | Middleware integration (Express) | Express app with Aegis middleware, authenticated request | Allowed requests pass, forbidden requests get 403 |
| E2E-014 | RBAC via ReBAC | Define RBAC roles as relationships, verify role-based access | Users with role can access, users without cannot |
| E2E-015 | Deep hierarchy (10 levels) | Build 10-level resource hierarchy, check descendant | Check returns allowed with complete trace path |
| E2E-016 | Large graph (1M tuples) | Insert 1M relationship tuples, run random checks | All checks complete within 10ms p99 |
| E2E-017 | Concurrent workloads | 50 concurrent clients writing + checking simultaneously | All operations complete without errors or deadlocks |
| E2E-018 | Schema hot-reload | Load schema with `watch: true`, modify schema file | New schema applied without restart, version bumped |
| E2E-019 | Watch subscription | Subscribe to changes on an object, write a tuple, verify event received | Event received with correct subject/relation/object |
| E2E-020 | Audit log queries | Perform writes/deletes, query audit log by time range | All relevant entries returned with correct timestamps |
| E2E-021 | GDPR user deletion with transfer | Delete user with `ownershipPolicy: "transfer"` | Ownership transferred, user's other relations removed |
| E2E-024 | CRDT sync (V3+) | Two nodes with CRDT sync, write on node A, read on node B | Node B converges to same state |
| E2E-025 | Edge replica read-only | Write to central, read from edge replica | Edge replica returns same result as central for reads |

### Test Environment Matrix

| Environment | Storage | Deployment | Purpose |
|---|---|---|---|
| CI (unit) | In-memory SQLite | Embedded | Fast integration tests |
| CI (integration) | SQLite file on disk | Embedded | Persistence and concurrent access |
| CI (e2e) | PostgreSQL | Embedded | Cross-backend compatibility |
| Staging | PostgreSQL | Distributed (2 instances) | Multi-instance consistency |
| Staging (edge) | SQLite + CRDT | Hybrid (central + edge) | CRDT sync correctness |
| Performance | RocksDB | Embedded | Throughput and latency benchmarks |

### Edge Cases Coverage

| Edge Case | Test Reference |
|---|---|
| Empty graph — any check returns denied | INT-002 |
| Self-referential relationship (cycle) | INT-011 |
| Very long relation chain (deep recursion) | E2E-015 |
| Concurrent deletion of the same tuple | INT-024 |
| Write during connection failure | Error test (INT-018) |
| Schema with no types defined | INT-008 |
| Migration from v0 (no version) to v1 | INT-009 |
| Extremely long subject name (256 chars) | INT-019 |
| Thousands of subjects on one object | E2E-016 |
| Check on object that was deleted | INT-023 |

---

## Appendix A: Error Handling Reference

All Aegis errors extend a base `AegisError` type:

| Error Type | HTTP-like Code | Cause |
|---|---|---|
| `AegisStorageError` | 500 | Database connection failed, disk full, corruption |
| `AegisSchemaError` | 400 | Invalid schema definition, incompatible version |
| `AegisValidationError` | 400 | Invalid input (subject/relation/object format) |
| `AegisConsistencyError` | 409 | Revision token from different node, stale token |
| `AegisNotFoundError` | 404 | Resource or subject not found in graph |
| `AegisRateLimitError` | 429 | Write rate limit exceeded |
| `AegisInternalError` | 500 | Unexpected internal failure (bug) |

**Failure mode policy:**
- **Storage errors** → fail-closed (deny all checks) by default; configurable to fail-open
- **Validation errors** → fail-closed with clear error message
- **Consistency errors** → caller should retry with fresh token

---

## Appendix B: Storage Decision Guide

| Scenario | Recommended Storage |
|---|---|
| Local development | SQLite (in-process, single file) |
| Unit / integration tests | In-memory SQLite (via `createTestAegis()`) |
| Single-server production | SQLite with persistent volume OR PostgreSQL |
| Multi-server production | PostgreSQL (shared) |
| High-write throughput | RocksDB |
| Browser / edge runtime | IndexedDB |
| Multi-region | PostgreSQL + CRDT replication (V3) |

---

## Appendix C: Security Checklist for Production

- [ ] Storage backend uses a persistent volume (not in-memory)
- [ ] Automated daily backups to external storage
- [ ] Event log enabled with retention policy configured
- [ ] Backup restore procedure tested quarterly
- [ ] No direct client writes to authorization graph (server-authoritative)
- [ ] Tenant scoping applied to all relationships
- [ ] OpenTelemetry metrics and tracing integrated
- [ ] Audit log retention policy defined and documented
- [ ] Schema reviewed for least-privilege principles

- [ ] All input validated (subject/relation/object format)
- [ ] WAL mode enabled for SQLite (local filesystem only)
- [ ] Write rate limits configured for production
- [ ] Health check endpoint configured for monitoring
- [ ] Migration rollback procedure documented and tested

---

## Appendix D: Configuration Reference

```typescript
interface AegisConfig {
  storage: "sqlite" | "postgres" | "mysql" | "rocksdb" | "indexeddb"
  path?: string                           // for sqlite, rocksdb
  connectionString?: string               // for postgres, mysql
  schema: string | SchemaDefinition       // path or inline definition
  pool?: {
    maxReaders?: number                   // default: 4
    busyTimeoutMs?: number                // default: 5000 (sqlite only)
    maxConnections?: number               // default: 10 (postgres only)
    idleTimeoutMs?: number                // default: 30000
  }
  cache?: {
    decisionTtlMs?: number                // default: 30000
    maxEntries?: number                   // default: 10000
    traversalCacheSize?: number           // default: 5000
  }
  consistency?: {
    defaultMode?: "minimize_latency" | "fully_consistent"  // default: "minimize_latency"
  }
  audit?: {
    retentionDays?: number                // default: 90
    compactionSchedule?: "daily" | "weekly" | "manual"
    archivePath?: string
  }
  openTelemetry?: {
    tracerProvider?: TracerProvider
    meterProvider?: MeterProvider
    serviceName?: string
    attributes?: Record<string, string>
  }
  logger?: (level: string, message: string, context?: any) => void
  hooks?: {
    onWrite?: (tuple: TupleEvent) => void
    onDelete?: (tuple: TupleEvent) => void
    onCheck?: (checkEvent: CheckEvent) => void
  }
  migrations?: {
    autoMigrate?: boolean                 // default: true
    targetVersion?: number
  }
}
```

---

*Document version 2.0 — Aegis: Make authorization powerful, scalable, understandable, and invisible.*
