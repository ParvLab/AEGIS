use crate::error::{AegisError, AegisResult};
use crate::types::schema::{Schema, SchemaCompatibilityReport};
use std::collections::HashSet;

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
