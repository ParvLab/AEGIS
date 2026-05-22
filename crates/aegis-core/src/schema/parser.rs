use crate::error::{AegisError, AegisResult};
use crate::schema::types::{LintDiagnostic, LintResult, LintSeverity};
use crate::types::schema::{PermissionDef, RawSchema, RawTypeDef, RelationDef, Schema, TypeDef};
use std::collections::{HashMap, HashSet};

/// Parse a YAML string into a validated Schema.
pub fn parse_schema(yaml: &str) -> AegisResult<Schema> {
    let raw: RawSchema = serde_yaml::from_str(yaml)
        .map_err(|e| AegisError::SchemaValidation(format!("invalid YAML: {e}")))?;

    let mut types = HashMap::new();

    let raw_types = raw
        .types
        .ok_or_else(|| AegisError::SchemaValidation("missing 'types' field".into()))?;

    for (type_name, raw_type) in raw_types {
        let relations = parse_relations(&raw_type)?;
        let permissions = parse_permissions(&raw_type)?;
        types.insert(
            type_name,
            TypeDef {
                relations,
                permissions,
            },
        );
    }

    let schema = Schema {
        schema_version: raw.schema_version.unwrap_or(1),
        namespace: raw.namespace.unwrap_or_else(|| "default".to_string()),
        types,
    };

    // Run validation after construction
    let lint = validate_schema(&schema)?;
    if !lint.valid {
        let errors: Vec<String> = lint
            .diagnostics
            .iter()
            .filter(|d| d.severity == LintSeverity::Error)
            .map(|d| d.message.clone())
            .collect();
        return Err(AegisError::SchemaValidation(format!(
            "schema validation failed: {}",
            errors.join("; ")
        )));
    }

    Ok(schema)
}

fn parse_relations(raw_type: &RawTypeDef) -> AegisResult<HashMap<String, RelationDef>> {
    let mut relations = HashMap::new();
    if let Some(ref raw_relations) = raw_type.relations {
        for (name, raw_rel) in raw_relations {
            let inherit_from = raw_rel.inherit_from.clone().unwrap_or_default();
            relations.insert(
                name.clone(),
                RelationDef {
                    inherit_from,
                    description: raw_rel.description.clone(),
                },
            );
        }
    }
    Ok(relations)
}

fn parse_permissions(raw_type: &RawTypeDef) -> AegisResult<HashMap<String, PermissionDef>> {
    let mut permissions = HashMap::new();
    if let Some(ref raw_permissions) = raw_type.permissions {
        for (name, raw_perm) in raw_permissions {
            let union_of = raw_perm.union_of.clone().unwrap_or_default();
            permissions.insert(
                name.clone(),
                PermissionDef {
                    union_of,
                    condition: raw_perm.condition.clone(),
                    description: raw_perm.description.clone(),
                },
            );
        }
    }
    Ok(permissions)
}

/// Lint a schema and return diagnostics.
pub fn lint_schema(schema: &Schema) -> LintResult {
    let mut diagnostics = Vec::new();

    // Collect all defined relation names across all types
    let mut all_relations: HashSet<&str> = HashSet::new();
    let mut defined_types: HashSet<&str> = HashSet::new();
    for (type_name, type_def) in &schema.types {
        defined_types.insert(type_name.as_str());
        for rel_name in type_def.relations.keys() {
            all_relations.insert(rel_name.as_str());
        }
    }

    // Check for type-level issues
    for (type_name, type_def) in &schema.types {
        // Circular relation inheritance detection
        let mut visited = HashSet::new();
        if has_circular_relations(type_name, &schema.types, &mut visited) {
            diagnostics.push(LintDiagnostic {
                severity: LintSeverity::Error,
                message: format!("circular relation inheritance detected in type '{type_name}'"),
                location: Some(format!("types.{type_name}")),
            });
        }

        // Check for orphan relations (defined but never referenced)
        for rel_name in type_def.relations.keys() {
            let is_referenced = schema.types.values().any(|t| {
                t.relations
                    .values()
                    .any(|r| r.inherit_from.iter().any(|s| s == rel_name))
            });
            if !is_referenced
                && type_def
                    .permissions
                    .values()
                    .all(|p| !p.union_of.contains(rel_name))
            {
                diagnostics.push(LintDiagnostic {
                    severity: LintSeverity::Warning,
                    message: format!(
                        "relation '{rel_name}' on type '{type_name}' is defined but never referenced"
                    ),
                    location: Some(format!("types.{type_name}.relations.{rel_name}")),
                });
            }
        }

        // Check for overly broad permissions
        for (perm_name, perm_def) in &type_def.permissions {
            if perm_def.union_of.is_empty() {
                diagnostics.push(LintDiagnostic {
                    severity: LintSeverity::Error,
                    message: format!(
                        "permission '{perm_name}' on type '{type_name}' has no granting relations"
                    ),
                    location: Some(format!("types.{type_name}.permissions.{perm_name}")),
                });
            }
        }

        // Check for permission references to undefined relations
        for (perm_name, perm_def) in &type_def.permissions {
            for rel_ref in &perm_def.union_of {
                if !type_def.relations.contains_key(rel_ref) {
                    diagnostics.push(LintDiagnostic {
                        severity: LintSeverity::Error,
                        message: format!(
                            "permission '{perm_name}' on type '{type_name}' references undefined relation '{rel_ref}'"
                        ),
                        location: Some(format!("types.{type_name}.permissions.{perm_name}")),
                    });
                }
            }
        }
    }

    LintResult::with_diagnostics(diagnostics)
}

fn validate_schema(schema: &Schema) -> AegisResult<LintResult> {
    let lint = lint_schema(schema);
    Ok(lint)
}

fn has_circular_relations(
    type_name: &str,
    types: &HashMap<String, TypeDef>,
    visited: &mut HashSet<String>,
) -> bool {
    if !visited.insert(type_name.to_string()) {
        return true;
    }
    if let Some(type_def) = types.get(type_name) {
        for rel_def in type_def.relations.values() {
            for inherit_ref in &rel_def.inherit_from {
                // Check if the reference is a type name (not a relation pattern)
                if types.contains_key(inherit_ref) {
                    if has_circular_relations(inherit_ref, types, visited) {
                        return true;
                    }
                }
            }
        }
    }
    visited.remove(type_name);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_schema_yaml() -> &'static str {
        r#"
schemaVersion: 1
namespace: acme
types:
  repo:
    relations:
      owner:
        inherit_from:
          - user
          - team#member
        description: "Owners have full control"
      editor:
        inherit_from:
          - owner
          - collaborator
      viewer:
        inherit_from:
          - editor
          - public
    permissions:
      read:
        union_of:
          - viewer
          - editor
          - owner
      write:
        union_of:
          - editor
          - owner
      delete:
        union_of:
          - owner
"#
    }

    #[test]
    fn parse_valid_schema() {
        let schema = parse_schema(valid_schema_yaml()).unwrap();
        assert_eq!(schema.schema_version, 1);
        assert_eq!(schema.namespace, "acme");
        assert!(schema.types.contains_key("repo"));
    }

    #[test]
    fn parse_invalid_yaml() {
        let err = parse_schema("not: valid: yaml: [[[").unwrap_err();
        assert!(matches!(err, AegisError::SchemaValidation(_)));
    }

    #[test]
    fn parse_missing_types() {
        let err = parse_schema("schemaVersion: 1\nnamespace: test").unwrap_err();
        assert!(matches!(err, AegisError::SchemaValidation(_)));
    }

    #[test]
    fn lint_empty_permission() {
        let mut types = std::collections::HashMap::new();
        let mut relations = std::collections::HashMap::new();
        relations.insert(
            "viewer".to_string(),
            RelationDef {
                inherit_from: vec!["user".to_string()],
                description: None,
            },
        );
        let mut permissions = std::collections::HashMap::new();
        permissions.insert(
            "read".to_string(),
            PermissionDef {
                union_of: vec![],
                condition: None,
                description: None,
            },
        );
        types.insert(
            "repo".to_string(),
            TypeDef {
                relations,
                permissions,
            },
        );
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types,
        };
        let result = lint_schema(&schema);
        assert!(!result.valid);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("has no granting relations"))
        );
    }

    #[test]
    fn lint_orphan_relation() {
        let mut types = std::collections::HashMap::new();
        let mut relations = std::collections::HashMap::new();
        relations.insert(
            "owner".to_string(),
            RelationDef {
                inherit_from: vec!["user".to_string()],
                description: None,
            },
        );
        relations.insert(
            "unused".to_string(),
            RelationDef {
                inherit_from: vec!["user".to_string()],
                description: None,
            },
        );
        let mut permissions = std::collections::HashMap::new();
        permissions.insert(
            "read".to_string(),
            PermissionDef {
                union_of: vec!["owner".to_string()],
                condition: None,
                description: None,
            },
        );
        types.insert(
            "repo".to_string(),
            TypeDef {
                relations,
                permissions,
            },
        );
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types,
        };
        let result = lint_schema(&schema);
        let orphan_warnings: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("never referenced"))
            .collect();
        assert_eq!(orphan_warnings.len(), 1);
    }

    #[test]
    fn lint_permission_references_undefined_relation() {
        let mut types = std::collections::HashMap::new();
        let mut relations = std::collections::HashMap::new();
        relations.insert(
            "owner".to_string(),
            RelationDef {
                inherit_from: vec!["user".to_string()],
                description: None,
            },
        );
        let mut permissions = std::collections::HashMap::new();
        permissions.insert(
            "read".to_string(),
            PermissionDef {
                union_of: vec!["owner".to_string(), "nonexistent".to_string()],
                condition: None,
                description: None,
            },
        );
        types.insert(
            "repo".to_string(),
            TypeDef {
                relations,
                permissions,
            },
        );
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types,
        };
        let result = lint_schema(&schema);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == LintSeverity::Error)
            .collect();
        assert!(!errors.is_empty());
    }

    #[test]
    fn parse_schema_with_minimal_fields() {
        let yaml = r#"
types:
  repo:
    relations:
      owner:
        inherit_from: [user]
"#;
        let schema = parse_schema(yaml).unwrap();
        assert_eq!(schema.schema_version, 1);
        assert_eq!(schema.namespace, "default");
    }

    #[test]
    fn lint_valid_schema() {
        let schema = parse_schema(valid_schema_yaml()).unwrap();
        let result = lint_schema(&schema);
        assert!(result.valid);
    }
}
