# aegis-go

Go bindings for the Aegis embedded authorization engine.

## Install

```bash
go get github.com/aegis-auth/aegis-go
```

Requires `libaegis_ffi` shared library on the library path.

## Usage

```go
package main

import (
    "fmt"
    "github.com/aegis-auth/aegis-go"
)

func main() {
    engine, err := aegis.NewEngine("aegis.db", schemaYAML)
    if err != nil { panic(err) }
    defer engine.Destroy()

    engine.Write("user:alice", "owner", "repo:myapp")
    result, _ := engine.Check("user:alice", "read", "repo:myapp")
    fmt.Println(result.Allowed) // true
}
```

## API

27 methods plus `NewEngineWithConfig`, `Watch`, `Transaction`, `SetRateLimiter`, and `SetLogger`.
