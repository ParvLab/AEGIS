use crate::types::schema::Schema;

/// Resolved policy: which relations satisfy a given permission.
pub struct ResolvedPolicy {
    /// The resource type name.
    pub resource_type: String,
    /// The permission name.
    pub permission: String,
    /// Relations that satisfy this permission (from `union_of` in the schema).
    /// The graph engine's traversal handles any inheritance chains.
    pub relations: Vec<String>,
}

/// Resolve a permission on a resource type into the set of relations to check.
///
/// Returns the `union_of` relations from the schema definition as-is.
/// Inheritance resolution happens at graph traversal time: if subject has
/// "editor" on repo and "editor" inherits from "owner", the traversal
/// follows tuple edges to find the path, not schema-level rewriting.
pub fn resolve_permission(
    schema: &Schema,
    resource_type: &str,
    permission: &str,
) -> Option<ResolvedPolicy> {
    let type_def = schema.types.get(resource_type)?;
    let perm_def = type_def.permissions.get(permission)?;

    Some(ResolvedPolicy {
        resource_type: resource_type.to_string(),
        permission: permission.to_string(),
        relations: perm_def.union_of.clone(),
    })
}

/// Check if a permission is defined for a given resource type.
pub fn permission_exists(schema: &Schema, resource_type: &str, permission: &str) -> bool {
    schema
        .types
        .get(resource_type)
        .and_then(|t| t.permissions.get(permission))
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::schema::*;
    use std::collections::HashMap;

    fn test_schema() -> Schema {
        let mut types = HashMap::new();
        let mut repo_rels = HashMap::new();
        repo_rels.insert(
            "owner".to_string(),
            RelationDef {
                inherit_from: vec![],
                description: None,
            },
        );
        repo_rels.insert(
            "editor".to_string(),
            RelationDef {
                inherit_from: vec!["owner".to_string()],
                description: None,
            },
        );
        repo_rels.insert(
            "viewer".to_string(),
            RelationDef {
                inherit_from: vec!["editor".to_string()],
                description: None,
            },
        );

        let mut repo_perms = HashMap::new();
        repo_perms.insert(
            "read".to_string(),
            PermissionDef {
                union_of: vec!["viewer".to_string(), "editor".to_string(), "owner".to_string()],
                condition: None,
                description: None,
            },
        );
        repo_perms.insert(
            "write".to_string(),
            PermissionDef {
                union_of: vec!["editor".to_string(), "owner".to_string()],
                condition: None,
                description: None,
            },
        );

        types.insert(
            "repo".to_string(),
            crate::types::schema::TypeDef {
                relations: repo_rels,
                permissions: repo_perms,
            },
        );

        Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types,
        }
    }

    #[test]
    fn test_resolve_read_permission() {
        let schema = test_schema();
        let resolved = resolve_permission(&schema, "repo", "read").unwrap();

        // union_of: ["viewer", "editor", "owner"] → returned as-is
        assert!(resolved.relations.contains(&"viewer".to_string()));
        assert!(resolved.relations.contains(&"editor".to_string()));
        assert!(resolved.relations.contains(&"owner".to_string()));
        assert_eq!(resolved.relations.len(), 3);
    }

    #[test]
    fn test_resolve_write_permission() {
        let schema = test_schema();
        let resolved = resolve_permission(&schema, "repo", "write").unwrap();

        // union_of: ["editor", "owner"] → returned as-is
        assert!(resolved.relations.contains(&"editor".to_string()));
        assert!(resolved.relations.contains(&"owner".to_string()));
        assert!(!resolved.relations.contains(&"viewer".to_string()));
        assert_eq!(resolved.relations.len(), 2);
    }

    #[test]
    fn test_resolve_unknown_permission() {
        let schema = test_schema();
        assert!(resolve_permission(&schema, "repo", "nonexistent").is_none());
    }

    #[test]
    fn test_permission_exists() {
        let schema = test_schema();
        assert!(permission_exists(&schema, "repo", "read"));
        assert!(permission_exists(&schema, "repo", "write"));
        assert!(!permission_exists(&schema, "repo", "nonexistent"));
        assert!(!permission_exists(&schema, "unknown", "read"));
    }
}
