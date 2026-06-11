use criterion::{black_box, criterion_group, criterion_main, Criterion};

use aegis_core::engine::GraphEngine;
use aegis_core::schema::types::{PermissionDef, RelationDef, Schema, TypeDef};
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::StorageBackend;
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
                RelationDef { inherit_from: vec![], description: None },
            );
            relations.insert(
                "viewer".to_string(),
                RelationDef { inherit_from: vec![], description: None },
            );
            let mut permissions = HashMap::new();
            permissions.insert(
                "read".to_string(),
                PermissionDef {
                    union_of: vec!["viewer".to_string(), "owner".to_string()],
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
    let engine = GraphEngine::new(Box::new(storage), schema);

    // Seed tuples
    for i in 0..1000 {
        let subject = SubjectId::new(&format!("user:{}", i)).unwrap();
        let repo = ResourceId::new(&format!("repo:bench{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(subject, Relation::new("owner").unwrap(), repo))
            .unwrap();
    }
    engine
}

fn bench_check_direct(c: &mut Criterion) {
    let engine = setup_engine();
    let subject = SubjectId::new("user:0").unwrap();
    let resource = ResourceId::new("repo:bench0").unwrap();

    c.bench_function("check_direct", |b| {
        b.iter(|| {
            let result = engine.check(black_box(&subject), "read", black_box(&resource), None);
            black_box(result)
        })
    });
}

fn bench_check_dry_run(c: &mut Criterion) {
    let engine = setup_engine();
    let subject = SubjectId::new("user:0").unwrap();
    let resource = ResourceId::new("repo:bench0").unwrap();

    c.bench_function("check_dry_run", |b| {
        b.iter(|| {
            let result =
                engine.check_dry_run(black_box(&subject), "read", black_box(&resource), None);
            black_box(result)
        })
    });
}

fn bench_write(c: &mut Criterion) {
    let engine = setup_engine();
    let subject = SubjectId::new("user:write").unwrap();
    let resource = ResourceId::new("repo:write").unwrap();

    c.bench_function("write_tuple", |b| {
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

fn bench_query(c: &mut Criterion) {
    let engine = setup_engine();
    let filter = TupleFilter {
        subject_type: Some("user".to_string()),
        relation: None,
        object_type: None,
        metadata_key: None,
        metadata_value: None,
    };
    let pagination = PaginationParams::new(100, None);

    c.bench_function("query_tuples", |b| {
        b.iter(|| {
            let result = engine.query(black_box(&filter), black_box(&pagination), None);
            black_box(result)
        })
    });
}

fn bench_with_cache_ttl(c: &mut Criterion) {
    let engine = setup_engine().with_cache_ttl(std::time::Duration::from_secs(60));
    let subject = SubjectId::new("user:0").unwrap();
    let resource = ResourceId::new("repo:bench0").unwrap();

    // Warm the cache with one check
    let _ = engine.check(&subject, "read", &resource, None);

    c.bench_function("check_cached", |b| {
        b.iter(|| {
            let result = engine.check(black_box(&subject), "read", black_box(&resource), None);
            black_box(result)
        })
    });
}

criterion_group!(
    benches,
    bench_check_direct,
    bench_check_dry_run,
    bench_write,
    bench_query,
    bench_with_cache_ttl,
);
criterion_main!(benches);
