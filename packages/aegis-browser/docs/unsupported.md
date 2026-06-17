# Unsupported Features

## Storage Backends Not Available in Browser

| Backend   | Reason                               |
|-----------|--------------------------------------|
| RocksDB   | Native C++ dependency, not WASM      |
| SQLite    | Requires `sqlite-wasm-rs` (separate) |
| PostgreSQL| Needs TCP sockets, not WASM          |
| MySQL     | Needs TCP sockets, not WASM          |

Only `InMemoryStorage` and `IndexedDbStorage` are available in the browser SDK.

## Features Not Available

| Feature               | Reason                                         |
|-----------------------|-------------------------------------------------|
| File-based export     | No filesystem in browser                       |
| OS-IO / threads       | WASM is single-threaded (Web Workers are separate instances) |
| Tokio async runtime   | WASM has no multi-threaded executor            |
| OpenTelemetry tracing | Requires gRPC export (not WASM-compatible)     |
| Hot-reload watcher    | No filesystem watch support                    |
| CLI / REPL            | Node.js binary, not browser                    |
| Postgres LISTEN/NOTIFY| TCP sockets not available                      |

## Behavioral Differences

| Behavior              | Native (SQLite)          | Browser (IndexedDB)      |
|-----------------------|--------------------------|--------------------------|
| Transaction atomicity | Full ACID                | Single-object-store atomicity |
| Max storage           | Disk-bound (GB+)         | Browser quota (typically 50 MB-1 GB) |
| Concurrent readers    | Multiple (WAL mode)      | Single-threaded (SharedWorker for cross-tab) |
| Revision token format | `revision:uuid`          | Same format              |
| Integrity check       | Full WAL + page check    | Event replay consistency |
