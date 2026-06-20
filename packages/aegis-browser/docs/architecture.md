# WASM Architecture

## Crate Layout

```
packages/aegis-browser/
  rust/                          # Rust crate compiled to WASM
    Cargo.toml
    src/lib.rs                   # #[wasm_bindgen] exports
    pkg/                         # wasm-pack output
  src/                           # TypeScript SDK
    types.ts                     # Public type definitions
    index.ts                     # AegisEngine class (main entry)
    worker.ts                    # Web Worker bridge
  demo/                          # Offline demo app
  docs/                          # Documentation
  package.json                   # npm package (@aegis-v/browser)
  tsconfig.json
```

## Storage Backends

Two storage modes expose the GraphEngine to browser JavaScript:

### Synchronous (InMemoryStorage)

Uses `aegis_core::storage::InMemoryStorage` — all data lives in WASM linear memory.
Fast but non-persistent. Useful for ephemeral checks, testing, and server-side rendering.

Exported via `init_sync(schemaJson)` — call from the main thread or worker.

### Asynchronous (IndexedDbStorage)

Uses `aegis_core::storage::async_traits::InMemoryAsyncStorage` wrapping
`aegis_core::storage::indexeddb::IndexedDbStorage` — data persisted to the browser's
IndexedDB via web-sys bindings.

Exported via `init_async(schemaJson)` — call in an async context.

## IndexedDB Store Layout

| Store       | Key            | Value             |
|-------------|----------------|-------------------|
| `tuples`    | `subject:relation:object` | RelationshipTuple JSON |
| `events`    | `revision`     | AuditEvent JSON   |
| `revision`  | `"counter"`    | `u64` revision    |
| `schema`    | `"schema"`     | Schema JSON       |
| `metadata`  | `"version"`    | `u32` version     |

## Web Worker Bridge

The Web Worker (`worker.ts`) loads the WASM module in a dedicated thread.
Messages flow via `postMessage`:

```
Main Thread                     Worker
    |                             |
    |--- { type, id, ... } ------>| (deserialize)
    |                             | call WASM function
    |<-- { type, id, result } ---| (serialize)
    |                             |
```

The `AegisEngine` class in `index.ts` supports both modes:
- `useWorker: true` (default) — routes calls through the worker
- `useWorker: false` — calls WASM directly from the main thread

## Bundle Size Budget

| Artifact           | Raw      | Gzip     | Target     |
|--------------------|----------|----------|------------|
| `aegis_browser_bg.wasm` | 383 KB | 155 KB | < 350 KB gzip |

## Feature Flags

- `wasm` — enables wasm32-specific deps (getrandom/js, web-sys, wasm-bindgen)
- `async-storage` — enables AsyncStorageBackend trait + IndexedDbStorage
- `sqlite` — NOT available on WASM target
- `postgres` — NOT available on WASM target
- `mysql` — NOT available on WASM target
- `rocksdb` — NOT available on WASM target
