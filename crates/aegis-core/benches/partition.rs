use criterion::{Criterion, black_box, criterion_group, criterion_main};

use aegis_core::engine::GraphEngine;
use aegis_core::storage::StorageBackend;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::types::schema::{PermissionDef, RelationDef, Schema, TypeDef};
use aegis_core::types::*;
use std::collections::HashMap;

const NUM_PARTITIONS: usize = 100;
const TUPLES_PER_PARTITION: usize = 100;

fn setup_partitions() -> GraphEngine {
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
            let mut permissions = HashMap::new();
            permissions.insert(
                "read".to_string(),
                PermissionDef {
                    union_of: vec!["owner".to_string()],
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

    for pi in 0..NUM_PARTITIONS {
        let pid = PartitionId::new(format!("p{:04}", pi)).unwrap();
        engine.with_partition(pid.clone()).unwrap();

        for ti in 0..TUPLES_PER_PARTITION {
            let subject = SubjectId::new(format!("user:{}", ti)).unwrap();
            let repo = ResourceId::new(format!("repo:bench{}", ti)).unwrap();
            engine
                .write(&RelationshipTuple::new(
                    subject,
                    Relation::new("owner").unwrap(),
                    repo,
                ))
                .unwrap();
        }
    }
    engine
}

fn bench_partition_write_throughput(c: &mut Criterion) {
    let engine = setup_partitions();
    let pid = PartitionId::new("p0000").unwrap();
    engine.with_partition(pid.clone()).unwrap();

    c.bench_function("partition_write", |b| {
        b.iter(|| {
            let subject = SubjectId::new("user:writes").unwrap();
            let repo = ResourceId::new("repo:wr").unwrap();
            let result = engine.write(black_box(&RelationshipTuple::new(
                subject,
                Relation::new("owner").unwrap(),
                repo,
            )));
            black_box(result)
        })
    });
}

fn bench_partition_check_throughput(c: &mut Criterion) {
    let engine = setup_partitions();
    let pid = PartitionId::new("p0000").unwrap();
    engine.with_partition(pid.clone()).unwrap();

    let subject = SubjectId::new("user:0").unwrap();
    let resource = ResourceId::new("repo:bench0").unwrap();

    c.bench_function("partition_check", |b| {
        b.iter(|| {
            let result = engine.check(black_box(&subject), "read", black_box(&resource), None);
            black_box(result)
        })
    });
}

criterion_group!(
    benches,
    bench_partition_write_throughput,
    bench_partition_check_throughput
);
criterion_main!(benches);
