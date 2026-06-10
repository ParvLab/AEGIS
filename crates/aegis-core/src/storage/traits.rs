use chrono::{DateTime, Utc};
use crate::error::AegisResult;
use crate::types::{
    AuditEntry, ConsistencyMode, PaginatedTuples, PaginationParams, Relation, RelationshipTuple,
    ResourceId, Revision, RevisionToken, SubjectId, TupleKey,
};

/// Pluggable storage backend for relationship tuples.
///
/// Each backend (SQLite, PostgreSQL, RocksDB, IndexedDB) implements this trait.
/// The trait is designed for:
/// - Single-process single-writer (serialized writes)
/// - Multiple concurrent readers
/// - Revision-based snapshot isolation
pub trait StorageBackend: Send + Sync {
    /// Initialize the storage backend.
    /// Creates tables, applies migrations, verifies integrity.
    fn initialize(&mut self) -> AegisResult<StorageMeta>;

    /// Write a single relationship tuple.
    /// Returns the new revision number.
    fn write_tuple(&self, tuple: &RelationshipTuple) -> AegisResult<Revision>;

    /// Write multiple tuples atomically within a single transaction.
    fn write_tuples_batch(&self, tuples: &[RelationshipTuple]) -> AegisResult<Revision>;

    /// Delete a single relationship tuple by key.
    fn delete_tuple(&self, key: &TupleKey) -> AegisResult<Revision>;

    /// Delete all tuples for a given subject.
    fn delete_subject(&self, subject: &SubjectId) -> AegisResult<Revision>;

    /// Delete all tuples for a given resource.
    fn delete_object(&self, object: &ResourceId) -> AegisResult<Revision>;

    /// Check if a tuple exists.
    fn has_tuple(&self, key: &TupleKey) -> AegisResult<bool>;

    /// Read a single tuple by key.
    fn read_tuple(&self, key: &TupleKey) -> AegisResult<Option<RelationshipTuple>>;

    /// List all tuples for a given object.
    fn list_by_object(
        &self,
        object: &ResourceId,
        relation: Option<&Relation>,
    ) -> AegisResult<Vec<RelationshipTuple>>;

    /// List all tuples for a given subject.
    fn list_by_subject(
        &self,
        subject: &SubjectId,
        relation: Option<&Relation>,
    ) -> AegisResult<Vec<RelationshipTuple>>;

    /// List all tuples matching a relation on an object.
    fn list_by_relation(
        &self,
        object: &ResourceId,
        relation: &Relation,
    ) -> AegisResult<Vec<RelationshipTuple>>;

    /// Paginated query with filters.
    fn query_tuples(
        &self,
        filter: &TupleFilter,
        pagination: &PaginationParams,
        consistency: &ConsistencyMode,
    ) -> AegisResult<PaginatedTuples>;

    /// Get the current revision number.
    fn current_revision(&self) -> AegisResult<Revision>;

    /// Read the stored schema version from the backend.
    /// Returns 0 if no schema version has been recorded.
    fn read_schema_version(&self) -> AegisResult<u32> {
        Ok(0)
    }

    /// Write the schema version to the backend for tracking.
    fn write_schema_version(&self, _version: u32) -> AegisResult<()> {
        Ok(())
    }

    /// Return the current revision token (revision + node_id + timestamp).
    fn current_token(&self) -> AegisResult<RevisionToken>;

    /// Begin a transaction. Returns a transaction handle.
    fn begin_transaction(&self) -> AegisResult<Box<dyn StorageTransaction>>;

    /// Query audit log for a given object within a time range.
    fn query_audit(
        &self,
        object: &ResourceId,
        from_revision: Option<Revision>,
        to_revision: Option<Revision>,
        pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>>;

    /// Return the backend type identifier.
    fn backend_type(&self) -> BackendType;

    /// Run a storage-level integrity check.
    fn integrity_check(&self) -> AegisResult<IntegrityReport>;

    /// Delete audit events older than the given cutoff timestamp.
    /// Returns the number of deleted events.
    fn delete_events_before(&self, _cutoff: DateTime<Utc>) -> AegisResult<usize> {
        Ok(0)
    }

    /// Permanently remove soft-deleted tuples whose deletion revision
    /// corresponds to a timestamp before the given cutoff.
    /// Returns the number of deleted tuples.
    fn delete_soft_deleted_tuples_before(&self, _cutoff: DateTime<Utc>) -> AegisResult<usize> {
        Ok(0)
    }

    /// Close the storage backend, flushing all pending operations.
    fn close(&self) -> AegisResult<()>;
}

/// A storage transaction supporting atomic multi-tuple writes.
pub trait StorageTransaction: Send {
    /// Write a tuple within this transaction.
    fn write(&mut self, tuple: &RelationshipTuple) -> AegisResult<()>;

    /// Delete a tuple within this transaction.
    fn delete(&mut self, key: &TupleKey) -> AegisResult<()>;

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
    pub subject_type: Option<String>,
    pub relation: Option<Relation>,
    pub object_type: Option<String>,
    pub metadata_key: Option<String>,
    pub metadata_value: Option<String>,
}

/// Result of an integrity check.
#[derive(Debug, Clone)]
pub struct IntegrityReport {
    pub passed: bool,
    pub details: Vec<String>,
    pub backend_type: BackendType,
}
