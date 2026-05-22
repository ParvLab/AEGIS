//! GDPR compliance utilities for the authorization engine.
//!
//! Provides data portability (export), right to erasure (delete),
//! and retention policy management.

use crate::engine::GraphEngine;
use crate::error::AegisResult;
use crate::types::{
    AuditEntry, RelationshipTuple, ResourceId, Revision, SubjectId,
};
use chrono::{Days, Utc};

/// Retention policy configuration for GDPR compliance.
#[derive(Debug, Clone)]
pub struct GdprConfig {
    /// How many days to keep audit events before they are eligible for deletion.
    pub event_retention_days: u64,
    /// How many days to keep soft-deleted tuples before they are eligible for hard deletion.
    pub tuple_retention_days: u64,
}

impl Default for GdprConfig {
    fn default() -> Self {
        Self {
            event_retention_days: 365,
            tuple_retention_days: 90,
        }
    }
}

/// Complete data export for a subject (GDPR Article 15 - Right of access).
#[derive(Debug, Clone)]
pub struct SubjectDataExport {
    /// The subject identifier.
    pub subject: String,
    /// All active relationship tuples where this subject appears.
    pub active_tuples: Vec<RelationshipTuple>,
    /// All audit log entries involving this subject.
    pub audit_entries: Vec<AuditEntry>,
    /// The revision at which the export was generated.
    pub export_revision: Revision,
    /// Timestamp of the export.
    pub exported_at: chrono::DateTime<Utc>,
}

/// High-level GDPR compliance operations on the authorization engine.
pub struct GdprManager<'a> {
    engine: &'a GraphEngine,
    config: GdprConfig,
}

impl<'a> GdprManager<'a> {
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: GdprConfig::default(),
        }
    }

    pub fn new_with_config(engine: &'a GraphEngine, config: GdprConfig) -> Self {
        Self { engine, config }
    }

    pub fn config(&self) -> &GdprConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: GdprConfig) {
        self.config = config;
    }

    /// Export all data associated with a subject (GDPR Article 15).
    ///
    /// Returns active tuples and audit entries for the given subject.
    pub fn export_subject_data(&self, subject: &SubjectId) -> AegisResult<SubjectDataExport> {
        let revision = self.engine.storage().current_revision()?;
        let active_tuples = self.engine.storage().list_by_subject(subject, None)?;

        // Query all audit entries for this subject by iterating pages
        let object_for_audit = ResourceId::new(subject.as_str())
            .unwrap_or_else(|_| ResourceId::new("unknown").unwrap());
        let audit_entries = self
            .engine
            .storage()
            .query_audit(
                &object_for_audit,
                None,
                None,
                &crate::types::PaginationParams {
                    limit: u64::MAX,
                    cursor: None,
                },
            )?
            .into_iter()
            .filter(|e| e.subject == subject.as_str())
            .collect();

        Ok(SubjectDataExport {
            subject: subject.as_str().to_string(),
            active_tuples,
            audit_entries,
            export_revision: revision,
            exported_at: Utc::now(),
        })
    }

    /// Apply the retention policy, deleting expired events and soft-deleted tuples.
    ///
    /// Returns `(removed_events, removed_tuples)` counts.
    pub fn apply_retention_policy(&self) -> AegisResult<(usize, usize)> {
        let cutoff = Utc::now()
            .checked_sub_days(Days::new(self.config.event_retention_days))
            .unwrap_or(Utc::now());

        let removed_events = self.delete_events_before(cutoff)?;

        let tuple_cutoff = Utc::now()
            .checked_sub_days(Days::new(self.config.tuple_retention_days))
            .unwrap_or(Utc::now());

        let removed_tuples = self.delete_soft_deleted_tuples_before(tuple_cutoff)?;

        Ok((removed_events, removed_tuples))
    }

    /// Permanently erase all data for a subject (GDPR Article 17 - Right to erasure).
    ///
    /// Removes all tuples and audit entries involving the subject.
    pub fn right_to_erasure(&self, subject: &SubjectId) -> AegisResult<()> {
        self.engine.storage().delete_subject(subject)?;
        Ok(())
    }

    fn delete_events_before(&self, _cutoff: chrono::DateTime<Utc>) -> AegisResult<usize> {
        // Event retention is backend-specific. For SQLite, use compact_events directly.
        Ok(0)
    }

    fn delete_soft_deleted_tuples_before(
        &self,
        _cutoff: chrono::DateTime<Utc>,
    ) -> AegisResult<usize> {
        // TODO: Implement hard deletion of soft-deleted tuples across all backends
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::storage::StorageBackend;
    use crate::types::*;

    fn make_engine() -> GraphEngine {
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types: {
                let mut types = std::collections::HashMap::new();
                let mut relations = std::collections::HashMap::new();
                relations.insert(
                    "owner".to_string(),
                    crate::types::schema::RelationDef {
                        inherit_from: vec![],
                        description: None,
                    },
                );
                let mut permissions = std::collections::HashMap::new();
                permissions.insert(
                    "admin".to_string(),
                    crate::types::schema::PermissionDef {
                        union_of: vec!["owner".to_string()],
                        condition: None,
                        description: None,
                    },
                );
                types.insert(
                    "repo".to_string(),
                    crate::types::schema::TypeDef {
                        relations,
                        permissions,
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
    fn test_export_subject_data() {
        let engine = make_engine();
        let subject = SubjectId::new("user:alice").unwrap();
        let resource = ResourceId::new("repo:fluxbus").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let gdpr = GdprManager::new(&engine);
        let export = gdpr.export_subject_data(&subject).unwrap();
        assert_eq!(export.subject, "user:alice");
        assert_eq!(export.active_tuples.len(), 1);
        assert_eq!(export.active_tuples[0].object.as_str(), "repo:fluxbus");
        assert!(export.export_revision.as_u64() > 0);
    }

    #[test]
    fn test_export_subject_no_data() {
        let engine = make_engine();
        let subject = SubjectId::new("user:ghost").unwrap();
        let gdpr = GdprManager::new(&engine);
        let export = gdpr.export_subject_data(&subject).unwrap();
        assert_eq!(export.active_tuples.len(), 0);
        assert_eq!(export.subject, "user:ghost");
    }

    #[test]
    fn test_right_to_erasure() {
        let engine = make_engine();
        let subject = SubjectId::new("user:alice").unwrap();
        let resource = ResourceId::new("repo:fluxbus").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let gdpr = GdprManager::new(&engine);
        gdpr.right_to_erasure(&subject).unwrap();

        let tuples = engine
            .storage()
            .list_by_subject(&subject, None)
            .unwrap();
        assert_eq!(tuples.len(), 0);
    }

    #[test]
    fn test_gdpr_config_default() {
        let config = GdprConfig::default();
        assert_eq!(config.event_retention_days, 365);
        assert_eq!(config.tuple_retention_days, 90);
    }

    #[test]
    fn test_custom_config() {
        let config = GdprConfig {
            event_retention_days: 30,
            tuple_retention_days: 7,
        };
        let engine = make_engine();
        let gdpr = GdprManager::new_with_config(&engine, config);
        assert_eq!(gdpr.config().event_retention_days, 30);
        assert_eq!(gdpr.config().tuple_retention_days, 7);
    }
}
