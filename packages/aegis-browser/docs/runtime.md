# Runtime Contract

## Initialization

- Call `init()` (the WASM default export) once before any other WASM function.
- Call `init_sync(schemaJson, inMemory)` or `init_async(schemaJson)` once to create the engine.
- Multiple `init_*` calls overwrite the previous engine (all state is lost).

## Thread Safety

- WASM runs on a single thread.
- In Web Worker mode, each Worker loads its own WASM instance with its own engine state.
- **Worker isolation**: The main thread is never blocked during WASM operations when `useWorker: true`. All WASM execution happens in the Worker's dedicated thread. Verified by the worker isolation test at `examples/worker-isolation-test/`.
- Cross-tab sharing requires a `SharedWorker` (not yet implemented — V6).

## IndexedDB Persistence

- `init_async` reads the schema from IndexedDB if present and compares with the provided schema.
- Writes (`write_relation`, `delete_relation`) are immediately committed to IndexedDB.
- Read-after-write consistency is guaranteed via an in-memory revision counter.
- Browser storage quota applies (typically 50 MB–1 GB depending on browser).

## Cache Behavior

- A single LRU cache (default 10,000 entries) lives in WASM linear memory.
- Cache is NOT persisted across page reloads (in-memory only).
- Cache is invalidated on schema change.

## Revision Tokens

- Format: `{revision}:{nodeId}` (e.g., `42:550e8400-e29b-41d4-a716-446655440000`)
- Every write/delete returns a revision token.
- Revision tokens are NOT persisted across sessions — they are consistent within a single page lifetime.

## Error Handling

- WASM functions return `Result<T, JsValue>`.
- In TypeScript, errors are thrown as `Error` objects with descriptive messages.
- Common error types:
  - `ValidationError` — invalid subject/relation/resource format
  - `SchemaError` — schema parse failure or missing type
  - `StorageError` — IndexedDB transaction failure
  - `PermissionDenied` — not applicable (check returns `false`, not an error)

## Memory

| Metric            | Target     |
|-------------------|------------|
| Idle              | < 50 MB    |
| Steady-state      | < 200 MB   |
| WASM binary load  | < 400 KB raw |

## Support Matrix

| Browser     | Status |
|-------------|--------|
| Chrome      | ✅     |
| Firefox     | ✅     |
| Safari      | ✅     |
| Edge        | ✅     |
| Web Worker  | ✅     |
| Deno        | ❌ V6  |
| Bun         | ❌ V6  |
| Cloudflare  | ❌ V6  |
