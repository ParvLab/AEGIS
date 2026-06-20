use criterion::{Criterion, black_box, criterion_group, criterion_main};

use aegis_core::engine::GraphEngine;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::{StorageBackend, TupleFilter};
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
        let repo = ResourceId::new(format!("repo:list{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                repo,
            ))
            .unwrap();
    }

    let resource = ResourceId::new("repo:populated").unwrap();
    for i in 0..100 {
        let subject = SubjectId::new(format!("user:pop{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject,
                Relation::new("viewer").unwrap(),
                resource.clone(),
            ))
            .unwrap();
    }

    let subject = SubjectId::new("user:manyrels").unwrap();
    for i in 0..50 {
        let repo = ResourceId::new(format!("repo:many{}", i)).unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                repo,
            ))
            .unwrap();
    }
    engine
}

fn bench_list_by_object(c: &mut Criterion) {
    let engine = setup_engine();
    let resource = ResourceId::new("repo:populated").unwrap();

    c.bench_function("list_by_object", |b| {
        b.iter(|| {
            let result = engine.list_by_object(black_box(&resource), None, None);
            black_box(result)
        })
    });
}

fn bench_list_by_subject(c: &mut Criterion) {
    let engine = setup_engine();
    let subject = SubjectId::new("user:manyrels").unwrap();

    c.bench_function("list_by_subject", |b| {
        b.iter(|| {
            let result = engine.list_by_subject(black_box(&subject), None, None);
            black_box(result)
        })
    });
}

fn bench_list_pagination(c: &mut Criterion) {
    let engine = setup_engine();
    let filter = TupleFilter::default();
    let pagination = PaginationParams::new(10, None);

    c.bench_function("list_pagination", |b| {
        b.iter(|| {
            let result = engine.query(black_box(&filter), black_box(&pagination), None);
            black_box(result)
        })
    });
}

fn bench_list_pagination_cursor(c: &mut Criterion) {
    let engine = setup_engine();
    let filter = TupleFilter::default();
    let first_page = engine
        .query(&filter, &PaginationParams::new(10, None), None)
        .unwrap();
    let cursor = first_page.next_cursor.unwrap();

    c.bench_function("list_pagination_cursor", |b| {
        b.iter(|| {
            let pagination = PaginationParams::new(10, Some(black_box(cursor.clone())));
            let result = engine.query(black_box(&filter), black_box(&pagination), None);
            black_box(result)
        })
    });
}

criterion_group!(
    benches,
    bench_list_by_object,
    bench_list_by_subject,
    bench_list_pagination,
    bench_list_pagination_cursor,
);
criterion_main!(benches);
