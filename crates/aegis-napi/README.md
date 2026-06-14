# @aegis-auth/engine

Aegis embedded authorization engine for Node.js.

## Install

```bash
npm install @aegis-auth/engine
```

## Usage

```javascript
const { Engine } = require('@aegis-auth/engine');

const engine = new Engine('aegis.db', `
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

30 methods covering check, write, delete, explain, list, batch, watch, transaction, GDPR, audit, schema management, rate limiting, and logging. Full TypeScript declarations in `index.d.ts`.
