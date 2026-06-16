use chrono::{DateTime, Utc};
use crate::error::AegisResult;
use crate::types::{
    AuditEntry, ConnectionStats, ConsistencyMode, PaginatedTuples, PaginationParams, PartitionId,
    Relation, RelationshipTuple, ResourceId, Revision, RevisionToken, SubjectId, TupleKey,
};

/// Pluggable storage backend for relationship tuples with partition awareness.
///
/// Each backend (SQLite, PostgreSQL, RocksDB, IndexedDB) implements this trait.
/// The trait is designed for:
/// - Single-process single-writer (serialized writes)
/// - Multiple concurrent readers
/// - Revision-based snapshot isolation
///
/// # Partitioning
///
/// Every storage operation takes a `partition_id` parameter that logically
/// isolates authorization graphs. Partitions share the same storage backend
/// but operate on independent tuple sets.
pub trait StorageBackend: Send + Sync {
    /// Initialize the storage backend.
    /// Creates tables, applies migrations, verifies integrity.
    fn initialize(&mut self) -> AegisResult<StorageMeta>;

    /// Write a single relationship tuple within a partition.
    /// Returns the new revision number.
    fn write_tuple(&self, partition_id: &PartitionId, tuple: &RelationshipTuple) -> AegisResult<Revision>;

    /// Write multiple tuples atomically within a single transaction in a partition.
    fn write_tuples_batch(&self, partition_id: &PartitionId, tuples: &[RelationshipTuple]) -> AegisResult<Revision>;

    /// Delete a single relationship tuple by key within a partition.
    fn delete_tuple(&self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<Revision>;

    /// Delete all tuples for a given subject within a partition.
    fn delete_subject(&self, partition_id: &PartitionId, subject: &SubjectId) -> AegisResult<Revision>;

    /// Delete all tuples for a given resource within a partition.
    fn delete_object(&self, partition_id: &PartitionId, object: &ResourceId) -> AegisResult<Revision>;

    /// Check if a tuple exists within a partition.
    fn has_tuple(&self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<bool>;

    /// Read a single tuple by key within a partition.
    fn read_tuple(&self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<Option<RelationshipTuple>>;

    /// List all tuples for a given object within a partition.
    fn list_by_object(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: Option<&Relation>,
        consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>>;

    /// List all tuples for a given subject within a partition.
    fn list_by_subject(
        &self,
        partition_id: &PartitionId,
        subject: &SubjectId,
        relation: Option<&Relation>,
        consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>>;

    /// List tuples where the subject is a subject-set referencing the given object.
    ///
    /// For example, if `object` = `team:eng`, returns tuples with subjects like
    /// `team:eng#member`, `team:eng#owner`, etc. — any subject of the form
    /// `{object}#{relation}`.
    ///
    /// The default implementation uses `query_tuples` with a subject prefix filter.
    /// Backends should override for efficient prefix-based lookups.
    fn list_by_subject_set_of(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: Option<&Relation>,
        _consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        // Default: scan using query_tuples with prefix-like filter.
        // Optimized overrides exist for SQLite (LIKE) and other indexed backends.
        let prefix = format!("{}#", object.as_str());
        let all = self.query_tuples(
            partition_id,
            &TupleFilter {
                subject_type: Some(prefix),
                relation: relation.cloned(),
                ..Default::default()
            },
            &PaginationParams { cursor: None, limit: 10_000 },
            _consistency,
        )?;
        Ok(all.tuples)
    }

    /// List all tuples matching a relation on an object within a partition.
    fn list_by_relation(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: &Relation,
    ) -> AegisResult<Vec<RelationshipTuple>>;

    /// Paginated query with filters within a partition.
    fn query_tuples(
        &self,
        partition_id: &PartitionId,
        filter: &TupleFilter,
        pagination: &PaginationParams,
        consistency: &ConsistencyMode,
    ) -> AegisResult<PaginatedTuples>;

    /// Get the current revision number for a partition.
    fn current_revision(&self, partition_id: &PartitionId) -> AegisResult<Revision>;

    /// Read the stored schema version from the backend.
    /// Returns 0 if no schema version has been recorded.
    fn read_schema_version(&self) -> AegisResult<u32>;

    /// Write the schema version to the backend for tracking.
    fn write_schema_version(&self, _version: u32) -> AegisResult<()>;

    /// Return the current revision token (revision + node_id + timestamp).
    fn current_token(&self) -> AegisResult<RevisionToken>;

    /// Begin a transaction. Returns a transaction handle.
    fn begin_transaction(&self, partition_id: &PartitionId) -> AegisResult<Box<dyn StorageTransaction>>;

    /// Query audit log for a given object (or all objects if None) within a partition.
    fn query_audit(
        &self,
        partition_id: &PartitionId,
        object: Option<&ResourceId>,
        from_revision: Option<Revision>,
        to_revision: Option<Revision>,
        pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>>;

    /// Return the backend type identifier.
    fn backend_type(&self) -> BackendType;

    /// Run a storage-level integrity check.
    fn integrity_check(&self) -> AegisResult<IntegrityReport>;

    /// Delete audit events older than the given cutoff timestamp within a partition.
    /// Returns the number of deleted events.
    fn delete_events_before(&self, partition_id: &PartitionId, _cutoff: DateTime<Utc>) -> AegisResult<usize>;

    /// Compact paired add/remove events to reduce audit log size.
    /// Only meaningful for backends that track individual events (SQLite, PostgreSQL).
    /// Returns the number of removed events.
    fn compact_events(&self, partition_id: &PartitionId) -> AegisResult<usize>;

    /// Permanently remove soft-deleted tuples whose deletion revision
    /// corresponds to a timestamp before the given cutoff within a partition.
    /// Returns the number of deleted tuples.
    fn delete_soft_deleted_tuples_before(&self, partition_id: &PartitionId, _cutoff: DateTime<Utc>) -> AegisResult<usize>;

    /// Recover the current state by replaying all logged events within a partition.
    /// This reconstructs the tuple store from scratch using the event log,
    /// returning the latest revision seen.
    fn recover_from_events(&self, partition_id: &PartitionId, to_revision: Option<Revision>) -> AegisResult<Revision>;

    /// Return a version string for the storage backend (e.g. "3.45.1").
    fn storage_version(&self) -> Option<String> {
        None
    }

    /// Return current connection pool statistics.
    fn connection_stats(&self) -> ConnectionStats {
        ConnectionStats {
            read_active: 0,
            read_idle: 0,
            write_busy: false,
        }
    }

    /// Return WAL size in megabytes, if applicable.
    fn wal_size_mb(&self) -> Option<f64> {
        None
    }

    /// Close the storage backend, flushing all pending operations.
    fn close(&self) -> AegisResult<()>;
}

/// A storage transaction supporting atomic multi-tuple writes within a partition.
pub trait StorageTransaction: Send {
    /// Write a tuple within this transaction.
    fn write(&mut self, partition_id: &PartitionId, tuple: &RelationshipTuple) -> AegisResult<()>;

    /// Delete a tuple within this transaction.
    fn delete(&mut self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<()>;

    /// Create a named savepoint within the transaction.
    fn savepoint(&self, name: &str) -> AegisResult<()>;

    /// Roll back to a named savepoint without ending the transaction.
    fn rollback_to_savepoint(&self, name: &str) -> AegisResult<()>;

    /// Release (forget) a named savepoint.
    fn release_savepoint(&self, name: &str) -> AegisResult<()>;

    /// Commit the transaction.
    fn commit(self: Box<Self>) -> AegisResult<Revision>;

    /// Roll back the transaction.
    fn rollback(self: Box<Self>) -> AegisResult<()>;
}

/// Metadata returned by storage initialization.
#[derive(Debug, Clone)]
pub struct StorageMeta {
    pub schema_version: u32,
    pub current_revision: Revision,
    pub backend_type: BackendType,
    pub healthy: bool,
}

/// The type of storage backend in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    Sqlite,
    Postgres,
    Mysql,
    RocksDB,
    IndexedDB,
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite => write!(f, "sqlite"),
            Self::Postgres => write!(f, "postgres"),
            Self::Mysql => write!(f, "mysql"),
            Self::RocksDB => write!(f, "rocksdb"),
            Self::IndexedDB => write!(f, "indexeddb"),
        }
    }
}

/// Filter parameters for querying tuples.
#[derive(Debug, Clone, Default)]
pub struct TupleFilter {
    pub partition_id: Option<String>,
    pub subject_type: Option<String>,
    pub relation: Option<Relation>,
    pub object_type: Option<String>,
    pub metadata_key: Option<String>,
    pub metadata_value: Option<String>,
    pub namespace: Option<String>,
}

/// Result of an integrity check.
#[derive(Debug, Clone)]
pub struct IntegrityReport {
    pub passed: bool,
    pub details: Vec<String>,
    pub backend_type: BackendType,
}
