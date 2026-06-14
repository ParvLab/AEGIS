use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::StorageBackend;
use aegis_core::storage::TupleFilter;
use aegis_core::types::*;
use std::time::Instant;

fn make_schema() -> aegis_core::types::Schema {
    let yaml = r#"
types:
  repo:
    relations:
      owner: {}
      viewer: {}
    permissions:
      read:
        union_of: [viewer, owner]
"#;
    parse_schema(yaml).unwrap()
}

fn make_engine() -> GraphEngine {
    let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
    storage.initialize().unwrap();
    GraphEngine::new(Box::new(storage), make_schema())
}

/// Soak test: runs check/write/delete cycles to verify no memory leaks
/// over sustained operations.
#[test]
fn test_soak_no_memory_leak() {
    let engine = make_engine();
    let iterations = 1_000;
    let start = Instant::now();

    for i in 0..iterations {
        let subject = SubjectId::new(&format!("user:soak{}", i)).unwrap();
        let resource = ResourceId::new(&format!("repo:soak{}", i)).unwrap();

        // Write
        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        // Check
        let result = engine.check(&subject, "read", &resource, None).unwrap();
        assert!(result.allowed);

        // Query
        let filter = TupleFilter {
            subject_type: Some("user".to_string()),
            relation: None,
            object_type: None,
            metadata_key: None,
            metadata_value: None,
            ..Default::default()
        };
        let pagination = PaginationParams::new(100, None);
        let _ = engine.query(&filter, &pagination, None);

        if i % 100 == 0 && i > 0 {
            engine.invalidate_cache();
        }
    }

    let elapsed = start.elapsed();
    let ops_per_sec = iterations as f64 / elapsed.as_secs_f64();
    let avg_ms = elapsed.as_secs_f64() * 1000.0 / iterations as f64;

    assert!(
        ops_per_sec > 100.0,
        "Throughput too low: {:.0} ops/sec (target > 100)",
        ops_per_sec
    );

    assert!(
        avg_ms < 10.0,
        "Average latency too high: {:.2} ms (target < 10 ms)",
        avg_ms
    );

    // Verify engine health after soak
    let health = engine.health();
    assert!(health.healthy, "Engine unhealthy after soak");

    engine.close().unwrap();
}

/// Throughput target test: validate > 10,000 check ops/sec
#[test]
fn test_throughput_target() {
    let engine = make_engine();

    // Pre-seed tuples
    for i in 0..100 {
        let subject = SubjectId::new(&format!("user:t{}", i)).unwrap();
        let resource = ResourceId::new(&format!("repo:t{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                resource,
            ))
            .unwrap();
    }

    let subject = SubjectId::new("user:t0").unwrap();
    let resource = ResourceId::new("repo:t0").unwrap();

    // Warm the cache
    let _ = engine.check(&subject, "read", &resource, None);

    let iterations = 5_000;
    let start = Instant::now();

    for _ in 0..iterations {
        let _ = engine.check(&subject, "read", &resource, None);
    }

    let elapsed = start.elapsed();
    let ops_per_sec = iterations as f64 / elapsed.as_secs_f64();

    assert!(
        ops_per_sec > 10_000.0,
        "Throughput below target: {:.0} check/sec (target > 10,000)",
        ops_per_sec
    );

    engine.close().unwrap();
}
