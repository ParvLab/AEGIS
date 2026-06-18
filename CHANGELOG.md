# Changelog

## v0.1.0 (unreleased)

### v6 — Intelligence Layer
- Core authorization engine with Check, Explain, Write, Delete
- Schema with types, relations, permissions
- Subject-set resolution and role hierarchy
- Partition-based multi-tenancy
- Decision caching and traversal caching
- Audit log with SHA-256 hash chain integrity verification
- GDPR export and right-to-erasure support
- Rate limiting with token bucket algorithm
- Hot-reload schema support
- NAPI Node.js bindings (@aegis-auth/engine)
- Python PyO3 bindings (aegis-auth)
- C FFI bindings for cross-language SDKs
- WebAssembly browser bindings (@aegis/browser)
- SQLite, PostgreSQL, MySQL, RocksDB, InMemory storage backends
- IndexedDB storage for browser environments
- Go language bindings via CGo
- Watch/subscribe event stream
- Policy versioning and rollback
- Access diff and dry-run check/write
- Who-can-access reverse search
- CLI with full command set and interactive REPL
- Comprehensive test suite (350+ tests)

### v7 — Operational Intelligence
- **Policy Lifecycle (M4):** Draft-create-validate-submit-approve-reject-publish-archive workflow
- **Event Stream API (M3):** Extended watch events with payload and subscribe() convenience
- **Scheduled Analysis (M1):** Cron-based recurring analysis with run tracking
- **Enforcement History (M2):** Opt-in sampling with rate-limited event recording and trends

### Storage
- Persistent storage for policy drafts, analysis schedules, runs, and enforcement events
- Storage migration framework (engine/migration.rs)
- SQLite (default), PostgreSQL, MySQL, RocksDB, InMemory, IndexedDB backends
