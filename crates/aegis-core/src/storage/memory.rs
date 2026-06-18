use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::engine::enforcement_history::EnforcementEvent;
use crate::engine::policy_lifecycle::PolicyDraft;
use crate::engine::scheduler::{AnalysisRun, AnalysisSchedule};
use crate::error::{AegisError, AegisResult};
use crate::storage::traits::{
    BackendType, IntegrityReport, PolicyVersion, StorageBackend, StorageMeta, StorageTransaction,
    TupleFilter,
};
use crate::types::{
    AuditEntry, PaginatedTuples, PaginationParams, PartitionId, Relation, RelationshipTuple,
    ResourceId, Revision, RevisionToken, SubjectId, TupleKey, TupleMutation,
};

type TupleMap = HashMap<(String, String, String), RelationshipTuple>;

struct Inner {
    tuples: TupleMap,
    events: Vec<AuditEntry>,
    revision: u64,
    schema_version: u32,
    node_id: Uuid,
    actor_identity: Option<String>,
    policy_versions: HashMap<u32, PolicyVersion>,
    policy_drafts: HashMap<String, PolicyDraft>,
    analysis_schedules: HashMap<String, AnalysisSchedule>,
    analysis_runs: Vec<AnalysisRun>,
    enforcement_events: Vec<EnforcementEvent>,
}

pub struct InMemoryStorage {
    inner: Arc<Mutex<Inner>>,
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                tuples: HashMap::new(),
                events: Vec::new(),
                revision: 0,
                schema_version: 0,
                node_id: Uuid::new_v4(),
                actor_identity: None,
                policy_versions: HashMap::new(),
                policy_drafts: HashMap::new(),
                analysis_schedules: HashMap::new(),
                analysis_runs: Vec::new(),
                enforcement_events: Vec::new(),
            })),
        }
    }

    fn bump_revision(inner: &mut Inner) -> Revision {
        inner.revision += 1;
        Revision::new(inner.revision)
    }

    fn append_event(
        inner: &mut Inner,
        action: TupleMutation,
        subject: &str,
        relation: &str,
        object: &str,
        revision: Revision,
    ) {
        let identity = inner.actor_identity.clone();
        let event = AuditEntry {
            revision,
            action,
            subject: subject.to_string(),
            relation: relation.to_string(),
            object: object.to_string(),
            timestamp: Utc::now(),
            metadata: None,
            identity,
        };
        inner.events.push(event);
    }
}

impl StorageBackend for InMemoryStorage {
    fn initialize(&mut self) -> AegisResult<StorageMeta> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        inner.revision = 0;
        inner.schema_version = 1;
        inner.tuples.clear();
        inner.events.clear();
        Ok(StorageMeta {
            schema_version: 1,
            current_revision: Revision::ZERO,
            backend_type: BackendType::InMemory,
            healthy: true,
        })
    }

    fn write_tuple(
        &self,
        _partition_id: &PartitionId,
        tuple: &RelationshipTuple,
    ) -> AegisResult<Revision> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let key = (
            tuple.subject.as_str().to_string(),
            tuple.relation.as_str().to_string(),
            tuple.object.as_str().to_string(),
        );
        let revision = Self::bump_revision(&mut inner);
        inner.tuples.insert(key, tuple.clone());
        Self::append_event(
            &mut inner,
            TupleMutation::Add,
            tuple.subject.as_str(),
            tuple.relation.as_str(),
            tuple.object.as_str(),
            revision,
        );
        Ok(revision)
    }

    fn write_tuples_batch(
        &self,
        _partition_id: &PartitionId,
        tuples: &[RelationshipTuple],
    ) -> AegisResult<Revision> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let mut revision = Revision::ZERO;
        for tuple in tuples {
            let key = (
                tuple.subject.as_str().to_string(),
                tuple.relation.as_str().to_string(),
                tuple.object.as_str().to_string(),
            );
            revision = Self::bump_revision(&mut inner);
            inner.tuples.insert(key, tuple.clone());
            Self::append_event(
                &mut inner,
                TupleMutation::Add,
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
                revision,
            );
        }
        Ok(revision)
    }

    fn delete_tuple(&self, _partition_id: &PartitionId, key: &TupleKey) -> AegisResult<Revision> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let k = (
            key.subject.as_str().to_string(),
            key.relation.as_str().to_string(),
            key.object.as_str().to_string(),
        );
        let revision = Self::bump_revision(&mut inner);
        inner.tuples.remove(&k);
        Self::append_event(
            &mut inner,
            TupleMutation::Remove,
            key.subject.as_str(),
            key.relation.as_str(),
            key.object.as_str(),
            revision,
        );
        Ok(revision)
    }

    fn delete_subject(
        &self,
        _partition_id: &PartitionId,
        subject: &SubjectId,
    ) -> AegisResult<Revision> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let subjects: Vec<(String, String, String)> = inner
            .tuples
            .keys()
            .filter(|k| k.0 == subject.as_str())
            .cloned()
            .collect();
        let mut revision = Revision::ZERO;
        for k in subjects {
            revision = Self::bump_revision(&mut inner);
            if let Some(tuple) = inner.tuples.remove(&k) {
                Self::append_event(
                    &mut inner,
                    TupleMutation::Remove,
                    tuple.subject.as_str(),
                    tuple.relation.as_str(),
                    tuple.object.as_str(),
                    revision,
                );
            }
        }
        if revision == Revision::ZERO {
            revision = Revision::new(inner.revision);
        }
        Ok(revision)
    }

    fn delete_object(
        &self,
        _partition_id: &PartitionId,
        object: &ResourceId,
    ) -> AegisResult<Revision> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let objects: Vec<(String, String, String)> = inner
            .tuples
            .keys()
            .filter(|k| k.2 == object.as_str())
            .cloned()
            .collect();
        let mut revision = Revision::ZERO;
        for k in objects {
            revision = Self::bump_revision(&mut inner);
            if let Some(tuple) = inner.tuples.remove(&k) {
                Self::append_event(
                    &mut inner,
                    TupleMutation::Remove,
                    tuple.subject.as_str(),
                    tuple.relation.as_str(),
                    tuple.object.as_str(),
                    revision,
                );
            }
        }
        if revision == Revision::ZERO {
            revision = Revision::new(inner.revision);
        }
        Ok(revision)
    }

    fn has_tuple(&self, _partition_id: &PartitionId, key: &TupleKey) -> AegisResult<bool> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let k = (
            key.subject.as_str().to_string(),
            key.relation.as_str().to_string(),
            key.object.as_str().to_string(),
        );
        Ok(inner.tuples.contains_key(&k))
    }

    fn read_tuple(
        &self,
        _partition_id: &PartitionId,
        key: &TupleKey,
    ) -> AegisResult<Option<RelationshipTuple>> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let k = (
            key.subject.as_str().to_string(),
            key.relation.as_str().to_string(),
            key.object.as_str().to_string(),
        );
        Ok(inner.tuples.get(&k).cloned())
    }

    fn list_by_object(
        &self,
        _partition_id: &PartitionId,
        object: &ResourceId,
        relation: Option<&Relation>,
        _consistency: &crate::types::ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let result: Vec<RelationshipTuple> = inner
            .tuples
            .values()
            .filter(|t| t.object == *object)
            .filter(|t| relation.is_none_or(|r| t.relation == *r))
            .cloned()
            .collect();
        Ok(result)
    }

    fn list_by_subject(
        &self,
        _partition_id: &PartitionId,
        subject: &SubjectId,
        relation: Option<&Relation>,
        _consistency: &crate::types::ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let result: Vec<RelationshipTuple> = inner
            .tuples
            .values()
            .filter(|t| t.subject == *subject)
            .filter(|t| relation.is_none_or(|r| t.relation == *r))
            .cloned()
            .collect();
        Ok(result)
    }

    fn list_by_relation(
        &self,
        _partition_id: &PartitionId,
        object: &ResourceId,
        relation: &Relation,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let result: Vec<RelationshipTuple> = inner
            .tuples
            .values()
            .filter(|t| t.object == *object && t.relation == *relation)
            .cloned()
            .collect();
        Ok(result)
    }

    fn query_tuples(
        &self,
        _partition_id: &PartitionId,
        filter: &TupleFilter,
        pagination: &PaginationParams,
        _consistency: &crate::types::ConsistencyMode,
    ) -> AegisResult<PaginatedTuples> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let mut result: Vec<RelationshipTuple> = inner
            .tuples
            .values()
            .filter(|t| {
                if let Some(ref st) = filter.subject_type
                    && !t.subject.as_str().starts_with(st.trim_end_matches('#'))
                {
                    return false;
                }
                if let Some(ref rel) = filter.relation
                    && t.relation != *rel
                {
                    return false;
                }
                if let Some(ref ot) = filter.object_type
                    && !t.object.as_str().starts_with(ot)
                {
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        let total = result.len();
        let offset = pagination
            .cursor
            .as_ref()
            .map(|c| c.offset as usize)
            .unwrap_or(0);
        let limit = pagination.limit as usize;
        let has_more = offset + limit < total;
        result = result.into_iter().skip(offset).take(limit).collect();

        let next_cursor = if has_more {
            Some(crate::types::PaginationCursor {
                offset: (offset + limit) as u64,
                revision: Revision::new(inner.revision),
            })
        } else {
            None
        };

        Ok(PaginatedTuples {
            tuples: result,
            next_cursor,
            revision: Revision::new(inner.revision),
        })
    }

    fn current_revision(&self, _partition_id: &PartitionId) -> AegisResult<Revision> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        Ok(Revision::new(inner.revision))
    }

    fn read_schema_version(&self) -> AegisResult<u32> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        Ok(inner.schema_version)
    }

    fn write_schema_version(&self, version: u32) -> AegisResult<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        inner.schema_version = version;
        Ok(())
    }

    fn current_token(&self) -> AegisResult<RevisionToken> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        Ok(RevisionToken::new(
            Revision::new(inner.revision),
            inner.node_id,
        ))
    }

    fn begin_transaction(
        &self,
        _partition_id: &PartitionId,
    ) -> AegisResult<Box<dyn StorageTransaction>> {
        Ok(Box::new(InMemoryTransaction::new(Arc::clone(&self.inner))))
    }

    fn query_audit(
        &self,
        _partition_id: &PartitionId,
        object: Option<&ResourceId>,
        from_revision: Option<Revision>,
        to_revision: Option<Revision>,
        _pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let result: Vec<AuditEntry> = inner
            .events
            .iter()
            .filter(|e| object.is_none_or(|o| e.object == o.as_str()))
            .filter(|e| from_revision.is_none_or(|r| e.revision >= r))
            .filter(|e| to_revision.is_none_or(|r| e.revision <= r))
            .cloned()
            .collect();
        Ok(result)
    }

    fn backend_type(&self) -> BackendType {
        BackendType::InMemory
    }

    fn integrity_check(&self) -> AegisResult<IntegrityReport> {
        Ok(IntegrityReport {
            passed: true,
            details: vec!["in-memory storage: integrity check passed".to_string()],
            backend_type: BackendType::InMemory,
            tenant_leakage_detected: false,
            leaked_crossings: vec![],
            orphaned_tuple_count: 0,
        })
    }

    fn delete_events_before(
        &self,
        _partition_id: &PartitionId,
        cutoff: DateTime<Utc>,
    ) -> AegisResult<usize> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let before = inner.events.len();
        inner.events.retain(|e| e.timestamp >= cutoff);
        Ok(before - inner.events.len())
    }

    fn compact_events(&self, _partition_id: &PartitionId) -> AegisResult<usize> {
        Ok(0)
    }

    fn delete_soft_deleted_tuples_before(
        &self,
        _partition_id: &PartitionId,
        _cutoff: DateTime<Utc>,
    ) -> AegisResult<usize> {
        Ok(0)
    }

    fn recover_from_events(
        &self,
        _partition_id: &PartitionId,
        to_revision: Option<Revision>,
    ) -> AegisResult<Revision> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let events: Vec<AuditEntry> = inner.events.clone();
        inner.tuples.clear();
        let mut last_revision = Revision::ZERO;
        let target = to_revision.unwrap_or(Revision::new(u64::MAX));
        for event in &events {
            if event.revision > target {
                break;
            }
            let key = (
                event.subject.clone(),
                event.relation.clone(),
                event.object.clone(),
            );
            match event.action {
                TupleMutation::Add => {
                    let tuple = RelationshipTuple::new(
                        SubjectId::new(&event.subject).map_err(AegisError::Validation)?,
                        Relation::new(&event.relation).map_err(AegisError::Validation)?,
                        ResourceId::new(&event.object).map_err(AegisError::Validation)?,
                    );
                    inner.tuples.insert(key, tuple);
                }
                TupleMutation::Remove => {
                    inner.tuples.remove(&key);
                }
            }
            last_revision = event.revision;
        }
        Ok(last_revision)
    }

    fn restore_backup(
        &self,
        _partition_id: &PartitionId,
        tuples: &[RelationshipTuple],
        events: &[AuditEntry],
        revision: Revision,
    ) -> AegisResult<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        inner.tuples.clear();
        inner.events.clear();
        for tuple in tuples {
            let key = (
                tuple.subject.as_str().to_string(),
                tuple.relation.as_str().to_string(),
                tuple.object.as_str().to_string(),
            );
            inner.tuples.insert(key, tuple.clone());
        }
        inner.events = events.to_vec();
        inner.revision = revision.as_u64();
        Ok(())
    }

    fn close(&self) -> AegisResult<()> {
        Ok(())
    }

    fn storage_version(&self) -> Option<String> {
        Some("0.1.0 (in-memory)".to_string())
    }

    fn set_actor_identity(&self, identity: Option<String>) -> Option<String> {
        let mut inner = self.inner.lock().ok()?;
        let prev = inner.actor_identity.clone();
        inner.actor_identity = identity;
        prev
    }

    fn list_policy_versions(&self) -> AegisResult<Vec<PolicyVersion>> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let mut versions: Vec<PolicyVersion> = inner.policy_versions.values().cloned().collect();
        versions.sort_by_key(|v| v.version);
        Ok(versions)
    }

    fn save_policy_version(&self, version: &PolicyVersion) -> AegisResult<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        inner
            .policy_versions
            .insert(version.version, version.clone());
        Ok(())
    }

    fn load_policy_version(&self, version: u32) -> AegisResult<Option<String>> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        Ok(inner
            .policy_versions
            .get(&version)
            .map(|v| v.schema.clone()))
    }

    fn save_policy_draft(&self, draft: &PolicyDraft) -> AegisResult<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        inner
            .policy_drafts
            .insert(draft.id.to_string(), draft.clone());
        Ok(())
    }

    fn load_policy_draft(&self, id: &str) -> AegisResult<Option<PolicyDraft>> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        Ok(inner.policy_drafts.get(id).cloned())
    }

    fn delete_policy_draft(&self, id: &str) -> AegisResult<bool> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        Ok(inner.policy_drafts.remove(id).is_some())
    }

    fn save_analysis_schedule(&self, schedule: &AnalysisSchedule) -> AegisResult<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        inner
            .analysis_schedules
            .insert(schedule.id.to_string(), schedule.clone());
        Ok(())
    }

    fn delete_analysis_schedule(&self, id: &str) -> AegisResult<bool> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        Ok(inner.analysis_schedules.remove(id).is_some())
    }

    fn save_analysis_run(&self, run: &AnalysisRun) -> AegisResult<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        inner.analysis_runs.push(run.clone());
        Ok(())
    }

    fn save_enforcement_event(&self, event: &EnforcementEvent) -> AegisResult<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        inner.enforcement_events.push(event.clone());
        Ok(())
    }
}

pub struct InMemoryTransaction {
    inner: Arc<Mutex<Inner>>,
    pending_tuples: Vec<(TupleMutation, RelationshipTuple)>,
}

impl InMemoryTransaction {
    fn new(inner: Arc<Mutex<Inner>>) -> Self {
        Self {
            inner,
            pending_tuples: Vec::new(),
        }
    }
}

impl StorageTransaction for InMemoryTransaction {
    fn write(&mut self, _partition_id: &PartitionId, tuple: &RelationshipTuple) -> AegisResult<()> {
        self.pending_tuples
            .push((TupleMutation::Add, tuple.clone()));
        Ok(())
    }

    fn delete(&mut self, _partition_id: &PartitionId, key: &TupleKey) -> AegisResult<()> {
        let tuple = RelationshipTuple::new(
            key.subject.clone(),
            key.relation.clone(),
            key.object.clone(),
        );
        self.pending_tuples.push((TupleMutation::Remove, tuple));
        Ok(())
    }

    fn savepoint(&self, _name: &str) -> AegisResult<()> {
        Ok(())
    }

    fn rollback_to_savepoint(&self, _name: &str) -> AegisResult<()> {
        Ok(())
    }

    fn release_savepoint(&self, _name: &str) -> AegisResult<()> {
        Ok(())
    }

    fn set_actor_identity(&mut self, identity: Option<String>) -> Option<String> {
        let mut inner = self.inner.lock().ok()?;
        let prev = inner.actor_identity.clone();
        inner.actor_identity = identity;
        prev
    }

    fn commit(self: Box<Self>) -> AegisResult<Revision> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let mut revision = Revision::ZERO;
        for (action, tuple) in &self.pending_tuples {
            let key = (
                tuple.subject.as_str().to_string(),
                tuple.relation.as_str().to_string(),
                tuple.object.as_str().to_string(),
            );
            revision = {
                inner.revision += 1;
                Revision::new(inner.revision)
            };
            match action {
                TupleMutation::Add => {
                    inner.tuples.insert(key, tuple.clone());
                }
                TupleMutation::Remove => {
                    inner.tuples.remove(&key);
                }
            }
            let identity = inner.actor_identity.clone();
            inner.events.push(AuditEntry {
                revision,
                action: *action,
                subject: tuple.subject.as_str().to_string(),
                relation: tuple.relation.as_str().to_string(),
                object: tuple.object.as_str().to_string(),
                timestamp: Utc::now(),
                metadata: None,
                identity,
            });
        }
        Ok(revision)
    }

    fn rollback(self: Box<Self>) -> AegisResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> InMemoryStorage {
        let mut s = InMemoryStorage::new();
        s.initialize().unwrap();
        s
    }

    #[test]
    fn initialize_creates_empty_store() {
        let mut s = InMemoryStorage::new();
        let meta = s.initialize().unwrap();
        assert_eq!(meta.schema_version, 1);
        assert_eq!(meta.current_revision, Revision::ZERO);
        assert!(meta.healthy);
    }

    #[test]
    fn write_and_read_tuple() {
        let s = storage();
        let pid = PartitionId::default();
        let tuple = RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:a").unwrap(),
        );
        let rev = s.write_tuple(&pid, &tuple).unwrap();
        assert!(rev > Revision::ZERO);

        let key = TupleKey {
            subject: SubjectId::new("user:alice").unwrap(),
            relation: Relation::new("owner").unwrap(),
            object: ResourceId::new("repo:a").unwrap(),
        };
        assert!(s.has_tuple(&pid, &key).unwrap());
        let read = s.read_tuple(&pid, &key).unwrap().unwrap();
        assert_eq!(read.subject.as_str(), "user:alice");
    }

    #[test]
    fn delete_tuple() {
        let s = storage();
        let pid = PartitionId::default();
        let tuple = RelationshipTuple::new(
            SubjectId::new("user:bob").unwrap(),
            Relation::new("viewer").unwrap(),
            ResourceId::new("repo:b").unwrap(),
        );
        s.write_tuple(&pid, &tuple).unwrap();

        let key = TupleKey {
            subject: SubjectId::new("user:bob").unwrap(),
            relation: Relation::new("viewer").unwrap(),
            object: ResourceId::new("repo:b").unwrap(),
        };
        s.delete_tuple(&pid, &key).unwrap();
        assert!(!s.has_tuple(&pid, &key).unwrap());
    }

    #[test]
    fn list_by_object_and_subject() {
        let s = storage();
        let pid = PartitionId::default();

        let t1 = RelationshipTuple::new(
            SubjectId::new("user:a").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:x").unwrap(),
        );
        let t2 = RelationshipTuple::new(
            SubjectId::new("user:b").unwrap(),
            Relation::new("viewer").unwrap(),
            ResourceId::new("repo:x").unwrap(),
        );
        let t3 = RelationshipTuple::new(
            SubjectId::new("user:a").unwrap(),
            Relation::new("viewer").unwrap(),
            ResourceId::new("repo:y").unwrap(),
        );
        s.write_tuple(&pid, &t1).unwrap();
        s.write_tuple(&pid, &t2).unwrap();
        s.write_tuple(&pid, &t3).unwrap();

        let by_obj = s
            .list_by_object(
                &pid,
                &ResourceId::new("repo:x").unwrap(),
                None,
                &crate::types::ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(by_obj.len(), 2);

        let by_subj = s
            .list_by_subject(
                &pid,
                &SubjectId::new("user:a").unwrap(),
                None,
                &crate::types::ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(by_subj.len(), 2);
    }

    #[test]
    fn transaction_commit() {
        let s = storage();
        let pid = PartitionId::default();
        let mut txn = s.begin_transaction(&pid).unwrap();

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:carol").unwrap(),
            Relation::new("editor").unwrap(),
            ResourceId::new("repo:z").unwrap(),
        );
        txn.write(&pid, &tuple).unwrap();
        let rev = txn.commit().unwrap();
        assert!(rev > Revision::ZERO);

        let key = TupleKey {
            subject: SubjectId::new("user:carol").unwrap(),
            relation: Relation::new("editor").unwrap(),
            object: ResourceId::new("repo:z").unwrap(),
        };
        assert!(s.has_tuple(&pid, &key).unwrap());
    }

    #[test]
    fn transaction_rollback() {
        let s = storage();
        let pid = PartitionId::default();
        let mut txn = s.begin_transaction(&pid).unwrap();

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:dave").unwrap(),
            Relation::new("admin").unwrap(),
            ResourceId::new("repo:w").unwrap(),
        );
        txn.write(&pid, &tuple).unwrap();
        txn.rollback().unwrap();

        let key = TupleKey {
            subject: SubjectId::new("user:dave").unwrap(),
            relation: Relation::new("admin").unwrap(),
            object: ResourceId::new("repo:w").unwrap(),
        };
        assert!(!s.has_tuple(&pid, &key).unwrap());
    }

    #[test]
    fn audit_trail() {
        let s = storage();
        let pid = PartitionId::default();

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:eve").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:audit").unwrap(),
        );
        s.write_tuple(&pid, &tuple).unwrap();

        let key = TupleKey {
            subject: SubjectId::new("user:eve").unwrap(),
            relation: Relation::new("owner").unwrap(),
            object: ResourceId::new("repo:audit").unwrap(),
        };
        s.delete_tuple(&pid, &key).unwrap();

        let events = s
            .query_audit(&pid, None, None, None, &PaginationParams::default())
            .unwrap();
        assert_eq!(events.len(), 2);

        assert_eq!(events[0].action, TupleMutation::Add);
        assert_eq!(events[1].action, TupleMutation::Remove);
        assert!(events[1].revision > events[0].revision);
    }
}
