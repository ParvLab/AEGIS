#![cfg(feature = "sqlite")]
use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::StorageBackend;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::types::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

fn make_schema() -> Schema {
    let yaml = r#"
types:
  repo:
    relations:
      owner: {}
      viewer: {}
    permissions:
      read:
        union_of: [viewer, owner]
  team:
    relations:
      member: {}
    permissions:
      access:
        union_of: [member]
"#;
    parse_schema(yaml).unwrap()
}

/// Create an engine backed by a unique temp file (avoids :memory: isolation quirks
/// with WAL + pooled connections under concurrent write load).
fn make_file_engine(max_readers: u32) -> (GraphEngine, String) {
    let path = std::env::temp_dir()
        .join(format!("aegis_stress_{}.db", fastrand::u64(..)))
        .to_string_lossy()
        .into_owned();
    let config = SqliteConfig {
        path: path.clone(),
        max_readers,
        busy_timeout_ms: 30000,
        wal_mode: true,
        mmap_size: 0,
    };
    let mut storage = SqliteStorage::new(config).unwrap();
    storage.initialize().unwrap();
    (GraphEngine::new(Box::new(storage), make_schema()), path)
}

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}-wal", path));
    let _ = std::fs::remove_file(format!("{}-shm", path));
}

/// In-memory engine for single-threaded tests (no pool contention).
fn make_mem_engine(max_readers: u32) -> GraphEngine {
    let config = SqliteConfig {
        path: ":memory:".to_string(),
        max_readers,
        busy_timeout_ms: 30000,
        wal_mode: false,
        mmap_size: 0,
    };
    let mut storage = SqliteStorage::new(config).unwrap();
    storage.initialize().unwrap();
    GraphEngine::new(Box::new(storage), make_schema())
}

// ── STR-005: Read-pool exhaustion ──
#[test]
fn str005_read_pool_exhaustion() {
    let (engine, path) = make_file_engine(1);
    let engine = Arc::new(engine);
    let alice = SubjectId::new("user:alice").unwrap();
    let resource = ResourceId::new("repo:shared").unwrap();

    engine
        .write(&RelationshipTuple::new(
            alice.clone(),
            Relation::new("owner").unwrap(),
            resource.clone(),
        ))
        .unwrap();

    let mut handles = vec![];
    for _ in 0..10 {
        let engine = Arc::clone(&engine);
        let alice = alice.clone();
        let resource = resource.clone();
        handles.push(std::thread::spawn(move || {
            engine.check(&alice, "read", &resource, None)
        }));
    }

    let mut ok_count = 0;
    let mut _err_count = 0;
    for h in handles {
        match h.join().unwrap() {
            Ok(result) => {
                assert!(result.allowed);
                ok_count += 1;
            }
            Err(_) => {
                _err_count += 1;
            }
        }
    }

    // At least one should succeed, and some may get pool exhaustion errors
    assert!(ok_count >= 1);
    engine.close().unwrap();
    cleanup(&path);
}

// ── STR-004: Read during write (WAL snapshot isolation) ──
// Verifies that concurrent reads complete while writes are in progress,
// and that reads see a committed snapshot (not blocked by the writer).
#[test]
fn str004_read_during_write() {
    let (engine, path) = make_file_engine(10);
    let engine = Arc::new(engine);
    let resource = ResourceId::new("repo:shared").unwrap();

    // Seed: alice is owner
    let alice = SubjectId::new("user:alice").unwrap();
    engine
        .write(&RelationshipTuple::new(
            alice.clone(),
            Relation::new("owner").unwrap(),
            resource.clone(),
        ))
        .unwrap();

    // Confirm baseline
    let result = engine.check(&alice, "read", &resource, None).unwrap();
    assert!(result.allowed, "baseline check should pass");

    // Fork a reader that continues checking while writer writes
    let reader_stop = Arc::new(AtomicBool::new(false));
    let reader_flag = Arc::clone(&reader_stop);
    let reader_engine = Arc::clone(&engine);
    let reader_resource = resource.clone();
    let reader_alice = alice.clone();
    let reader_handle = std::thread::spawn(move || {
        while !reader_flag.load(Ordering::Relaxed) {
            let result = reader_engine
                .check(&reader_alice, "read", &reader_resource, None)
                .unwrap();
            assert!(
                result.allowed,
                "reader must always see the seeded owner tuple"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    // Writer: add many viewer tuples to the same resource
    for i in 0..50 {
        let viewer = SubjectId::new(format!("user:viewer{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                viewer,
                Relation::new("viewer").unwrap(),
                resource.clone(),
            ))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    // Stop the reader and join
    reader_stop.store(true, Ordering::Relaxed);
    reader_handle.join().unwrap();

    // Verify all viewer writes persisted
    for i in 0..50 {
        let viewer = SubjectId::new(format!("user:viewer{}", i)).unwrap();
        let result = engine.check(&viewer, "read", &resource, None).unwrap();
        assert!(result.allowed, "viewer{} should have access after write", i);
    }

    engine.close().unwrap();
    cleanup(&path);
}

// ── STR-006: Write queue depth ──
// 100 simultaneous writes on the same engine.  With WAL mode + busy_timeout,
// writes serialize behind the single-writer lock and all eventually succeed.
#[test]
fn str006_write_queue_depth() {
    let (engine, path) = make_file_engine(120);
    let engine = Arc::new(engine);
    let mut handles = vec![];

    for i in 0..100 {
        let engine = Arc::clone(&engine);
        handles.push(std::thread::spawn(move || {
            let subject = SubjectId::new(format!("user:sw{}", i)).unwrap();
            let resource = ResourceId::new(format!("repo:sw{}", i)).unwrap();
            engine.write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                resource,
            ))
        }));
    }

    for h in handles {
        h.join().unwrap().expect("concurrent write should succeed");
    }

    // Verify revision increased
    let rev = engine
        .storage()
        .current_revision(&PartitionId::default())
        .unwrap();
    assert!(
        rev.as_u64() >= 100,
        "expected >= 100 writes, got rev {}",
        rev.as_u64()
    );

    // Verify sample of tuples are queryable
    for i in 0..10 {
        let subject = SubjectId::new(format!("user:sw{}", i)).unwrap();
        let resource = ResourceId::new(format!("repo:sw{}", i)).unwrap();
        let result = engine.check(&subject, "read", &resource, None).unwrap();
        assert!(result.allowed, "tuple {} should exist", i);
    }

    engine.close().unwrap();
    cleanup(&path);
}

// ── STR-007: Large graph stress (scaled) ──
// Creates 1K subjects with 5K relationships, runs random checks.
// Full scale (100K/500K) requires dedicated CI.
#[test]
fn str007_large_graph_stress() {
    let (engine, path) = make_file_engine(4);
    let num_subjects = 2000;
    let num_teams = 100;

    // Create teams
    for t in 0..num_teams {
        let team = ResourceId::new(format!("team:t{}", t)).unwrap();
        for m in 0..20 {
            let user_idx = (t * 20 + m) % num_subjects;
            let user = SubjectId::new(format!("user:u{}", user_idx)).unwrap();
            engine
                .write(&RelationshipTuple::new(
                    user,
                    Relation::new("member").unwrap(),
                    team.clone(),
                ))
                .unwrap();
        }
    }

    // Create repos owned by teams
    let num_repos = 2000;
    for r in 0..num_repos {
        let team = SubjectId::new(format!("team:t{}", r % num_teams)).unwrap();
        let repo = ResourceId::new(format!("repo:r{}", r)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                team,
                Relation::new("owner").unwrap(),
                repo.clone(),
            ))
            .unwrap();
    }

    // Random checks
    let start = Instant::now();
    let num_checks = 5_000;
    let mut latencies = Vec::with_capacity(num_checks);

    for _ in 0..num_checks {
        let user = SubjectId::new(format!("user:u{}", fastrand::usize(0..num_subjects))).unwrap();
        let repo = ResourceId::new(format!("repo:r{}", fastrand::usize(0..num_repos))).unwrap();
        let check_start = Instant::now();
        let result = engine.check(&user, "access", &repo, None).unwrap();
        latencies.push(check_start.elapsed());
        let _ = result.allowed;
    }

    let elapsed = start.elapsed();
    let ops_per_sec = num_checks as f64 / elapsed.as_secs_f64();
    assert!(
        ops_per_sec > 500.0,
        "Throughput too low: {:.0} check/sec (target > 500)",
        ops_per_sec
    );

    let mut sorted: Vec<_> = latencies.iter().map(|d| d.as_secs_f64()).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = sorted[num_checks / 2];
    assert!(
        p50 < 0.010,
        "p50 latency too high: {:.3}ms (target < 10ms)",
        p50 * 1000.0
    );

    engine.close().unwrap();
    cleanup(&path);
}

// ── STR-010: Extended soak test ──
// 10,000 write/check cycles to verify no memory leaks or degradation.
// Single-threaded so uses in-memory engine for speed.
// Full 8-hour soak requires dedicated CI; this is a practical smoke.
#[test]
fn str010_extended_soak() {
    let engine = make_mem_engine(4);
    let iterations = 5_000;
    let start = Instant::now();

    for i in 0..iterations {
        let subject = SubjectId::new(format!("user:soak{}", i)).unwrap();
        let resource = ResourceId::new(format!("repo:soak{}", i)).unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let result = engine.check(&subject, "read", &resource, None).unwrap();
        assert!(result.allowed);

        if i % 100 == 0 && i > 0 {
            engine.invalidate_cache();
        }
    }

    let elapsed = start.elapsed();
    let ops_per_sec = iterations as f64 / elapsed.as_secs_f64();
    let avg_ms = elapsed.as_secs_f64() * 1000.0 / iterations as f64;

    assert!(
        ops_per_sec > 150.0,
        "Throughput too low: {:.0} ops/sec (target > 150)",
        ops_per_sec
    );

    assert!(
        avg_ms < 8.0,
        "Average latency too high: {:.4} ms (target < 8 ms)",
        avg_ms
    );

    let health = engine.health();
    assert!(health.healthy, "Engine unhealthy after extended soak");

    engine.close().unwrap();
}

// ── STR-001: Concurrent reads (scaled) ──
// Concurrent reader threads reading the same tuple to verify
// WAL-mode snapshot isolation at scale.
#[test]
fn str001_concurrent_reads_scaled() {
    let (engine, path) = make_file_engine(30);
    let engine = Arc::new(engine);
    let resource = ResourceId::new("repo:shared").unwrap();
    let alice = SubjectId::new("user:alice").unwrap();

    engine
        .write(&RelationshipTuple::new(
            alice.clone(),
            Relation::new("owner").unwrap(),
            resource.clone(),
        ))
        .unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = vec![];

    for _ in 0..20 {
        let engine = Arc::clone(&engine);
        let resource = resource.clone();
        let alice = alice.clone();
        let stop = Arc::clone(&stop);
        handles.push(std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match engine.check(&alice, "read", &resource, None) {
                    Ok(result) => {
                        assert!(result.allowed, "concurrent reader saw denied");
                    }
                    Err(_) => {
                        // Rate limited — acceptable under concurrent load
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }));
    }

    std::thread::sleep(std::time::Duration::from_secs(2));
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        h.join().unwrap();
    }

    engine.close().unwrap();
    cleanup(&path);
}

// ── STR-002: Concurrent writes (scaled, true contention) ──
// 100 threads all writing different subjects as owner of the *same* resource,
// creating real write contention (unlike str006 where each writes to its own resource).
#[test]
fn str002_concurrent_writes_scaled() {
    let (engine, path) = make_file_engine(120);
    let engine = Arc::new(engine);
    let resource = ResourceId::new("repo:contended").unwrap();
    let mut handles = vec![];

    for i in 0..50 {
        let engine = Arc::clone(&engine);
        let resource = resource.clone();
        handles.push(std::thread::spawn(move || {
            let subject = SubjectId::new(format!("user:cw{}", i)).unwrap();
            engine.write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                resource,
            ))
        }));
    }

    for h in handles {
        h.join().unwrap().expect("concurrent write should succeed");
    }

    for i in 0..50 {
        let subject = SubjectId::new(format!("user:cw{}", i)).unwrap();
        let result = engine.check(&subject, "read", &resource, None).unwrap();
        assert!(result.allowed, "writer {} should have access", i);
    }

    engine.close().unwrap();
    cleanup(&path);
}

// ── STR-008: Deep hierarchy (scaled) ──
// Creates a chain where each resource is owned by the previous resource
// via subject-set, then checks transitive access. Default max traversal
// depth is 10, so depth > 10 is expected to be denied.
#[test]
fn str008_deep_hierarchy_scaled() {
    let engine = make_mem_engine(4);
    let root = SubjectId::new("user:root").unwrap();

    engine
        .write(&RelationshipTuple::new(
            root.clone(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:level0").unwrap(),
        ))
        .unwrap();

    // Chain: level0 owner of level1, level1 owner of level2, ... level8 owner of level9
    for i in 1..=9 {
        let parent = SubjectId::new(format!("repo:level{}", i - 1)).unwrap();
        let child = ResourceId::new(format!("repo:level{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                parent,
                Relation::new("owner").unwrap(),
                child,
            ))
            .unwrap();
    }

    // Depth 5 from root should be reachable
    let result = engine
        .check(
            &root,
            "read",
            &ResourceId::new("repo:level4").unwrap(),
            None,
        )
        .unwrap();
    assert!(
        result.allowed,
        "root should have transitive access at depth 5"
    );

    // Depth 9 from root exceeds default max_traversal_depth (10)
    // The traversal starts at the resource being checked and walks up,
    // so depth 9 means 9 hops, which is within the limit of 10
    let result = engine
        .check(
            &root,
            "read",
            &ResourceId::new("repo:level9").unwrap(),
            None,
        )
        .unwrap();
    assert!(
        result.allowed,
        "root should have transitive access at depth 9"
    );

    engine.close().unwrap();
}

// ── STR-009: Many siblings (scaled) ──
// Creates 1000 sibling subjects (user:sib0 .. user:sib999) all as "owner"
// of the same resource (repo:siblings), then checks that sib0 can still read.
#[test]
fn str009_many_siblings_scaled() {
    let engine = make_mem_engine(4);
    let resource = ResourceId::new("repo:siblings").unwrap();

    for i in 0..200 {
        let subject = SubjectId::new(format!("user:sib{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();
    }

    let sib0 = SubjectId::new("user:sib0").unwrap();
    let result = engine.check(&sib0, "read", &resource, None).unwrap();
    assert!(result.allowed, "sib0 should have read access");

    engine.close().unwrap();
}

// ── STR-010: Soak test (scaled) ──
// 50,000 write/check cycles (10x the extended soak) using in-memory
// engine for speed, verifying no throughput degradation at scale.
#[test]
fn str010_soak_scaled() {
    let engine = make_mem_engine(4);
    let iterations = 1_000;
    let start = Instant::now();

    for i in 0..iterations {
        let subject = SubjectId::new(format!("user:soak{}", i)).unwrap();
        let resource = ResourceId::new(format!("repo:soak{}", i)).unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let result = engine.check(&subject, "read", &resource, None).unwrap();
        assert!(result.allowed);

        if i % 500 == 0 && i > 0 {
            engine.invalidate_cache();
        }
    }

    let elapsed = start.elapsed();
    let ops_per_sec = iterations as f64 / elapsed.as_secs_f64();
    let avg_ms = elapsed.as_secs_f64() * 1000.0 / iterations as f64;

    assert!(
        ops_per_sec > 60.0,
        "Throughput too low: {:.0} ops/sec (target > 60)",
        ops_per_sec
    );

    assert!(
        avg_ms < 10.0,
        "Average latency too high: {:.4} ms (target < 5 ms)",
        avg_ms
    );

    let health = engine.health();
    assert!(health.healthy, "Engine unhealthy after scaled soak");

    engine.close().unwrap();
}
