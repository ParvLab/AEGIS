use crate::error::{AegisError, AegisResult};
use crate::storage::StorageBackend;
use crate::types::schema::{Schema, SchemaCompatibilityReport};
use crate::types::MigrationResult;

/// A single schema migration step.
pub struct MigrationStep {
    pub version: u32,
    pub description: String,
    pub up: Box<dyn Fn(&dyn StorageBackend) -> AegisResult<()> + Send + Sync>,
    pub down: Box<dyn Fn(&dyn StorageBackend) -> AegisResult<()> + Send + Sync>,
}

/// Schema migration runner.
///
/// Reads the current schema version from `_aegis_schema` table,
/// applies pending migrations in order, and records each application.
pub struct MigrationRunner {
    migrations: Vec<MigrationStep>,
}

impl MigrationRunner {
    pub fn new() -> Self {
        Self {
            migrations: Vec::new(),
        }
    }

    /// Register a migration step.
    pub fn register(
        &mut self,
        version: u32,
        description: &str,
        up: Box<dyn Fn(&dyn StorageBackend) -> AegisResult<()> + Send + Sync>,
        down: Box<dyn Fn(&dyn StorageBackend) -> AegisResult<()> + Send + Sync>,
    ) {
        self.migrations.push(MigrationStep {
            version,
            description: description.to_string(),
            up,
            down,
        });
    }

    /// Run all pending migrations to reach the target version.
    pub fn migrate(
        &self,
        storage: &dyn StorageBackend,
        current_version: u32,
        target_version: u32,
    ) -> AegisResult<MigrationResult> {
        let mut pending: Vec<&MigrationStep> = self
            .migrations
            .iter()
            .filter(|m| m.version > current_version && m.version <= target_version)
            .collect();

        pending.sort_by_key(|m| m.version);

        let mut applied = Vec::new();

        for step in &pending {
            (step.up)(storage).map_err(|e| {
                AegisError::SchemaMigration(format!(
                    "migration V{} ({}) failed: {}",
                    step.version, step.description, e
                ))
            })?;
            applied.push(format!("V{}: {}", step.version, step.description));
        }

        Ok(MigrationResult {
            from_version: current_version,
            to_version: target_version,
            applied_migrations: applied,
        })
    }

    /// Roll back migrations from the current version to a target version.
    pub fn rollback(
        &self,
        storage: &dyn StorageBackend,
        current_version: u32,
        target_version: u32,
    ) -> AegisResult<MigrationResult> {
        let mut to_rollback: Vec<&MigrationStep> = self
            .migrations
            .iter()
            .filter(|m| m.version > target_version && m.version <= current_version)
            .collect();

        to_rollback.sort_by_key(|m| std::cmp::Reverse(m.version));

        let mut applied = Vec::new();

        for step in &to_rollback {
            (step.down)(storage).map_err(|e| {
                AegisError::SchemaMigration(format!(
                    "rollback V{} ({}) failed: {}",
                    step.version, step.description, e
                ))
            })?;
            applied.push(format!("V{}: {} (rolled back)", step.version, step.description));
        }

        Ok(MigrationResult {
            from_version: current_version,
            to_version: target_version,
            applied_migrations: applied,
        })
    }

    /// Get all registered migration versions.
    pub fn available_versions(&self) -> Vec<u32> {
        let mut versions: Vec<u32> = self.migrations.iter().map(|m| m.version).collect();
        versions.sort();
        versions
    }
}

impl Default for MigrationRunner {
    fn default() -> Self {
        let mut runner = Self::new();
        register_default_migrations(&mut runner);
        runner
    }
}

/// Register the built-in migration steps.
///
/// V1: Initial schema version (baseline no-op — DDL is handled by `run_ddl`).
/// V2: Reserved for future schema format changes.
/// Additional migrations are added incrementally across versions.
pub fn register_default_migrations(runner: &mut MigrationRunner) {
    runner.register(
        1,
        "Initial core schema (records: _aegis_meta, _aegis_tuples, _aegis_events, _aegis_schema)",
        Box::new(|_storage| Ok(())),
        Box::new(|_storage| Ok(())),
    );
    runner.register(
        2,
        "Add valid_until column to _aegis_tuples (DDL handled by storage initialization)",
        Box::new(|_storage| Ok(())),
        Box::new(|_storage| Ok(())),
    );
}

/// Check schema compatibility between an existing and new schema.
pub fn check_compatibility(
    existing: &Schema,
    new_schema: &Schema,
) -> SchemaCompatibilityReport {
    let mut warnings = Vec::new();
    let mut breaking = Vec::new();

    // Check for removed types
    for type_name in existing.type_names() {
        if !new_schema.types.contains_key(type_name) {
            breaking.push(format!("removed type '{}'", type_name));
        }
    }

    // Check for removed relations
    for (type_name, type_def) in &existing.types {
        if let Some(new_type) = new_schema.types.get(type_name) {
            for rel_name in type_def.relations.keys() {
                if !new_type.relations.contains_key(rel_name) {
                    breaking.push(format!(
                        "removed relation '{}.{}'",
                        type_name, rel_name
                    ));
                }
            }
        }
    }

    // Check for removed permissions
    for (type_name, type_def) in &existing.types {
        if let Some(new_type) = new_schema.types.get(type_name) {
            for perm_name in type_def.permissions.keys() {
                if !new_type.permissions.contains_key(perm_name) {
                    warnings.push(format!(
                        "removed permission '{}.{}'",
                        type_name, perm_name
                    ));
                }
            }
        }
    }

    // Check for new types (warnings only)
    for type_name in new_schema.type_names() {
        if !existing.types.contains_key(type_name) {
            warnings.push(format!("new type '{}' added", type_name));
        }
    }

    // Check for new relations on existing types
    for (type_name, type_def) in &new_schema.types {
        if let Some(existing_type) = existing.types.get(type_name) {
            for rel_name in type_def.relations.keys() {
                if !existing_type.relations.contains_key(rel_name) {
                    warnings.push(format!(
                        "new relation '{}.{}' added",
                        type_name, rel_name
                    ));
                }
            }
        }
    }

    SchemaCompatibilityReport {
        compatible: breaking.is_empty(),
        warnings,
        breaking,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::schema::*;
    use std::collections::HashMap;

    fn schema_v1() -> Schema {
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
            "viewer".to_string(),
            RelationDef {
                inherit_from: vec![],
                description: None,
            },
        );
        let mut repo_perms = HashMap::new();
        repo_perms.insert(
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
                relations: repo_rels,
                permissions: repo_perms,
                ..Default::default()
            },
        );
        Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types,
        }
    }

    fn schema_v2() -> Schema {
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
                inherit_from: vec![],
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
                ..Default::default()
            },
        );
        repo_perms.insert(
            "write".to_string(),
            PermissionDef {
                union_of: vec!["editor".to_string(), "owner".to_string()],
                condition: None,
                description: None,
                ..Default::default()
            },
        );
        types.insert(
            "repo".to_string(),
            TypeDef {
                relations: repo_rels,
                permissions: repo_perms,
                ..Default::default()
            },
        );
        Schema {
            schema_version: 2,
            namespace: "test".to_string(),
            types,
        }
    }

    #[test]
    fn test_compatible_additive_change() {
        let report = check_compatibility(&schema_v1(), &schema_v2());
        assert!(report.compatible);
        assert!(report.warnings.iter().any(|w| w.contains("editor")));
    }

    #[test]
    fn test_breaking_remove_relation() {
        let report = check_compatibility(&schema_v2(), &schema_v1());
        assert!(!report.compatible);
        assert!(report.breaking.iter().any(|b| b.contains("editor")));
    }

    #[test]
    fn test_migration_runner_default() {
        let runner = MigrationRunner::new();
        assert!(runner.available_versions().is_empty());
    }

    #[test]
    fn test_migration_versions() {
        let mut runner = MigrationRunner::new();
        runner.register(
            1,
            "Initial schema",
            Box::new(|_| Ok(())),
            Box::new(|_| Ok(())),
        );
        runner.register(
            2,
            "Add editor role",
            Box::new(|_| Ok(())),
            Box::new(|_| Ok(())),
        );
        assert_eq!(runner.available_versions(), vec![1, 2]);
    }
}
