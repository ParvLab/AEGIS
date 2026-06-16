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

### Performance
- check(): 0.1-0.5ms (warm cache, <100K tuples)
- check(): 1-10ms (cold, <1M tuples)
- write(): 1-5ms
- write_batch(100): 50-200ms
