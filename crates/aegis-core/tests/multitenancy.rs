#![cfg(feature = "sqlite")]
use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::StorageBackend;
use aegis_core::storage::TupleFilter;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::types::*;
use std::sync::Arc;

fn make_engine_for_tenant(tenant: &str) -> GraphEngine {
    let yaml = format!(
        r#"
types:
  {}_repo:
    relations:
      owner: {{}}
    permissions:
      read:
        union_of: [owner]
"#,
        tenant
    );
    let schema = parse_schema(&yaml).unwrap();
    let config = SqliteConfig {
        path: ":memory:".to_string(),
        max_readers: 4,
        busy_timeout_ms: 30000,
        wal_mode: false,
        mmap_size: 0,
    };
    let mut storage = SqliteStorage::new(config).unwrap();
    storage.initialize().unwrap();
    GraphEngine::new(Box::new(storage), schema)
}

fn make_file_engine_for_tenant(tenant: &str) -> (GraphEngine, String) {
    let yaml = format!(
        r#"
types:
  {}_repo:
    relations:
      owner: {{}}
    permissions:
      read:
        union_of: [owner]
"#,
        tenant
    );
    let schema = parse_schema(&yaml).unwrap();
    let path = std::env::temp_dir()
        .join(format!("aegis_mt_{}.db", fastrand::u64(..)))
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
    (GraphEngine::new(Box::new(storage), schema), path)
}

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}-wal", path));
    let _ = std::fs::remove_file(format!("{}-shm", path));
}

// ── TEN-001: Tenant isolation write/read ──
#[test]
fn test_tenant_isolation_write_read() {
    let engine_a = make_engine_for_tenant("tenant_a");
    let engine_b = make_engine_for_tenant("tenant_b");

    let subject = SubjectId::new("user:alice").unwrap();
    let resource = ResourceId::new("tenant_a_repo:myrepo").unwrap();

    engine_a
        .write(&RelationshipTuple::new(
            subject.clone(),
            Relation::new("owner").unwrap(),
            resource.clone(),
        ))
        .unwrap();

    let result = engine_a.check(&subject, "read", &resource, None).unwrap();
    assert!(result.allowed, "tenant_a should read its own tuple");

    let result = engine_b.check(&subject, "read", &resource, None).unwrap();
    assert!(!result.allowed, "tenant_b should not read tenant_a's tuple");

    engine_a.close().unwrap();
    engine_b.close().unwrap();
}

// ── TEN-002: Tenant data not leaking ──
#[test]
fn test_tenant_data_not_leaking() {
    let engine_a = make_engine_for_tenant("tenant_a");
    let engine_b = make_engine_for_tenant("tenant_b");

    for i in 0..5 {
        let subject = SubjectId::new(format!("user:a{}", i)).unwrap();
        let resource = ResourceId::new(format!("tenant_a_repo:repo{}", i)).unwrap();
        engine_a
            .write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                resource,
            ))
            .unwrap();
    }
    for i in 0..5 {
        let subject = SubjectId::new(format!("user:b{}", i)).unwrap();
        let resource = ResourceId::new(format!("tenant_b_repo:repo{}", i)).unwrap();
        engine_b
            .write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                resource,
            ))
            .unwrap();
    }

    let result_a = engine_a
        .query(
            &TupleFilter::default(),
            &PaginationParams::new(100, None),
            None,
        )
        .unwrap();
    assert_eq!(
        result_a.tuples.len(),
        5,
        "tenant_a should have exactly 5 tuples"
    );

    let result_b = engine_b
        .query(
            &TupleFilter::default(),
            &PaginationParams::new(100, None),
            None,
        )
        .unwrap();
    assert_eq!(
        result_b.tuples.len(),
        5,
        "tenant_b should have exactly 5 tuples"
    );

    engine_a.close().unwrap();
    engine_b.close().unwrap();
}

// ── TEN-003: Tenant admin cannot access other tenant's data ──
#[test]
fn test_tenant_admin_cannot_access_other_tenant() {
    let engine_a = make_engine_for_tenant("tenant_a");
    let engine_b = make_engine_for_tenant("tenant_b");

    let admin_a = SubjectId::new("user:admin_tenant_a").unwrap();
    let admin_b = SubjectId::new("user:admin_tenant_b").unwrap();
    let resource_a = ResourceId::new("tenant_a_repo:secret").unwrap();
    let resource_b = ResourceId::new("tenant_b_repo:secret").unwrap();

    engine_a
        .write(&RelationshipTuple::new(
            admin_a.clone(),
            Relation::new("owner").unwrap(),
            resource_a.clone(),
        ))
        .unwrap();
    engine_b
        .write(&RelationshipTuple::new(
            admin_b.clone(),
            Relation::new("owner").unwrap(),
            resource_b.clone(),
        ))
        .unwrap();

    let result = engine_a.check(&admin_a, "read", &resource_a, None).unwrap();
    assert!(result.allowed, "admin_tenant_a should access tenant_a data");

    let result = engine_b.check(&admin_a, "read", &resource_b, None).unwrap();
    assert!(
        !result.allowed,
        "admin_tenant_a should not access tenant_b data"
    );

    engine_a.close().unwrap();
    engine_b.close().unwrap();
}

// ── TEN-004: Super admin override ──
#[test]
fn test_super_admin_override() {
    let yaml = r#"
types:
  tenant_a_repo:
    relations:
      owner: {}
    permissions:
      read:
        union_of: [owner]
  tenant_b_repo:
    relations:
      owner: {}
    permissions:
      read:
        union_of: [owner]
"#;
    let schema = parse_schema(yaml).unwrap();
    let config = SqliteConfig {
        path: ":memory:".to_string(),
        max_readers: 4,
        busy_timeout_ms: 30000,
        wal_mode: false,
        mmap_size: 0,
    };
    let mut storage = SqliteStorage::new(config).unwrap();
    storage.initialize().unwrap();
    let engine = GraphEngine::new(Box::new(storage), schema);

    let super_admin = SubjectId::new("super:admin").unwrap();
    let resource_a = ResourceId::new("tenant_a_repo:project_x").unwrap();
    let resource_b = ResourceId::new("tenant_b_repo:project_y").unwrap();

    let user_a = SubjectId::new("user:alice").unwrap();
    let user_b = SubjectId::new("user:bob").unwrap();

    engine
        .write(&RelationshipTuple::new(
            user_a.clone(),
            Relation::new("owner").unwrap(),
            resource_a.clone(),
        ))
        .unwrap();
    engine
        .write(&RelationshipTuple::new(
            user_b.clone(),
            Relation::new("owner").unwrap(),
            resource_b.clone(),
        ))
        .unwrap();
    engine
        .write(&RelationshipTuple::new(
            super_admin.clone(),
            Relation::new("owner").unwrap(),
            resource_a.clone(),
        ))
        .unwrap();
    engine
        .write(&RelationshipTuple::new(
            super_admin.clone(),
            Relation::new("owner").unwrap(),
            resource_b.clone(),
        ))
        .unwrap();

    let result = engine
        .check(&super_admin, "read", &resource_a, None)
        .unwrap();
    assert!(result.allowed, "super:admin should read tenant_a resource");

    let result = engine
        .check(&super_admin, "read", &resource_b, None)
        .unwrap();
    assert!(
        result.allowed,
        "super:admin should read tenant_b resource (cross-tenant)"
    );

    engine.close().unwrap();
}

// ── TEN-005: Namespace isolation via object_type filter ──
#[test]
fn test_namespace_isolation() {
    let yaml = r#"
types:
  repo:
    relations:
      owner: {}
    permissions:
      read:
        union_of: [owner]
  doc:
    relations:
      owner: {}
    permissions:
      read:
        union_of: [owner]
"#;
    let schema = parse_schema(yaml).unwrap();
    let config = SqliteConfig {
        path: ":memory:".to_string(),
        max_readers: 4,
        busy_timeout_ms: 30000,
        wal_mode: false,
        mmap_size: 0,
    };
    let mut storage = SqliteStorage::new(config).unwrap();
    storage.initialize().unwrap();
    let engine = GraphEngine::new(Box::new(storage), schema);

    for i in 0..5 {
        let subject = SubjectId::new(format!("user:{}", i)).unwrap();
        let resource = ResourceId::new(format!("repo:r{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                resource,
            ))
            .unwrap();
    }

    for i in 0..5 {
        let subject = SubjectId::new(format!("user:{}", i + 10)).unwrap();
        let resource = ResourceId::new(format!("doc:d{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                resource,
            ))
            .unwrap();
    }

    let filter = TupleFilter {
        object_type: Some("repo".to_string()),
        ..TupleFilter::default()
    };
    let result = engine
        .query(&filter, &PaginationParams::new(100, None), None)
        .unwrap();
    assert_eq!(result.tuples.len(), 5, "should get exactly 5 repo tuples");
    for t in &result.tuples {
        assert!(
            t.object.as_str().starts_with("repo:"),
            "all returned tuples should be repo type, got: {}",
            t.object.as_str()
        );
    }

    let filter = TupleFilter {
        object_type: Some("doc".to_string()),
        ..TupleFilter::default()
    };
    let result = engine
        .query(&filter, &PaginationParams::new(100, None), None)
        .unwrap();
    assert_eq!(result.tuples.len(), 5, "should get exactly 5 doc tuples");
    for t in &result.tuples {
        assert!(
            t.object.as_str().starts_with("doc:"),
            "all returned tuples should be doc type, got: {}",
            t.object.as_str()
        );
    }

    engine.close().unwrap();
}

// ── TEN-006: Concurrent tenant operations ──
#[test]
fn test_concurrent_tenant_operations() {
    let (engine_a, path_a) = make_file_engine_for_tenant("tenant_a");
    let (engine_b, path_b) = make_file_engine_for_tenant("tenant_b");
    let engine_a = Arc::new(engine_a);
    let engine_b = Arc::new(engine_b);

    let handle_a = {
        let engine = Arc::clone(&engine_a);
        std::thread::spawn(move || {
            for i in 0..20 {
                let subject = SubjectId::new(format!("user:a{}", i)).unwrap();
                let resource = ResourceId::new(format!("tenant_a_repo:r{}", i)).unwrap();
                engine
                    .write(&RelationshipTuple::new(
                        subject,
                        Relation::new("owner").unwrap(),
                        resource,
                    ))
                    .unwrap();
            }
        })
    };

    let handle_b = {
        let engine = Arc::clone(&engine_b);
        std::thread::spawn(move || {
            for i in 0..20 {
                let subject = SubjectId::new(format!("user:b{}", i)).unwrap();
                let resource = ResourceId::new(format!("tenant_b_repo:r{}", i)).unwrap();
                engine
                    .write(&RelationshipTuple::new(
                        subject,
                        Relation::new("owner").unwrap(),
                        resource,
                    ))
                    .unwrap();
            }
        })
    };

    handle_a.join().unwrap();
    handle_b.join().unwrap();

    let result = engine_a
        .query(
            &TupleFilter::default(),
            &PaginationParams::new(100, None),
            None,
        )
        .unwrap();
    assert_eq!(result.tuples.len(), 20, "tenant_a should have 20 tuples");

    let result = engine_b
        .query(
            &TupleFilter::default(),
            &PaginationParams::new(100, None),
            None,
        )
        .unwrap();
    assert_eq!(result.tuples.len(), 20, "tenant_b should have 20 tuples");

    engine_a.close().unwrap();
    engine_b.close().unwrap();
    cleanup(&path_a);
    cleanup(&path_b);
}
