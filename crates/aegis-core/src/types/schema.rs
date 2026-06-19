use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A validated authorization schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub schema_version: u32,
    pub namespace: String,
    pub types: HashMap<String, TypeDef>,
}

/// Definition of a single resource type in the schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeDef {
    pub relations: HashMap<String, RelationDef>,
    pub permissions: HashMap<String, PermissionDef>,
    pub roles: HashMap<String, RoleDef>,
    pub deny: Vec<DenyDef>,
}

/// Defines which subject types can hold this relation, and what relations it inherits from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationDef {
    /// Allowed subject type references (e.g. "user", "team#member").
    pub inherit_from: Vec<String>,
    /// Human-readable description of this relation.
    pub description: Option<String>,
}

/// The effect of a permission or deny rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Effect {
    #[default]
    Allow,
    Deny,
}

/// Defines a computed permission from a set of relations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionDef {
    /// Relations that satisfy this permission (e.g. ["viewer", "editor", "owner"]).
    pub union_of: Vec<String>,
    /// Whether this permission grants Allow or Deny.
    #[serde(default)]
    pub effect: Effect,
    /// Optional ABAC conditions.
    pub condition: Option<String>,
    /// Human-readable description of this permission.
    pub description: Option<String>,
}

/// Defines a role and its mapped permissions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleDef {
    pub permissions: Vec<String>,
    /// Parent roles this role inherits from.
    /// e.g. `admin` inherits from `editor` means admin gets all editor permissions.
    #[serde(default)]
    pub inherits_from: Vec<String>,
    pub description: Option<String>,
}

impl RoleDef {
    /// Returns the transitive closure of all permission names for this role,
    /// including permissions inherited from parent roles via `inherits_from`.
    /// Uses the provided `roles` map to resolve the inheritance chain.
    /// Returns `None` if a circular dependency is detected.
    pub fn resolved_permissions(
        &self,
        role_name: &str,
        all_roles: &std::collections::HashMap<String, RoleDef>,
    ) -> Option<Vec<String>> {
        let mut result = std::collections::BTreeSet::new();
        let mut visited = std::collections::HashSet::new();
        visited.insert(role_name.to_string());
        self.collect_permissions(&mut result, &mut visited, all_roles)?;
        Some(result.into_iter().collect())
    }

    fn collect_permissions(
        &self,
        acc: &mut std::collections::BTreeSet<String>,
        visited: &mut std::collections::HashSet<String>,
        all_roles: &std::collections::HashMap<String, RoleDef>,
    ) -> Option<()> {
        for p in &self.permissions {
            acc.insert(p.clone());
        }
        for parent in &self.inherits_from {
            if !visited.insert(parent.clone()) {
                // Circular dependency detected
                return None;
            }
            if let Some(parent_def) = all_roles.get(parent) {
                parent_def.collect_permissions(acc, visited, all_roles)?;
            }
        }
        Some(())
    }
}

/// Defines an explicit deny rule for a relation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DenyDef {
    pub relations: Vec<String>,
    pub description: Option<String>,
}

/// The raw YAML schema format before parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RawSchema {
    #[serde(rename = "schemaVersion")]
    pub schema_version: Option<u32>,
    pub namespace: Option<String>,
    pub types: Option<HashMap<String, RawTypeDef>>,
    pub roles: Option<HashMap<String, RawRoleDef>>,
    pub deny: Option<Vec<RawDenyDef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RawTypeDef {
    pub relations: Option<HashMap<String, RawRelationDef>>,
    pub permissions: Option<HashMap<String, RawPermissionDef>>,
    pub roles: Option<HashMap<String, RawRoleDef>>,
    pub deny: Option<Vec<RawDenyDef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RawRelationDef {
    pub inherit_from: Option<Vec<String>>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RawPermissionDef {
    pub union_of: Option<Vec<String>>,
    pub effect: Option<Effect>,
    pub condition: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RawRoleDef {
    pub permissions: Option<Vec<String>>,
    #[serde(default)]
    pub inherits_from: Option<Vec<String>>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RawDenyDef {
    pub relations: Option<Vec<String>>,
    pub description: Option<String>,
}

/// Result of a schema compatibility check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaCompatibilityReport {
    pub compatible: bool,
    pub warnings: Vec<String>,
    pub breaking: Vec<String>,
}

/// Result of a schema migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationResult {
    pub from_version: u32,
    pub to_version: u32,
    pub applied_migrations: Vec<String>,
}

impl Schema {
    pub fn type_names(&self) -> impl Iterator<Item = &String> {
        self.types.keys()
    }

    pub fn relation_names(&self, type_name: &str) -> Option<impl Iterator<Item = &String>> {
        self.types.get(type_name).map(|t| t.relations.keys())
    }

    pub fn permission_names(&self, type_name: &str) -> Option<impl Iterator<Item = &String>> {
        self.types.get(type_name).map(|t| t.permissions.keys())
    }

    /// Check if a given relation exists on a given type.
    pub fn has_relation(&self, type_name: &str, relation: &str) -> bool {
        self.types
            .get(type_name)
            .map(|t| t.relations.contains_key(relation))
            .unwrap_or(false)
    }

    /// Check if a given permission exists on a given type.
    pub fn has_permission(&self, type_name: &str, permission: &str) -> bool {
        self.types
            .get(type_name)
            .map(|t| t.permissions.contains_key(permission))
            .unwrap_or(false)
    }

    /// Get the relations that satisfy a permission for a given type.
    pub fn relations_for_permission(
        &self,
        type_name: &str,
        permission: &str,
    ) -> Option<&Vec<String>> {
        self.types
            .get(type_name)
            .and_then(|t| t.permissions.get(permission))
            .map(|p| &p.union_of)
    }

    /// Get the inheritance chain for a relation on a given type.
    pub fn inheritance_for_relation(
        &self,
        type_name: &str,
        relation: &str,
    ) -> Option<&Vec<String>> {
        self.types
            .get(type_name)
            .and_then(|t| t.relations.get(relation))
            .map(|r| &r.inherit_from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_schema() -> Schema {
        let mut types = HashMap::new();
        let mut repo_relations = HashMap::new();
        repo_relations.insert(
            "owner".to_string(),
            RelationDef {
                inherit_from: vec!["user".to_string(), "team#member".to_string()],
                description: Some("Owners have full control".to_string()),
            },
        );
        repo_relations.insert(
            "editor".to_string(),
            RelationDef {
                inherit_from: vec!["owner".to_string(), "collaborator".to_string()],
                description: None,
            },
        );
        repo_relations.insert(
            "viewer".to_string(),
            RelationDef {
                inherit_from: vec!["editor".to_string(), "public".to_string()],
                description: None,
            },
        );

        let mut repo_permissions = HashMap::new();
        repo_permissions.insert(
            "read".to_string(),
            PermissionDef {
                union_of: vec![
                    "viewer".to_string(),
                    "editor".to_string(),
                    "owner".to_string(),
                ],
                condition: None,
                description: Some("Can read the repository".to_string()),
                ..Default::default()
            },
        );
        repo_permissions.insert(
            "write".to_string(),
            PermissionDef {
                union_of: vec!["editor".to_string(), "owner".to_string()],
                condition: None,
                description: None,
                ..Default::default()
            },
        );
        repo_permissions.insert(
            "delete".to_string(),
            PermissionDef {
                union_of: vec!["owner".to_string()],
                condition: None,
                description: None,
                ..Default::default()
            },
        );

        types.insert(
            "repo".to_string(),
            TypeDef {
                relations: repo_relations,
                permissions: repo_permissions,
                ..Default::default()
            },
        );

        Schema {
            schema_version: 1,
            namespace: "acme".to_string(),
            types,
        }
    }

    #[test]
    fn schema_has_relation() {
        let s = sample_schema();
        assert!(s.has_relation("repo", "owner"));
        assert!(s.has_relation("repo", "editor"));
        assert!(!s.has_relation("repo", "nonexistent"));
    }

    #[test]
    fn schema_has_permission() {
        let s = sample_schema();
        assert!(s.has_permission("repo", "read"));
        assert!(s.has_permission("repo", "write"));
        assert!(!s.has_permission("repo", "nonexistent"));
    }

    #[test]
    fn schema_relations_for_permission() {
        let s = sample_schema();
        let rels = s.relations_for_permission("repo", "read").unwrap();
        assert_eq!(rels.len(), 3);
        assert!(rels.contains(&"viewer".to_string()));
        assert!(rels.contains(&"editor".to_string()));
        assert!(rels.contains(&"owner".to_string()));
    }

    #[test]
    fn schema_inheritance_chain() {
        let s = sample_schema();
        let chain = s.inheritance_for_relation("repo", "editor").unwrap();
        assert!(chain.contains(&"owner".to_string()));
        assert!(chain.contains(&"collaborator".to_string()));
    }

    #[test]
    fn schema_type_names() {
        let s = sample_schema();
        let names: Vec<&String> = s.type_names().collect();
        assert_eq!(names, vec!["repo"]);
    }

    #[test]
    fn schema_permission_names() {
        let s = sample_schema();
        let perms: Vec<&String> = s.permission_names("repo").unwrap().collect();
        assert_eq!(perms.len(), 3);
    }

    #[test]
    fn schema_missing_type_returns_none() {
        let s = sample_schema();
        assert!(!s.has_relation("nonexistent", "owner"));
        assert!(s.relations_for_permission("nonexistent", "read").is_none());
    }
}
