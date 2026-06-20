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
    engine
}

fn bench_write_latency(c: &mut Criterion) {
    let engine = setup_engine();
    let subject = SubjectId::new("user:latency").unwrap();
    let resource = ResourceId::new("repo:latency").unwrap();

    c.bench_function("write_latency", |b| {
        b.iter(|| {
            let tuple = RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            );
            let result = engine.write(black_box(&tuple));
            black_box(result)
        })
    });
}

fn bench_write_batch_100(c: &mut Criterion) {
    let engine = setup_engine();

    c.bench_function("write_batch_100", |b| {
        b.iter(|| {
            let tuples: Vec<RelationshipTuple> = (0..100)
                .map(|i| {
                    RelationshipTuple::new(
                        SubjectId::new(format!("user:bch{}", i)).unwrap(),
                        Relation::new("owner").unwrap(),
                        ResourceId::new(format!("repo:bch{}", i)).unwrap(),
                    )
                })
                .collect();
            let result = engine.write_batch(black_box(&tuples));
            black_box(result)
        })
    });
}

criterion_group!(benches, bench_write_latency, bench_write_batch_100);
criterion_main!(benches);
