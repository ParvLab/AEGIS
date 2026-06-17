# Changelog v6.0.0 — Authorization Intelligence

## Core Engine
- `explain_v2()` — Multi-step access explanation with depth-annotated trace, cache hit indicator, and full path resolution
- `who_can_access()` — Enumerate subjects that have a given permission on a resource, with pagination and optional path inclusion
- `access_diff()` — Semantic diff between two policy schemas: reports added/removed access relationships with human-readable summary
- `analysis_report()` — Full integrity analysis across all categories (tenant leakage, orphaned tuples, high-access subjects)
- `simulate_changes()` — What-if simulation of policy changes without affecting production state
- `reachable_subjects()` — Graph traversal from a subject to discover all reachable resources
- `find_orphaned_tuples()` — Detect relationship tuples that reference deleted or non-existent subjects/resources
- `find_high_access_subjects()` — Discover subjects with unusually broad access across resources
- `tenant_leakage_detection()` — Cross-partition traversal detection for multi-tenant isolation verification
- `list_policy_versions()` — Query policy version history with metadata
- `rollback_policy()` — Safe rollback to any previous policy version

## Schema & Analysis Types
- `AnalysisFinding` — Structured finding with severity, category, and contextual detail
- `IntegrityReport` — Extended with `tenant_leakage_detected`, `leaked_crossings`, `orphaned_tuple_count`
- `AccessDiffReport` — Full diff with added/removed access entries and summary

## Bindings (All Languages)

### C FFI
- `aegis_engine_explain_v2()`
- `aegis_engine_who_can_access()`
- `aegis_engine_access_diff()`
- `aegis_engine_list_policy_versions()`
- `aegis_engine_rollback_policy()`

### Node NAPI
- `explainV2()`, `whoCanAccess()`, `accessDiff()`, `listPolicyVersions()`, `rollbackPolicy()`

### Python PyO3
- All V6 analysis methods exposed via Python bindings

### Browser WASM
- All V6 analysis methods exposed via WASM exports + TypeScript SDK

## IndexedDB
- `verify_audit_chain()` now computes and validates SHA-256 hash chain for tamper-evident audit log
- Events stored with `previous_hash` and `event_hash` cryptographic fields

## Breaking Changes
- `explain()` is superseded by `explain_v2()`. The old `explain()` API still works but is deprecated.

## Migration Guide
- Replace `engine.explain(...)` calls with `engine.explain_v2(...)`
- The new `explain_v2()` returns enriched trace data including depth and cache hit info
- No storage schema migration required — existing data remains compatible
