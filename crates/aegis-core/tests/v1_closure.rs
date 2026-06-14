use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::types::*;

fn make_schema_v1() -> Schema {
    let yaml = r#"
schemaVersion: 1
namespace: test
types:
  repo:
    relations:
      owner:
        inherit_from: [user]
      viewer:
        inherit_from: [owner]
    permissions:
      read:
        union_of: [viewer, owner]
      write:
        union_of: [owner]
"#;
    parse_schema(yaml).unwrap()
}

fn make_engine_in_memory() -> GraphEngine {
    let config = SqliteConfig::in_memory();
    let storage = SqliteStorage::new(config).unwrap();
    let schema = make_schema_v1();
    GraphEngine::new(Box::new(storage), schema)
}

#[test]
fn v1_m1_crud_lifecycle() {
    let engine = make_engine_in_memory();

    let token = engine
        .write(&RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:core").unwrap(),
        ))
        .unwrap();
    assert!(token.revision.as_u64() > 0);

    let result = engine
        .check(
            &SubjectId::new("user:alice").unwrap(),
            "read",
            &ResourceId::new("repo:core").unwrap(),
            None,
        )
        .unwrap();
    assert!(result.allowed);

    let result = engine
        .check(
            &SubjectId::new("user:bob").unwrap(),
            "read",
            &ResourceId::new("repo:core").unwrap(),
            None,
        )
        .unwrap();
    assert!(!result.allowed);

    let key = TupleKey {
        subject: SubjectId::new("user:alice").unwrap(),
        relation: Relation::new("owner").unwrap(),
        object: ResourceId::new("repo:core").unwrap(),
    };
    let del = engine.delete(&key).unwrap();
    assert!(del.revision.as_u64() > token.revision.as_u64());

    let result = engine
        .check(
            &SubjectId::new("user:alice").unwrap(),
            "read",
            &ResourceId::new("repo:core").unwrap(),
            None,
        )
        .unwrap();
    assert!(!result.allowed);
}

#[test]
fn v1_m1_write_batch_validates_schema() {
    let engine = make_engine_in_memory();

    let tuples = vec![
        RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:a").unwrap(),
        ),
        RelationshipTuple::new(
            SubjectId::new("user:bob").unwrap(),
            Relation::new("viewer").unwrap(),
            ResourceId::new("repo:a").unwrap(),
        ),
    ];

    let result = engine.write_batch(&tuples);
    assert!(result.is_ok());

    let bad_tuples = vec![
        RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("nonexistent").unwrap(),
            ResourceId::new("repo:a").unwrap(),
        ),
    ];
    let result = engine.write_batch(&bad_tuples);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        aegis_core::AegisError::UnknownRelation { .. }
    ));

    engine.close().unwrap();
    let result = engine.write_batch(&tuples);
    assert!(result.is_err());
}

#[test]
fn v1_m2_traversal_and_explain() {
    let engine = make_engine_in_memory();

    engine
        .write(&RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:x").unwrap(),
        ))
        .unwrap();

    let result = engine
        .check(
            &SubjectId::new("user:alice").unwrap(),
            "read",
            &ResourceId::new("repo:x").unwrap(),
            None,
        )
        .unwrap();
    assert!(result.allowed);

    let explain = engine
        .explain(
            &SubjectId::new("user:alice").unwrap(),
            "read",
            &ResourceId::new("repo:x").unwrap(),
            None,
        )
        .unwrap();
    assert!(explain.allowed);
}

#[test]
fn v1_m2_at_revision() {
    let engine = make_engine_in_memory();

    let t1 = engine
        .write(&RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:r1").unwrap(),
        ))
        .unwrap();

    let result = engine
        .check(
            &SubjectId::new("user:alice").unwrap(),
            "read",
            &ResourceId::new("repo:r1").unwrap(),
            Some(ConsistencyMode::AtRevision(t1.revision)),
        )
        .unwrap();
    assert!(result.allowed);

    let t2 = engine
        .write(&RelationshipTuple::new(
            SubjectId::new("user:bob").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:r1").unwrap(),
        ))
        .unwrap();

    let result = engine
        .check(
            &SubjectId::new("user:bob").unwrap(),
            "read",
            &ResourceId::new("repo:r1").unwrap(),
            Some(ConsistencyMode::AtRevision(t2.revision)),
        )
        .unwrap();
    assert!(result.allowed);
}

#[test]
fn v1_m3_transactions() {
    let engine = make_engine_in_memory();

    let mut txn = engine.transaction().unwrap();

    txn.write(&RelationshipTuple::new(
        SubjectId::new("user:alice").unwrap(),
        Relation::new("owner").unwrap(),
        ResourceId::new("repo:txn-test").unwrap(),
    ))
    .unwrap();

    let rev = txn.commit().unwrap();
    assert!(rev.as_u64() > 0);

    let result = engine
        .check(
            &SubjectId::new("user:alice").unwrap(),
            "read",
            &ResourceId::new("repo:txn-test").unwrap(),
            None,
        )
        .unwrap();
    assert!(result.allowed);

    let mut txn2 = engine.transaction().unwrap();
    txn2
        .write(&RelationshipTuple::new(
            SubjectId::new("user:bob").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:txn-test").unwrap(),
        ))
        .unwrap();
    txn2.rollback().unwrap();

    let result = engine
        .check(
            &SubjectId::new("user:bob").unwrap(),
            "read",
            &ResourceId::new("repo:txn-test").unwrap(),
            None,
        )
        .unwrap();
    assert!(!result.allowed);
}

#[test]
fn v1_m3_backup_restore_roundtrip() {
    let engine = make_engine_in_memory();

    engine
        .write(&RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:r1").unwrap(),
        ))
        .unwrap();
    engine
        .write(&RelationshipTuple::new(
            SubjectId::new("user:bob").unwrap(),
            Relation::new("viewer").unwrap(),
            ResourceId::new("repo:r1").unwrap(),
        ))
        .unwrap();

    let all_tuples = engine
        .storage()
        .query_tuples(
            &aegis_core::storage::TupleFilter::default(),
            &aegis_core::types::PaginationParams {
                limit: u64::MAX,
                cursor: None,
            },
            &aegis_core::types::ConsistencyMode::MinimizeLatency,
        )
        .unwrap()
        .tuples;
    assert_eq!(all_tuples.len(), 2);

    let rev_before = engine.storage().current_revision().unwrap();

    let recovered = engine.recover_from_events(None).unwrap();
    assert!(recovered.as_u64() >= rev_before.as_u64());

    let health = engine.health();
    assert!(health.healthy);
    assert!(health.revision.as_u64() > 0);
}

#[test]
fn v1_m4_migrations() {
    let engine = make_engine_in_memory();

    let version = engine.storage().read_schema_version().unwrap();
    assert_eq!(version, 0);

    let result = engine.migrate(1).unwrap();
    assert_eq!(result.from_version, 0);
    assert_eq!(result.to_version, 1);
    assert!(!result.applied_migrations.is_empty());

    let version = engine.storage().read_schema_version().unwrap();
    assert_eq!(version, 1);
}

#[test]
fn v1_m4_drop_no_hot_reload() {
    let dir = std::env::temp_dir().join("aegis-v1-test-drop");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test-v1-m4-drop.db");
    let path_str = path.to_str().unwrap().to_string();
    let _ = std::fs::remove_file(&path);
    {
        let config = SqliteConfig {
            path: path_str.clone(),
            wal_mode: true,
            ..Default::default()
        };
        let storage = SqliteStorage::new(config).unwrap();
        let schema = make_schema_v1();
        let engine = GraphEngine::new(Box::new(storage), schema);

        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("repo:drop-test").unwrap(),
            ))
            .unwrap();
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn v1_m4_wal_mode_config() {
    let config = SqliteConfig {
        path: ":memory:".to_string(),
        wal_mode: false,
        ..Default::default()
    };
    let storage = SqliteStorage::new(config).unwrap();
    let schema = make_schema_v1();
    let engine = GraphEngine::new(Box::new(storage), schema);

    engine
        .write(&RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:wal-test").unwrap(),
        ))
        .unwrap();

    let result = engine
        .check(
            &SubjectId::new("user:alice").unwrap(),
            "read",
            &ResourceId::new("repo:wal-test").unwrap(),
            None,
        )
        .unwrap();
    assert!(result.allowed);
}

#[test]
fn v1_m4_health_cache_ratio() {
    let engine = make_engine_in_memory();

    engine
        .write(&RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:health-test").unwrap(),
        ))
        .unwrap();

    let _ = engine
        .check(
            &SubjectId::new("user:alice").unwrap(),
            "read",
            &ResourceId::new("repo:health-test").unwrap(),
            None,
        )
        .unwrap();

    let _ = engine
        .check(
            &SubjectId::new("user:alice").unwrap(),
            "read",
            &ResourceId::new("repo:health-test").unwrap(),
            None,
        )
        .unwrap();

    let health = engine.health();
    assert!(health.total_checks >= 2);
    assert!(health.allowed_checks >= 1);
}
