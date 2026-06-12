use crate::error::{AegisError, AegisResult};
use crate::types::schema::{Schema, SchemaCompatibilityReport};
use std::collections::HashSet;

/// Lint result from schema analysis.
#[derive(Debug, Clone, Default)]
pub struct LintReport {
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub passed: bool,
}

impl LintReport {
    pub fn is_clean(&self) -> bool {
        self.passed && self.warnings.is_empty()
    }
}

/// Run lint checks on a schema, returning warnings and errors.
/// When `strict` is true, warnings are promoted to errors.
pub fn lint_schema(schema: &Schema, strict: bool) -> LintReport {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    for (type_name, type_def) in &schema.types {
        // Check for missing documentation on relations
        for (rel_name, rel_def) in &type_def.relations {
            if rel_def.description.is_none() || rel_def.description.as_deref().unwrap_or("").is_empty() {
                let msg = format!("relation '{rel_name}' on type '{type_name}' has no description");
                if strict { errors.push(msg); } else { warnings.push(msg); }
            }
        }

        // Check for missing documentation on permissions
        for (perm_name, perm_def) in &type_def.permissions {
            if perm_def.description.is_none() || perm_def.description.as_deref().unwrap_or("").is_empty() {
                let msg = format!("permission '{perm_name}' on type '{type_name}' has no description");
                if strict { errors.push(msg); } else { warnings.push(msg); }
            }
        }

        // Check for overly broad permissions (wildcard *)
        for (perm_name, perm_def) in &type_def.permissions {
            let combined = perm_def.union_of.join(" ");
            if combined.contains('*') {
                let msg = format!("permission '{perm_name}' on type '{type_name}' uses wildcard '*' — overly broad");
                if strict { errors.push(msg); } else { warnings.push(msg); }
            }
        }

        // Check condition syntax validity on permissions
        for (perm_name, perm_def) in &type_def.permissions {
            if let Some(ref cond) = perm_def.condition {
                if let Err(e) = crate::engine::condition::parse_condition(cond) {
                    let msg = format!("permission '{perm_name}' on type '{type_name}' has invalid condition syntax: {e}");
                    if strict { errors.push(msg); } else { errors.push(msg); }
                }
            }
        }
    }

    // Check for unused types (types with no relations/permissions or never referenced)
    for type_name in schema.types.keys() {
        let type_def = &schema.types[type_name];
        let has_content = !type_def.relations.is_empty() || !type_def.permissions.is_empty();
        if !has_content {
            let msg = format!("type '{type_name}' is defined but has no relations or permissions");
            if strict { errors.push(msg); } else { warnings.push(msg); }
        } else if schema.types.len() > 1 {
            let is_referenced = schema.types.iter().filter(|(k, _)| *k != type_name).any(|(_, t)| {
                t.relations.values().any(|r| r.inherit_from.iter().any(|s| s == type_name))
                    || t.permissions.values().any(|p| p.union_of.iter().any(|s| s == type_name))
            });
            if !is_referenced {
                let msg = format!("type '{type_name}' is defined but never referenced by any other type's relations or permissions");
                if strict { errors.push(msg); } else { warnings.push(msg); }
            }
        }
    }

    let passed = errors.is_empty();
    LintReport {
        warnings,
        errors,
        passed,
    }
}

/// Check whether a new schema is compatible with the existing stored data.
/// This is a dry-run validation: no changes are committed.
pub fn check_schema_compatibility(
    existing_schema: &Schema,
    new_schema: &Schema,
) -> SchemaCompatibilityReport {
    let mut warnings = Vec::new();
    let mut breaking = Vec::new();

    let existing_types: HashSet<&str> = existing_schema.types.keys().map(|s| s.as_str()).collect();
    let new_types: HashSet<&str> = new_schema.types.keys().map(|s| s.as_str()).collect();

    // Removed types are breaking
    for removed_type in existing_types.difference(&new_types) {
        breaking.push(format!(
            "type '{removed_type}' is removed. Existing tuples for this type will become inaccessible."
        ));
    }

    // For existing types, check for removed relations and permissions
    for type_name in existing_types.intersection(&new_types) {
        let existing_type = &existing_schema.types[*type_name];
        let new_type = &new_schema.types[*type_name];

        let existing_relations: HashSet<&str> =
            existing_type.relations.keys().map(|s| s.as_str()).collect();
        let new_relations: HashSet<&str> = new_type.relations.keys().map(|s| s.as_str()).collect();

        for removed_rel in existing_relations.difference(&new_relations) {
            breaking.push(format!(
                "relation '{removed_rel}' removed from type '{type_name}'. Existing tuples with this relation will break."
            ));
        }

        let existing_perms: HashSet<&str> = existing_type
            .permissions
            .keys()
            .map(|s| s.as_str())
            .collect();
        let new_perms: HashSet<&str> = new_type.permissions.keys().map(|s| s.as_str()).collect();

        for removed_perm in existing_perms.difference(&new_perms) {
            warnings.push(format!(
                "permission '{removed_perm}' removed from type '{type_name}'. Checks for this permission will always return deny."
            ));
        }
    }

    // New types are always safe
    for added_type in new_types.difference(&existing_types) {
        warnings.push(format!(
            "new type '{added_type}' added. No existing tuples yet — safe."
        ));
    }

    SchemaCompatibilityReport {
        compatible: breaking.is_empty(),
        warnings,
        breaking,
    }
}

/// Verify that a resource type name exists in the schema.
pub fn validate_resource_type(schema: &Schema, resource: &str) -> AegisResult<()> {
    let type_name = resource
        .split(':')
        .next()
        .ok_or_else(|| AegisError::Validation(crate::types::ValidationError::Empty))?;

    if !schema.types.contains_key(type_name) {
        return Err(AegisError::UnknownSubjectType(type_name.to_string()));
    }
    Ok(())
}

/// Verify that a relation exists for the given resource type.
pub fn validate_relation(schema: &Schema, resource: &str, relation: &str) -> AegisResult<()> {
    let type_name = resource
        .split(':')
        .next()
        .ok_or_else(|| AegisError::Validation(crate::types::ValidationError::Empty))?;

    if !schema.has_relation(type_name, relation) && !schema.has_permission(type_name, relation) {
        return Err(AegisError::UnknownRelation {
            type_name: type_name.to_string(),
            relation: relation.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::parser::parse_schema;

    fn make_schema(yaml: &str) -> Schema {
        parse_schema(yaml).unwrap()
    }

    fn schema_v1() -> Schema {
        make_schema(
            r#"
types:
  repo:
    relations:
      owner:
        inherit_from: [user]
      editor:
        inherit_from: [owner]
    permissions:
      read:
        union_of: [owner, editor]
      write:
        union_of: [owner]
"#,
        )
    }

    fn schema_v2_removes_editor() -> Schema {
        make_schema(
            r#"
types:
  repo:
    relations:
      owner:
        inherit_from: [user]
    permissions:
      read:
        union_of: [owner]
      write:
        union_of: [owner]
"#,
        )
    }

    fn schema_v2_adds_viewer() -> Schema {
        make_schema(
            r#"
types:
  repo:
    relations:
      owner:
        inherit_from: [user]
      editor:
        inherit_from: [owner]
      viewer:
        inherit_from: [editor]
    permissions:
      read:
        union_of: [owner, editor, viewer]
      write:
        union_of: [owner]
"#,
        )
    }

    #[test]
    fn compatible_additive_change() {
        let report = check_schema_compatibility(&schema_v1(), &schema_v2_adds_viewer());
        assert!(report.compatible);
        assert!(report.breaking.is_empty());
    }

    #[test]
    fn breaking_remove_relation() {
        let report = check_schema_compatibility(&schema_v1(), &schema_v2_removes_editor());
        assert!(!report.compatible);
        assert!(report.breaking.iter().any(|m| m.contains("editor")));
    }

    #[test]
    fn lint_schema_clean() {
        let schema = make_schema(
            r#"
types:
  repo:
    relations:
      owner:
        inherit_from: [user]
        description: "The owner of the repository"
      viewer:
        inherit_from: [user]
        description: "A viewer of the repository"
    permissions:
      read:
        union_of: [viewer, owner]
        description: "Can read the repository"
      write:
        union_of: [owner]
        description: "Can write to the repository"
"#,
        );
        let report = lint_schema(&schema, false);
        assert!(report.is_clean(), "expected clean lint: {:?}", report.warnings);
    }

    #[test]
    fn lint_schema_missing_descriptions() {
        let schema = make_schema(
            r#"
types:
  repo:
    relations:
      owner:
        inherit_from: [user]
    permissions:
      read:
        union_of: [owner]
"#,
        );
        let report = lint_schema(&schema, false);
        assert!(!report.warnings.is_empty());
        assert!(report.warnings.iter().any(|w| w.contains("owner") && w.contains("description")));
        assert!(report.warnings.iter().any(|w| w.contains("read") && w.contains("description")));
    }



    #[test]
    fn lint_schema_condition_syntax() {
        use crate::types::schema::{PermissionDef, RelationDef, TypeDef};
        let mut types = std::collections::HashMap::new();
        let mut relations = std::collections::HashMap::new();
        relations.insert("owner".to_string(), RelationDef {
            inherit_from: vec!["user".to_string()],
            description: Some("owner".to_string()),
        });
        let mut permissions = std::collections::HashMap::new();
        permissions.insert("admin".to_string(), PermissionDef {
            union_of: vec!["owner".to_string()],
            condition: Some("role eq admin".to_string()),
            description: Some("admin".to_string()),
        });
        permissions.insert("invalid".to_string(), PermissionDef {
            union_of: vec!["owner".to_string()],
            condition: Some("bad syntax here".to_string()),
            description: Some("invalid".to_string()),
        });
        types.insert("repo".to_string(), TypeDef { relations, permissions });
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types,
        };
        let report = lint_schema(&schema, false);
        assert!(report.errors.iter().any(|w| w.contains("condition")), "expected condition syntax error: {:?}", report.errors);
    }

    #[test]
    fn lint_schema_strict() {
        let schema = make_schema(
            r#"
types:
  repo:
    relations:
      owner:
        inherit_from: [user]
    permissions:
      read:
        union_of: [owner]
"#,
        );
        let report = lint_schema(&schema, true);
        assert!(!report.errors.is_empty());
    }

    #[test]
    fn warning_on_removed_permission() {
        let existing = schema_v1();
        let new = make_schema(
            r#"
types:
  repo:
    relations:
      owner:
        inherit_from: [user]
      editor:
        inherit_from: [owner]
    permissions:
      read:
        union_of: [owner, editor]
"#,
        );
        let report = check_schema_compatibility(&existing, &new);
        assert!(report.compatible);
        assert!(report.warnings.iter().any(|w| w.contains("write")));
    }

    #[test]
    fn warning_on_new_type() {
        let existing = schema_v1();
        let new = make_schema(
            r#"
types:
  repo:
    relations:
      owner:
        inherit_from: [user]
      editor:
        inherit_from: [owner]
    permissions:
      read:
        union_of: [owner, editor]
      write:
        union_of: [owner]
  workspace:
    relations:
      member:
        inherit_from: [user]
    permissions:
      read:
        union_of: [member]
"#,
        );
        let report = check_schema_compatibility(&existing, &new);
        assert!(report.compatible);
        assert!(report.warnings.iter().any(|w| w.contains("workspace")));
    }

    #[test]
    fn validate_resource_type_ok() {
        let schema = schema_v1();
        assert!(validate_resource_type(&schema, "repo:fluxbus").is_ok());
    }

    #[test]
    fn validate_resource_type_unknown() {
        let schema = schema_v1();
        let err = validate_resource_type(&schema, "unknown:foo").unwrap_err();
        assert!(matches!(err, AegisError::UnknownSubjectType(_)));
    }

    #[test]
    fn validate_relation_ok() {
        let schema = schema_v1();
        assert!(validate_relation(&schema, "repo:fluxbus", "owner").is_ok());
    }

    #[test]
    fn validate_relation_unknown() {
        let schema = schema_v1();
        let err = validate_relation(&schema, "repo:fluxbus", "superadmin").unwrap_err();
        assert!(matches!(err, AegisError::UnknownRelation { .. }));
    }
}
