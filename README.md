# Aegis

Embedded authorization engine. Single-process, zero servers, ReBAC-native.

## SDKs

| Language | Package | Directory |
|----------|---------|-----------|
| Node.js | `@aegis-auth/engine` | `crates/aegis-napi/` |
| Python | `aegis-auth` | `crates/aegis-pyo3/` |
| C | header `aegis_ffi.h` | `crates/aegis-ffi/` |
| Go | `aegis-go` | `crates/aegis-go/` |

## Quick Start

```bash
# Node.js
npm install @aegis-auth/engine

# Python
pip install aegis-auth

# Go
go get github.com/anomalyco/aegis/crates/aegis-go
```

See `examples/` for full working examples in each language.

## Documentation

- [Migration Guide: V1 → V3](docs/migration-v1-to-v3.md)
- [Implementation Plan](AEGIS_IMPLEMENTATION_PLAN.md)
- [Technical Specification](aegis-spec.md)
- [Test Plan](aegis-test-plan.md)

## License

MIT
