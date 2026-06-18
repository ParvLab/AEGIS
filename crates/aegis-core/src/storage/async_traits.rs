use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::{AegisError, AegisResult};
use crate::storage::memory::InMemoryStorage;
use crate::storage::traits::{
    BackendType, IntegrityReport, StorageBackend, StorageMeta, TupleFilter,
};
use crate::engine::enforcement_history::EnforcementEvent;
use crate::engine::policy_lifecycle::PolicyDraft;
use crate::engine::scheduler::{AnalysisRun, AnalysisSchedule};
use crate::types::{
    AuditEntry, ConsistencyMode, PaginatedTuples, PaginationParams, PartitionId, Relation,
    RelationshipTuple, ResourceId, Revision, RevisionToken, SubjectId, TupleKey, TupleMutation,
};

/// Describes the capabilities of a storage backend.
/// Clients can query this to determine what operations are supported.
#[derive(Debug, Clone)]
pub struct StorageCapabilities {
    pub persistent: bool,
    pub transactional: bool,
    pub audit_supported: bool,
    pub backup_supported: bool,
    pub export_import_supported: bool,
    pub async_only: bool,
    pub backend_type: BackendType,
}

impl StorageCapabilities {
    pub fn sqlite() -> Self {
        Self {
            persistent: true,
            transactional: true,
            audit_supported: true,
            backup_supported: true,
            export_import_supported: true,
            async_only: false,
            backend_type: BackendType::Sqlite,
        }
    }

    pub fn in_memory() -> Self {
        Self {
            persistent: false,
            transactional: true,
            audit_supported: true,
            backup_supported: true,
            export_import_supported: true,
            async_only: false,
            backend_type: BackendType::InMemory,
        }
    }

    pub fn indexeddb() -> Self {
        Self {
            persistent: true,
            transactional: true,
            audit_supported: true,
            backup_supported: true,
            export_import_supported: true,
            async_only: true,
            backend_type: BackendType::IndexedDB,
        }
    }
}

/// Async storage transaction supporting atomic multi-tuple writes within a partition.
#[async_trait(?Send)]
pub trait AsyncStorageTransaction: Send {
    async fn write(&mut self, partition_id: &PartitionId, tuple: &RelationshipTuple)
        -> AegisResult<()>;

    async fn delete(&mut self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<()>;

    async fn savepoint(&self, name: &str) -> AegisResult<()>;

    async fn rollback_to_savepoint(&self, name: &str) -> AegisResult<()>;

    async fn release_savepoint(&self, name: &str) -> AegisResult<()>;

    async fn set_actor_identity(
        &mut self,
        identity: Option<String>,
    ) -> Option<String> {
        let _ = identity;
        None
    }

    async fn commit(self: Box<Self>) -> AegisResult<Revision>;

    async fn rollback(self: Box<Self>) -> AegisResult<()>;
}

/// Async storage backend for relationship tuples with partition awareness.
///
/// Mirrors `StorageBackend` with async methods. Designed for browser/edge
/// backends (IndexedDB, D1, KV) where all I/O is inherently async.
#[async_trait(?Send)]
pub trait AsyncStorageBackend: Send + Sync {
    fn capabilities(&self) -> StorageCapabilities;

    async fn initialize(&mut self) -> AegisResult<StorageMeta>;

    async fn write_tuple(
        &self,
        partition_id: &PartitionId,
        tuple: &RelationshipTuple,
    ) -> AegisResult<Revision>;

    async fn write_tuples_batch(
        &self,
        partition_id: &PartitionId,
        tuples: &[RelationshipTuple],
    ) -> AegisResult<Revision>;

    async fn delete_tuple(
        &self,
        partition_id: &PartitionId,
        key: &TupleKey,
    ) -> AegisResult<Revision>;

    async fn delete_subject(
        &self,
        partition_id: &PartitionId,
        subject: &SubjectId,
    ) -> AegisResult<Revision>;

    async fn delete_object(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
    ) -> AegisResult<Revision>;

    async fn has_tuple(
        &self,
        partition_id: &PartitionId,
        key: &TupleKey,
    ) -> AegisResult<bool>;

    async fn read_tuple(
        &self,
        partition_id: &PartitionId,
        key: &TupleKey,
    ) -> AegisResult<Option<RelationshipTuple>>;

    async fn list_by_object(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: Option<&Relation>,
        consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>>;

    async fn list_by_subject(
        &self,
        partition_id: &PartitionId,
        subject: &SubjectId,
        relation: Option<&Relation>,
        consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>>;

    async fn list_by_subject_set_of(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: Option<&Relation>,
        consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let prefix = format!("{}#", object.as_str());
        let all = self
            .query_tuples(
                partition_id,
                &TupleFilter {
                    subject_type: Some(prefix),
                    relation: relation.cloned(),
                    ..Default::default()
                },
                &PaginationParams {
                    cursor: None,
                    limit: 10_000,
                },
                consistency,
            )
            .await?;
        Ok(all.tuples)
    }

    async fn list_by_relation(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: &Relation,
    ) -> AegisResult<Vec<RelationshipTuple>>;

    async fn query_tuples(
        &self,
        partition_id: &PartitionId,
        filter: &TupleFilter,
        pagination: &PaginationParams,
        consistency: &ConsistencyMode,
    ) -> AegisResult<PaginatedTuples>;

    async fn current_revision(
        &self,
        partition_id: &PartitionId,
    ) -> AegisResult<Revision>;

    async fn read_schema_version(&self) -> AegisResult<u32>;

    async fn write_schema_version(&self, version: u32) -> AegisResult<()>;

    async fn current_token(&self) -> AegisResult<RevisionToken>;

    async fn begin_transaction(
        &self,
        partition_id: &PartitionId,
    ) -> AegisResult<Box<dyn AsyncStorageTransaction>>;

    async fn query_audit(
        &self,
        partition_id: &PartitionId,
        object: Option<&ResourceId>,
        from_revision: Option<Revision>,
        to_revision: Option<Revision>,
        pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>>;

    async fn integrity_check(&self) -> AegisResult<IntegrityReport>;

    async fn delete_events_before(
        &self,
        partition_id: &PartitionId,
        cutoff: DateTime<Utc>,
    ) -> AegisResult<usize>;

    async fn compact_events(
        &self,
        partition_id: &PartitionId,
    ) -> AegisResult<usize>;

    async fn delete_soft_deleted_tuples_before(
        &self,
        partition_id: &PartitionId,
        cutoff: DateTime<Utc>,
    ) -> AegisResult<usize>;

    async fn recover_from_events(
        &self,
        partition_id: &PartitionId,
        to_revision: Option<Revision>,
    ) -> AegisResult<Revision>;

    async fn restore_backup(
        &self,
        partition_id: &PartitionId,
        tuples: &[RelationshipTuple],
        events: &[AuditEntry],
        revision: Revision,
    ) -> AegisResult<()>;

    fn storage_version(&self) -> Option<String> {
        None
    }

    async fn set_actor_identity(
        &self,
        identity: Option<String>,
    ) -> Option<String> {
        let _ = identity;
        None
    }

    async fn close(&self) -> AegisResult<()>;

    async fn verify_audit_chain(
        &self,
        partition_id: &PartitionId,
    ) -> AegisResult<Option<String>> {
        let _ = partition_id;
        Ok(None)
    }

    async fn save_policy_draft(&self, _draft: &PolicyDraft) -> AegisResult<()> {
        Err(AegisError::UnsupportedStorageOperation("async save_policy_draft not supported".into()))
    }
    async fn load_policy_draft(&self, _id: &str) -> AegisResult<Option<PolicyDraft>> {
        Err(AegisError::UnsupportedStorageOperation("async load_policy_draft not supported".into()))
    }
    async fn delete_policy_draft(&self, _id: &str) -> AegisResult<bool> {
        Err(AegisError::UnsupportedStorageOperation("async delete_policy_draft not supported".into()))
    }
    async fn save_analysis_schedule(&self, _schedule: &AnalysisSchedule) -> AegisResult<()> {
        Err(AegisError::UnsupportedStorageOperation("async save_analysis_schedule not supported".into()))
    }
    async fn delete_analysis_schedule(&self, _id: &str) -> AegisResult<bool> {
        Err(AegisError::UnsupportedStorageOperation("async delete_analysis_schedule not supported".into()))
    }
    async fn save_analysis_run(&self, _run: &AnalysisRun) -> AegisResult<()> {
        Err(AegisError::UnsupportedStorageOperation("async save_analysis_run not supported".into()))
    }
    async fn save_enforcement_event(&self, _event: &EnforcementEvent) -> AegisResult<()> {
        Err(AegisError::UnsupportedStorageOperation("async save_enforcement_event not supported".into()))
    }
}

/// In-memory async storage backend for testing and non-persistent use.
/// Wraps `InMemoryStorage` to provide async-compatible methods.
pub struct InMemoryAsyncStorage {
    storage: Arc<Mutex<InMemoryStorage>>,
}

impl InMemoryAsyncStorage {
    pub fn new() -> Self {
        let mut inner = InMemoryStorage::new();
        let _ = inner.initialize();
        Self {
            storage: Arc::new(Mutex::new(inner)),
        }
    }
}

fn lock_storage(storage: &Arc<Mutex<InMemoryStorage>>) -> AegisResult<std::sync::MutexGuard<'_, InMemoryStorage>> {
    storage.lock().map_err(|e| crate::error::AegisError::Internal(e.to_string()))
}

#[async_trait(?Send)]
impl AsyncStorageBackend for InMemoryAsyncStorage {
    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::in_memory()
    }

    async fn initialize(&mut self) -> AegisResult<StorageMeta> {
        lock_storage(&self.storage)?.initialize()
    }

    async fn write_tuple(
        &self,
        partition_id: &PartitionId,
        tuple: &RelationshipTuple,
    ) -> AegisResult<Revision> {
        lock_storage(&self.storage)?.write_tuple(partition_id, tuple)
    }

    async fn write_tuples_batch(
        &self,
        partition_id: &PartitionId,
        tuples: &[RelationshipTuple],
    ) -> AegisResult<Revision> {
        lock_storage(&self.storage)?.write_tuples_batch(partition_id, tuples)
    }

    async fn delete_tuple(
        &self,
        partition_id: &PartitionId,
        key: &TupleKey,
    ) -> AegisResult<Revision> {
        lock_storage(&self.storage)?.delete_tuple(partition_id, key)
    }

    async fn delete_subject(
        &self,
        partition_id: &PartitionId,
        subject: &SubjectId,
    ) -> AegisResult<Revision> {
        lock_storage(&self.storage)?.delete_subject(partition_id, subject)
    }

    async fn delete_object(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
    ) -> AegisResult<Revision> {
        lock_storage(&self.storage)?.delete_object(partition_id, object)
    }

    async fn has_tuple(
        &self,
        partition_id: &PartitionId,
        key: &TupleKey,
    ) -> AegisResult<bool> {
        lock_storage(&self.storage)?.has_tuple(partition_id, key)
    }

    async fn read_tuple(
        &self,
        partition_id: &PartitionId,
        key: &TupleKey,
    ) -> AegisResult<Option<RelationshipTuple>> {
        lock_storage(&self.storage)?.read_tuple(partition_id, key)
    }

    async fn list_by_object(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: Option<&Relation>,
        consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        lock_storage(&self.storage)?
            .list_by_object(partition_id, object, relation, consistency)
    }

    async fn list_by_subject(
        &self,
        partition_id: &PartitionId,
        subject: &SubjectId,
        relation: Option<&Relation>,
        consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        lock_storage(&self.storage)?
            .list_by_subject(partition_id, subject, relation, consistency)
    }

    async fn list_by_relation(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: &Relation,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        lock_storage(&self.storage)?
            .list_by_relation(partition_id, object, relation)
    }

    async fn query_tuples(
        &self,
        partition_id: &PartitionId,
        filter: &TupleFilter,
        pagination: &PaginationParams,
        consistency: &ConsistencyMode,
    ) -> AegisResult<PaginatedTuples> {
        lock_storage(&self.storage)?
            .query_tuples(partition_id, filter, pagination, consistency)
    }

    async fn current_revision(
        &self,
        partition_id: &PartitionId,
    ) -> AegisResult<Revision> {
        lock_storage(&self.storage)?.current_revision(partition_id)
    }

    async fn read_schema_version(&self) -> AegisResult<u32> {
        lock_storage(&self.storage)?.read_schema_version()
    }

    async fn write_schema_version(&self, version: u32) -> AegisResult<()> {
        lock_storage(&self.storage)?.write_schema_version(version)
    }

    async fn current_token(&self) -> AegisResult<RevisionToken> {
        lock_storage(&self.storage)?.current_token()
    }

    async fn begin_transaction(
        &self,
        _partition_id: &PartitionId,
    ) -> AegisResult<Box<dyn AsyncStorageTransaction>> {
        Ok(Box::new(InMemoryAsyncTransaction {
            storage: Arc::clone(&self.storage),
            pending: Vec::new(),
        }))
    }

    async fn query_audit(
        &self,
        partition_id: &PartitionId,
        object: Option<&ResourceId>,
        from_revision: Option<Revision>,
        to_revision: Option<Revision>,
        pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>> {
        lock_storage(&self.storage)?
            .query_audit(partition_id, object, from_revision, to_revision, pagination)
    }

    async fn integrity_check(&self) -> AegisResult<IntegrityReport> {
        lock_storage(&self.storage)?.integrity_check()
    }

    async fn delete_events_before(
        &self,
        partition_id: &PartitionId,
        cutoff: DateTime<Utc>,
    ) -> AegisResult<usize> {
        lock_storage(&self.storage)?.delete_events_before(partition_id, cutoff)
    }

    async fn compact_events(
        &self,
        partition_id: &PartitionId,
    ) -> AegisResult<usize> {
        lock_storage(&self.storage)?.compact_events(partition_id)
    }

    async fn delete_soft_deleted_tuples_before(
        &self,
        partition_id: &PartitionId,
        cutoff: DateTime<Utc>,
    ) -> AegisResult<usize> {
        lock_storage(&self.storage)?
            .delete_soft_deleted_tuples_before(partition_id, cutoff)
    }

    async fn recover_from_events(
        &self,
        partition_id: &PartitionId,
        to_revision: Option<Revision>,
    ) -> AegisResult<Revision> {
        lock_storage(&self.storage)?.recover_from_events(partition_id, to_revision)
    }

    async fn restore_backup(
        &self,
        partition_id: &PartitionId,
        tuples: &[RelationshipTuple],
        events: &[AuditEntry],
        revision: Revision,
    ) -> AegisResult<()> {
        lock_storage(&self.storage)?
            .restore_backup(partition_id, tuples, events, revision)
    }

    async fn set_actor_identity(
        &self,
        identity: Option<String>,
    ) -> Option<String> {
        lock_storage(&self.storage).ok()?.set_actor_identity(identity)
    }

    async fn close(&self) -> AegisResult<()> {
        lock_storage(&self.storage)?.close()
    }

    async fn verify_audit_chain(
        &self,
        partition_id: &PartitionId,
    ) -> AegisResult<Option<String>> {
        lock_storage(&self.storage)?.verify_audit_chain(partition_id)
    }

    async fn save_policy_draft(&self, draft: &PolicyDraft) -> AegisResult<()> {
        lock_storage(&self.storage)?.save_policy_draft(draft)
    }
    async fn load_policy_draft(&self, id: &str) -> AegisResult<Option<PolicyDraft>> {
        lock_storage(&self.storage)?.load_policy_draft(id)
    }
    async fn delete_policy_draft(&self, id: &str) -> AegisResult<bool> {
        lock_storage(&self.storage)?.delete_policy_draft(id)
    }
    async fn save_analysis_schedule(&self, schedule: &AnalysisSchedule) -> AegisResult<()> {
        lock_storage(&self.storage)?.save_analysis_schedule(schedule)
    }
    async fn delete_analysis_schedule(&self, id: &str) -> AegisResult<bool> {
        lock_storage(&self.storage)?.delete_analysis_schedule(id)
    }
    async fn save_analysis_run(&self, run: &AnalysisRun) -> AegisResult<()> {
        lock_storage(&self.storage)?.save_analysis_run(run)
    }
    async fn save_enforcement_event(&self, event: &EnforcementEvent) -> AegisResult<()> {
        lock_storage(&self.storage)?.save_enforcement_event(event)
    }
}

struct InMemoryAsyncTransaction {
    storage: Arc<Mutex<InMemoryStorage>>,
    pending: Vec<(PartitionId, TupleMutation, RelationshipTuple)>,
}

#[async_trait(?Send)]
impl AsyncStorageTransaction for InMemoryAsyncTransaction {
    async fn write(
        &mut self,
        partition_id: &PartitionId,
        tuple: &RelationshipTuple,
    ) -> AegisResult<()> {
        self.pending
            .push((partition_id.clone(), TupleMutation::Add, tuple.clone()));
        Ok(())
    }

    async fn delete(
        &mut self,
        partition_id: &PartitionId,
        key: &TupleKey,
    ) -> AegisResult<()> {
        let tuple = RelationshipTuple::new(
            key.subject.clone(),
            key.relation.clone(),
            key.object.clone(),
        );
        self.pending
            .push((partition_id.clone(), TupleMutation::Remove, tuple));
        Ok(())
    }

    async fn savepoint(&self, _name: &str) -> AegisResult<()> {
        Ok(())
    }

    async fn rollback_to_savepoint(&self, _name: &str) -> AegisResult<()> {
        Ok(())
    }

    async fn release_savepoint(&self, _name: &str) -> AegisResult<()> {
        Ok(())
    }

    async fn commit(self: Box<Self>) -> AegisResult<Revision> {
        let storage = lock_storage(&self.storage)?;
        let mut revision = Revision::ZERO;
        for (partition_id, action, tuple) in &self.pending {
            match action {
                TupleMutation::Add => {
                    revision = storage.write_tuple(partition_id, tuple)?;
                }
                TupleMutation::Remove => {
                    let key = TupleKey {
                        subject: tuple.subject.clone(),
                        relation: tuple.relation.clone(),
                        object: tuple.object.clone(),
                    };
                    revision = storage.delete_tuple(partition_id, &key)?;
                }
            }
        }
        Ok(revision)
    }

    async fn rollback(self: Box<Self>) -> AegisResult<()> {
        Ok(())
    }
}
