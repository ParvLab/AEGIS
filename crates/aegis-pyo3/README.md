# aegis-auth

Aegis embedded authorization engine for Python.

## Install

```bash
pip install aegis-auth
```

## Usage

```python
from aegis import Aegis

engine = Aegis("aegis.db", """
namespace: app
types:
  repo:
    relations:
      owner: {}
      viewer: {}
    permissions:
      read: { union_of: [viewer, owner] }
""")

engine.write("user:alice", "owner", "repo:myapp")
result = engine.check("user:alice", "read", "repo:myapp")
print(result.allowed)  # True
```

## API

27 methods covering check, write, delete, explain, list, batch, watch, transaction, GDPR, audit, schema management, rate limiting, and logging.
