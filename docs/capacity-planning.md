# Capacity Planning Guide

## Storage Sizing

### SQLite
- Each relationship tuple: ~200 bytes
- Each audit event: ~300 bytes
- 100K tuples ≈ 20 MB (data) + ~10 MB (indexes)
- WAL growth: ~1 MB per 1000 writes (checkpoint resets)
- Cache: 10,000 entries ≈ 50 MB peak

### Memory
- Engine base: ~10 MB
- Per 10K cached decisions: ~5 MB
- Traversal cache (1K entries): ~2 MB

### V6 Analysis Memory
- `analysis_report()`: full scan loads all tuples + events into memory
  - 100K tuples + 100K events ≈ 8-10 MB temporary
  - Consider running during off-peak for large datasets
- `who_can_access()`: builds subject set from graph traversal
  - Additional ~2-5 MB for path tracking when `include_paths: true`
- `access_diff()`: holds two schemas + diff result set in memory
  - Minimal (~200 KB per schema)

### V7 Enforcement History (opt-in)
- Each enforcement event: ~150 bytes (subject, permission, resource, timestamp, partition)
- At `DeniedOnly` with 5% denial rate, 50K checks/sec:
  - 2,500 events/sec → ~215M/day → ~64 GB/day raw
  - With `SamplingRate(0.1)`: 250 events/sec → ~21M/day → ~3 GB/day
- Retention: `MaxDays` or `MaxRows` policy cleans up automatically
- Estimate footprint before enabling; use sampling rate to control

### V7 Policy Draft Storage
- Each draft: schema JSON + metadata ≈ 2-10 KB
- Negligible storage impact (< 1 MB even for hundreds of drafts)

## Performance
- check(): 0.1-0.5ms (warm cache, <100K tuples)
- check(): 1-10ms (cold, <1M tuples)
- write(): 1-5ms
- write_batch(100): 50-200ms
- analysis_report(): scales linearly with tuple count
  - 100K tuples: ~500ms-2s
  - 1M tuples: ~5-20s
