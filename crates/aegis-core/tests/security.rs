#![cfg(feature = "sqlite")]
use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::{StorageBackend, TupleFilter};
use aegis_core::types::*;
use std::time::Instant;

fn make_schema() -> Schema {
    let yaml = r#"
schemaVersion: 2
namespace: test
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

#[test]
fn test_pagination_large_dataset() {
    let engine = make_engine();

    for i in 0..10_000 {
        let subject = SubjectId::new(format!("user:page{}", i)).unwrap();
        let resource = ResourceId::new(format!("repo:page{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                resource,
            ))
            .unwrap();
    }

    let filter = TupleFilter::default();
    let pagination = PaginationParams::new(1000, None);
    let result = engine.query(&filter, &pagination, None).unwrap();
    assert!(result.tuples.len() <= 1000);
    assert!(result.next_cursor.is_some());

    let mut cursor = result.next_cursor;
    for _ in 0..10 {
        if let Some(c) = cursor {
            let pagination = PaginationParams::new(1000, Some(c));
            let result = engine.query(&filter, &pagination, None).unwrap();
            assert!(result.tuples.len() <= 1000);
            cursor = result.next_cursor;
        } else {
            break;
        }
    }
}

#[test]
fn test_subject_with_many_relationships() {
    let engine = make_engine();
    let subject = SubjectId::new("user:power").unwrap();

    for i in 0..1000 {
        let resource = ResourceId::new(format!("repo:rel{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource,
            ))
            .unwrap();
    }

    let resource = ResourceId::new("repo:rel0").unwrap();
    let start = Instant::now();
    let result = engine.check(&subject, "read", &resource, None).unwrap();
    let elapsed = start.elapsed();

    assert!(result.allowed);
    assert!(elapsed.as_secs() < 5, "check took too long: {:?}", elapsed);
}

#[test]
fn test_sql_injection_in_identifiers() {
    let err = SubjectId::new("user:'; DROP TABLE;--").unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidCharacters(_)),
        "SQL injection pattern should be rejected by SubjectId validation"
    );

    let err = ResourceId::new("repo:'; DROP TABLE;--").unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidCharacters(_)),
        "SQL injection pattern should be rejected by ResourceId validation"
    );
}
