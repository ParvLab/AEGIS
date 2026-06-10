use crate::error::{AegisError, AegisResult};
use crate::storage::traits::{
    BackendType, IntegrityReport, StorageBackend, StorageMeta, StorageTransaction, TupleFilter,
};
use crate::types::{
    AuditEntry, ConsistencyMode, PaginatedTuples, PaginationCursor, PaginationParams, Relation,
    RelationshipTuple, ResourceId, Revision, RevisionToken, SubjectId, TupleKey, TupleMutation,
};
use chrono::{DateTime, Utc};
use rocksdb::{
    ColumnFamily, ColumnFamilyDescriptor, DBIterator, Direction, IteratorMode, Options, DB,
};
use serde_json;
use std::collections::HashMap;
use uuid::Uuid;

const CF_META: &str = "meta";
const CF_TUPLES: &str = "tuples";
const CF_IDX_OBJECT: &str = "idx_object";
const CF_EVENTS: &str = "events";

const META_REVISION: &str = "revision";
const META_SCHEMA_VERSION: &str = "schema_version";

fn tuple_key(subject: &str, relation: &str, object: &str) -> Vec<u8> {
    format!("{}\x00{}\x00{}", subject, relation, object).into_bytes()
}

fn object_idx_key(object: &str, relation: &str, subject: &str) -> Vec<u8> {
    format!("{}\x00{}\x00{}", object, relation, subject).into_bytes()
}

fn event_key(revision: Revision, id: Uuid) -> Vec<u8> {
    format!("{:016x}:{}", revision.as_u64(), id).into_bytes()
}

fn tuple_from_value(value: &[u8]) -> AegisResult<RelationshipTuple> {
    let s = std::str::from_utf8(value)
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
    serde_json::from_str(s)
        .map_err(|e| AegisError::StorageQuery(e.to_string()))
}

pub struct RocksDbStorage {
    db: DB,
    node_id: Uuid,
}

impl RocksDbStorage {
    pub fn new(path: &str) -> AegisResult<Self> {
        let mut opts = Options::default();
        opts.create_missing_column_families(true);
        opts.create_if_missing(true);

        let cfs = vec![
            CF_META,
            CF_TUPLES,
            CF_IDX_OBJECT,
            CF_EVENTS,
        ];

        let db = DB::open_cf(&opts, path, cfs)
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        // Initialize revision if not present
        let cf_meta = db.cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;

        if db.get_cf(&cf_meta, META_REVISION.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            .is_none()
        {
            db.put_cf(&cf_meta, META_REVISION.as_bytes(), &0u64.to_le_bytes())
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        }

        if db.get_cf(&cf_meta, META_SCHEMA_VERSION.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            .is_none()
        {
            db.put_cf(&cf_meta, META_SCHEMA_VERSION.as_bytes(), &1u32.to_le_bytes())
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        }

        Ok(Self {
            db,
            node_id: Uuid::new_v4(),
        })
    }

    fn read_schema_version(&self) -> AegisResult<u32> {
        let cf = self.db.cf_handle(CF_META)
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
        let cf = self.db.cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;
        self.db.put_cf(&cf, META_SCHEMA_VERSION.as_bytes(), &version.to_le_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))
    }

    fn read_revision(&self) -> AegisResult<Revision> {
        let cf = self.db.cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;
        let val = self.db.get_cf(&cf, META_REVISION.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            .unwrap_or_else(|| 0u64.to_le_bytes().to_vec());
        let rev = u64::from_le_bytes(
            val.try_into().map_err(|_| AegisError::StorageQuery("invalid revision".into()))?,
        );
        Ok(Revision::new(rev))
    }

    fn bump_revision(&self) -> AegisResult<Revision> {
        let cf = self.db.cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;
        let val = self.db.get_cf(&cf, META_REVISION.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            .unwrap_or_else(|| 0u64.to_le_bytes().to_vec());
        let rev = u64::from_le_bytes(
            val.try_into().map_err(|_| AegisError::StorageQuery("invalid revision".into()))?,
        );
        let new_rev = rev + 1;
        self.db.put_cf(&cf, META_REVISION.as_bytes(), &new_rev.to_le_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(Revision::new(new_rev))
    }

    fn append_event(
        &self,
        revision: Revision,
        action: &str,
        subject: &str,
        relation: &str,
        object: &str,
        metadata: Option<&str>,
    ) -> AegisResult<()> {
        let cf = self.db.cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;
        let event = serde_json::json!({
            "revision": revision.as_u64(),
            "action": action,
            "subject": subject,
            "relation": relation,
            "object": object,
            "metadata": metadata,
            "timestamp": Utc::now().to_rfc3339(),
        });
        let event_id = Uuid::new_v4();
        let key = event_key(revision, event_id);
        let val = serde_json::to_string(&event)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        self.db.put_cf(&cf, key, val.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn put_tuple(&self, tuple: &RelationshipTuple, revision: Revision) -> AegisResult<()> {
        let cf_tuples = self.db.cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self.db.cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;

        let val = serde_json::to_string(tuple)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        let key = tuple_key(tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str());
        let idx_key = object_idx_key(tuple.object.as_str(), tuple.relation.as_str(), tuple.subject.as_str());

        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(&cf_tuples, &key, val.as_bytes());
        batch.put_cf(&cf_idx, &idx_key, &[]);
        self.db.write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn delete_tuple_key(&self, key: &TupleKey, revision: Revision) -> AegisResult<()> {
        let cf_tuples = self.db.cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self.db.cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;

        let pk = tuple_key(key.subject.as_str(), key.relation.as_str(), key.object.as_str());
        let idx_key = object_idx_key(key.object.as_str(), key.relation.as_str(), key.subject.as_str());

        let mut batch = rocksdb::WriteBatch::default();
        batch.delete_cf(&cf_tuples, &pk);
        batch.delete_cf(&cf_idx, &idx_key);
        self.db.write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }
}

impl StorageBackend for RocksDbStorage {
    fn backend_type(&self) -> BackendType {
        BackendType::RocksDB
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

    fn write_tuple(&self, tuple: &RelationshipTuple) -> AegisResult<Revision> {
        let revision = self.bump_revision()?;

        // Remove existing tuple if present
        let existing = self.has_tuple(&tuple.key())?;
        if existing {
            self.delete_tuple_key(&tuple.key(), revision)?;
        }

        self.put_tuple(tuple, revision)?;

        let metadata_json = tuple.metadata.as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default());

        self.append_event(
            revision,
            "add",
            tuple.subject.as_str(),
            tuple.relation.as_str(),
            tuple.object.as_str(),
            metadata_json.as_deref(),
        )?;

        Ok(revision)
    }

    fn write_tuples_batch(&self, tuples: &[RelationshipTuple]) -> AegisResult<Revision> {
        if tuples.is_empty() {
            return self.current_revision();
        }

        let revision = self.bump_revision()?;

        let cf_tuples = self.db.cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self.db.cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;
        let cf_events = self.db.cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;

        let mut batch = rocksdb::WriteBatch::default();
        for tuple in tuples {
            let pk = tuple_key(tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str());
            let idx_key = object_idx_key(tuple.object.as_str(), tuple.relation.as_str(), tuple.subject.as_str());
            let val = serde_json::to_string(tuple)
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            // Remove existing
            batch.delete_cf(&cf_tuples, &pk);
            batch.delete_cf(&cf_idx, &idx_key);
            // Insert new
            batch.put_cf(&cf_tuples, &pk, val.as_bytes());
            batch.put_cf(&cf_idx, &idx_key, &[]);

            let event = serde_json::json!({
                "revision": revision.as_u64(),
                "action": "add",
                "subject": tuple.subject.as_str(),
                "relation": tuple.relation.as_str(),
                "object": tuple.object.as_str(),
                "metadata": tuple.metadata.as_ref().map(|m| serde_json::to_string(m).unwrap_or_default()),
                "timestamp": Utc::now().to_rfc3339(),
            });
            let event_id = Uuid::new_v4();
            batch.put_cf(&cf_events, event_key(revision, event_id), serde_json::to_string(&event).unwrap().as_bytes());
        }
        self.db.write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Ok(revision)
    }

    fn delete_tuple(&self, key: &TupleKey) -> AegisResult<Revision> {
        let exists = self.has_tuple(key)?;
        if !exists {
            return self.current_revision();
        }

        let revision = self.bump_revision()?;
        self.delete_tuple_key(key, revision)?;

        self.append_event(
            revision,
            "remove",
            key.subject.as_str(),
            key.relation.as_str(),
            key.object.as_str(),
            None,
        )?;

        Ok(revision)
    }

    fn delete_subject(&self, subject: &SubjectId) -> AegisResult<Revision> {
        let tuples = self.list_by_subject(subject, None)?;
        if tuples.is_empty() {
            return self.current_revision();
        }

        let revision = self.bump_revision()?;

        let cf_tuples = self.db.cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self.db.cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;
        let cf_events = self.db.cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;

        let prefix = format!("{}\x00", subject.as_str()).into_bytes();
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
        let objects_prefix = b""; // need to scan all idx entries matching this subject
        let idx_iter = self.db.prefix_iterator_cf(&cf_idx, objects_prefix);
        for item in idx_iter {
            let (k, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            // idx key format: object\x00relation\x00subject
            if let Ok(s) = std::str::from_utf8(&k) {
                let parts: Vec<&str> = s.split('\x00').collect();
                if parts.len() == 3 && parts[2] == subject.as_str() {
                    batch.delete_cf(&cf_idx, &k);
                }
            }
        }

        for tuple in &tuples {
            let event = serde_json::json!({
                "revision": revision.as_u64(),
                "action": "remove",
                "subject": tuple.subject.as_str(),
                "relation": tuple.relation.as_str(),
                "object": tuple.object.as_str(),
                "metadata": None::<String>,
                "timestamp": Utc::now().to_rfc3339(),
            });
            let event_id = Uuid::new_v4();
            batch.put_cf(&cf_events, event_key(revision, event_id), serde_json::to_string(&event).unwrap().as_bytes());
        }

        self.db.write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Ok(revision)
    }

    fn delete_object(&self, object: &ResourceId) -> AegisResult<Revision> {
        let tuples = self.list_by_object(object, None)?;
        if tuples.is_empty() {
            return self.current_revision();
        }

        let revision = self.bump_revision()?;

        let cf_tuples = self.db.cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self.db.cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;
        let cf_events = self.db.cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;

        let mut batch = rocksdb::WriteBatch::default();

        for tuple in &tuples {
            let pk = tuple_key(tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str());
            let idx_key = object_idx_key(tuple.object.as_str(), tuple.relation.as_str(), tuple.subject.as_str());
            batch.delete_cf(&cf_tuples, &pk);
            batch.delete_cf(&cf_idx, &idx_key);

            let event = serde_json::json!({
                "revision": revision.as_u64(),
                "action": "remove",
                "subject": tuple.subject.as_str(),
                "relation": tuple.relation.as_str(),
                "object": tuple.object.as_str(),
                "metadata": None::<String>,
                "timestamp": Utc::now().to_rfc3339(),
            });
            let event_id = Uuid::new_v4();
            batch.put_cf(&cf_events, event_key(revision, event_id), serde_json::to_string(&event).unwrap().as_bytes());
        }

        self.db.write(batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Ok(revision)
    }

    fn has_tuple(&self, key: &TupleKey) -> AegisResult<bool> {
        let cf = self.db.cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let pk = tuple_key(key.subject.as_str(), key.relation.as_str(), key.object.as_str());
        let val = self.db.get_cf(&cf, &pk)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(val.is_some())
    }

    fn read_tuple(&self, key: &TupleKey) -> AegisResult<Option<RelationshipTuple>> {
        let cf = self.db.cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let pk = tuple_key(key.subject.as_str(), key.relation.as_str(), key.object.as_str());
        let val = self.db.get_cf(&cf, &pk)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        match val {
            Some(v) => Ok(Some(tuple_from_value(&v)?)),
            None => Ok(None),
        }
    }

    fn list_by_object(
        &self,
        object: &ResourceId,
        relation: Option<&Relation>,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let cf = self.db.cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let prefix = match relation {
            Some(rel) => format!("{}\x00{}\x00", object.as_str(), rel.as_str()).into_bytes(),
            None => format!("{}\x00", object.as_str()).into_bytes(),
        };

        // We need to find tuples by object: scan idx_object first, then fetch from tuples
        let cf_idx = self.db.cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;
        let iter = self.db.prefix_iterator_cf(&cf_idx, &prefix);
        let mut results = Vec::new();
        for item in iter {
            let (k, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if !k.starts_with(&prefix) {
                break;
            }
            if let Ok(s) = std::str::from_utf8(&k) {
                let parts: Vec<&str> = s.split('\x00').collect();
                if parts.len() == 3 {
                    let obj = parts[0];
                    let rel = parts[1];
                    let subj = parts[2];
                    let pk = tuple_key(subj, rel, obj);
                    if let Some(val) = self.db.get_cf(&cf, &pk)
                        .map_err(|e| AegisError::StorageQuery(e.to_string()))?
                    {
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
        subject: &SubjectId,
        relation: Option<&Relation>,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let cf = self.db.cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let prefix = match relation {
            Some(rel) => format!("{}\x00{}\x00", subject.as_str(), rel.as_str()).into_bytes(),
            None => format!("{}\x00", subject.as_str()).into_bytes(),
        };

        let iter = self.db.prefix_iterator_cf(&cf, &prefix);
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
        object: &ResourceId,
        relation: &Relation,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        self.list_by_object(object, Some(relation))
    }

    fn query_tuples(
        &self,
        filter: &TupleFilter,
        pagination: &PaginationParams,
        _consistency: &ConsistencyMode,
    ) -> AegisResult<PaginatedTuples> {
        let revision = self.read_revision()?;
        let cf_tuples = self.db.cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self.db.cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;

        let offset = pagination.cursor.as_ref().map(|c| c.offset).unwrap_or(0);
        let limit = pagination.limit as usize;

        let mut all_tuples = Vec::new();

        if filter.object_type.is_some() || filter.relation.is_some() {
            // Use the object index for efficient filtering
            let iter = self.db.iterator_cf(&cf_idx, IteratorMode::Start);
            for item in iter {
                let (key, _) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let key_str = std::str::from_utf8(&key)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let parts: Vec<&str> = key_str.split('\x00').collect();
                if parts.len() < 3 { continue; }
                let obj = parts[0];
                let rel = parts[1];
                let subj = parts[2];

                if let Some(ref ot) = filter.object_type {
                    if !obj.starts_with(&format!("{ot}:")) { continue; }
                }
                if let Some(ref r) = filter.relation {
                    if rel != r.as_str() { continue; }
                }
                if let Some(ref st) = filter.subject_type {
                    if !subj.starts_with(&format!("{st}:")) { continue; }
                }

                let pk = tuple_key(subj, rel, obj);
                if let Ok(Some(value)) = self.db.get_cf(&cf_tuples, &pk) {
                    if let Ok(tuple) = tuple_from_value(&value) {
                        if let Some(ref mk) = filter.metadata_key {
                            let has_key = tuple.metadata.as_ref()
                                .map(|m| m.contains_key(mk))
                                .unwrap_or(false);
                            if !has_key { continue; }
                        }
                        all_tuples.push(tuple);
                    }
                }
            }
        } else if let Some(ref st) = filter.subject_type {
            // Prefix scan by subject type
            let prefix = format!("{st}:");
            let iter = self.db.iterator_cf(&cf_tuples, IteratorMode::From(
                prefix.as_bytes(), Direction::Forward,
            ));
            for item in iter {
                let (key, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let key_str = std::str::from_utf8(&key)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                if !key_str.starts_with(&prefix) { break; }
                if let Ok(tuple) = tuple_from_value(&value) {
                    all_tuples.push(tuple);
                }
            }
        } else {
            // Full scan
            let iter = self.db.iterator_cf(&cf_tuples, IteratorMode::Start);
            for item in iter {
                let (_, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                if let Ok(tuple) = tuple_from_value(&value) {
                    all_tuples.push(tuple);
                }
            }
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

        Ok(PaginatedTuples { tuples, next_cursor, revision })
    }

    fn current_revision(&self) -> AegisResult<Revision> {
        self.read_revision()
    }

    fn current_token(&self) -> AegisResult<RevisionToken> {
        let revision = self.current_revision()?;
        Ok(RevisionToken::new(revision, self.node_id))
    }

    fn begin_transaction(&self) -> AegisResult<Box<dyn StorageTransaction>> {
        let cf_tuples = self.db.cf_handle(CF_TUPLES)
            .ok_or_else(|| AegisError::StorageConnection("missing tuples cf".into()))?;
        let cf_idx = self.db.cf_handle(CF_IDX_OBJECT)
            .ok_or_else(|| AegisError::StorageConnection("missing idx_object cf".into()))?;
        let cf_events = self.db.cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;
        let cf_meta = self.db.cf_handle(CF_META)
            .ok_or_else(|| AegisError::StorageConnection("missing meta cf".into()))?;

        Ok(Box::new(RocksDbTransaction {
            db: self.db.clone(),
            batch: rocksdb::WriteBatch::default(),
            cf_tuples,
            cf_idx,
            cf_events,
            cf_meta,
            node_id: self.node_id,
        }))
    }

    fn query_audit(
        &self,
        object: &ResourceId,
        from_revision: Option<Revision>,
        to_revision: Option<Revision>,
        pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>> {
        let cf_events = self.db.cf_handle(CF_EVENTS)
            .ok_or_else(|| AegisError::StorageConnection("missing events cf".into()))?;
        let obj_str = object.as_str();

        let offset = pagination.cursor.as_ref().map(|c| c.offset).unwrap_or(0);
        let limit = pagination.limit as usize;

        let from = from_revision.map(|r| r.as_u64()).unwrap_or(0);
        let to = to_revision.map(|r| r.as_u64()).unwrap_or(u64::MAX);

        let iter = self.db.iterator_cf(&cf_events, IteratorMode::Start);
        let mut results: Vec<AuditEntry> = Vec::new();

        for item in iter {
            let (_, value) = item.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&value) {
                let rev = event["revision"].as_u64().unwrap_or(0);
                if rev < from { continue; }
                if rev > to { break; }

                let event_obj = event["object"].as_str().unwrap_or("");
                if event_obj != obj_str { continue; }

                let action = if event["action"] == "add" {
                    TupleMutation::Add
                } else {
                    TupleMutation::Remove
                };

                let ts_str = event["timestamp"].as_str().unwrap_or("");
                let timestamp: DateTime<Utc> = ts_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata: Option<HashMap<String, String>> = event.get("metadata")
                    .and_then(|m| serde_json::from_value(m.clone()).ok());

                results.push(AuditEntry {
                    revision: Revision::new(rev),
                    action,
                    subject: event["subject"].as_str().unwrap_or("").to_string(),
                    relation: event["relation"].as_str().unwrap_or("").to_string(),
                    object: event_obj.to_string(),
                    timestamp,
                    metadata,
                });
            }
        }

        results.sort_by_key(|e| e.revision);

        let total = results.len();
        let page: Vec<AuditEntry> = results.into_iter()
            .skip(offset as usize)
            .take(limit)
            .collect();

        if page.len() < limit && (offset as usize + page.len()) < total {
            // More results exist beyond pagination
        }

        Ok(page)
    }

    fn integrity_check(&self) -> AegisResult<IntegrityReport> {
        let cf_meta = match self.db.cf_handle(CF_META) {
            Some(cf) => cf,
            None => {
                return Ok(IntegrityReport {
                    passed: false,
                    details: vec!["missing meta column family".to_string()],
                    backend_type: BackendType::RocksDB,
                })
            }
        };
        match self.db.get_cf(&cf_meta, META_REVISION.as_bytes()) {
            Ok(Some(_)) => Ok(IntegrityReport {
                passed: true,
                details: vec!["ok".to_string()],
                backend_type: BackendType::RocksDB,
            }),
            Ok(None) => Ok(IntegrityReport {
                passed: false,
                details: vec!["revision counter not found".to_string()],
                backend_type: BackendType::RocksDB,
            }),
            Err(e) => Ok(IntegrityReport {
                passed: false,
                details: vec![e.to_string()],
                backend_type: BackendType::RocksDB,
            }),
        }
    }

    fn close(&self) -> AegisResult<()> {
        // RocksDB flushes on drop
        Ok(())
    }
}

/// A RocksDB transaction using WriteBatch for atomicity.
pub struct RocksDbTransaction {
    db: DB,
    batch: rocksdb::WriteBatch,
    cf_tuples: rocksdb::ColumnFamily,
    cf_idx: rocksdb::ColumnFamily,
    cf_events: rocksdb::ColumnFamily,
    cf_meta: rocksdb::ColumnFamily,
    node_id: Uuid,
}

impl RocksDbTransaction {
    fn append_event_to_batch(
        &self,
        revision: Revision,
        action: &str,
        subject: &str,
        relation: &str,
        object: &str,
        metadata: Option<&str>,
    ) -> AegisResult<()> {
        let event = serde_json::json!({
            "revision": revision.as_u64(),
            "action": action,
            "subject": subject,
            "relation": relation,
            "object": object,
            "metadata": metadata,
            "timestamp": Utc::now().to_rfc3339(),
        });
        let event_id = Uuid::new_v4();
        let key = event_key(revision, event_id);
        let val = serde_json::to_string(&event)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        self.batch.put_cf(&self.cf_events, key, val.as_bytes());
        Ok(())
    }

    fn put_tuple_to_batch(
        &self,
        subject: &str,
        relation: &str,
        object: &str,
        value: &[u8],
    ) -> AegisResult<()> {
        let pk = tuple_key(subject, relation, object);
        let idx_key = object_idx_key(object, relation, subject);
        self.batch.put_cf(&self.cf_tuples, &pk, value);
        self.batch.put_cf(&self.cf_idx, &idx_key, &[]);
        Ok(())
    }

    fn delete_tuple_from_batch(&self, subject: &str, relation: &str, object: &str) -> AegisResult<()> {
        let pk = tuple_key(subject, relation, object);
        let idx_key = object_idx_key(object, relation, subject);
        self.batch.delete_cf(&self.cf_tuples, &pk);
        self.batch.delete_cf(&self.cf_idx, &idx_key);
        Ok(())
    }
}

impl StorageTransaction for RocksDbTransaction {
    fn write(&mut self, tuple: &RelationshipTuple) -> AegisResult<()> {
        let rev = self.db.get_cf(&self.cf_meta, META_REVISION.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            .map(|v| u64::from_le_bytes(v.as_slice().try_into().unwrap_or([0; 8])))
            .unwrap_or(0);
        let new_rev = rev + 1;

        let val = serde_json::to_string(tuple)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        // Remove existing if present
        let pk = tuple_key(tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str());
        let idx_key = object_idx_key(tuple.object.as_str(), tuple.relation.as_str(), tuple.subject.as_str());
        self.batch.delete_cf(&self.cf_tuples, &pk);
        self.batch.delete_cf(&self.cf_idx, &idx_key);
        // Insert new
        self.batch.put_cf(&self.cf_tuples, &pk, val.as_bytes());
        self.batch.put_cf(&self.cf_idx, &idx_key, &[]);

        let metadata_json = tuple.metadata.as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default());
        self.append_event_to_batch(
            Revision::new(new_rev), "add",
            tuple.subject.as_str(), tuple.relation.as_str(),
            tuple.object.as_str(), metadata_json.as_deref(),
        )?;

        Ok(())
    }

    fn delete(&mut self, key: &TupleKey) -> AegisResult<()> {
        let rev = self.db.get_cf(&self.cf_meta, META_REVISION.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            .map(|v| u64::from_le_bytes(v.as_slice().try_into().unwrap_or([0; 8])))
            .unwrap_or(0);
        let new_rev = rev + 1;

        self.delete_tuple_from_batch(key.subject.as_str(), key.relation.as_str(), key.object.as_str())?;
        self.append_event_to_batch(
            Revision::new(new_rev), "remove",
            key.subject.as_str(), key.relation.as_str(), key.object.as_str(), None,
        )?;

        Ok(())
    }

    fn savepoint(&self, _name: &str) -> AegisResult<()> {
        // RocksDB WriteBatch does not support named savepoints natively in the Rust crate.
        // For transactional semantics, use the SQLite backend.
        Ok(())
    }

    fn rollback_to_savepoint(&self, _name: &str) -> AegisResult<()> {
        // Not supported with WriteBatch
        Ok(())
    }

    fn release_savepoint(&self, _name: &str) -> AegisResult<()> {
        // Not supported with WriteBatch
        Ok(())
    }

    fn commit(self: Box<Self>) -> AegisResult<Revision> {
        let s = *self;
        // Bump revision
        let rev = s.db.get_cf(&s.cf_meta, META_REVISION.as_bytes())
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            .map(|v| u64::from_le_bytes(v.as_slice().try_into().unwrap_or([0; 8])))
            .unwrap_or(0);
        let new_rev = rev + 1;

        // Update revision in batch before writing
        s.batch.put_cf(&s.cf_meta, META_REVISION.as_bytes(), &new_rev.to_le_bytes());

        // Write the batch atomically
        s.db.write(s.batch)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Ok(Revision::new(new_rev))
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
        let dir = std::env::temp_dir()
            .join(format!("aegis_rocksdb_test_{}", Uuid::new_v4()));
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
        let rev = storage.write_tuple(&tuple).unwrap();
        assert!(rev.as_u64() > 0);

        let found = storage.read_tuple(&tuple.key()).unwrap();
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
        assert!(!storage.has_tuple(&key).unwrap());

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:bob").unwrap(),
            Relation::new("viewer").unwrap(),
            ResourceId::new("repo:other").unwrap(),
        );
        storage.write_tuple(&tuple).unwrap();
        assert!(storage.has_tuple(&key).unwrap());
    }

    #[test]
    fn test_delete_tuple() {
        let storage = make_storage();
        let tuple = RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:fluxbus").unwrap(),
        );
        storage.write_tuple(&tuple).unwrap();
        assert!(storage.has_tuple(&tuple.key()).unwrap());

        let del_rev = storage.delete_tuple(&tuple.key()).unwrap();
        assert!(del_rev.as_u64() > 0);
        assert!(!storage.has_tuple(&tuple.key()).unwrap());
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
        let rev = storage.write_tuples_batch(&[t1, t2]).unwrap();
        assert!(rev.as_u64() > 0);

        let all = storage.list_by_subject(&SubjectId::new("user:a").unwrap(), None).unwrap();
        assert_eq!(all.len(), 1);
        let all = storage.list_by_subject(&SubjectId::new("user:b").unwrap(), None).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_list_by_subject() {
        let storage = make_storage();
        let alice = SubjectId::new("user:alice").unwrap();
        let bob = SubjectId::new("user:bob").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage.write_tuple(&RelationshipTuple::new(alice.clone(), Relation::new("owner").unwrap(), repo.clone())).unwrap();
        storage.write_tuple(&RelationshipTuple::new(alice.clone(), Relation::new("viewer").unwrap(), repo.clone())).unwrap();
        storage.write_tuple(&RelationshipTuple::new(bob.clone(), Relation::new("viewer").unwrap(), repo.clone())).unwrap();

        let alice_tuples = storage.list_by_subject(&alice, None).unwrap();
        assert_eq!(alice_tuples.len(), 2);

        let alice_owner = storage.list_by_subject(&alice, Some(&Relation::new("owner").unwrap())).unwrap();
        assert_eq!(alice_owner.len(), 1);

        let bob_tuples = storage.list_by_subject(&bob, None).unwrap();
        assert_eq!(bob_tuples.len(), 1);
    }

    #[test]
    fn test_list_by_object() {
        let storage = make_storage();
        let repo_a = ResourceId::new("repo:a").unwrap();
        let repo_b = ResourceId::new("repo:b").unwrap();

        storage.write_tuple(&RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(), Relation::new("owner").unwrap(), repo_a.clone(),
        )).unwrap();
        storage.write_tuple(&RelationshipTuple::new(
            SubjectId::new("user:bob").unwrap(), Relation::new("viewer").unwrap(), repo_a.clone(),
        )).unwrap();
        storage.write_tuple(&RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(), Relation::new("owner").unwrap(), repo_b.clone(),
        )).unwrap();

        let a_tuples = storage.list_by_object(&repo_a, None).unwrap();
        assert_eq!(a_tuples.len(), 2);

        let a_owners = storage.list_by_object(&repo_a, Some(&Relation::new("owner").unwrap())).unwrap();
        assert_eq!(a_owners.len(), 1);

        let b_tuples = storage.list_by_object(&repo_b, None).unwrap();
        assert_eq!(b_tuples.len(), 1);
    }

    #[test]
    fn test_delete_subject() {
        let storage = make_storage();
        let alice = SubjectId::new("user:alice").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage.write_tuple(&RelationshipTuple::new(
            alice.clone(), Relation::new("owner").unwrap(), repo.clone(),
        )).unwrap();
        storage.write_tuple(&RelationshipTuple::new(
            alice.clone(), Relation::new("viewer").unwrap(), repo.clone(),
        )).unwrap();

        let rev = storage.delete_subject(&alice).unwrap();
        assert!(rev.as_u64() > 0);

        let tuples = storage.list_by_subject(&alice, None).unwrap();
        assert_eq!(tuples.len(), 0);
    }

    #[test]
    fn test_delete_object() {
        let storage = make_storage();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage.write_tuple(&RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(), Relation::new("owner").unwrap(), repo.clone(),
        )).unwrap();
        storage.write_tuple(&RelationshipTuple::new(
            SubjectId::new("user:bob").unwrap(), Relation::new("viewer").unwrap(), repo.clone(),
        )).unwrap();

        let rev = storage.delete_object(&repo).unwrap();
        assert!(rev.as_u64() > 0);

        let tuples = storage.list_by_object(&repo, None).unwrap();
        assert_eq!(tuples.len(), 0);
    }

    #[test]
    fn test_current_revision() {
        let storage = make_storage();
        assert_eq!(storage.current_revision().unwrap().as_u64(), 0);

        storage.write_tuple(&RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(), Relation::new("owner").unwrap(),
            ResourceId::new("repo:fluxbus").unwrap(),
        )).unwrap();

        assert_eq!(storage.current_revision().unwrap().as_u64(), 1);
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
        let r1 = storage.write_tuple(&tuple).unwrap();
        let r2 = storage.write_tuple(&tuple).unwrap();
        assert!(r2 > r1);
        // Only one active tuple after idempotent write
        let count = storage.list_by_subject(&SubjectId::new("user:alice").unwrap(), None).unwrap().len();
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
        ).unwrap();
        storage.write_tuple(&tuple).unwrap();
        let found = storage.read_tuple(&tuple.key()).unwrap().unwrap();
        assert_eq!(found.metadata.as_ref().unwrap().get("key1").unwrap(), "val1");
    }

    #[test]
    fn test_integrity_check_passes() {
        let storage = make_storage();
        let report = storage.integrity_check().unwrap();
        assert!(report.passed);
    }
}
