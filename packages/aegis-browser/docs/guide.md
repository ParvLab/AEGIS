# Browser Getting-Started Guide

## Install

```bash
npm install @aegis-v/browser
```

Or use directly via CDN/ESM import from the WASM package.

## Quick Start

```typescript
import init, * as wasm from '@aegis-v/browser/pkg/aegis_browser.js';
import { AegisEngine } from '@aegis-v/browser';

// 1. Initialize WASM runtime
await init();

// 2. Create engine
const schema = JSON.stringify({
  namespaces: [{
    name: "doc",
    relations: {
      owner: {},
      writer: { union: ["owner"] },
      reader: { union: ["writer"] }
    }
  }]
});
wasm.init_sync(schema, true);

// 3. Write a relation
wasm.write_relation("user:alice", "reader", "doc:report");

// 4. Check permission
const allowed = wasm.check("user:alice", "read", "doc:report");
console.log(allowed); // true

// 5. List by object
const tuples = JSON.parse(wasm.list_by_object("doc:report"));

// 6. Export / Import
const json = wasm.export_json();
wasm.import_json(json);
```

## Using the TypeScript SDK

```typescript
import { AegisEngine } from '@aegis-v/browser';

const engine = new AegisEngine({ useWorker: true });
await engine.init(schema);
await engine.write("user:alice", "reader", "doc:report");
const allowed = await engine.check("user:alice", "read", "doc:report");
const tuples = await engine.listByObject("doc:report");
const json = await engine.exportToJson();
engine.destroy();
```

## Worker vs Direct Mode

| Mode | Use Case | Thread |
|------|----------|--------|
| `useWorker: true` | Production / offline PWA | Web Worker (non-blocking) |
| `useWorker: false` | Testing / simple pages | Main thread |

## Offline-First Setup

1. Register the service worker:
```javascript
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.register('/sw.js');
}
```

2. The service worker precaches the WASM binary and JS glue code.
3. On subsequent visits, the app works entirely offline (IndexedDB persists data).

## Schema Migration on Browser

Schema changes follow the same rules as the server:
- Adding new types, relations, or permissions is compatible (auto-migration).
- Removing or renaming existing relations/permissions requires a breaking migration.
- Migration runs on `init_sync`/`init_async` if the stored schema version differs.
