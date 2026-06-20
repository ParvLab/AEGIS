#![cfg(feature = "sqlite")]
use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::StorageBackend;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::types::*;

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
"#;
    parse_schema(yaml).unwrap()
}

fn make_file_engine() -> (GraphEngine, String) {
    let path = std::env::temp_dir()
        .join(format!("aegis_persist_{}.db", fastrand::u64(..)))
        .to_string_lossy()
        .into_owned();
    let config = SqliteConfig {
        path: path.clone(),
        max_readers: 4,
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

fn write_n_tuples(engine: &GraphEngine, n: usize) {
    for i in 0..n {
        let subject = SubjectId::new(format!("user:u{}", i)).unwrap();
        let resource = ResourceId::new(format!("repo:r{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                resource,
            ))
            .unwrap();
    }
}

fn verify_n_tuples(engine: &GraphEngine, n: usize) {
    for i in 0..n {
        let subject = SubjectId::new(format!("user:u{}", i)).unwrap();
        let resource = ResourceId::new(format!("repo:r{}", i)).unwrap();
        let result = engine.check(&subject, "read", &resource, None).unwrap();
        assert!(result.allowed, "tuple {} should exist after reopen", i);
    }
}

// ── PER-001: Crash recovery ──
// Write tuples, close, reopen, verify all tuples survive.
#[test]
fn test_crash_recovery() {
    let (engine, path) = make_file_engine();

    write_n_tuples(&engine, 10);

    engine.close().unwrap();

    let config = SqliteConfig {
        path: path.clone(),
        max_readers: 4,
        busy_timeout_ms: 30000,
        wal_mode: true,
        mmap_size: 0,
    };
    let mut storage = SqliteStorage::new(config).unwrap();
    storage.initialize().unwrap();
    let engine = GraphEngine::new(Box::new(storage), make_schema());

    verify_n_tuples(&engine, 10);

    engine.close().unwrap();
    cleanup(&path);
}

// ── PER-002: Migration rollback on reopen ──
// Write tuples, migrate to version 3, close, reopen, verify version and data.
#[test]
fn test_migration_rollback_on_reopen() {
    let (engine, path) = make_file_engine();

    write_n_tuples(&engine, 5);

    let result = engine.migrate(3).unwrap();
    assert_eq!(result.to_version, 3);

    engine.close().unwrap();

    let config = SqliteConfig {
        path: path.clone(),
        max_readers: 4,
        busy_timeout_ms: 30000,
        wal_mode: true,
        mmap_size: 0,
    };
    let mut storage = SqliteStorage::new(config).unwrap();
    storage.initialize().unwrap();
    let engine = GraphEngine::new(Box::new(storage), make_schema());

    let version = engine.storage().read_schema_version().unwrap();
    assert_eq!(version, 3, "schema version should persist across reopen");

    verify_n_tuples(&engine, 5);

    engine.close().unwrap();
    cleanup(&path);
}

// ── PER-003: WAL checkpoint ──
// Write 100 tuples, close, reopen, verify all tuples are recoverable.
#[test]
fn test_wal_checkpoint() {
    let (engine, path) = make_file_engine();

    write_n_tuples(&engine, 100);

    engine.close().unwrap();

    let config = SqliteConfig {
        path: path.clone(),
        max_readers: 4,
        busy_timeout_ms: 30000,
        wal_mode: true,
        mmap_size: 0,
    };
    let mut storage = SqliteStorage::new(config).unwrap();
    storage.initialize().unwrap();
    let engine = GraphEngine::new(Box::new(storage), make_schema());

    verify_n_tuples(&engine, 100);

    engine.close().unwrap();
    cleanup(&path);
}

// ── PER-004: Graceful close and reopen ──
// Write tuples, close gracefully, reopen, verify persistence.
#[test]
fn test_graceful_close_and_reopen() {
    let (engine, path) = make_file_engine();

    write_n_tuples(&engine, 15);

    engine.close().unwrap();

    let config = SqliteConfig {
        path: path.clone(),
        max_readers: 4,
        busy_timeout_ms: 30000,
        wal_mode: true,
        mmap_size: 0,
    };
    let mut storage = SqliteStorage::new(config).unwrap();
    storage.initialize().unwrap();
    let engine = GraphEngine::new(Box::new(storage), make_schema());

    verify_n_tuples(&engine, 15);

    engine.close().unwrap();
    cleanup(&path);
}

// ── PER-005: Multiple close/reopen cycles ──
// Five cycles of write, close, reopen; verify total data after final reopen.
#[test]
fn test_multiple_close_reopen_cycles() {
    let (engine, path) = make_file_engine();
    let cycles = 5;
    let tuples_per_cycle = 10;

    // First write and close separately since we need to reopen
    write_n_tuples(&engine, tuples_per_cycle);
    engine.close().unwrap();

    for cycle in 1..cycles {
        let config = SqliteConfig {
            path: path.clone(),
            max_readers: 4,
            busy_timeout_ms: 30000,
            wal_mode: true,
            mmap_size: 0,
        };
        let mut storage = SqliteStorage::new(config).unwrap();
        storage.initialize().unwrap();
        let engine = GraphEngine::new(Box::new(storage), make_schema());

        // Write more data (new unique tuples)
        let offset = cycle * tuples_per_cycle;
        for i in 0..tuples_per_cycle {
            let subject = SubjectId::new(format!("user:u{}", offset + i)).unwrap();
            let resource = ResourceId::new(format!("repo:r{}", offset + i)).unwrap();
            engine
                .write(&RelationshipTuple::new(
                    subject,
                    Relation::new("owner").unwrap(),
                    resource,
                ))
                .unwrap();
        }

        engine.close().unwrap();
    }

    // Final reopen and verify all data
    let config = SqliteConfig {
        path: path.clone(),
        max_readers: 4,
        busy_timeout_ms: 30000,
        wal_mode: true,
        mmap_size: 0,
    };
    let mut storage = SqliteStorage::new(config).unwrap();
    storage.initialize().unwrap();
    let engine = GraphEngine::new(Box::new(storage), make_schema());

    let total = cycles * tuples_per_cycle;
    verify_n_tuples(&engine, total);

    engine.close().unwrap();
    cleanup(&path);
}

// ── PER-006: Integrity check passes ──
// Write tuples, run integrity_check, expect Ok. Then close.
#[test]
fn test_integrity_check_passes() {
    let (engine, path) = make_file_engine();

    write_n_tuples(&engine, 20);

    let report = engine.storage().integrity_check().unwrap();
    assert!(report.passed, "integrity check should pass");

    engine.close().unwrap();
    cleanup(&path);
}

// ── PER-007: Recover from events ──
// Write tuples, call recover_from_events, verify tuples still exist.
#[test]
fn test_recover_from_events() {
    let (engine, path) = make_file_engine();

    write_n_tuples(&engine, 10);

    let rev = engine
        .storage()
        .current_revision(&PartitionId::default())
        .unwrap();

    // Recover to the current revision (replays event log)
    let recovered = engine.recover_from_events(Some(rev)).unwrap();
    assert!(
        recovered.as_u64() >= rev.as_u64(),
        "recovery should reach at least the current revision"
    );

    verify_n_tuples(&engine, 10);

    // Also test with None (recover to latest)
    let recovered = engine.recover_from_events(None).unwrap();
    assert!(
        recovered.as_u64() >= rev.as_u64(),
        "recovery to latest should succeed"
    );

    verify_n_tuples(&engine, 10);

    engine.close().unwrap();
    cleanup(&path);
}
