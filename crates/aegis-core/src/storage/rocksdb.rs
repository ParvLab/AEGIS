use crate::engine::enforcement_history::EnforcementEvent;
use crate::engine::policy_lifecycle::PolicyDraft;
use crate::engine::scheduler::{AnalysisRun, AnalysisSchedule};
use crate::error::{AegisError, AegisResult};
use crate::storage::traits::{
    BackendType, IntegrityReport, PolicyVersion, StorageBackend, StorageMeta, StorageTransaction,
    TupleFilter,
};
use crate::types::{
    AuditEntry, ConsistencyMode, PaginatedTuples, PaginationCursor, PaginationParams, PartitionId,
    Relation, RelationshipTuple, ResourceId, Revision, RevisionToken, SubjectId, TupleKey,
    TupleMutation,
};
use chrono::{DateTime, Utc};
use rocksdb::{
    BlockBasedOptions, Cache, ColumnFamily, ColumnFamilyDescriptor, DB, DBIterator, Direction,
    IteratorMode, Options,
};
use serde_json;
use std::collections::HashMap;
use uuid::Uuid;

const CF_META: &str = "meta";
const CF_TUPLES: &str = "tuples";
const CF_IDX_OBJECT: &str = "idx_object";
const CF_EVENTS: &str = "events";
const CF_POLICY_VERSIONS: &str = "policy_versions";
const CF_POLICY_DRAFTS: &str = "policy_drafts";
const CF_ANALYSIS_SCHEDULES: &str = "analysis_schedules";
const CF_ANALYSIS_RUNS: &str = "analysis_runs";
const CF_ENFORCEMENT_EVENTS: &str = "enforcement_events";

const META_REVISION: &str = "revision";
const META_SCHEMA_VERSION: &str = "schema_version";

fn tuple_key(partition_id: &str, subject: &str, relation: &str, object: &str) -> Vec<u8> {
    format!(
        "{}\x00{}\x00{}\x00{}",
        partition_id, subject, relation, object
    )
    .into_bytes()
}

fn object_idx_key(partition_id: &str, object: &str, relation: &str, subject: &str) -> Vec<u8> {
    format!(
        "{}\x00{}\x00{}\x00{}",
        partition_id, object, relation, subject
    )
    .into_bytes()
}

fn event_key(partition_id: &str, revision: Revision, id: Uuid) -> Vec<u8> {
    format!("{}:{:016x}:{}", partition_id, revision.as_u64(), id).into_bytes()
}

fn tuple_from_value(value: &[u8]) -> AegisResult<RelationshipTuple> {
    let s = std::str::from_utf8(value).map_err(|e| AegisError::StorageQuery(e.to_string()))?;
    serde_json::from_str(s).map_err(|e| AegisError::StorageQuery(e.to_string()))
}

pub struct RocksDbStorage {
    db: DB,
    node_id: Uuid,
    revision_mutex: std::sync::Mutex<()>,
    actor_identity: std::sync::Mutex<Option<String>>,
}

impl RocksDbStorage {
    pub fn new(path: &str) -> AegisResult<Self> {
        let mut opts = Options::default();
        opts.create_missing_column_families(true);
        opts.create_if_missing(true);

        // Configure block cache (8 MiB per column family)
        let cache = Cache::new(8 * 1024 * 1024);
        let mut block_opts = BlockBasedOptions::default();
        block_opts.set_block_cache(&cache);
        block_opts.set_block_size(4 * 1024); // 4 KiB blocks
        block_opts.set_cache_index_and_filter_blocks(true);
        opts.set_block_based_table_factory(&block_opts);

        let cfs = vec![
            CF_META,
            CF_TUPLES,
            CF_IDX_OBJECT,
            CF_EVENTS,
            CF_POLICY_VERSIONS,
            CF_POLICY_DRAFTS,
            CF_ANALYSIS_SCHEDULES,
            CF_ANALYSIS_RUNS,
            CF_ENFORCEMENT_EVENTS,
        ];

        let db = DB::open_cf(&opts, path, cfs)
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        // Initialize revision if not present
        let cf_meta = db
            .cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;

        if db
            .get_cf(&cf_meta, META_REVISION.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            .is_none()
        {
            db.put_cf(&cf_meta, META_REVISION.as_bytes(), &0u64.to_le_bytes())
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        }

        if db
            .get_cf(&cf_meta, META_SCHEMA_VERSION.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            .is_none()
        {
            db.put_cf(
                &cf_meta,
                META_SCHEMA_VERSION.as_bytes(),
                &1u32.to_le_bytes(),
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        }

        Ok(Self {
            db,
            node_id,
            revision_mutex: std::sync::Mutex::new(()),
            actor_identity: std::sync::Mutex::new(None),
        })
    }

    fn read_schema_version(&self) -> AegisResult<u32> {
        let cf = self
            .db
            .cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;
        match self.db.get_cf(&cf, META_SCHEMA_VERSION.as_bytes()) {
            Ok(Some(val)) if val.len() >= 4 => {
                let bytes: [u8; 4] = val[..4].try_into().unwrap_or([0; 4]);
                Ok(u32::from_le_bytes(bytes))
            }
            Ok(_) => Ok(0),
            Err(e) => Err(AegisError::StorageQuery(e.to_string())),
        }
    }

    fn write_schema_version(&self, version: u32) -> AegisResult<()> {
        let cf = self
            .db
            .cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;
        self.db
            .put_cf(&cf, META_SCHEMA_VERSION.as_bytes(), &version.to_le_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))
    }

    fn read_revision(&self) -> AegisResult<Revision> {
        let cf = self
            .db
            .cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;
        let val = self
            .db
            .get_cf(&cf, META_REVISION.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            .unwrap_or_else(|| 0u64.to_le_bytes().to_vec());
        let rev = u64::from_le_bytes(
            val.try_into()
                .map_err(|_| AegisError::StorageQuery("invalid revision".into()))?,
        );
        Ok(Revision::new(rev))
    }

    fn bump_revision(&self) -> AegisResult<Revision> {
        let _guard = self
            .revision_mutex
            .lock()
            .map_err(|_| AegisError::Internal("revision mutex poisoned".into()))?;
        let cf = self
            .db
            .cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;
        let val = self
            .db
            .get_cf(&cf, META_REVISION.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            .unwrap_or_else(|| 0u64.to_le_bytes().to_vec());
        let rev = u64::from_le_bytes(
            val.try_into()
                .map_err(|_| AegisError::StorageQuery("invalid revision".into()))?,
        );
        let new_rev = rev
            .checked_add(1)
            .ok_or_else(|| AegisError::Internal("revision overflow".into()))?;
        self.db
            .put_cf(&cf, META_REVISION.as_bytes(), &new_rev.to_le_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(Revision::new(new_rev))
    }

    fn last_event_hash(&self, partition_id: &PartitionId) -> AegisResult<String> {
        let cf = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;
        let pid_prefix = format!("{}:", partition_id.as_str()).into_bytes();
        // Seek to the end of the partition's event range
        let mut iter = self.db.prefix_iterator_cf(&cf, &pid_prefix);
        let mut last_hash = String::new();
        for item in &mut iter {
            let (key, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !key.starts_with(&pid_prefix) {
                break;
            }
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&value) {
                if let Some(h) = event["event_hash"].as_str() {
                    last_hash = h.to_string();
                }
            }
        }
        Ok(last_hash)
    }

    fn append_event(
        &self,
        partition_id: &PartitionId,
        revision: Revision,
        action: &str,
        subject: &str,
        relation: &str,
        object: &str,
        metadata: Option<&str>,
        identity: Option<&str>,
    ) -> AegisResult<()> {
        let cf = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;
        let now = Utc::now().to_rfc3339();
        let previous_hash = self.last_event_hash(partition_id)?;
        let event_hash = crate::storage::compute_event_hash(
            &previous_hash,
            revision.as_u64() as i64,
            action,
            subject,
            relation,
            object,
            partition_id.as_str(),
            metadata,
            &now,
            identity,
        );
        let event = serde_json::json!({
            "revision": revision.as_u64(),
            "action": action,
            "subject": subject,
            "relation": relation,
            "object": object,
            "metadata": metadata,
            "timestamp": now,
            "previous_hash": previous_hash,
            "event_hash": event_hash,
            "identity": identity,
        });
        let event_id = Uuid::new_v4();
        let key = event_key(partition_id.as_str(), revision, event_id);
        let val =
            serde_json::to_string(&event).map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        self.db
            .put_cf(&cf, key, val.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn put_tuple(
        &self,
        partition_id: &PartitionId,
        tuple: &RelationshipTuple,
        revision: Revision,
    ) -> AegisResult<()> {
        let cf_tuples = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self
            .db
            .cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;

        let val =
            serde_json::to_string(tuple).map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        let key = tuple_key(
            partition_id.as_str(),
            tuple.subject.as_str(),
            tuple.relation.as_str(),
            tuple.object.as_str(),
        );
        let idx_key = object_idx_key(
            partition_id.as_str(),
            tuple.object.as_str(),
            tuple.relation.as_str(),
            tuple.subject.as_str(),
        );

        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(&cf_tuples, &key, val.as_bytes());
        batch.put_cf(&cf_idx, &idx_key, &[]);
        self.db
            .write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn delete_tuple_key(
        &self,
        partition_id: &PartitionId,
        key: &TupleKey,
        revision: Revision,
    ) -> AegisResult<()> {
        let cf_tuples = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self
            .db
            .cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;

        let pk = tuple_key(
            partition_id.as_str(),
            key.subject.as_str(),
            key.relation.as_str(),
            key.object.as_str(),
        );
        let idx_key = object_idx_key(
            partition_id.as_str(),
            key.object.as_str(),
            key.relation.as_str(),
            key.subject.as_str(),
        );

        let mut batch = rocksdb::WriteBatch::default();
        batch.delete_cf(&cf_tuples, &pk);
        batch.delete_cf(&cf_idx, &idx_key);
        self.db
            .write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }
}

impl StorageBackend for RocksDbStorage {
    fn backend_type(&self) -> BackendType {
        BackendType::RocksDB
    }

    fn set_actor_identity(&self, identity: Option<String>) -> Option<String> {
        let mut guard = self.actor_identity.lock().unwrap();
        let prev = guard.take();
        *guard = identity;
        prev
    }

    fn initialize(&mut self) -> AegisResult<StorageMeta> {
        let rev = self.read_revision()?;
        Ok(StorageMeta {
            schema_version: 1,
            current_revision: rev,
            backend_type: BackendType::RocksDB,
            healthy: true,
        })
    }

    fn write_tuple(
        &self,
        partition_id: &PartitionId,
        tuple: &RelationshipTuple,
    ) -> AegisResult<Revision> {
        let revision = self.bump_revision()?;

        // Remove existing tuple if present
        let existing = self.has_tuple(partition_id, &tuple.key())?;
        if existing {
            self.delete_tuple_key(partition_id, &tuple.key(), revision)?;
        }

        self.put_tuple(partition_id, tuple, revision)?;

        let metadata_json = tuple
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;

        let identity = self.actor_identity.lock().unwrap().clone();
        self.append_event(
            partition_id,
            revision,
            "add",
            tuple.subject.as_str(),
            tuple.relation.as_str(),
            tuple.object.as_str(),
            metadata_json.as_deref(),
            identity.as_deref(),
        )?;

        Ok(revision)
    }

    fn write_tuples_batch(
        &self,
        partition_id: &PartitionId,
        tuples: &[RelationshipTuple],
    ) -> AegisResult<Revision> {
        if tuples.is_empty() {
            return self.current_revision(partition_id);
        }

        let revision = self.bump_revision()?;

        let cf_tuples = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self
            .db
            .cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;
        let cf_events = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;

        let mut batch = rocksdb::WriteBatch::default();
        let now = Utc::now().to_rfc3339();
        let mut previous_hash = self.last_event_hash(partition_id)?;
        for tuple in tuples {
            let pk = tuple_key(
                partition_id.as_str(),
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
            );
            let idx_key = object_idx_key(
                partition_id.as_str(),
                tuple.object.as_str(),
                tuple.relation.as_str(),
                tuple.subject.as_str(),
            );
            let val = serde_json::to_string(tuple)
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            // Remove existing
            batch.delete_cf(&cf_tuples, &pk);
            batch.delete_cf(&cf_idx, &idx_key);
            // Insert new
            batch.put_cf(&cf_tuples, &pk, val.as_bytes());
            batch.put_cf(&cf_idx, &idx_key, &[]);

            let metadata_str = tuple
                .metadata
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;
            let identity = self.actor_identity.lock().unwrap().clone();
            let event_hash = crate::storage::compute_event_hash(
                &previous_hash,
                revision.as_u64() as i64,
                "add",
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
                partition_id.as_str(),
                metadata_str.as_deref(),
                &now,
                identity.as_deref(),
            );
            let event = serde_json::json!({
                "revision": revision.as_u64(),
                "action": "add",
                "subject": tuple.subject.as_str(),
                "relation": tuple.relation.as_str(),
                "object": tuple.object.as_str(),
                "metadata": metadata_str,
                "timestamp": now,
                "previous_hash": previous_hash,
                "event_hash": event_hash,
                "identity": identity,
            });
            previous_hash = event_hash;
            let event_id = Uuid::new_v4();
            batch.put_cf(
                &cf_events,
                event_key(partition_id.as_str(), revision, event_id),
                serde_json::to_string(&event)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?
                    .as_bytes(),
            );
        }
        self.db
            .write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Ok(revision)
    }

    fn delete_tuple(&self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<Revision> {
        let exists = self.has_tuple(partition_id, key)?;
        if !exists {
            return self.current_revision(partition_id);
        }

        let revision = self.bump_revision()?;
        self.delete_tuple_key(partition_id, key, revision)?;

        let identity = self.actor_identity.lock().unwrap().clone();
        self.append_event(
            partition_id,
            revision,
            "remove",
            key.subject.as_str(),
            key.relation.as_str(),
            key.object.as_str(),
            None,
            identity.as_deref(),
        )?;

        Ok(revision)
    }

    fn delete_subject(
        &self,
        partition_id: &PartitionId,
        subject: &SubjectId,
    ) -> AegisResult<Revision> {
        let tuples = self.list_by_subject(
            partition_id,
            subject,
            None,
            &ConsistencyMode::MinimizeLatency,
        )?;
        if tuples.is_empty() {
            return self.current_revision(partition_id);
        }

        let revision = self.bump_revision()?;

        let cf_tuples = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self
            .db
            .cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;
        let cf_events = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;

        let prefix = format!("{}\x00{}\x00", partition_id.as_str(), subject.as_str()).into_bytes();
        let mut batch = rocksdb::WriteBatch::default();
        let iter = self.db.prefix_iterator_cf(&cf_tuples, &prefix);
        let mut keys_to_delete = Vec::new();
        for item in iter {
            let (k, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !k.starts_with(&prefix) {
                break;
            }
            keys_to_delete.push(k);
        }
        for k in &keys_to_delete {
            batch.delete_cf(&cf_tuples, k);
        }

        // Also remove from idx_object
        let partition_prefix = format!("{}\x00", partition_id.as_str()).into_bytes();
        let idx_iter = self.db.prefix_iterator_cf(&cf_idx, &partition_prefix);
        for item in idx_iter {
            let (k, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !k.starts_with(&partition_prefix) {
                break;
            }
            // idx key format: partition_id\x00object\x00relation\x00subject
            if let Ok(s) = std::str::from_utf8(&k) {
                let parts: Vec<&str> = s.split('\x00').collect();
                if parts.len() == 4 && parts[3] == subject.as_str() {
                    batch.delete_cf(&cf_idx, &k);
                }
            }
        }

        let now = Utc::now().to_rfc3339();
        let mut previous_hash = self.last_event_hash(partition_id)?;
        let identity = self.actor_identity.lock().unwrap().clone();
        for tuple in &tuples {
            let event_hash = crate::storage::compute_event_hash(
                &previous_hash,
                revision.as_u64() as i64,
                "remove",
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
                partition_id.as_str(),
                None,
                &now,
                identity.as_deref(),
            );
            let event = serde_json::json!({
                "revision": revision.as_u64(),
                "action": "remove",
                "subject": tuple.subject.as_str(),
                "relation": tuple.relation.as_str(),
                "object": tuple.object.as_str(),
                "metadata": None::<String>,
                "timestamp": now,
                "previous_hash": previous_hash,
                "event_hash": event_hash,
                "identity": identity,
            });
            previous_hash = event_hash;
            let event_id = Uuid::new_v4();
            let json_bytes = serde_json::to_string(&event)
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?
                .into_bytes();
            batch.put_cf(
                &cf_events,
                event_key(partition_id.as_str(), revision, event_id),
                &json_bytes,
            );
        }

        self.db
            .write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Ok(revision)
    }

    fn delete_object(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
    ) -> AegisResult<Revision> {
        let tuples = self.list_by_object(
            partition_id,
            object,
            None,
            &ConsistencyMode::MinimizeLatency,
        )?;
        if tuples.is_empty() {
            return self.current_revision(partition_id);
        }

        let revision = self.bump_revision()?;

        let cf_tuples = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self
            .db
            .cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;
        let cf_events = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;

        let mut batch = rocksdb::WriteBatch::default();

        let now = Utc::now().to_rfc3339();
        let mut previous_hash = self.last_event_hash(partition_id)?;
        let identity = self.actor_identity.lock().unwrap().clone();
        for tuple in &tuples {
            let pk = tuple_key(
                partition_id.as_str(),
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
            );
            let idx_key = object_idx_key(
                partition_id.as_str(),
                tuple.object.as_str(),
                tuple.relation.as_str(),
                tuple.subject.as_str(),
            );
            batch.delete_cf(&cf_tuples, &pk);
            batch.delete_cf(&cf_idx, &idx_key);

            let event_hash = crate::storage::compute_event_hash(
                &previous_hash,
                revision.as_u64() as i64,
                "remove",
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
                partition_id.as_str(),
                None,
                &now,
                identity.as_deref(),
            );
            let event = serde_json::json!({
                "revision": revision.as_u64(),
                "action": "remove",
                "subject": tuple.subject.as_str(),
                "relation": tuple.relation.as_str(),
                "object": tuple.object.as_str(),
                "metadata": None::<String>,
                "timestamp": now,
                "previous_hash": previous_hash,
                "event_hash": event_hash,
                "identity": identity,
            });
            previous_hash = event_hash;
            let event_id = Uuid::new_v4();
            let json_bytes = serde_json::to_string(&event)
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?
                .into_bytes();
            batch.put_cf(
                &cf_events,
                event_key(partition_id.as_str(), revision, event_id),
                &json_bytes,
            );
        }

        self.db
            .write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Ok(revision)
    }

    fn has_tuple(&self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<bool> {
        let cf = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let pk = tuple_key(
            partition_id.as_str(),
            key.subject.as_str(),
            key.relation.as_str(),
            key.object.as_str(),
        );
        let val = self
            .db
            .get_cf(&cf, &pk)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(val.is_some())
    }

    fn read_tuple(
        &self,
        partition_id: &PartitionId,
        key: &TupleKey,
    ) -> AegisResult<Option<RelationshipTuple>> {
        let cf = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let pk = tuple_key(
            partition_id.as_str(),
            key.subject.as_str(),
            key.relation.as_str(),
            key.object.as_str(),
        );
        let val = self
            .db
            .get_cf(&cf, &pk)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        match val {
            Some(v) => Ok(Some(tuple_from_value(&v)?)),
            None => Ok(None),
        }
    }

    fn list_by_object(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: Option<&Relation>,
        consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let cf = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let prefix = match relation {
            Some(rel) => format!(
                "{}\x00{}\x00{}\x00",
                partition_id.as_str(),
                object.as_str(),
                rel.as_str()
            )
            .into_bytes(),
            None => format!("{}\x00{}\x00", partition_id.as_str(), object.as_str()).into_bytes(),
        };

        // For FullyConsistent, use a snapshot to get a consistent view
        let snapshot = if *consistency == ConsistencyMode::FullyConsistent {
            Some(self.db.snapshot())
        } else {
            None
        };

        // We need to find tuples by object: scan idx_object first, then fetch from tuples
        let cf_idx = self
            .db
            .cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;

        let iter = if let Some(ref snap) = snapshot {
            snap.prefix_iterator_cf(&cf_idx, &prefix)
        } else {
            self.db.prefix_iterator_cf(&cf_idx, &prefix)
        };

        let mut results = Vec::new();
        for item in iter {
            let (k, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !k.starts_with(&prefix) {
                break;
            }
            if let Ok(s) = std::str::from_utf8(&k) {
                let parts: Vec<&str> = s.split('\x00').collect();
                if parts.len() == 4 {
                    let obj = parts[1];
                    let rel = parts[2];
                    let subj = parts[3];
                    let pk = tuple_key(partition_id.as_str(), subj, rel, obj);
                    let val = if let Some(ref snap) = snapshot {
                        snap.get_cf(&cf, &pk)
                    } else {
                        self.db.get_cf(&cf, &pk)
                    };
                    if let Some(val) = val.map_err(|e| AegisError::StorageQuery(e.to_string()))? {
                        if let Ok(tuple) = tuple_from_value(&val) {
                            results.push(tuple);
                        }
                    }
                }
            }
        }
        Ok(results)
    }

    fn list_by_subject(
        &self,
        partition_id: &PartitionId,
        subject: &SubjectId,
        relation: Option<&Relation>,
        consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let cf = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let prefix = match relation {
            Some(rel) => format!(
                "{}\x00{}\x00{}\x00",
                partition_id.as_str(),
                subject.as_str(),
                rel.as_str()
            )
            .into_bytes(),
            None => format!("{}\x00{}\x00", partition_id.as_str(), subject.as_str()).into_bytes(),
        };

        let snapshot = if *consistency == ConsistencyMode::FullyConsistent {
            Some(self.db.snapshot())
        } else {
            None
        };

        let iter = if let Some(ref snap) = snapshot {
            snap.prefix_iterator_cf(&cf, &prefix)
        } else {
            self.db.prefix_iterator_cf(&cf, &prefix)
        };

        let mut results = Vec::new();
        for item in iter {
            let (k, v) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !k.starts_with(&prefix) {
                break;
            }
            if let Ok(tuple) = tuple_from_value(&v) {
                results.push(tuple);
            }
        }
        Ok(results)
    }

    fn list_by_relation(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: &Relation,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        self.list_by_object(
            partition_id,
            object,
            Some(relation),
            &ConsistencyMode::MinimizeLatency,
        )
    }

    fn query_tuples(
        &self,
        partition_id: &PartitionId,
        filter: &TupleFilter,
        pagination: &PaginationParams,
        consistency: &ConsistencyMode,
    ) -> AegisResult<PaginatedTuples> {
        let revision = self.read_revision()?;

        // For FullyConsistent, use a snapshot
        let snapshot = if *consistency == ConsistencyMode::FullyConsistent {
            Some(self.db.snapshot())
        } else {
            None
        };

        let cf_tuples = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self
            .db
            .cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;

        let offset = pagination.cursor.as_ref().map(|c| c.offset).unwrap_or(0);
        let limit = pagination.limit as usize;

        let mut all_tuples = Vec::with_capacity(limit);

        let pid_prefix = format!("{}\x00", partition_id.as_str());

        if filter.object_type.is_some() || filter.relation.is_some() {
            // Use the object index for efficient filtering
            let pid_prefix_bytes = pid_prefix.as_bytes();
            let iter: Box<dyn Iterator<Item = Result<_, _>>> = if let Some(ref snap) = snapshot {
                Box::new(snap.prefix_iterator_cf(&cf_idx, pid_prefix_bytes))
            } else {
                Box::new(self.db.prefix_iterator_cf(&cf_idx, pid_prefix_bytes))
            };
            for item in iter {
                if all_tuples.len() >= limit {
                    break;
                }
                let (key, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                if !key.starts_with(pid_prefix_bytes) {
                    break;
                }
                let key_str = std::str::from_utf8(&key)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let parts: Vec<&str> = key_str.split('\x00').collect();
                if parts.len() < 4 {
                    continue;
                }
                let obj = parts[1];
                let rel = parts[2];
                let subj = parts[3];

                if let Some(ref ot) = filter.object_type {
                    if !obj.starts_with(&format!("{ot}:")) {
                        continue;
                    }
                }
                if let Some(ref r) = filter.relation {
                    if rel != r.as_str() {
                        continue;
                    }
                }
                if let Some(ref st) = filter.subject_type {
                    if !subj.starts_with(&format!("{st}:")) {
                        continue;
                    }
                }

                let pk = tuple_key(partition_id.as_str(), subj, rel, obj);
                let value_opt = if let Some(ref snap) = snapshot {
                    snap.get_cf(&cf_tuples, &pk)
                } else {
                    self.db.get_cf(&cf_tuples, &pk)
                };
                if let Ok(Some(value)) = value_opt {
                    if let Ok(tuple) = tuple_from_value(&value) {
                        if let Some(ref mk) = filter.metadata_key {
                            let has_key = tuple
                                .metadata
                                .as_ref()
                                .map(|m| m.contains_key(mk))
                                .unwrap_or(false);
                            if !has_key {
                                continue;
                            }
                        }
                        all_tuples.push(tuple);
                    }
                }
            }
        } else if let Some(ref st) = filter.subject_type {
            // Prefix scan by subject type within partition
            let prefix = format!("{}\x00{}:", partition_id.as_str(), st);
            let prefix_bytes = prefix.as_bytes();
            let iter: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), _>>> =
                if let Some(ref snap) = snapshot {
                    Box::new(snap.iterator_cf(
                        &cf_tuples,
                        IteratorMode::From(prefix_bytes, Direction::Forward),
                    ))
                } else {
                    Box::new(self.db.iterator_cf(
                        &cf_tuples,
                        IteratorMode::From(prefix_bytes, Direction::Forward),
                    ))
                };
            for item in iter {
                if all_tuples.len() >= limit {
                    break;
                }
                let (key, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let key_str = std::str::from_utf8(&key)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                if !key_str.starts_with(&prefix) {
                    break;
                }
                if let Ok(tuple) = tuple_from_value(&value) {
                    all_tuples.push(tuple);
                }
            }
        } else if filter.metadata_key.is_some() || filter.metadata_value.is_some() {
            // Full scan within partition for metadata filtering, but bounded by limit
            let pid_prefix_bytes = pid_prefix.as_bytes();
            let iter: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), _>>> =
                if let Some(ref snap) = snapshot {
                    Box::new(snap.prefix_iterator_cf(&cf_tuples, pid_prefix_bytes))
                } else {
                    Box::new(self.db.prefix_iterator_cf(&cf_tuples, pid_prefix_bytes))
                };
            for item in iter {
                if all_tuples.len() >= limit {
                    break;
                }
                let (key, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                if !key.starts_with(pid_prefix_bytes) {
                    break;
                }
                if let Ok(tuple) = tuple_from_value(&value) {
                    if let Some(ref mk) = filter.metadata_key {
                        let has_key = tuple
                            .metadata
                            .as_ref()
                            .map(|m| m.contains_key(mk))
                            .unwrap_or(false);
                        if !has_key {
                            continue;
                        }
                    }
                    if let Some(ref mv) = filter.metadata_value {
                        let has_val = tuple
                            .metadata
                            .as_ref()
                            .and_then(|m| m.values().find(|v| *v == mv))
                            .is_some();
                        if !has_val {
                            continue;
                        }
                    }
                    all_tuples.push(tuple);
                }
            }
        } else {
            // No filters — return empty rather than full scan
            return Ok(PaginatedTuples {
                tuples: Vec::new(),
                next_cursor: None,
                revision,
            });
        }

        let total = all_tuples.len();
        let tuples: Vec<RelationshipTuple> = all_tuples
            .into_iter()
            .skip(offset as usize)
            .take(limit)
            .collect();

        let next_cursor = if (offset as usize + tuples.len()) < total {
            Some(PaginationCursor {
                offset: offset + limit,
                revision,
            })
        } else {
            None
        };

        Ok(PaginatedTuples {
            tuples,
            next_cursor,
            revision,
        })
    }

    fn current_revision(&self, _partition_id: &PartitionId) -> AegisResult<Revision> {
        self.read_revision()
    }

    fn current_token(&self) -> AegisResult<RevisionToken> {
        let revision = self.current_revision()?;
        Ok(RevisionToken::new(revision, self.node_id))
    }

    fn begin_transaction(
        &self,
        partition_id: &PartitionId,
    ) -> AegisResult<Box<dyn StorageTransaction>> {
        let cf_tuples = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self
            .db
            .cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;
        let cf_events = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;
        let cf_meta = self
            .db
            .cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;

        let identity = self.actor_identity.lock().unwrap().clone();
        Ok(Box::new(RocksDbTransaction {
            db: self.db.clone(),
            partition_id: partition_id.as_str().to_string(),
            batch: rocksdb::WriteBatch::default(),
            cf_tuples,
            cf_idx,
            cf_events,
            cf_meta,
            node_id: self.node_id,
            revision_mutex: std::sync::Arc::new(std::sync::Mutex::new(())),
            actor_identity: identity,
            pending_events: Vec::new(),
        }))
    }

    fn query_audit(
        &self,
        partition_id: &PartitionId,
        object: Option<&ResourceId>,
        from_revision: Option<Revision>,
        to_revision: Option<Revision>,
        pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>> {
        let cf_events = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;

        let offset = pagination.cursor.as_ref().map(|c| c.offset).unwrap_or(0);
        let limit = pagination.limit as usize;

        let from_rev = from_revision.map(|r| r.as_u64());
        let to_rev = to_revision.map(|r| r.as_u64());

        let pid_prefix = format!("{}:", partition_id.as_str());
        let pid_prefix_bytes = pid_prefix.as_bytes();

        // Seek to the partition + `from_revision` key boundary for efficient range scan
        let seek_key = match from_rev {
            Some(rev) => format!("{}{:016x}:", pid_prefix, rev).into_bytes(),
            None => pid_prefix_bytes.to_vec(),
        };
        let mode = if seek_key.is_empty() {
            IteratorMode::Start
        } else {
            IteratorMode::From(&seek_key, Direction::Forward)
        };

        let iter = self.db.iterator_cf(&cf_events, mode);
        let mut results: Vec<AuditEntry> = Vec::with_capacity(limit.min(1000));

        for item in iter {
            if results.len() >= limit {
                break;
            }
            let (key, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            // Filter by partition
            if !key.starts_with(pid_prefix_bytes) {
                break;
            }
            // Extract revision part (after partition_id:)
            let key_pid_len = pid_prefix_bytes.len();
            let rev_part = &key[key_pid_len..];
            if let Some(to) = to_rev {
                if rev_part.len() < 16 {
                    break;
                }
                let key_rev =
                    u64::from_str_radix(std::str::from_utf8(&rev_part[..16]).unwrap_or(""), 16)
                        .unwrap_or(0);
                if key_rev > to {
                    break;
                }
            }
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&value) {
                let rev = event["revision"].as_u64().unwrap_or(0);
                if from_rev.map(|f| rev < f).unwrap_or(false) {
                    continue;
                }
                if to_rev.map(|t| rev > t).unwrap_or(false) {
                    break;
                }

                if let Some(obj) = object {
                    let event_obj = event["object"].as_str().unwrap_or("");
                    if event_obj != obj.as_str() {
                        continue;
                    }
                }

                let action = if event["action"] == "add" {
                    TupleMutation::Add
                } else {
                    TupleMutation::Remove
                };

                let ts_str = event["timestamp"].as_str().unwrap_or("");
                let timestamp: DateTime<Utc> = ts_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata: Option<HashMap<String, String>> = event
                    .get("metadata")
                    .and_then(|m| serde_json::from_value(m.clone()).ok());

                let identity: Option<String> = event
                    .get("identity")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                results.push(AuditEntry {
                    revision: Revision::new(rev),
                    action,
                    subject: event["subject"].as_str().unwrap_or("").to_string(),
                    relation: event["relation"].as_str().unwrap_or("").to_string(),
                    object: event_obj.to_string(),
                    timestamp,
                    metadata,
                    identity,
                });
            }
        }

        Ok(results)
    }

    fn integrity_check(&self) -> AegisResult<IntegrityReport> {
        let cf_meta = match self.db.cf_handle(CF_META) {
            Some(cf) => cf,
            None => {
                return Ok(IntegrityReport {
                    passed: false,
                    details: vec!["missing meta column family".to_string()],
                    backend_type: BackendType::RocksDB,
                    tenant_leakage_detected: false,
                    leaked_crossings: vec![],
                    orphaned_tuple_count: 0,
                });
            }
        };
        match self.db.get_cf(&cf_meta, META_REVISION.as_bytes()) {
            Ok(Some(_)) => Ok(IntegrityReport {
                passed: true,
                details: vec!["ok".to_string()],
                backend_type: BackendType::RocksDB,
                tenant_leakage_detected: false,
                leaked_crossings: vec![],
                orphaned_tuple_count: 0,
            }),
            Ok(None) => Ok(IntegrityReport {
                passed: false,
                details: vec!["revision counter not found".to_string()],
                backend_type: BackendType::RocksDB,
                tenant_leakage_detected: false,
                leaked_crossings: vec![],
                orphaned_tuple_count: 0,
            }),
            Err(e) => Ok(IntegrityReport {
                passed: false,
                details: vec![e.to_string()],
                backend_type: BackendType::RocksDB,
                tenant_leakage_detected: false,
                leaked_crossings: vec![],
                orphaned_tuple_count: 0,
            }),
        }
    }

    fn read_schema_version(&self) -> AegisResult<u32> {
        let cf = self
            .db
            .cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;
        match self.db.get_cf(&cf, META_SCHEMA_VERSION.as_bytes()) {
            Ok(Some(val)) if val.len() >= 4 => {
                let bytes: [u8; 4] = val[..4].try_into().unwrap_or([0; 4]);
                Ok(u32::from_le_bytes(bytes))
            }
            Ok(_) => Ok(0),
            Err(e) => Err(AegisError::StorageQuery(e.to_string())),
        }
    }

    fn write_schema_version(&self, version: u32) -> AegisResult<()> {
        let cf = self
            .db
            .cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;
        self.db
            .put_cf(&cf, META_SCHEMA_VERSION.as_bytes(), &version.to_le_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))
    }

    fn delete_events_before(
        &self,
        partition_id: &PartitionId,
        cutoff: DateTime<Utc>,
    ) -> AegisResult<usize> {
        let cf = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;
        let pid_prefix = format!("{}:", partition_id.as_str()).into_bytes();
        let iter = self.db.prefix_iterator_cf(&cf, &pid_prefix);
        let mut to_delete: Vec<Vec<u8>> = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !key.starts_with(&pid_prefix) {
                break;
            }
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&value) {
                let ts_str = event["timestamp"].as_str().unwrap_or("");
                if let Ok(ts) = ts_str.parse::<DateTime<Utc>>() {
                    if ts < cutoff {
                        to_delete.push(key.to_vec());
                    }
                }
            }
        }
        if to_delete.is_empty() {
            return Ok(0);
        }
        let mut batch = rocksdb::WriteBatch::default();
        for key in &to_delete {
            batch.delete_cf(&cf, key);
        }
        self.db
            .write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(to_delete.len())
    }

    fn delete_soft_deleted_tuples_before(
        &self,
        _partition_id: &PartitionId,
        _cutoff: DateTime<Utc>,
    ) -> AegisResult<usize> {
        // RocksDB does not maintain soft-deleted tuples — tuples are removed
        // from the CF on delete. Nothing to clean up.
        Ok(0)
    }

    fn recover_from_events(
        &self,
        partition_id: &PartitionId,
        to_revision: Option<Revision>,
    ) -> AegisResult<Revision> {
        let _guard = self
            .revision_mutex
            .lock()
            .map_err(|_| AegisError::Internal("revision mutex poisoned".into()))?;

        let cf_tuples = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self
            .db
            .cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;
        let cf_events = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;
        let cf_meta = self
            .db
            .cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;

        let pid_prefix = format!("{}:", partition_id.as_str());
        let pid_prefix_bytes = pid_prefix.as_bytes();

        let mut delete_batch = rocksdb::WriteBatch::default();

        // Delete tuples and index entries for this partition only
        let pid_tuple_prefix = format!("{}\x00", partition_id.as_str()).into_bytes();
        let tuples_iter = self.db.prefix_iterator_cf(&cf_tuples, &pid_tuple_prefix);
        for item in tuples_iter {
            let (key, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !key.starts_with(&pid_tuple_prefix) {
                break;
            }
            delete_batch.delete_cf(&cf_tuples, &key);
        }
        let idx_iter = self.db.prefix_iterator_cf(&cf_idx, &pid_tuple_prefix);
        for item in idx_iter {
            let (key, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !key.starts_with(&pid_tuple_prefix) {
                break;
            }
            delete_batch.delete_cf(&cf_idx, &key);
        }
        self.db
            .write(delete_batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let events_iter = self.db.prefix_iterator_cf(&cf_events, pid_prefix_bytes);
        let mut last_revision = Revision::ZERO;
        let mut replay_batch = rocksdb::WriteBatch::default();

        for item in events_iter {
            let (key, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !key.starts_with(pid_prefix_bytes) {
                break;
            }
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&value) {
                let rev = event["revision"].as_u64().unwrap_or(0);
                let action = event["action"].as_str().unwrap_or("");
                let subject = event["subject"].as_str().unwrap_or("");
                let relation = event["relation"].as_str().unwrap_or("");
                let object = event["object"].as_str().unwrap_or("");
                let revision = Revision::new(rev);
                if let Some(target) = to_revision {
                    if revision > target {
                        continue;
                    }
                }

                match action {
                    "add" => {
                        let tuple = RelationshipTuple::new(
                            SubjectId::new(subject)
                                .map_err(|e| AegisError::StorageQuery(e.to_string()))?,
                            Relation::new(relation)
                                .map_err(|e| AegisError::StorageQuery(e.to_string()))?,
                            ResourceId::new(object)
                                .map_err(|e| AegisError::StorageQuery(e.to_string()))?,
                        );
                        let val = serde_json::to_string(&tuple)
                            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                        let pk = tuple_key(partition_id.as_str(), subject, relation, object);
                        let idx_key =
                            object_idx_key(partition_id.as_str(), object, relation, subject);
                        replay_batch.put_cf(&cf_tuples, &pk, val.as_bytes());
                        replay_batch.put_cf(&cf_idx, &idx_key, &[]);
                    }
                    "remove" => {
                        let pk = tuple_key(partition_id.as_str(), subject, relation, object);
                        let idx_key =
                            object_idx_key(partition_id.as_str(), object, relation, subject);
                        replay_batch.delete_cf(&cf_tuples, &pk);
                        replay_batch.delete_cf(&cf_idx, &idx_key);
                    }
                    _ => {}
                }

                last_revision = revision;
            }
        }

        self.db
            .write(replay_batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        if last_revision != Revision::ZERO {
            self.db
                .put_cf(
                    &cf_meta,
                    META_REVISION.as_bytes(),
                    &last_revision.as_u64().to_le_bytes(),
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        }

        Ok(last_revision)
    }

    fn compact_events(&self, partition_id: &PartitionId) -> AegisResult<usize> {
        let cf = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;
        let pid_prefix = format!("{}:", partition_id.as_str()).into_bytes();
        let iter = self.db.prefix_iterator_cf(&cf, &pid_prefix);
        let mut adds: HashMap<(String, String, String), Vec<u8>> = HashMap::new();
        let mut to_delete: Vec<Vec<u8>> = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !key.starts_with(&pid_prefix) {
                break;
            }
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&value) {
                let action = event["action"].as_str().unwrap_or("").to_string();
                let subject = event["subject"].as_str().unwrap_or("").to_string();
                let relation = event["relation"].as_str().unwrap_or("").to_string();
                let object = event["object"].as_str().unwrap_or("").to_string();
                let tuple_key = (subject, relation, object);
                match action.as_str() {
                    "add" => {
                        adds.insert(tuple_key, key.to_vec());
                    }
                    "remove" => {
                        if let Some(add_key) = adds.remove(&tuple_key) {
                            to_delete.push(add_key);
                            to_delete.push(key.to_vec());
                        }
                    }
                    _ => {}
                }
            }
        }
        if to_delete.is_empty() {
            return Ok(0);
        }
        let mut batch = rocksdb::WriteBatch::default();
        for key in &to_delete {
            batch.delete_cf(&cf, key);
        }
        self.db
            .write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(to_delete.len())
    }

    fn close(&self) -> AegisResult<()> {
        // RocksDB flushes on drop
        Ok(())
    }

    fn verify_audit_chain(&self, partition_id: &PartitionId) -> AegisResult<Option<String>> {
        let cf = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;
        let pid_prefix = format!("{}:", partition_id.as_str()).into_bytes();
        let iter = self.db.prefix_iterator_cf(&cf, &pid_prefix);

        let mut last_event_hash = String::new();
        for item in iter {
            let (key, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !key.starts_with(&pid_prefix) {
                break;
            }
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&value) {
                let rev = event["revision"].as_i64().unwrap_or(0);
                let action = event["action"].as_str().unwrap_or("");
                let subject = event["subject"].as_str().unwrap_or("");
                let relation = event["relation"].as_str().unwrap_or("");
                let object = event["object"].as_str().unwrap_or("");
                let metadata = event["metadata"].as_str().or_else(|| {
                    // metadata can be JSON null or absent
                    None
                });
                let timestamp = event["timestamp"].as_str().unwrap_or("");
                let identity = event["identity"].as_str();
                let prev_hash = event["previous_hash"].as_str().unwrap_or("");
                let event_hash = event["event_hash"].as_str().unwrap_or("");

                if prev_hash != last_event_hash {
                    return Ok(Some(format!(
                        "Chain break: expected previous_hash='{}', got '{}'",
                        last_event_hash, prev_hash
                    )));
                }

                let expected = crate::storage::compute_event_hash(
                    &last_event_hash,
                    rev,
                    action,
                    subject,
                    relation,
                    object,
                    partition_id.as_str(),
                    metadata,
                    timestamp,
                    identity,
                );

                if expected != event_hash {
                    return Ok(Some(format!(
                        "Hash mismatch: expected '{}', got '{}'",
                        expected, event_hash
                    )));
                }

                last_event_hash = event_hash.to_string();
            }
        }

        Ok(None)
    }

    fn restore_backup(
        &self,
        partition_id: &PartitionId,
        tuples: &[RelationshipTuple],
        events: &[AuditEntry],
        revision: Revision,
    ) -> AegisResult<()> {
        let cf_meta = self
            .db
            .cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;
        let cf_tuples = self
            .db
            .cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self
            .db
            .cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;
        let cf_events = self
            .db
            .cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;

        let mut batch = rocksdb::WriteBatch::default();

        // Clear existing data (iterate and delete)
        let iter = self
            .db
            .iterator_cf(&cf_tuples, rocksdb::IteratorMode::Start);
        for item in iter {
            let (key, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            batch.delete_cf(&cf_tuples, &key);
        }
        let iter = self.db.iterator_cf(&cf_idx, rocksdb::IteratorMode::Start);
        for item in iter {
            let (key, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            batch.delete_cf(&cf_idx, &key);
        }
        let iter = self
            .db
            .iterator_cf(&cf_events, rocksdb::IteratorMode::Start);
        for item in iter {
            let (key, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            batch.delete_cf(&cf_events, &key);
        }

        for tuple in tuples {
            let pk = tuple_key(
                partition_id.as_str(),
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
            );
            let idx_key = object_idx_key(
                partition_id.as_str(),
                tuple.object.as_str(),
                tuple.relation.as_str(),
                tuple.subject.as_str(),
            );
            let val = serde_json::to_string(tuple)
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            batch.put_cf(&cf_tuples, &pk, val.as_bytes());
            batch.put_cf(&cf_idx, &idx_key, &[]);
        }

        for event in events {
            let action_str = match event.action {
                TupleMutation::Add => "add",
                TupleMutation::Remove => "remove",
            };
            let event_json = serde_json::json!({
                "revision": event.revision.as_u64(),
                "action": action_str,
                "subject": event.subject,
                "relation": event.relation,
                "object": event.object,
                "metadata": event.metadata,
                "timestamp": event.timestamp.to_rfc3339(),
                "previous_hash": "",
                "event_hash": "",
                "identity": event.identity,
            });
            let event_id = Uuid::new_v4();
            let key = event_key(partition_id.as_str(), event.revision, event_id);
            let event_bytes = serde_json::to_string(&event_json)
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            batch.put_cf(&cf_events, &key, event_bytes.as_bytes());
        }

        // Set revision
        batch.put_cf(
            &cf_meta,
            META_REVISION.as_bytes(),
            &revision.as_u64().to_le_bytes(),
        );

        self.db
            .write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn list_policy_versions(&self) -> AegisResult<Vec<PolicyVersion>> {
        let versions_cf = self
            .db
            .cf_handle(CF_POLICY_VERSIONS)
            .ok_or_else(|| AegisError::StorageConnection("missing policy_versions cf".into()))?;

        let mut iter = self.db.raw_iterator_cf(&versions_cf);
        iter.seek_to_first();

        let mut versions = Vec::new();
        while iter.valid() {
            if let (Some(key), Some(value)) = (iter.key(), iter.value()) {
                let key_str = String::from_utf8_lossy(key);
                if let Ok(version_num) = key_str.parse::<u32>() {
                    if let Ok(pv) = serde_json::from_slice::<PolicyVersion>(value) {
                        versions.push(pv);
                    }
                }
            }
            iter.next();
        }
        versions.sort_by_key(|v| v.version);
        Ok(versions)
    }

    fn save_policy_version(&self, version: &PolicyVersion) -> AegisResult<()> {
        let versions_cf = self
            .db
            .cf_handle(CF_POLICY_VERSIONS)
            .ok_or_else(|| AegisError::StorageConnection("missing policy_versions cf".into()))?;

        let key = version.version.to_string();
        let value =
            serde_json::to_string(version).map_err(|e| AegisError::Internal(e.to_string()))?;

        self.db
            .put_cf(&versions_cf, key.as_bytes(), value.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn load_policy_version(&self, version: u32) -> AegisResult<Option<String>> {
        let versions_cf = self
            .db
            .cf_handle(CF_POLICY_VERSIONS)
            .ok_or_else(|| AegisError::StorageConnection("missing policy_versions cf".into()))?;

        let key = version.to_string();
        match self.db.get_cf(&versions_cf, key.as_bytes()) {
            Ok(Some(val)) => {
                let pv: PolicyVersion = serde_json::from_slice(&val)
                    .map_err(|e| AegisError::Internal(e.to_string()))?;
                Ok(Some(pv.schema))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(AegisError::StorageQuery(e.to_string())),
        }
    }

    fn save_policy_draft(&self, draft: &PolicyDraft) -> AegisResult<()> {
        let cf = self
            .db
            .cf_handle(CF_POLICY_DRAFTS)
            .ok_or_else(|| AegisError::StorageConnection("missing policy_drafts cf".into()))?;

        let key = draft.id.to_string();
        let value =
            serde_json::to_string(draft).map_err(|e| AegisError::Internal(e.to_string()))?;

        self.db
            .put_cf(&cf, key.as_bytes(), value.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn load_policy_draft(&self, id: &str) -> AegisResult<Option<PolicyDraft>> {
        let cf = self
            .db
            .cf_handle(CF_POLICY_DRAFTS)
            .ok_or_else(|| AegisError::StorageConnection("missing policy_drafts cf".into()))?;

        match self.db.get_cf(&cf, id.as_bytes()) {
            Ok(Some(val)) => {
                let draft: PolicyDraft = serde_json::from_slice(&val)
                    .map_err(|e| AegisError::Internal(e.to_string()))?;
                Ok(Some(draft))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(AegisError::StorageQuery(e.to_string())),
        }
    }

    fn delete_policy_draft(&self, id: &str) -> AegisResult<bool> {
        let cf = self
            .db
            .cf_handle(CF_POLICY_DRAFTS)
            .ok_or_else(|| AegisError::StorageConnection("missing policy_drafts cf".into()))?;

        let existing = self
            .db
            .get_cf(&cf, id.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        if existing.is_none() {
            return Ok(false);
        }

        self.db
            .delete_cf(&cf, id.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(true)
    }

    fn save_analysis_schedule(&self, schedule: &AnalysisSchedule) -> AegisResult<()> {
        let cf = self
            .db
            .cf_handle(CF_ANALYSIS_SCHEDULES)
            .ok_or_else(|| AegisError::StorageConnection("missing analysis_schedules cf".into()))?;

        let key = schedule.id.to_string();
        let value =
            serde_json::to_string(schedule).map_err(|e| AegisError::Internal(e.to_string()))?;

        self.db
            .put_cf(&cf, key.as_bytes(), value.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn delete_analysis_schedule(&self, id: &str) -> AegisResult<bool> {
        let cf = self
            .db
            .cf_handle(CF_ANALYSIS_SCHEDULES)
            .ok_or_else(|| AegisError::StorageConnection("missing analysis_schedules cf".into()))?;

        let existing = self
            .db
            .get_cf(&cf, id.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        if existing.is_none() {
            return Ok(false);
        }

        self.db
            .delete_cf(&cf, id.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(true)
    }

    fn save_analysis_run(&self, run: &AnalysisRun) -> AegisResult<()> {
        let cf = self
            .db
            .cf_handle(CF_ANALYSIS_RUNS)
            .ok_or_else(|| AegisError::StorageConnection("missing analysis_runs cf".into()))?;

        let key = run.id.to_string();
        let value = serde_json::to_string(run).map_err(|e| AegisError::Internal(e.to_string()))?;

        self.db
            .put_cf(&cf, key.as_bytes(), value.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn save_enforcement_event(&self, event: &EnforcementEvent) -> AegisResult<()> {
        let cf = self
            .db
            .cf_handle(CF_ENFORCEMENT_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing enforcement_events cf".into()))?;

        let key = event.id.to_string();
        let value =
            serde_json::to_string(event).map_err(|e| AegisError::Internal(e.to_string()))?;

        self.db
            .put_cf(&cf, key.as_bytes(), value.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }
}

/// A RocksDB transaction using WriteBatch for atomicity.
pub struct RocksDbTransaction {
    db: DB,
    partition_id: String,
    batch: rocksdb::WriteBatch,
    cf_tuples: rocksdb::ColumnFamily,
    cf_idx: rocksdb::ColumnFamily,
    cf_events: rocksdb::ColumnFamily,
    cf_meta: rocksdb::ColumnFamily,
    node_id: Uuid,
    revision_mutex: std::sync::Arc<std::sync::Mutex<()>>,
    actor_identity: Option<String>,
    /// Staged events — written in `commit()` with the final revision.
    pending_events: Vec<(String, String, String, String, Option<String>)>,
}

impl RocksDbTransaction {
    fn write_pending_events(&self, revision: Revision) -> AegisResult<()> {
        let now = Utc::now().to_rfc3339();
        let mut previous_hash = self.get_last_event_hash()?;
        for (action, subject, relation, object, metadata) in &self.pending_events {
            let event_hash = crate::storage::compute_event_hash(
                &previous_hash,
                revision.as_u64() as i64,
                action,
                subject,
                relation,
                object,
                &self.partition_id,
                metadata.as_deref(),
                &now,
                self.actor_identity.as_deref(),
            );
            let event = serde_json::json!({
                "revision": revision.as_u64(),
                "action": action,
                "subject": subject,
                "relation": relation,
                "object": object,
                "metadata": metadata,
                "timestamp": now,
                "previous_hash": previous_hash,
                "event_hash": event_hash,
                "identity": self.actor_identity,
            });
            previous_hash = event_hash;
            let event_id = Uuid::new_v4();
            let key = event_key(&self.partition_id, revision, event_id);
            let val = serde_json::to_string(&event)
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            self.batch.put_cf(&self.cf_events, key, val.as_bytes());
        }
        Ok(())
    }

    fn put_tuple_to_batch(
        &self,
        partition_id: &str,
        subject: &str,
        relation: &str,
        object: &str,
        value: &[u8],
    ) -> AegisResult<()> {
        let pk = tuple_key(partition_id, subject, relation, object);
        let idx_key = object_idx_key(partition_id, object, relation, subject);
        self.batch.put_cf(&self.cf_tuples, &pk, value);
        self.batch.put_cf(&self.cf_idx, &idx_key, &[]);
        Ok(())
    }

    fn delete_tuple_from_batch(
        &self,
        partition_id: &str,
        subject: &str,
        relation: &str,
        object: &str,
    ) -> AegisResult<()> {
        let pk = tuple_key(partition_id, subject, relation, object);
        let idx_key = object_idx_key(partition_id, object, relation, subject);
        self.batch.delete_cf(&self.cf_tuples, &pk);
        self.batch.delete_cf(&self.cf_idx, &idx_key);
        Ok(())
    }

    fn get_last_event_hash(&self) -> AegisResult<String> {
        let pid_prefix = format!("{}:", self.partition_id).into_bytes();
        let mut iter = self.db.prefix_iterator_cf(&self.cf_events, &pid_prefix);
        let mut last_hash = String::new();
        for item in &mut iter {
            let (key, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !key.starts_with(&pid_prefix) {
                break;
            }
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&value) {
                if let Some(h) = event["event_hash"].as_str() {
                    last_hash = h.to_string();
                }
            }
        }
        Ok(last_hash)
    }
}

impl StorageTransaction for RocksDbTransaction {
    fn set_actor_identity(&mut self, identity: Option<String>) -> Option<String> {
        let prev = self.actor_identity.take();
        self.actor_identity = identity;
        prev
    }

    fn write(&mut self, partition_id: &PartitionId, tuple: &RelationshipTuple) -> AegisResult<()> {
        let val =
            serde_json::to_string(tuple).map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        // Remove existing if present
        let pk = tuple_key(
            partition_id.as_str(),
            tuple.subject.as_str(),
            tuple.relation.as_str(),
            tuple.object.as_str(),
        );
        let idx_key = object_idx_key(
            partition_id.as_str(),
            tuple.object.as_str(),
            tuple.relation.as_str(),
            tuple.subject.as_str(),
        );
        self.batch.delete_cf(&self.cf_tuples, &pk);
        self.batch.delete_cf(&self.cf_idx, &idx_key);
        // Insert new
        self.batch.put_cf(&self.cf_tuples, &pk, val.as_bytes());
        self.batch.put_cf(&self.cf_idx, &idx_key, &[]);

        let metadata_json = tuple
            .metadata
            .as_ref()
            .map(|m| serde_json::to_string(m))
            .transpose()
            .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;
        self.pending_events.push((
            "add".to_string(),
            tuple.subject.as_str().to_string(),
            tuple.relation.as_str().to_string(),
            tuple.object.as_str().to_string(),
            metadata_json,
        ));

        Ok(())
    }

    fn delete(&mut self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<()> {
        self.delete_tuple_from_batch(
            partition_id.as_str(),
            key.subject.as_str(),
            key.relation.as_str(),
            key.object.as_str(),
        )?;
        self.pending_events.push((
            "remove".to_string(),
            key.subject.as_str().to_string(),
            key.relation.as_str().to_string(),
            key.object.as_str().to_string(),
            None,
        ));

        Ok(())
    }

    fn savepoint(&self, _name: &str) -> AegisResult<()> {
        Err(AegisError::OperationNotPermitted(
            "savepoints not supported on RocksDB backend; use SQLite for transactional semantics"
                .into(),
        ))
    }

    fn rollback_to_savepoint(&self, _name: &str) -> AegisResult<()> {
        Err(AegisError::OperationNotPermitted(
            "savepoints not supported on RocksDB backend; use SQLite for transactional semantics"
                .into(),
        ))
    }

    fn release_savepoint(&self, _name: &str) -> AegisResult<()> {
        Err(AegisError::OperationNotPermitted(
            "savepoints not supported on RocksDB backend; use SQLite for transactional semantics"
                .into(),
        ))
    }

    fn commit(self: Box<Self>) -> AegisResult<Revision> {
        let s = *self;
        let _guard = s
            .revision_mutex
            .lock()
            .map_err(|_| AegisError::Internal("revision mutex poisoned".into()))?;

        // Read current revision atomically
        let rev =
            s.db.get_cf(&s.cf_meta, META_REVISION.as_bytes())
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?
                .map(|v| u64::from_le_bytes(v.as_slice().try_into().unwrap_or([0; 8])))
                .unwrap_or(0);
        let new_rev = rev
            .checked_add(1)
            .ok_or_else(|| AegisError::Internal("revision overflow".into()))?;
        let revision = Revision::new(new_rev);

        // Write pending events with final revision
        s.write_pending_events(revision)?;

        // Update revision in batch
        s.batch
            .put_cf(&s.cf_meta, META_REVISION.as_bytes(), &new_rev.to_le_bytes());

        // Write the batch atomically
        s.db.write(s.batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Ok(revision)
    }

    fn rollback(self: Box<Self>) -> AegisResult<()> {
        // Just drop the batch — nothing is persisted
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageBackend;
    use std::fs;

    fn temp_dir() -> String {
        let dir = std::env::temp_dir().join(format!("aegis_rocksdb_test_{}", Uuid::new_v4()));
        let path = dir.to_str().unwrap().to_string();
        let _ = fs::remove_dir_all(&path);
        path
    }

    fn make_storage() -> RocksDbStorage {
        let path = temp_dir();
        RocksDbStorage::new(&path).unwrap()
    }

    #[test]
    fn test_initialize_returns_meta() {
        let mut storage = make_storage();
        let meta = storage.initialize().unwrap();
        assert_eq!(meta.backend_type, BackendType::RocksDB);
        assert!(meta.healthy);
    }

    #[test]
    fn test_write_and_read() {
        let storage = make_storage();
        let tuple = RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:fluxbus").unwrap(),
        );
        let rev = storage
            .write_tuple(&PartitionId::default(), &tuple)
            .unwrap();
        assert!(rev.as_u64() > 0);

        let found = storage
            .read_tuple(&PartitionId::default(), &tuple.key())
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().subject.as_str(), "user:alice");
    }

    #[test]
    fn test_has_tuple() {
        let storage = make_storage();
        let key = TupleKey {
            subject: SubjectId::new("user:bob").unwrap(),
            relation: Relation::new("viewer").unwrap(),
            object: ResourceId::new("repo:other").unwrap(),
        };
        assert!(!storage.has_tuple(&PartitionId::default(), &key).unwrap());

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:bob").unwrap(),
            Relation::new("viewer").unwrap(),
            ResourceId::new("repo:other").unwrap(),
        );
        storage
            .write_tuple(&PartitionId::default(), &tuple)
            .unwrap();
        assert!(storage.has_tuple(&PartitionId::default(), &key).unwrap());
    }

    #[test]
    fn test_delete_tuple() {
        let storage = make_storage();
        let tuple = RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:fluxbus").unwrap(),
        );
        storage
            .write_tuple(&PartitionId::default(), &tuple)
            .unwrap();
        assert!(
            storage
                .has_tuple(&PartitionId::default(), &tuple.key())
                .unwrap()
        );

        let del_rev = storage
            .delete_tuple(&PartitionId::default(), &tuple.key())
            .unwrap();
        assert!(del_rev.as_u64() > 0);
        assert!(
            !storage
                .has_tuple(&PartitionId::default(), &tuple.key())
                .unwrap()
        );
    }

    #[test]
    fn test_write_batch() {
        let storage = make_storage();
        let t1 = RelationshipTuple::new(
            SubjectId::new("user:a").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:1").unwrap(),
        );
        let t2 = RelationshipTuple::new(
            SubjectId::new("user:b").unwrap(),
            Relation::new("viewer").unwrap(),
            ResourceId::new("repo:2").unwrap(),
        );
        let rev = storage
            .write_tuples_batch(&PartitionId::default(), &[t1, t2])
            .unwrap();
        assert!(rev.as_u64() > 0);

        let all = storage
            .list_by_subject(
                &PartitionId::default(),
                &SubjectId::new("user:a").unwrap(),
                None,
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(all.len(), 1);
        let all = storage
            .list_by_subject(
                &PartitionId::default(),
                &SubjectId::new("user:b").unwrap(),
                None,
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_list_by_subject() {
        let storage = make_storage();
        let alice = SubjectId::new("user:alice").unwrap();
        let bob = SubjectId::new("user:bob").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage
            .write_tuple(
                &PartitionId::default(),
                &RelationshipTuple::new(
                    alice.clone(),
                    Relation::new("owner").unwrap(),
                    repo.clone(),
                ),
            )
            .unwrap();
        storage
            .write_tuple(
                &PartitionId::default(),
                &RelationshipTuple::new(
                    alice.clone(),
                    Relation::new("viewer").unwrap(),
                    repo.clone(),
                ),
            )
            .unwrap();
        storage
            .write_tuple(
                &PartitionId::default(),
                &RelationshipTuple::new(
                    bob.clone(),
                    Relation::new("viewer").unwrap(),
                    repo.clone(),
                ),
            )
            .unwrap();

        let alice_tuples = storage
            .list_by_subject(
                &PartitionId::default(),
                &alice,
                None,
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(alice_tuples.len(), 2);

        let alice_owner = storage
            .list_by_subject(
                &PartitionId::default(),
                &alice,
                Some(&Relation::new("owner").unwrap()),
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(alice_owner.len(), 1);

        let bob_tuples = storage
            .list_by_subject(
                &PartitionId::default(),
                &bob,
                None,
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(bob_tuples.len(), 1);
    }

    #[test]
    fn test_list_by_object() {
        let storage = make_storage();
        let repo_a = ResourceId::new("repo:a").unwrap();
        let repo_b = ResourceId::new("repo:b").unwrap();

        storage
            .write_tuple(
                &PartitionId::default(),
                &RelationshipTuple::new(
                    SubjectId::new("user:alice").unwrap(),
                    Relation::new("owner").unwrap(),
                    repo_a.clone(),
                ),
            )
            .unwrap();
        storage
            .write_tuple(
                &PartitionId::default(),
                &RelationshipTuple::new(
                    SubjectId::new("user:bob").unwrap(),
                    Relation::new("viewer").unwrap(),
                    repo_a.clone(),
                ),
            )
            .unwrap();
        storage
            .write_tuple(
                &PartitionId::default(),
                &RelationshipTuple::new(
                    SubjectId::new("user:alice").unwrap(),
                    Relation::new("owner").unwrap(),
                    repo_b.clone(),
                ),
            )
            .unwrap();

        let a_tuples = storage
            .list_by_object(
                &PartitionId::default(),
                &repo_a,
                None,
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(a_tuples.len(), 2);

        let a_owners = storage
            .list_by_object(
                &PartitionId::default(),
                &repo_a,
                Some(&Relation::new("owner").unwrap()),
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(a_owners.len(), 1);

        let b_tuples = storage
            .list_by_object(
                &PartitionId::default(),
                &repo_b,
                None,
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(b_tuples.len(), 1);
    }

    #[test]
    fn test_delete_subject() {
        let storage = make_storage();
        let alice = SubjectId::new("user:alice").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage
            .write_tuple(
                &PartitionId::default(),
                &RelationshipTuple::new(
                    alice.clone(),
                    Relation::new("owner").unwrap(),
                    repo.clone(),
                ),
            )
            .unwrap();
        storage
            .write_tuple(
                &PartitionId::default(),
                &RelationshipTuple::new(
                    alice.clone(),
                    Relation::new("viewer").unwrap(),
                    repo.clone(),
                ),
            )
            .unwrap();

        let rev = storage
            .delete_subject(&PartitionId::default(), &alice)
            .unwrap();
        assert!(rev.as_u64() > 0);

        let tuples = storage
            .list_by_subject(
                &PartitionId::default(),
                &alice,
                None,
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(tuples.len(), 0);
    }

    #[test]
    fn test_delete_object() {
        let storage = make_storage();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage
            .write_tuple(
                &PartitionId::default(),
                &RelationshipTuple::new(
                    SubjectId::new("user:alice").unwrap(),
                    Relation::new("owner").unwrap(),
                    repo.clone(),
                ),
            )
            .unwrap();
        storage
            .write_tuple(
                &PartitionId::default(),
                &RelationshipTuple::new(
                    SubjectId::new("user:bob").unwrap(),
                    Relation::new("viewer").unwrap(),
                    repo.clone(),
                ),
            )
            .unwrap();

        let rev = storage
            .delete_object(&PartitionId::default(), &repo)
            .unwrap();
        assert!(rev.as_u64() > 0);

        let tuples = storage
            .list_by_object(
                &PartitionId::default(),
                &repo,
                None,
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(tuples.len(), 0);
    }

    #[test]
    fn test_current_revision() {
        let storage = make_storage();
        assert_eq!(
            storage
                .current_revision(&PartitionId::default())
                .unwrap()
                .as_u64(),
            0
        );

        storage
            .write_tuple(
                &PartitionId::default(),
                &RelationshipTuple::new(
                    SubjectId::new("user:alice").unwrap(),
                    Relation::new("owner").unwrap(),
                    ResourceId::new("repo:fluxbus").unwrap(),
                ),
            )
            .unwrap();

        assert_eq!(
            storage
                .current_revision(&PartitionId::default())
                .unwrap()
                .as_u64(),
            1
        );
    }

    #[test]
    fn test_current_token() {
        let storage = make_storage();
        let token = storage.current_token().unwrap();
        assert_eq!(token.revision.as_u64(), 0);
    }

    #[test]
    fn test_idempotent_write() {
        let storage = make_storage();
        let tuple = RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:fluxbus").unwrap(),
        );
        let r1 = storage
            .write_tuple(&PartitionId::default(), &tuple)
            .unwrap();
        let r2 = storage
            .write_tuple(&PartitionId::default(), &tuple)
            .unwrap();
        assert!(r2 > r1);
        // Only one active tuple after idempotent write
        let count = storage
            .list_by_subject(
                &PartitionId::default(),
                &SubjectId::new("user:alice").unwrap(),
                None,
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap()
            .len();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_write_with_metadata() {
        use std::collections::HashMap;
        let storage = make_storage();
        let mut meta = HashMap::new();
        meta.insert("key1".to_string(), "val1".to_string());
        let tuple = RelationshipTuple::with_metadata(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:fluxbus").unwrap(),
            meta,
        )
        .unwrap();
        storage
            .write_tuple(&PartitionId::default(), &tuple)
            .unwrap();
        let found = storage
            .read_tuple(&PartitionId::default(), &tuple.key())
            .unwrap()
            .unwrap();
        assert_eq!(
            found.metadata.as_ref().unwrap().get("key1").unwrap(),
            "val1"
        );
    }

    #[test]
    fn test_integrity_check_passes() {
        let storage = make_storage();
        let report = storage.integrity_check().unwrap();
        assert!(report.passed);
    }
}
