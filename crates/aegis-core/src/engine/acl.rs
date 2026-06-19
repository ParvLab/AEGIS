use crate::engine::GraphEngine;
use crate::error::{AegisError, AegisResult};
use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Grant a permission directly to a subject on a resource.
/// Writes a relationship tuple for the given permission/relation.
pub fn grant(
    engine: &GraphEngine,
    subject: &SubjectId,
    permission: &str,
    resource: &ResourceId,
) -> AegisResult<RevisionToken> {
    let resource_type = resource_type_name(resource.as_str());
    let schema = engine.schema();
    let rels = schema
        .relations_for_permission(&resource_type, permission)
        .ok_or_else(|| {
            AegisError::SchemaValidation(format!(
                "permission '{permission}' not found for type '{resource_type}'"
            ))
        })?
        .clone();
    drop(schema);

    if rels.is_empty() {
        return Err(AegisError::SchemaValidation(format!(
            "permission '{permission}' has no granting relations"
        )));
    }

    let tuple = RelationshipTuple::new(subject.clone(), Relation::new(&rels[0])?, resource.clone());
    engine.write(&tuple)
}

/// Revoke a direct permission grant from a subject on a resource.
pub fn revoke(
    engine: &GraphEngine,
    subject: &SubjectId,
    permission: &str,
    resource: &ResourceId,
) -> AegisResult<RevisionToken> {
    let resource_type = resource_type_name(resource.as_str());
    let schema = engine.schema();
    let rels = schema
        .relations_for_permission(&resource_type, permission)
        .ok_or_else(|| {
            AegisError::SchemaValidation(format!(
                "permission '{permission}' not found for type '{resource_type}'"
            ))
        })?
        .clone();
    drop(schema);

    if rels.is_empty() {
        return Err(AegisError::SchemaValidation(format!(
            "permission '{permission}' has no granting relations"
        )));
    }

    let key = TupleKey {
        subject: subject.clone(),
        relation: Relation::new(&rels[0])?,
        object: resource.clone(),
    };
    engine.delete(&key)
}

/// List all direct relationship tuples on a resource.
pub fn list_acls(
    engine: &GraphEngine,
    resource: &ResourceId,
) -> AegisResult<Vec<RelationshipTuple>> {
    engine.list_by_object(resource, None, None)
}

fn resource_type_name(id: &str) -> String {
    id.split(':').next().unwrap_or(id).to_string()
}

/// A portable JSON-serializable ACL entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedAclEntry {
    pub subject: String,
    pub relation: String,
    pub object: String,
    pub metadata: Option<HashMap<String, String>>,
    pub condition: Option<String>,
}

/// A collection of ACL entries for import/export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedAclCollection {
    pub schema_version: u32,
    pub namespace: String,
    pub entries: Vec<SerializedAclEntry>,
}

/// Export all ACL entries for a given resource as JSON.
pub fn serialize_acls(engine: &GraphEngine, resource: &ResourceId) -> AegisResult<String> {
    let tuples = engine.list_by_object(resource, None, None)?;
    let schema = engine.schema();
    let entries: Vec<SerializedAclEntry> = tuples
        .iter()
        .map(|t| SerializedAclEntry {
            subject: t.subject.to_string(),
            relation: t.relation.to_string(),
            object: t.object.to_string(),
            metadata: t.metadata.clone(),
            condition: t.condition.clone(),
        })
        .collect();
    let collection = SerializedAclCollection {
        schema_version: schema.schema_version,
        namespace: schema.namespace.clone(),
        entries,
    };
    drop(schema);
    serde_json::to_string_pretty(&collection)
        .map_err(|e| AegisError::MetadataValidation(e.to_string()))
}

/// Import ACL entries from JSON, writing each entry as a relationship tuple.
/// Returns a list of revision tokens, one per successful write.
pub fn deserialize_acls(engine: &GraphEngine, json: &str) -> AegisResult<Vec<RevisionToken>> {
    let collection: SerializedAclCollection =
        serde_json::from_str(json).map_err(|e| AegisError::MetadataValidation(e.to_string()))?;
    let mut tokens = Vec::new();
    for entry in &collection.entries {
        let subject = SubjectId::new(&entry.subject).map_err(AegisError::Validation)?;
        let relation = Relation::new(&entry.relation).map_err(AegisError::Validation)?;
        let object = ResourceId::new(&entry.object).map_err(AegisError::Validation)?;

        let tuple = match (&entry.metadata, &entry.condition) {
            (Some(meta), Some(cond)) => {
                let mut t =
                    RelationshipTuple::with_condition(subject, relation, object, cond.clone());
                t.metadata = Some(meta.clone());
                t
            }
            (Some(meta), None) => {
                RelationshipTuple::with_metadata(subject, relation, object, meta.clone())
                    .map_err(|e| AegisError::MetadataValidation(e.to_string()))?
            }
            (None, Some(cond)) => {
                RelationshipTuple::with_condition(subject, relation, object, cond.clone())
            }
            (None, None) => RelationshipTuple::new(subject, relation, object),
        };
        tokens.push(engine.write(&tuple)?);
    }
    Ok(tokens)
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use crate::storage::StorageBackend;
    #[cfg(feature = "sqlite")]
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::types::schema::*;

    fn make_engine() -> GraphEngine {
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types: {
                let mut types = std::collections::HashMap::new();
                let mut relations = std::collections::HashMap::new();
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
                let mut permissions = std::collections::HashMap::new();
                permissions.insert(
                    "read".to_string(),
                    PermissionDef {
                        union_of: vec!["viewer".to_string(), "owner".to_string()],
                        condition: None,
                        description: None,
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
        GraphEngine::new(Box::new(storage), schema)
    }

    #[test]
    fn test_serialize_roundtrip() {
        let engine = make_engine();
        let alice = SubjectId::new("user:alice").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();
        engine
            .write(&RelationshipTuple::new(
                alice.clone(),
                Relation::new("owner").unwrap(),
                repo.clone(),
            ))
            .unwrap();

        let json = serialize_acls(&engine, &repo).unwrap();
        assert!(json.contains("user:alice"));
        assert!(json.contains("owner"));

        let tokens = deserialize_acls(&engine, &json).unwrap();
        assert!(!tokens.is_empty());
    }

    #[test]
    fn test_serialize_with_condition() {
        let engine = make_engine();
        let alice = SubjectId::new("user:alice").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();
        engine
            .write(&RelationshipTuple::with_condition(
                alice.clone(),
                Relation::new("viewer").unwrap(),
                repo.clone(),
                "role eq admin".to_string(),
            ))
            .unwrap();

        let json = serialize_acls(&engine, &repo).unwrap();
        assert!(json.contains("role eq admin"));

        let _tokens = deserialize_acls(&engine, &json).unwrap();
        // Verify written tuple has condition
        let tuples = engine.list_by_object(&repo, None, None).unwrap();
        assert!(
            tuples
                .iter()
                .any(|t| t.condition.as_deref() == Some("role eq admin"))
        );
    }

    #[test]
    fn test_deserialize_invalid_json() {
        let engine = make_engine();
        let result = deserialize_acls(&engine, "not json");
        assert!(result.is_err());
    }
}
