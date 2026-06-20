use criterion::{Criterion, black_box, criterion_group, criterion_main};

use aegis_core::engine::GraphEngine;
use aegis_core::storage::StorageBackend;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::types::schema::{PermissionDef, RelationDef, Schema, TypeDef};
use aegis_core::types::*;
use std::collections::HashMap;

fn setup_engine() -> GraphEngine {
    let schema = Schema {
        schema_version: 1,
        namespace: "bench".to_string(),
        types: {
            let mut types = HashMap::new();
            let mut relations = HashMap::new();
            relations.insert(
                "owner".to_string(),
                RelationDef {
                    inherit_from: vec![],
                    description: None,
                },
            );
            relations.insert(
                "viewer".to_string(),
                RelationDef {
                    inherit_from: vec![],
                    description: None,
                },
            );
            let mut permissions = HashMap::new();
            permissions.insert(
                "read".to_string(),
                PermissionDef {
                    union_of: vec!["viewer".to_string(), "owner".to_string()],
                    ..Default::default()
                },
            );
            types.insert(
                "repo".to_string(),
                TypeDef {
                    relations,
                    permissions,
                    ..Default::default()
                },
            );
            types
        },
    };
    let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
    storage.initialize().unwrap();
    let engine = GraphEngine::new(Box::new(storage), schema);

    for i in 0..1000 {
        let subject = SubjectId::new(format!("user:{}", i)).unwrap();
        let repo = ResourceId::new(format!("repo:bench{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                repo,
            ))
            .unwrap();
    }

    // Hierarchy: subjects owned by other subjects (chain of viewer relations)
    for i in 1..500 {
        let owner = SubjectId::new(format!("user:{}", i - 1)).unwrap();
        let member = ResourceId::new(format!("user:{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                owner,
                Relation::new("viewer").unwrap(),
                member,
            ))
            .unwrap();
    }

    engine
}

fn bench_explain_direct(c: &mut Criterion) {
    let engine = setup_engine();
    let subject = SubjectId::new("user:0").unwrap();
    let resource = ResourceId::new("repo:bench0").unwrap();

    c.bench_function("explain_direct", |b| {
        b.iter(|| {
            let result = engine.explain(black_box(&subject), "read", black_box(&resource), None);
            black_box(result)
        })
    });
}

fn bench_explain_missing(c: &mut Criterion) {
    let engine = setup_engine();
    let subject = SubjectId::new("user:9999").unwrap();
    let resource = ResourceId::new("repo:nonexistent").unwrap();

    c.bench_function("explain_missing", |b| {
        b.iter(|| {
            let result = engine.explain(black_box(&subject), "read", black_box(&resource), None);
            black_box(result)
        })
    });
}

criterion_group!(benches, bench_explain_direct, bench_explain_missing);
criterion_main!(benches);
