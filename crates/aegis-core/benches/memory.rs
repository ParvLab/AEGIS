use criterion::{Criterion, black_box, criterion_group, criterion_main};

use aegis_core::engine::GraphEngine;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::StorageBackend;
use aegis_core::types::schema::{PermissionDef, RelationDef, Schema, TypeDef};
use aegis_core::types::*;
use std::collections::HashMap;

fn current_rss_kb() -> u64 {
    #[cfg(target_os = "windows")]
    {
        let pid = std::process::id();
        let output = std::process::Command::new("tasklist")
            .args([
                "/FI",
                &format!("PID eq {}", pid),
                "/FO",
                "CSV",
                "/NH",
            ])
            .output()
            .expect("tasklist failed");
        let stdout = String::from_utf8(output.stdout).expect("invalid utf8");
        let line = stdout.lines().next().expect("no output from tasklist");
        let parts: Vec<&str> = line.trim_matches('"').split("\",\"").collect();
        let mem = parts
            .get(4)
            .expect("missing mem field")
            .trim_end_matches(" K");
        mem.replace(',', "").parse().expect("parse mem failed")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let pid = std::process::id();
        let content =
            std::fs::read_to_string(format!("/proc/{}/status", pid)).expect("no /proc/pid/status");
        for line in content.lines() {
            if line.starts_with("VmRSS:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                return parts[1].parse().expect("parse VmRSS failed");
            }
        }
        panic!("VmRSS not found in /proc/self/status");
    }
}

fn setup_engine_with_10k_writes() -> GraphEngine {
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

    for i in 0..10_000 {
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

fn bench_memory_after_writes(c: &mut Criterion) {
    let before = current_rss_kb();
    let _engine = setup_engine_with_10k_writes();
    let after = current_rss_kb();
    eprintln!(
        "mem_before={}KB mem_after={}KB delta={}KB",
        before,
        after,
        after.saturating_sub(before)
    );

    c.bench_function("memory_after_writes", |b| {
        b.iter(|| {
            let rss = current_rss_kb();
            black_box(rss)
        })
    });
}

criterion_group!(benches, bench_memory_after_writes);
criterion_main!(benches);
