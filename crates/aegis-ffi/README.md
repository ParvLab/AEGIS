# Aegis C FFI

C bindings for the Aegis embedded authorization engine.

## Usage

```c
#include "aegis_ffi.h"

AegisEngine* engine = aegis_engine_create("aegis.db", schema_yaml);

AegisCheckResult res = aegis_engine_check(engine, "user:alice", "read", "repo:myapp");

aegis_engine_destroy(engine);
```

## Building

Link against `libaegis_ffi` (shared library). The header declares 27 C functions plus watch and transaction opaque handles.

## Logging

```c
void my_logger(int level, const char* target, const char* msg, void* user_data) {
    printf("[%d] %s: %s\n", level, target, msg);
}
aegis_engine_set_logger(engine, my_logger, NULL);
```

## Rate Limiting

```c
aegis_engine_set_rate_limiter(engine, "{\"max_traversal_depth\": 20}");
```
