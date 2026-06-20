# @aegis-v/browser

Aegis authorization engine compiled to WASM for browser environments.
Check permissions, write/delete relations, and export/import data — all offline.

## Install

```bash
npm install @aegis-v/browser
```

## Quick Example

```typescript
import init, * as wasm from '@aegis-v/browser/pkg/aegis_browser.js';

const schema = JSON.stringify({
  namespaces: [{ name: "doc", relations: { owner: {}, writer: { union: ["owner"] }, reader: { union: ["writer"] } } }]
});

await init();
wasm.init_sync(schema, true);
wasm.write_relation("user:alice", "reader", "doc:report");
const allowed = wasm.check("user:alice", "read", "doc:report"); // true
```

## Features

- **Offline-first**: All data persists in IndexedDB. Service worker precaches WASM.
- **ReBAC-native**: Supports ReBAC, RBAC, ACL, ABAC from the same engine.
- **Web Worker**: Offload check/write to a dedicated thread (non-blocking UI).
- **Small bundle**: ~155 KB gzipped.

## API

| Function | Description |
|----------|-------------|
| `init_sync(schema, inMemory)` | Initialize sync engine (InMemoryStorage) |
| `init_async(schema)` | Initialize async engine (IndexedDbStorage) |
| `check(subject, permission, resource)` | Check permission → `boolean` |
| `write_relation(subject, relation, resource)` | Add tuple → revision string |
| `delete_relation(subject, relation, resource)` | Remove tuple → revision string |
| `list_by_object(resource)` | List tuples for object → JSON string |
| `list_by_subject(subject)` | List tuples for subject → JSON string |
| `export_json()` | Export all tuples → JSON string |
| `import_json(json)` | Import tuples from JSON string |

## Docs

- [Architecture](docs/architecture.md)
- [Getting Started Guide](docs/guide.md)
- [Unsupported Features](docs/unsupported.md)
- [Runtime Contract](docs/runtime.md)

## License

MIT
