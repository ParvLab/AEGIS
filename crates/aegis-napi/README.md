# @aegis-v/engine

Aegis embedded authorization engine for Node.js.

## Install

```bash
npm install @aegis-v/engine
```

## Usage

```javascript
const { initialize } = require('@aegis-v/engine');

const engine = initialize('aegis.db', `
namespace: app
types:
  repo:
    relations:
      owner: {}
      viewer: {}
    permissions:
      read: { union_of: [viewer, owner] }
`);

engine.write('user:alice', 'owner', 'repo:myapp');
const result = engine.check('user:alice', 'read', 'repo:myapp');
console.log(result.allowed); // true
```

## API

68+ methods covering check, explain (V1/V2), who-can-access, write, delete, batch, list, query, watch, subscribe, transaction (with savepoints), GDPR (exportSubject, deleteSubjectWithPolicy), audit (queryAudit, verifyAuditChain), schema management, cache, migration, multi-tenancy partitions, backup/restore, analysis (analysisReport, accessReview, accessDiff), policy lifecycle (draft→validate→approve→publish→archive), scheduler, and enforcement history. Full TypeScript declarations in `index.d.ts`.

*Note: Currently pre-compiled for Windows x64. For other platforms, compile from source.*
