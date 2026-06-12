use criterion::{black_box, criterion_group, criterion_main, Criterion};

use aegis_core::engine::GraphEngine;
use aegis_core::types::schema::{PermissionDef, RelationDef, Schema, TypeDef};
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::StorageBackend;
use aegis_core::types::*;
use std::collections::HashMap;

fn setup_engine_with_cache(capacity: usize) -> GraphEngine {
    let schema = Schema {
        schema_version: 1,
        namespace: "bench".to_string(),
        types: {
            let mut types = HashMap::new();
            let mut relations = HashMap::new();
            relations.insert(
                "owner".to_string(),
                RelationDef { inherit_from: vec![], description: None },
            );
            let mut permissions = HashMap::new();
            permissions.insert(
                "read".to_string(),
                PermissionDef {
                    union_of: vec!["owner".to_string()],
                    condition: None,
                    description: None,
                },
            );
            types.insert("repo".to_string(), TypeDef { relations, permissions });
            types
        },
    };
    let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
    storage.initialize().unwrap();
    let engine = GraphEngine::new(Box::new(storage), schema).with_cache_capacity(capacity);

    for i in 0..500 {
        let subject = SubjectId::new(&format!("user:{}", i)).unwrap();
        let repo = ResourceId::new(&format!("repo:bench{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(subject, Relation::new("owner").unwrap(), repo))
            .unwrap();
    }
    engine
}

fn bench_cache_lru_zipfian(c: &mut Criterion) {
    let engine = setup_engine_with_cache(100);

    c.bench_function("cache_lru_zipfian", |b| {
        b.iter(|| {
            let i = fastrand::usize(0..500);
            let hot = i < 100;
            let idx = if hot { fastrand::usize(0..100) } else { fastrand::usize(100..500) };
            let subject = SubjectId::new(&format!("user:{}", idx)).unwrap();
            let repo = ResourceId::new(&format!("repo:bench{}", idx)).unwrap();
            let result = engine.check(black_box(&subject), "read", black_box(&repo), None);
            black_box(result)
        })
    });
}

criterion_group!(benches, bench_cache_lru_zipfian);
criterion_main!(benches);
