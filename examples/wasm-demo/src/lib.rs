use aegis_core::engine::GraphEngine;
use aegis_core::storage::{InMemoryStorage, StorageBackend};
use aegis_core::types::schema::{Effect, PermissionDef, RelationDef, TypeDef};
use aegis_core::types::{Relation, RelationshipTuple, ResourceId, Schema, SubjectId};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn run_check() -> String {
    let mut storage = InMemoryStorage::new();
    storage.initialize().unwrap();

    let schema = make_schema();
    let engine = GraphEngine::new(Box::new(storage), schema);

    let subject = SubjectId::new("user:alice").unwrap();
    let resource = ResourceId::new("repo:hello-wasm").unwrap();

    engine
        .write(&RelationshipTuple::new(
            subject.clone(),
            Relation::new("owner").unwrap(),
            resource.clone(),
        ))
        .unwrap();

    let result = engine
        .check(&subject, "read", &resource, None)
        .unwrap();

    format!(
        "alice allowed read on repo:hello-wasm = {}",
        result.allowed
    )
}

fn make_schema() -> Schema {
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
            effect: Effect::Allow,
            condition: None,
            description: None,
        },
    );

    let mut types = HashMap::new();
    types.insert(
        "repo".to_string(),
        TypeDef {
            relations,
            permissions,
            roles: HashMap::new(),
            deny: vec![],
        },
    );

    Schema {
        schema_version: 1,
        namespace: "demo".to_string(),
        types,
    }
}
