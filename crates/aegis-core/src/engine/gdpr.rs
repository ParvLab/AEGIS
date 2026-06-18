//! GDPR compliance utilities for the authorization engine.
//!
//! Provides data portability (export), right to erasure (delete),
//! and retention policy management.

use crate::engine::GraphEngine;
use crate::error::{AegisError, AegisResult};
use crate::types::{
    AuditEntry, ConsistencyMode, PaginationParams, PartitionId, RelationshipTuple, Revision,
    SubjectId,
};
use chrono::{DateTime, Days, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// A cryptographically signed export package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedExport {
    /// The export data (Serialized SubjectDataExport bytes)
    pub data: Vec<u8>,
    /// SHA-256 digest of `data`
    pub data_hash: String,
    /// Ed25519 signature over `data_hash`
    pub signature: Vec<u8>,
    /// Signature algorithm identifier
    pub algorithm: String,
    /// Public key identifier (fingerprint) so verifier knows which key to use
    pub key_id: String,
    /// When the export was signed
    pub signed_at: DateTime<Utc>,
}

impl SignedExport {
    /// Verify the signature on this export.
    pub fn verify(&self, public_key: &[u8]) -> Result<bool, String> {
        // Recompute hash
        let mut hasher = Sha256::new();
        hasher.update(&self.data);
        let computed_hash = hex::encode(hasher.finalize());

        if computed_hash != self.data_hash {
            return Err("data hash mismatch: export has been modified".to_string());
        }

        // Verify Ed25519 signature
        let pub_key = ed25519_dalek::VerifyingKey::from_bytes(
            public_key
                .try_into()
                .map_err(|_| "invalid public key length".to_string())?,
        )
        .map_err(|e| format!("invalid public key: {e}"))?;

        Ok(pub_key
            .verify_strict(
                computed_hash.as_bytes(),
                &ed25519_dalek::Signature::from_bytes(
                    self.signature
                        .as_slice()
                        .try_into()
                        .map_err(|_| "invalid signature length".to_string())?,
                ),
            )
            .is_ok())
    }
}

/// High-level GDPR compliance operations on the authorization engine.
pub struct GdprManager<'a> {
    engine: &'a GraphEngine,
    partition_id: PartitionId,
    config: GdprConfig,
    pub signing_key: Option<ed25519_dalek::SigningKey>,
}

impl<'a> GdprManager<'a> {
    pub fn new(engine: &'a GraphEngine, partition_id: PartitionId) -> Self {
        Self {
            engine,
            partition_id,
            config: GdprConfig::default(),
            signing_key: None,
        }
    }

    pub fn new_with_config(
        engine: &'a GraphEngine,
        partition_id: PartitionId,
        config: GdprConfig,
    ) -> Self {
        Self {
            engine,
            partition_id,
            config,
            signing_key: None,
        }
    }

    pub fn config(&self) -> &GdprConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: GdprConfig) {
        self.config = config;
    }

    /// Configure the signing key for signed exports.
    pub fn with_signing_key(mut self, key: &[u8]) -> Self {
        let bytes: [u8; 32] = key.try_into().expect("signing key must be 32 bytes");
        self.signing_key = Some(ed25519_dalek::SigningKey::from_bytes(&bytes));
        self
    }

    /// Sign an existing `SubjectDataExport` with the configured key.
    pub fn sign_export(&self, export: &SubjectDataExport) -> AegisResult<SignedExport> {
        let signing_key = self
            .signing_key
            .as_ref()
            .ok_or_else(|| AegisError::Internal("no signing key configured".to_string()))?;

        let data = serde_json::to_vec(export)
            .map_err(|e| AegisError::Internal(format!("serialization failed: {e}")))?;

        let mut hasher = Sha256::new();
        hasher.update(&data);
        let data_hash = hex::encode(hasher.finalize());

        use ed25519_dalek::Signer;
        let signature = signing_key.sign(data_hash.as_bytes());

        let verifying_key = signing_key.verifying_key();
        let mut hasher = Sha256::new();
        hasher.update(verifying_key.as_bytes());
        let key_id = hex::encode(hasher.finalize());

        Ok(SignedExport {
            data,
            data_hash,
            signature: signature.to_bytes().to_vec(),
            algorithm: "ed25519-dalek".to_string(),
            key_id,
            signed_at: Utc::now(),
        })
    }

    /// Export all data for a subject and return it in a signed package.
    pub fn export_signed_subject_data(&self, subject: &SubjectId) -> AegisResult<SignedExport> {
        let export = self.export_subject_data(subject)?;
        self.sign_export(&export)
    }

    /// Export all data associated with a subject (GDPR Article 15).
    ///
    /// Returns active tuples and audit entries for the given subject.
    pub fn export_subject_data(&self, subject: &SubjectId) -> AegisResult<SubjectDataExport> {
        let revision = self.engine.storage().current_revision(&self.partition_id)?;
        let active_tuples = self.engine.storage().list_by_subject(
            &self.partition_id,
            subject,
            None,
            &ConsistencyMode::MinimizeLatency,
        )?;

        // Query audit entries in pages to avoid OOM, filter by subject
        const PAGE_SIZE: u64 = 1000;
        let mut audit_entries = Vec::new();
        let mut cursor: Option<crate::types::PaginationCursor> = None;
        loop {
            let page = self.engine.storage().query_audit(
                &self.partition_id,
                None,
                None,
                None,
                &PaginationParams {
                    limit: PAGE_SIZE,
                    cursor,
                },
            )?;
            let count_before = audit_entries.len();
            audit_entries.extend(page.into_iter().filter(|e| e.subject == subject.as_str()));
            if audit_entries.len() - count_before < PAGE_SIZE as usize {
                break;
            }
            cursor = Some(crate::types::PaginationCursor {
                offset: audit_entries.len() as u64,
                revision: Revision::ZERO,
            });
        }

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
        self.engine
            .storage()
            .delete_subject(&self.partition_id, subject)?;
        Ok(())
    }

    fn delete_events_before(&self, cutoff: DateTime<Utc>) -> AegisResult<usize> {
        self.engine
            .storage()
            .delete_events_before(&self.partition_id, cutoff)
    }

    fn delete_soft_deleted_tuples_before(&self, cutoff: DateTime<Utc>) -> AegisResult<usize> {
        self.engine
            .storage()
            .delete_soft_deleted_tuples_before(&self.partition_id, cutoff)
    }

    /// Compact the audit log by removing pair-matched add/remove entries.
    ///
    /// Removes event pairs where a tuple was added and later removed with
    /// no intermediate add for the same key — these are semantically no-ops
    /// and safe to delete.
    pub fn compact_events(&self) -> AegisResult<usize> {
        self.engine.storage().compact_events(&self.partition_id)
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use crate::storage::StorageBackend;
    #[cfg(feature = "sqlite")]
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::types::*;

    fn make_engine_and_partition() -> (GraphEngine, PartitionId) {
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
                relations.insert(
                    "editor".to_string(),
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
                        ..Default::default()
                    },
                );
                types.insert(
                    "repo".to_string(),
                    crate::types::schema::TypeDef {
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
        let partition_id = PartitionId::default();
        (engine, partition_id)
    }

    #[test]
    fn test_export_subject_data() {
        let (engine, partition_id) = make_engine_and_partition();
        let subject = SubjectId::new("user:alice").unwrap();
        let resource = ResourceId::new("repo:fluxbus").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let gdpr = GdprManager::new(&engine, partition_id.clone());
        let export = gdpr.export_subject_data(&subject).unwrap();
        assert_eq!(export.subject, "user:alice");
        assert_eq!(export.active_tuples.len(), 1);
        assert_eq!(export.active_tuples[0].object.as_str(), "repo:fluxbus");
        assert!(export.export_revision.as_u64() > 0);
    }

    #[test]
    fn test_export_subject_no_data() {
        let (engine, partition_id) = make_engine_and_partition();
        let subject = SubjectId::new("user:ghost").unwrap();
        let gdpr = GdprManager::new(&engine, partition_id.clone());
        let export = gdpr.export_subject_data(&subject).unwrap();
        assert_eq!(export.active_tuples.len(), 0);
        assert_eq!(export.subject, "user:ghost");
    }

    #[test]
    fn test_right_to_erasure() {
        let (engine, partition_id) = make_engine_and_partition();
        let subject = SubjectId::new("user:alice").unwrap();
        let resource = ResourceId::new("repo:fluxbus").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let gdpr = GdprManager::new(&engine, partition_id.clone());
        gdpr.right_to_erasure(&subject).unwrap();

        let tuples = engine
            .storage()
            .list_by_subject(
                &partition_id,
                &subject,
                None,
                &ConsistencyMode::MinimizeLatency,
            )
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
        let (engine, partition_id) = make_engine_and_partition();
        let gdpr = GdprManager::new_with_config(&engine, partition_id.clone(), config);
        assert_eq!(gdpr.config().event_retention_days, 30);
        assert_eq!(gdpr.config().tuple_retention_days, 7);
    }

    #[test]
    fn test_gdpr_e2e_transfer_ownership() {
        let (engine, partition_id) = make_engine_and_partition();
        let alice = SubjectId::new("user:alice").unwrap();
        let bob = SubjectId::new("user:bob").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        // Write tuples for alice
        engine
            .write(&RelationshipTuple::new(
                alice.clone(),
                Relation::new("owner").unwrap(),
                repo.clone(),
            ))
            .unwrap();

        let gdpr = GdprManager::new(&engine, partition_id.clone());

        // Export before transfer — alice has 1 tuple
        let export_before = gdpr.export_subject_data(&alice).unwrap();
        assert_eq!(export_before.active_tuples.len(), 1);

        // Transfer ownership from alice to bob
        let result = engine
            .delete_subject_with_policy(&alice, "transfer", Some(&bob))
            .unwrap();
        assert!(result.revision.as_u64() > 0);

        // Export after transfer — alice has 0 tuples
        let export_alice = gdpr.export_subject_data(&alice).unwrap();
        assert_eq!(export_alice.active_tuples.len(), 0);

        // Bob now has the tuple
        let bob_tuples = engine
            .storage()
            .list_by_subject(&partition_id, &bob, None, &ConsistencyMode::MinimizeLatency)
            .unwrap();
        assert_eq!(bob_tuples.len(), 1);
        assert_eq!(bob_tuples[0].object.as_str(), "repo:fluxbus");
        assert_eq!(bob_tuples[0].relation.as_str(), "owner");
    }

    #[test]
    fn test_gdpr_e2e_erase_and_export() {
        let (engine, partition_id) = make_engine_and_partition();
        let subject = SubjectId::new("user:alice").unwrap();
        let repo_a = ResourceId::new("repo:a").unwrap();
        let repo_b = ResourceId::new("repo:b").unwrap();

        // Write multiple tuples
        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                repo_a.clone(),
            ))
            .unwrap();
        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("editor").unwrap(),
                repo_b.clone(),
            ))
            .unwrap();

        let gdpr = GdprManager::new(&engine, partition_id.clone());

        // Export has 2 tuples
        let export = gdpr.export_subject_data(&subject).unwrap();
        assert_eq!(export.active_tuples.len(), 2);

        // Right to erasure (cascade delete)
        gdpr.right_to_erasure(&subject).unwrap();
        let after = gdpr.export_subject_data(&subject).unwrap();
        assert_eq!(after.active_tuples.len(), 0);

        // Erase again on empty subject is a no-op
        gdpr.right_to_erasure(&subject).unwrap();
    }

    #[test]
    fn test_gdpr_e2e_compact_events() {
        let (engine, partition_id) = make_engine_and_partition();
        let subject = SubjectId::new("user:alice").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();
        let key = crate::types::TupleKey {
            subject: subject.clone(),
            relation: Relation::new("owner").unwrap(),
            object: repo.clone(),
        };

        // Write then delete — creates an add/remove pair
        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                repo.clone(),
            ))
            .unwrap();
        engine.delete(&key).unwrap();

        let gdpr = GdprManager::new(&engine, partition_id.clone());

        // Export shows 0 active tuples (add+remove cancel out for state)
        let export = gdpr.export_subject_data(&subject).unwrap();
        assert_eq!(export.active_tuples.len(), 0);

        // Audit events exist before compaction
        assert!(export.audit_entries.len() >= 2);

        // Compact the event log
        let removed = gdpr.compact_events().unwrap();
        assert!(removed > 0, "should have removed paired add/remove events");

        // After compaction, export still shows 0 active tuples
        let after = gdpr.export_subject_data(&subject).unwrap();
        assert_eq!(after.active_tuples.len(), 0);
    }

    #[test]
    fn test_gdpr_e2e_cascade_policy() {
        let (engine, partition_id) = make_engine_and_partition();
        let subject = SubjectId::new("user:alice").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                repo.clone(),
            ))
            .unwrap();

        // Cascade delete via delete_subject_with_policy
        let result = engine
            .delete_subject_with_policy(&subject, "cascade", None)
            .unwrap();
        assert!(result.revision.as_u64() > 0);

        let tuples = engine
            .storage()
            .list_by_subject(
                &partition_id,
                &subject,
                None,
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(tuples.len(), 0);
    }

    #[test]
    fn test_gdpr_e2e_fail_policy() {
        let (engine, _partition_id) = make_engine_and_partition();
        let subject = SubjectId::new("user:alice").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        // No tuples — fail policy should succeed (no-op)
        let result = engine
            .delete_subject_with_policy(&subject, "fail", None)
            .unwrap();
        // revision is 0 because no tuples were ever written
        assert_eq!(result.revision.as_u64(), 0);

        // Write a tuple
        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                repo.clone(),
            ))
            .unwrap();

        // Fail policy should error because tuples exist
        let err = engine
            .delete_subject_with_policy(&subject, "fail", None)
            .unwrap_err();
        assert!(
            matches!(err, crate::error::AegisError::OperationNotPermitted(_)),
            "expected OperationNotPermitted, got {:?}",
            err
        );
    }
}
