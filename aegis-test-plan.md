# Aegis — Integration & End-to-End Test Plan
### Comprehensive test specification for the embedded ReBAC authorization runtime

---

## Table of Contents

1. [Test Philosophy](#1-test-philosophy)
2. [Test Environment Setup](#2-test-environment-setup)
3. [Integration Tests](#3-integration-tests)
4. [End-to-End Tests](#4-end-to-end-tests)
5. [Error Handling Tests](#5-error-handling-tests)
6. [Concurrency & Stress Tests](#6-concurrency--stress-tests)
7. [Persistence & Recovery Tests](#7-persistence--recovery-tests)
8. [Multi-Tenancy Isolation Tests](#8-multi-tenancy-isolation-tests)
9. [Security & Boundary Tests](#9-security--boundary-tests)
10. [SDK Cross-Language Tests](#10-sdk-cross-language-tests)
11. [Performance Benchmarks](#11-performance-benchmarks)
12. [Test Fixtures](#12-test-fixtures)

---

## 1. Test Philosophy

### Testing Levels

| Level | Scope | Run Frequency | Environment |
|---|---|---|---|
| **Unit** | Individual functions, schema parser, validator | Every commit | In-memory SQLite |
| **Integration** | Component interactions (write + check + transaction + cache) | Every commit | File-based SQLite |
| **E2E** | Full SDK lifecycle across language runtimes | Every PR | PostgreSQL |
| **Stress** | Concurrent access, large graphs, edge cases | Nightly | RocksDB / SQLite |
| **Recovery** | Backup/restore, event log replay, crash recovery | Weekly | File-based SQLite |

### Test Isolation Rules

- Each integration test creates a fresh in-memory or temporary-file Aegis instance
- No shared state between tests
- Tests clean up their temporary storage after completion
- E2E tests use dedicated database names in PostgreSQL (dropped after test run)

### Assertion Conventions

| Prefix | Meaning |
|---|---|
| `allow` | Permission check returns `{ allowed: true }` |
| `deny` | Permission check returns `{ allowed: false }` |
| `error` | Operation throws a specific `AegisError` |
| `revision(N)` | Current revision equals N |
| `token` | Operation returns a valid revision token |

---

## 2. Test Environment Setup

### Local (CI)

```typescript
import { createTestAegis } from "@aegis/testing"

// In-memory SQLite — fast, isolated, no cleanup needed
const auth = await createTestAegis({
  fixtures: ["./fixtures/basic-team.yaml"]
})
```

### File-Based (Integration)

```typescript
import { tmpdir } from "os"
import { join } from "path"
import { mkdtempSync, rmSync } from "fs"

const dir = mkdtempSync(join(tmpdir(), "aegis-test-"))
const auth = new Aegis({
  storage: "sqlite",
  path: join(dir, "test.db"),
  schema: "./fixtures/test-schema.yaml"
})
await auth.initialize()

// ... tests ...

// Cleanup
await auth.close()
rmSync(dir, { recursive: true, force: true })
```

### PostgreSQL (E2E)

PostgreSQL service required for E2E tests. Start via:

```bash
docker run -d --name aegis-pg \
  -e POSTGRES_DB=aegis_test \
  -e POSTGRES_USER=aegis \
  -e POSTGRES_PASSWORD=aegis_test \
  -p 5432:5432 postgres:16-alpine
```

### RocksDB (Stress)

```typescript
const dir = mkdtempSync(join(tmpdir(), "aegis-rocks-test-"))
const auth = new Aegis({
  storage: "rocksdb",
  path: dir
})
```

---

## 3. Integration Tests

### 3.1 Basic Write + Check Cycle

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| INT-001 | Direct permission check | Write `user:123 editor repo:a`, check `user:123 edit repo:a` | `allow` |
| INT-002 | Permission denied | Check `user:123 edit repo:a` without writing any tuple | `deny` |
| INT-003 | Empty graph | Check any permission on a fresh instance | `deny` |
| INT-004 | Idempotent write | Write same tuple twice | Second write is no-op; single revision bump |
| INT-005 | Write with metadata | Write tuple with `{ metadata: { source: "admin" } }`, read it back | Metadata preserved |

### 3.2 Transaction Semantics

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| INT-010 | Atomic multi-write | Transaction with 3 writes, commit | All 3 tuples visible, single revision bump |
| INT-011 | Atomic rollback | Transaction with 2 writes, force error in 3rd | None of the 3 writes visible |
| INT-012 | Nested savepoints | Outer write + inner savepoint write + fail inner | Outer write persists, inner does not |
| INT-013 | Empty transaction | Transaction with no writes | No revision bump, no error |
| INT-014 | Transaction with reads | Transaction: check → write → check | Second check sees the write (within transaction) |

### 3.3 Revision & Consistency

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| INT-020 | Revision increments by 1 | Write 5 tuples sequentially | Revision increases 1, 2, 3, 4, 5 |
| INT-021 | Read-your-writes via token | Write `(u:1, editor, repo:a)`, check with `atRevision: token` | `allow` |
| INT-022 | Token prevents stale read | Write `(u:1, editor, repo:a)`, get token T1; write `(u:1, viewer, repo:a)`; check with T1 | `deny` (old snapshot) |
| INT-023 | Fully consistent mode | Write, immediately check with `fully_consistent` | Latest state always visible |
| INT-024 | minimize_latency default | Check without specifying consistency | Returns result (may be slightly stale, but never older than current local revision) |

### 3.4 Schema & Migration

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| INT-030 | Schema validation — invalid relation | Schema references `team#nonexistent` | `AegisSchemaError` |
| INT-031 | Schema validation — circular type | Type A inherits from Type B which inherits from Type A | `AegisSchemaError` |
| INT-032 | Schema validation — orphan type | Type defined but never referenced | Warning (non-fatal) |
| INT-033 | Auto-migration on open | DB has schema v1, bundled schema is v2, `autoMigrate: true` | DB upgraded to v2 |
| INT-034 | Migration version tracking | After migration, `health().schemaVersion` | Returns correct version |
| INT-035 | Migration rollback | Apply v2, then rollback to v1 | Schema v1 active, all tuples preserved |
| INT-036 | Schema compatibility check | Call `checkSchema()` with compatible schema | `{ compatible: true }` |
| INT-037 | Schema compatibility check — breaking | Call `checkSchema()` with schema that removes a used relation | `{ compatible: false, breaking: [...] }` |

### 3.5 Cache Behavior

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| INT-040 | Cache hit on repeated check | Check same permission twice | Second call is faster (`durationMs` drops significantly) |
| INT-041 | Cache invalidation on write | Check permission, write affecting tuple, check again | Second check shows new result |
| INT-042 | Cache TTL expiry | Check, wait for TTL, check again | Cache miss, re-evaluated |
| INT-043 | Cache max size eviction | Fill cache beyond `maxEntries`, verify oldest evicted | Cache size <= maxEntries |
| INT-044 | Cache miss on different subject | Cache result for `(u:1, edit, repo:a)`, check `(u:2, edit, repo:a)` | Cache miss (different subject) |

### 3.6 Dry-Run Mode

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| INT-050 | Dry-run check | `check(..., { dryRun: true })` | Returns decision, revision unchanged |
| INT-051 | Dry-run write | `write(..., { dryRun: true })` | Returns validation result, nothing persisted |
| INT-052 | Dry-run write validation failure | Dry-run write with invalid data | Returns validation errors, nothing persisted |
| INT-053 | Dry-run does not affect cache | Dry-run write, then `check()` | Check result not affected by dry-run |

### 3.7 Deletion

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| INT-060 | Delete existing tuple | Write, delete, check | `deny` |
| INT-061 | Delete non-existent tuple | Delete tuple that doesn't exist | No-op, no revision bump |
| INT-062 | Delete one of many | Write 3 tuples for same subject, delete 1 | Only that tuple removed, others intact |
| INT-063 | Bulk delete subject | Write 5 tuples for `user:123`, call `deleteSubject("user:123")` | All 5 removed, revision bumped once |

### 3.8 Query & List

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| INT-070 | List by object | Write `(u:1, e, repo:a)`, `(u:2, v, repo:a)`, list `{ object: "repo:a" }` | Returns both tuples |
| INT-071 | List by subject | List `{ subject: "user:123" }` | Returns all tuples for that subject |
| INT-072 | List by relation | List `{ object: "repo:a", relation: "editor" }` | Only editor tuples |
| INT-073 | List with pagination | Write 150 tuples, list with `{ limit: 100 }` | Returns first 100, `nextCursor` present |
| INT-074 | Cursor pagination | Get first page, use `nextCursor` for second page | Second page has remaining 50 |

### 3.9 Watch / Subscription

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| INT-080 | Subscribe to object changes | Subscribe to `repo:a`, write tuple for `repo:a` | Event received with correct data |
| INT-081 | Subscribe with sinceRevision | Subscribe `sinceRevision: 5`, write 3 new tuples | Only events from rev≥5 received |
| INT-082 | Unsubscribe | Subscribe, unsubscribe, write | No event after unsubscribe |
| INT-083 | Multiple subscribers | 3 subscribers on same object | All receive the same events |
| INT-084 | Subscribe to wildcard | Subscribe to no specific object (all changes) | Events for all objects received |

### 3.10 Audit Log

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| INT-090 | Audit all mutations | Write 3 tuples, perform 2 deletes, query audit | All 5 events returned in order |
| INT-091 | Audit time range filter | Write at T1, T2, T3; query from T2 | Only T2 and T3 events returned |
| INT-092 | Audit object filter | Write for repo:a and repo:b; query by object "repo:a" | Only repo:a events |
| INT-093 | Audit entry structure | Single write, inspect audit entry | Contains revision, action, subject, relation, object, timestamp |

---

## 4. End-to-End Tests

### 4.1 Full SDK Lifecycle

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| E2E-001 | TypeScript full lifecycle | Init → write → check → explain → list → health → close | All succeed, clean shutdown |
| E2E-002 | Go full lifecycle | Same as E2E-001 via Go SDK | Identical results |
| E2E-003 | Rust full lifecycle | Same as E2E-001 via Rust SDK | Identical results |
| E2E-004 | Python full lifecycle | Same as E2E-001 via Python SDK | Identical results |

### 4.2 Multi-Language Interop

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| E2E-010 | Write in Go, read in Node | Go process writes tuple to shared DB, Node reads same DB | Node sees the write |
| E2E-011 | Write in Node, read in Python | Python reads a tuple written by Node | Same result |
| E2E-012 | Cross-language consistency | Write + read-your-writes token round-trip through different languages | Token valid across language runtimes |

### 4.3 Persistence & Recovery

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| E2E-020 | SQLite process restart | Init, write 10 tuples, close, re-init same DB | All 10 tuples present |
| E2E-021 | PostgreSQL process restart | Same as E2E-020 with PostgreSQL | All 10 tuples present |
| E2E-022 | Backup and restore | Create backup, delete graph, restore | Full graph state restored |
| E2E-023 | Export and import to new instance | Export graph as JSON, import to empty instance | New instance has identical state |

### 4.4 Event Log Recovery

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| E2E-030 | Recover from scratch | Delete all tuples, recover from event log | All tuples restored |
| E2E-031 | Point-in-time recovery | Write 10 tuples, record rev at 5, recover to rev 5 | Graph matches state at rev 5 |
| E2E-032 | Event log with compaction | Write 100 tuples (alternating adds/removes), compact, recover | Compaction reduces events without changing final state |

### 4.5 Middleware Integration

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| E2E-040 | Express middleware — allowed | Request with valid permission | 200 OK |
| E2E-041 | Express middleware — forbidden | Request without permission | 403 Forbidden |
| E2E-042 | Express middleware — missing auth | Request without user identity | 403 Forbidden |
| E2E-043 | Hono middleware | Same as E2E-040 but with Hono | Same behavior |

### 4.6 Deployment Modes

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| E2E-050 | Embedded single-process | Application with embedded Aegis, perform operations | All operations succeed in-process |
| E2E-051 | Multi-instance shared PostgreSQL | 2 app instances sharing same PostgreSQL backend | Both instances see same data |

---

## 5. Error Handling Tests

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| ERR-001 | Storage connection failure | Start Aegis with invalid DB path | `AegisStorageError` |
| ERR-002 | Schema validation failure | Incompatible schema on existing DB | `AegisSchemaError` |
| ERR-003 | Invalid subject format | Write with subject containing spaces | `AegisValidationError` |
| ERR-004 | Invalid relation name | Write with empty relation string | `AegisValidationError` |
| ERR-005 | Metadata exceeds limits | Write with 20 metadata keys | `AegisValidationError` |
| ERR-006 | Subject name too long | Write with 300-char subject | `AegisValidationError` |
| ERR-007 | Rate limited | Exceed write rate limit | `AegisRateLimitError` |
| ERR-008 | Fail-closed on storage error | Storage fails, attempt check | `deny` (fail-closed) |
| ERR-009 | Fail-open configuration | Storage fails with `failOpen: true`, attempt check | `allow` with warning (configurable) |
| ERR-010 | Revision token from unknown node | Pass token from node B to node A | `AegisConsistencyError` with suggestion |
| ERR-011 | Close and reuse | Call `close()`, then attempt check | `AegisStorageError` (not connected) |
| ERR-012 | Double initialize | Call `initialize()` twice | Second call is no-op or returns error |

---

## 6. Concurrency & Stress Tests

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| STR-001 | 100 concurrent reads | 100 goroutines/tasks reading simultaneously | All succeed, no contention errors |
| STR-002 | 50 concurrent writes | 50 tasks writing different tuples simultaneously | All succeed, revision increases by 50 |
| STR-003 | Mixed read/write | 20 writers + 80 readers simultaneously | No deadlocks, no corruption |
| STR-004 | Long-running read during write | Start long traversal, write during traversal | Write not blocked (WAL mode), read sees snapshot at start |
| STR-005 | Connection exhaustion | 100 reads exceed read pool of 4 | Reads queue up; some may block, none fail |
| STR-006 | Write queue depth | 100 simultaneous writes on single-writer connection | Writes serialize, all eventually succeed |
| STR-007 | Large graph stress | 100K subjects, 500K relationships, random checks | All checks complete; p50 < 2ms, p99 < 20ms |
| STR-008 | Deep hierarchy stress | 20-level nested resource hierarchy | Traversal completes within timeout |
| STR-009 | Many siblings stress | 1000 direct relationships on same object | Check traverses all, returns correct result |
| STR-010 | 8-hour soak test | Continuous writes + checks for 8 hours | Memory stable, no leaks, no errors |

---

## 7. Persistence & Recovery Tests

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| PER-001 | Crash recovery (SQLite) | Write, crash process (SIGKILL), restart | WAL auto-recovers, data intact |
| PER-002 | Crash during migration | Crash mid-migration, restart | Migration resumes or rolls back |
| PER-003 | WAL checkpoint on close | Write, call `close()`, inspect WAL file | WAL checkpointed, main DB file up to date |
| PER-004 | Disk full handling | Fill disk, attempt write | `AegisStorageError` with disk-full message |
| PER-005 | Recovery after disk freed | Clear space, retry write | Write succeeds |
| PER-006 | Database integrity check | `health().integrityStatus` after normal operations | `"ok"` |
| PER-007 | Database integrity check after crash | Crash-recover, check integrity | `"ok"` |

---

## 8. Multi-Tenancy Isolation Tests

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| TEN-001 | Same resource, different tenants | Write `repo:a` in tenant alpha and beta | Both exist independently |
| TEN-002 | Cross-tenant check denied | User from alpha checks resource in beta | `deny` |
| TEN-003 | Cross-tenant admin | Tenant admin attempts action in another tenant | `deny` |
| TEN-004 | Super-admin override | Super-admin with explicit policy accesses cross-tenant | `allow` (governed by policy) |
| TEN-005 | Tenant-scoped list | List objects in tenant alpha | Only alpha objects returned |
| TEN-006 | Tenant isolation under stress | 10 tenants, 100 concurrent operations each | No cross-tenant data leakage |

---

## 9. Security & Boundary Tests

| ID | Test | Steps | Expect |
|----|------|-------|--------|
| SEC-001 | SQL injection attempt | Subject: `"'; DROP TABLE; --"` | `AegisValidationError` (rejected by input validation) |
| SEC-002 | Extremely nested traversal | 100-level nesting cycle | Engine detects and breaks cycle, returns `deny` |
| SEC-003 | Max traversal depth exceeded | 35-level valid chain (depth limit is 32) | Engine returns `deny` with depth-exceeded trace |
| SEC-004 | Unbounded list | List with no filter/pagination on graph with 10K tuples | Returns first 1000 (page limit), pagination cursor present |
| SEC-005 | High-cardinality subject | One subject with 10K relationships | Operations perform within acceptable time |

---

## 10. SDK Cross-Language Tests

| ID | Language | Test | Expect |
|----|----------|------|--------|
| SDK-001 | TypeScript | NAPI binding loads, all APIs accessible | True |
| SDK-002 | TypeScript | Async operations with proper error types | Errors are typed `AegisError` |
| SDK-003 | Go | CGo binding initializes | True |
| SDK-004 | Go | Idiomatic `context.Context` support | Operations respect context cancellation |
| SDK-005 | Rust | Direct `use aegis` import compiles | True |
| SDK-006 | Rust | `Send + Sync` trait bounds satisfied | True |
| SDK-007 | Python | PyO3 binding loads | True |
| SDK-008 | Python | Async/await support | True |
| SDK-009 | All | Same fixture, same operation, same result | All SDKs return identical results |

---

## 11. Performance Benchmarks

### Baseline Targets (Single-Threaded, SQLite, 100K Tuples)

| Metric | Target | Test |
|--------|--------|------|
| `check()` latency p50 | < 1ms | BENCH-001 |
| `check()` latency p99 | < 10ms | BENCH-001 |
| `check()` latency p99.9 | < 50ms | BENCH-001 |
| `check()` throughput (single core) | > 10,000/sec | BENCH-002 |
| `write()` throughput (single core) | > 5,000/sec | BENCH-003 |
| `writeBatch(100)` latency | < 10ms | BENCH-004 |
| `list({ object })` latency p50 | < 1ms | BENCH-005 |
| `explain()` latency p50 | < 2ms | BENCH-006 |
| Memory per 1M tuples | < 512MB | BENCH-007 |
| Cache hit ratio (realistic workload) | > 80% | BENCH-008 |
| Cache miss penalty | < 5ms | BENCH-009 |
| 100 concurrent checks | All < 50ms | BENCH-010 |

### Benchmarks to Run

| ID | Scenario | Measurement | Acceptable Range |
|----|----------|-------------|-----------------|
| BENCH-001 | Random checks on 100K tuple graph | Latency distribution | p50 < 1ms, p99 < 10ms |
| BENCH-002 | Sequential checks on cached graph | Throughput | > 10K/sec |
| BENCH-003 | Sequential writes (different tuples) | Throughput | > 5K/sec |
| BENCH-004 | writeBatch(100) | Latency | < 10ms |
| BENCH-005 | list({ object }) on object with 500 relationships | Latency | < 1ms |
| BENCH-006 | explain() on 5-level deep hierarchy | Latency | < 2ms |
| BENCH-007 | Graph with 1M tuples | Memory usage | < 512MB |
| BENCH-008 | Mix of 80% checks, 20% writes | Cache hit ratio | > 80% |
| BENCH-009 | First check (cold cache) on 5-level hierarchy | Latency | < 5ms |
| BENCH-010 | 100 concurrent check goroutines | Max latency | All < 50ms |
| BENCH-011 | WAL file growth over 1M writes | WAL size | < 100MB (auto-checkpoint) |

---

## 12. Test Fixtures

### fixture-basic-team.yaml

```yaml
schema:
  schemaVersion: 1
  namespace: acme
  types:
    repo:
      relations:
        owner:
          inherit_from: [user, team#member]
        editor:
          inherit_from: [owner, collaborator]
        viewer:
          inherit_from: [editor, public]
      permissions:
        read:
          union_of: [viewer, editor, owner]
        write:
          union_of: [editor, owner]
        delete:
          union_of: [owner]

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
  - subject: "user:456"
    relation: "collaborator"
    object: "repo:fluxbus"
```

### fixture-deep-hierarchy.yaml

```yaml
schema:
  schemaVersion: 1
  namespace: acme
  types:
    org:
      relations:
        member:
          inherit_from: [user]
    workspace:
      relations:
        parent:
          inherit_from: [org, workspace]
        member:
          inherit_from: [user, team#member]
    repo:
      relations:
        parent:
          inherit_from: [workspace, repo]
        viewer:
          inherit_from: [parent#member]
      permissions:
        read:
          union_of: [viewer]

tuples:
  - subject: "user:1"  relation: "member" object: "org:root"
  - subject: "org:root" relation: "member" object: "org:a"
  - subject: "org:a" relation: "member" object: "org:b"
  - subject: "org:b" relation: "member" object: "workspace:1"
  - subject: "workspace:1" relation: "member" object: "workspace:2"
  - subject: "workspace:2" relation: "member" object: "workspace:3"
  - subject: "workspace:3" relation: "member" object: "workspace:4"
  - subject: "workspace:4" relation: "member" object: "repo:deep"
```

### fixture-multi-tenant.yaml

```yaml
schema:
  schemaVersion: 1
  namespace: acme
  types:
    tenant:
      relations:
        member:
          inherit_from: [user]
    workspace:
      relations:
        parent:
          inherit_from: [tenant]
        member:
          inherit_from: [tenant#member, user]

tuples:
  # Tenant alpha
  - subject: "user:alpha1" relation: "member" object: "tenant:alpha"
  - subject: "tenant:alpha" relation: "member" object: "workspace:core"
  # Tenant beta
  - subject: "user:beta1" relation: "member" object: "tenant:beta"
  - subject: "tenant:beta" relation: "member" object: "workspace:core"
```

### fixture-circular.yaml

```yaml
schema:
  schemaVersion: 1
  namespace: test
  types:
    node:
      relations:
        linked:
          inherit_from: [node]

tuples:
  - subject: "node:a" relation: "linked" object: "node:b"
  - subject: "node:b" relation: "linked" object: "node:c"
  - subject: "node:c" relation: "linked" object: "node:a"
```

### fixture-large-scale.yaml

```yaml
# Programmatically generated in test code
# Pattern:
#   - 10K users
#   - 100 teams
#   - 1K workspaces
#   - 100K repos
#   - user:n member team:{n % 100}
#   - team:{n} owner workspace:{n % 1000}
#   - workspace:{n} contains repo:{n % 100000}
#   - user:{n} editor repo:{n}
```

---

## Appendix: CI Pipeline Integration

```yaml
# .github/workflows/test.yml
jobs:
  unit:
    runs-on: ubuntu-latest
    steps:
      - run: npm test -- --testPathPattern=unit

  integration:
    runs-on: ubuntu-latest
    steps:
      - run: npm test -- --testPathPattern=integration

  e2e:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16-alpine
        env:
          POSTGRES_DB: aegis_test
        options: >-
          --health-cmd pg_isready
          --health-interval 10s
    steps:
      - run: npm test -- --testPathPattern=e2e

  stress:
    runs-on: ubuntu-latest
    if: github.ref == 'refs/heads/main'
    steps:
      - run: npm test -- --testPathPattern=stress
```

---

*Document version 1.0 — Test plan for Aegis v2.0 specification*
