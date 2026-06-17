use crate::error::{AegisError, AegisResult};
use crate::storage::traits::{
    BackendType, IntegrityReport, PolicyVersion, StorageBackend, StorageMeta, StorageTransaction,
    TupleFilter,
};
    use crate::types::{
        AuditEntry, ConnectionStats, ConsistencyMode, PaginatedTuples, PaginationCursor, PaginationParams, PartitionId, Relation,
        RelationshipTuple, ResourceId, Revision, RevisionToken, SubjectId, TupleKey, TupleMutation,
    };
use chrono::{DateTime, Utc};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection};
use serde_json;
use std::collections::HashMap;
use uuid::Uuid;

// ── DDL Constants ──────────────────────────────────────────────

const META_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS _aegis_meta (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    )";

const TUPLES_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS _aegis_tuples (
        row_id           INTEGER PRIMARY KEY AUTOINCREMENT,
        subject          TEXT NOT NULL,
        relation         TEXT NOT NULL,
        object           TEXT NOT NULL,
        partition_id     TEXT NOT NULL DEFAULT 'default',
        created_at       TEXT NOT NULL,
        metadata         TEXT,
        valid_until      TEXT,
        revision_added   INTEGER NOT NULL,
        revision_removed INTEGER DEFAULT NULL
    )";

const TUPLES_ACTIVE_IDX: &str =
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_tuples_active ON _aegis_tuples(subject, relation, object) WHERE revision_removed IS NULL";
const TUPLES_OBJECT_IDX: &str =
    "CREATE INDEX IF NOT EXISTS idx_tuples_object ON _aegis_tuples(object)";
const TUPLES_SUBJECT_IDX: &str =
    "CREATE INDEX IF NOT EXISTS idx_tuples_subject ON _aegis_tuples(subject)";
const TUPLES_OBJ_REL_IDX: &str =
    "CREATE INDEX IF NOT EXISTS idx_tuples_object_relation ON _aegis_tuples(object, relation)";
const TUPLES_SUB_REL_IDX: &str =
    "CREATE INDEX IF NOT EXISTS idx_tuples_subject_relation ON _aegis_tuples(subject, relation)";
const TUPLES_PARTITION_IDX: &str =
    "CREATE INDEX IF NOT EXISTS idx_tuples_partition ON _aegis_tuples(partition_id)";

const EVENTS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS _aegis_events (
        event_id      INTEGER PRIMARY KEY AUTOINCREMENT,
        revision      INTEGER NOT NULL,
        action        TEXT NOT NULL,
        subject       TEXT NOT NULL,
        relation      TEXT NOT NULL,
        object        TEXT NOT NULL,
        partition_id  TEXT NOT NULL DEFAULT 'default',
        metadata      TEXT,
        timestamp     TEXT NOT NULL,
        identity      TEXT,
        previous_hash TEXT NOT NULL DEFAULT '',
        event_hash    TEXT NOT NULL DEFAULT ''
    )";

const EVENTS_HASH_IDX: &str =
    "CREATE INDEX IF NOT EXISTS idx_events_event_hash ON _aegis_events(event_hash)";

const SCHEMA_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS _aegis_schema (
        version    INTEGER NOT NULL,
        applied_at TEXT NOT NULL,
        checksum   TEXT NOT NULL
    )";

// ── Config ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SqliteConfig {
    pub path: String,
    pub max_readers: u32,
    pub busy_timeout_ms: u32,
    pub wal_mode: bool,
    pub mmap_size: u64,
}

impl Default for SqliteConfig {
    fn default() -> Self {
        Self {
            path: "aegis.db".to_string(),
            max_readers: 4,
            busy_timeout_ms: 5000,
            wal_mode: true,
            mmap_size: 0,
        }
    }
}

impl SqliteConfig {
    pub fn in_memory() -> Self {
        Self {
            path: ":memory:".to_string(),
            max_readers: 4,
            busy_timeout_ms: 5000,
            wal_mode: false,
            mmap_size: 0,
        }
    }
}

// ── Connection Customizer ─────────────────────────────────────

#[derive(Debug)]
struct SqliteConnectionConfigurator {
    config: SqliteConfig,
}

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for SqliteConnectionConfigurator {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch(&format!("PRAGMA busy_timeout = {};", self.config.busy_timeout_ms))?;
        if self.config.wal_mode {
            conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        }
        conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        if self.config.mmap_size > 0 {
            conn.execute_batch(&format!("PRAGMA mmap_size = {};", self.config.mmap_size))?;
        }
        Ok(())
    }
}

// ── Storage Adapter ────────────────────────────────────────────

pub struct SqliteStorage {
    pool: Pool<SqliteConnectionManager>,
    config: SqliteConfig,
    node_id: Uuid,
    actor_identity: std::sync::Mutex<Option<String>>,
}

impl SqliteStorage {
    /// Open a new SQLite storage backend.
    ///
    /// Creates the connection pool, runs DDL, and verifies integrity.
    /// Does NOT apply schema migrations (call `initialize()` for that).
    pub fn new(config: SqliteConfig) -> AegisResult<Self> {
        let manager = if config.path == ":memory:" {
            SqliteConnectionManager::memory()
        } else {
            SqliteConnectionManager::file(&config.path)
        };

        let customizer = SqliteConnectionConfigurator { config: config.clone() };
        let pool = Pool::builder()
            .max_size(config.max_readers + 1) // +1 for potential write connection
            .connection_customizer(Box::new(customizer))
            .build(manager)
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        let node_id = Uuid::new_v4();

        let storage = Self {
            pool,
            config,
            node_id,
            actor_identity: std::sync::Mutex::new(None),
        };

        // Configure all initial connections
        storage.configure_all_connections()?;

        Ok(storage)
    }

    /// Run DDL to create all required tables and indexes.
    fn run_ddl(conn: &Connection) -> AegisResult<()> {
        conn.execute_batch(META_TABLE)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        conn.execute_batch(TUPLES_TABLE)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        conn.execute_batch(TUPLES_OBJECT_IDX)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        conn.execute_batch(TUPLES_SUBJECT_IDX)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        conn.execute_batch(TUPLES_ACTIVE_IDX)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        conn.execute_batch(TUPLES_OBJ_REL_IDX)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        conn.execute_batch(TUPLES_SUB_REL_IDX)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        conn.execute_batch(TUPLES_PARTITION_IDX)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        conn.execute_batch(EVENTS_TABLE)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        conn.execute_batch(EVENTS_HASH_IDX)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        conn.execute_batch(SCHEMA_TABLE)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        // V2: Add valid_until and condition columns (no-op if columns already exist)
        let _ = conn.execute_batch("ALTER TABLE _aegis_tuples ADD COLUMN valid_until TEXT");
        let _ = conn.execute_batch("ALTER TABLE _aegis_tuples ADD COLUMN condition TEXT");
        // V3: Add audit hash columns (no-op if columns already exist)
        let _ = conn.execute_batch("ALTER TABLE _aegis_events ADD COLUMN previous_hash TEXT NOT NULL DEFAULT ''");
        let _ = conn.execute_batch("ALTER TABLE _aegis_events ADD COLUMN event_hash TEXT NOT NULL DEFAULT ''");
        Ok(())
    }

    /// Configure PRAGMA settings on a connection.
    /// (Connection customizer handles PRAGMAs on every checkout; this exists
    /// for the initial DDL connection before the pool is fully established.)
    fn configure_connection(conn: &Connection, config: &SqliteConfig) -> AegisResult<()> {
        conn.execute_batch(&format!(
            "PRAGMA busy_timeout = {};",
            config.busy_timeout_ms
        ))
        .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        if config.wal_mode {
            conn.execute_batch("PRAGMA journal_mode = WAL;")
                .map_err(|e| AegisError::StorageConnection(e.to_string()))?;
        }

        conn.execute_batch("PRAGMA synchronous = NORMAL;")
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        if config.mmap_size > 0 {
            conn.execute_batch(&format!("PRAGMA mmap_size = {};", config.mmap_size))
                .map_err(|e| AegisError::StorageConnection(e.to_string()))?;
        }

        Ok(())
    }

    fn configure_all_connections(&self) -> AegisResult<()> {
        // Get a connection to configure and run DDL
        let conn = self
            .pool
            .get()
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        Self::configure_connection(&conn, &self.config)?;
        Self::run_ddl(&conn)?;

        // Initialize meta revision if not exists
        conn.execute(
            "INSERT OR IGNORE INTO _aegis_meta (key, value) VALUES ('revision', '0')",
            [],
        )
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Ok(())
    }

    /// Get a connection from the pool.
    fn conn(&self) -> AegisResult<r2d2::PooledConnection<SqliteConnectionManager>> {
        self.pool
            .get()
            .map_err(|e| AegisError::StorageConnection(e.to_string()))
    }

    /// Bump the revision counter and return the new value.
    fn bump_revision(conn: &Connection) -> AegisResult<Revision> {
        conn.execute(
            "UPDATE _aegis_meta SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'revision'",
            [],
        )
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let rev: i64 = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM _aegis_meta WHERE key = 'revision'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Ok(Revision::new(rev as u64))
    }

    /// Read the current revision without bumping.
    fn read_revision(conn: &Connection) -> AegisResult<Revision> {
        let rev: i64 = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM _aegis_meta WHERE key = 'revision'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(Revision::new(rev as u64))
    }

    /// Append an event to the event log, computing hash-chained integrity fields.
    fn append_event(
        conn: &Connection,
        revision: Revision,
        action: &str,
        subject: &str,
        relation: &str,
        object: &str,
        partition_id: &str,
        metadata: Option<&str>,
        identity: Option<&str>,
    ) -> AegisResult<()> {
        let now = Utc::now().to_rfc3339();
        let previous_hash: String = conn
            .query_row(
                "SELECT COALESCE((SELECT event_hash FROM _aegis_events ORDER BY event_id DESC LIMIT 1), '')",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let event_hash = crate::storage::compute_event_hash(
            &previous_hash,
            revision.as_u64() as i64,
            action,
            subject,
            relation,
            object,
            partition_id,
            metadata,
            &now,
            identity,
        );
        conn.execute(
            "INSERT INTO _aegis_events (revision, action, subject, relation, object, partition_id, metadata, timestamp, identity, previous_hash, event_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                revision.as_u64() as i64,
                action,
                subject,
                relation,
                object,
                partition_id,
                metadata,
                now,
                identity,
                previous_hash,
                event_hash,
            ],
        )
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    /// Read tuples at a specific revision (for revision-based snapshots).
    /// Returns tuples that were active (revision_added <= target) and not yet removed
    /// (revision_removed IS NULL OR revision_removed > target) at that revision.
    pub fn read_tuples_at_revision(
        conn: &Connection,
        target_revision: i64,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let mut stmt = conn
            .prepare(
                "SELECT subject, relation, object, created_at, metadata, valid_until, condition
                 FROM _aegis_tuples
                 WHERE revision_added <= ?1
                   AND (revision_removed IS NULL OR revision_removed > ?1)
                 ORDER BY subject, relation, object",
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let rows = stmt
                .query_map(params![target_revision], |row| {
                let subject_str: String = row.get(0)?;
                let relation_str: String = row.get(1)?;
                let object_str: String = row.get(2)?;
                let created_at_str: String = row.get(3)?;
                let metadata_json: Option<String> = row.get(4)?;
                let valid_until_str: Option<String> = row.get(5)?;
                let condition_str: Option<String> = row.get(6)?;

                let subject = SubjectId::new(&subject_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let relation = Relation::new(&relation_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let object = ResourceId::new(&object_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata = metadata_json
                    .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());
                let valid_until = valid_until_str.and_then(|s| s.parse::<DateTime<Utc>>().ok());

                Ok(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at,
                    metadata,
                    valid_until,
                    condition: condition_str,
                })
            })
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| AegisError::StorageQuery(e.to_string()))?);
        }
        Ok(results)
    }

    /// Execute a write operation within an immediate-mode transaction.
    fn with_write_tx<F, T>(&self, f: F) -> AegisResult<T>
    where
        F: FnOnce(&Connection) -> AegisResult<T>,
    {
        let conn = self.conn()?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        match f(&conn) {
            Ok(result) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                Ok(result)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
}

impl StorageBackend for SqliteStorage {
    fn backend_type(&self) -> BackendType {
        BackendType::Sqlite
    }

    fn set_actor_identity(&self, identity: Option<String>) -> Option<String> {
        let mut guard = self.actor_identity.lock().unwrap();
        let prev = guard.take();
        *guard = identity;
        prev
    }

    fn initialize(&mut self) -> AegisResult<StorageMeta> {
        let conn = self.conn()?;
        let current_revision = Self::read_revision(&conn)?;

        // Run integrity check
        let healthy = conn
            .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
            .map(|s| s == "ok")
            .unwrap_or(false);

        Ok(StorageMeta {
            schema_version: 1,
            current_revision,
            backend_type: BackendType::Sqlite,
            healthy,
        })
    }

    fn write_tuple(&self, partition_id: &PartitionId, tuple: &RelationshipTuple) -> AegisResult<Revision> {
        self.with_write_tx(|conn| {
            let revision = Self::bump_revision(conn)?;
            let metadata_json = tuple
                .metadata
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;

            conn.execute(
                "UPDATE _aegis_tuples SET revision_removed = ?1
                 WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND partition_id = ?5 AND revision_removed IS NULL",
                params![
                    revision.as_u64() as i64,
                    tuple.subject.as_str(),
                    tuple.relation.as_str(),
                    tuple.object.as_str(),
                    partition_id.as_str(),
                ],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let valid_until_str = tuple.valid_until.map(|v| v.to_rfc3339());
            let condition_str = tuple.condition.as_deref();
            conn.execute(
                "INSERT INTO _aegis_tuples (subject, relation, object, partition_id, created_at, metadata, valid_until, condition, revision_added, revision_removed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL)",
                params![
                    tuple.subject.as_str(),
                    tuple.relation.as_str(),
                    tuple.object.as_str(),
                    partition_id.as_str(),
                    tuple.created_at.to_rfc3339(),
                    metadata_json,
                    valid_until_str,
                    condition_str,
                    revision.as_u64() as i64,
                ],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let identity = self.actor_identity.lock().unwrap().clone();
            Self::append_event(
                conn,
                revision,
                "add",
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
                partition_id.as_str(),
                metadata_json.as_deref(),
                identity.as_deref(),
            )?;

            Ok(revision)
        })
    }

    fn write_tuples_batch(&self, partition_id: &PartitionId, tuples: &[RelationshipTuple]) -> AegisResult<Revision> {
        if tuples.is_empty() {
            return self.current_revision(partition_id);
        }

        self.with_write_tx(|conn| {
            let revision = Self::bump_revision(conn)?;

            for tuple in tuples {
                let metadata_json = tuple
                    .metadata
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;

                conn.execute(
                    "UPDATE _aegis_tuples SET revision_removed = ?1
                     WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND partition_id = ?5 AND revision_removed IS NULL",
                    params![
                        revision.as_u64() as i64,
                        tuple.subject.as_str(),
                        tuple.relation.as_str(),
                        tuple.object.as_str(),
                        partition_id.as_str(),
                    ],
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

                let valid_until_str = tuple.valid_until.map(|v| v.to_rfc3339());
                let condition_str = tuple.condition.as_deref();
                conn.execute(
                    "INSERT INTO _aegis_tuples (subject, relation, object, partition_id, created_at, metadata, valid_until, condition, revision_added, revision_removed)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL)",
                    params![
                        tuple.subject.as_str(),
                        tuple.relation.as_str(),
                        tuple.object.as_str(),
                        partition_id.as_str(),
                        tuple.created_at.to_rfc3339(),
                        metadata_json,
                        valid_until_str,
                        condition_str,
                        revision.as_u64() as i64,
                    ],
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

                let identity = self.actor_identity.lock().unwrap().clone();
                Self::append_event(
                    conn,
                    revision,
                    "add",
                    tuple.subject.as_str(),
                    tuple.relation.as_str(),
                    tuple.object.as_str(),
                    partition_id.as_str(),
                    metadata_json.as_deref(),
                    identity.as_deref(),
                )?;
            }

            Ok(revision)
        })
    }

    fn delete_tuple(&self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<Revision> {
        self.with_write_tx(|conn| {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM _aegis_tuples
                     WHERE subject = ?1 AND relation = ?2 AND object = ?3 AND partition_id = ?4 AND revision_removed IS NULL",
                    params![key.subject.as_str(), key.relation.as_str(), key.object.as_str(), partition_id.as_str()],
                    |row| row.get::<_, i64>(0),
                )
                .map(|count| count > 0)
                .unwrap_or(false);

            if !exists {
                return Self::read_revision(conn);
            }

            let revision = Self::bump_revision(conn)?;

            conn.execute(
                "UPDATE _aegis_tuples SET revision_removed = ?1
                 WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND partition_id = ?5 AND revision_removed IS NULL",
                params![
                    revision.as_u64() as i64,
                    key.subject.as_str(),
                    key.relation.as_str(),
                    key.object.as_str(),
                    partition_id.as_str(),
                ],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let identity = self.actor_identity.lock().unwrap().clone();
            Self::append_event(
                conn,
                revision,
                "remove",
                key.subject.as_str(),
                key.relation.as_str(),
                key.object.as_str(),
                partition_id.as_str(),
                None,
                identity.as_deref(),
            )?;

            Ok(revision)
        })
    }

    fn delete_subject(&self, partition_id: &PartitionId, subject: &SubjectId) -> AegisResult<Revision> {
        self.with_write_tx(|conn| {
            let revision = Self::bump_revision(conn)?;

            let mut stmt = conn
                .prepare(
                    "SELECT relation, object FROM _aegis_tuples
                     WHERE subject = ?1 AND partition_id = ?2 AND revision_removed IS NULL",
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let tuples: Vec<(String, String)> = stmt
                .query_map(params![subject.as_str(), partition_id.as_str()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            drop(stmt);

            conn.execute(
                "UPDATE _aegis_tuples SET revision_removed = ?1
                 WHERE subject = ?2 AND partition_id = ?3 AND revision_removed IS NULL",
                params![revision.as_u64() as i64, subject.as_str(), partition_id.as_str()],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let identity = self.actor_identity.lock().unwrap().clone();
            for (relation, object) in &tuples {
                Self::append_event(
                    conn,
                    revision,
                    "remove",
                    subject.as_str(),
                    relation,
                    object,
                    partition_id.as_str(),
                    None,
                    identity.as_deref(),
                )?;
            }

            Ok(revision)
        })
    }

    fn delete_object(&self, partition_id: &PartitionId, object: &ResourceId) -> AegisResult<Revision> {
        self.with_write_tx(|conn| {
            let revision = Self::bump_revision(conn)?;

            let mut stmt = conn
                .prepare(
                    "SELECT subject, relation FROM _aegis_tuples
                     WHERE object = ?1 AND partition_id = ?2 AND revision_removed IS NULL",
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let tuples: Vec<(String, String)> = stmt
                .query_map(params![object.as_str(), partition_id.as_str()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            drop(stmt);

            conn.execute(
                "UPDATE _aegis_tuples SET revision_removed = ?1
                 WHERE object = ?2 AND partition_id = ?3 AND revision_removed IS NULL",
                params![revision.as_u64() as i64, object.as_str(), partition_id.as_str()],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let identity = self.actor_identity.lock().unwrap().clone();
            for (subject, relation) in &tuples {
                Self::append_event(
                    conn,
                    revision,
                    "remove",
                    subject,
                    relation,
                    object.as_str(),
                    partition_id.as_str(),
                    None,
                    identity.as_deref(),
                )?;
            }

            Ok(revision)
        })
    }

    fn has_tuple(&self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<bool> {
        let conn = self.conn()?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM _aegis_tuples
                 WHERE subject = ?1 AND relation = ?2 AND object = ?3 AND partition_id = ?4 AND revision_removed IS NULL",
                params![key.subject.as_str(), key.relation.as_str(), key.object.as_str(), partition_id.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(count > 0)
    }

    fn read_tuple(&self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<Option<RelationshipTuple>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT subject, relation, object, created_at, metadata, valid_until, condition, revision_added
                 FROM _aegis_tuples
                 WHERE subject = ?1 AND relation = ?2 AND object = ?3 AND partition_id = ?4 AND revision_removed IS NULL",
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let result = stmt
            .query_row(
                params![key.subject.as_str(), key.relation.as_str(), key.object.as_str(), partition_id.as_str()],
                |row| {
                    let subject_str: String = row.get(0)?;
                    let relation_str: String = row.get(1)?;
                    let object_str: String = row.get(2)?;
                    let created_at_str: String = row.get(3)?;
                    let metadata_json: Option<String> = row.get(4)?;
                    let valid_until_str: Option<String> = row.get(5)?;
                    let condition_str: Option<String> = row.get(6)?;

                    let subject = SubjectId::new(&subject_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let relation = Relation::new(&relation_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let object = ResourceId::new(&object_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                    let metadata = metadata_json
                        .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());
                    let valid_until = valid_until_str.and_then(|s| s.parse::<DateTime<Utc>>().ok());

                    Ok(RelationshipTuple {
                        subject,
                        relation,
                        object,
                        created_at,
                        metadata,
                        valid_until,
                        condition: condition_str,
                    })
                },
            );

        match result {
            Ok(tuple) => Ok(Some(tuple)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AegisError::StorageQuery(e.to_string())),
        }
    }

    fn list_by_object(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: Option<&Relation>,
        consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let conn = self.conn()?;

        if *consistency == ConsistencyMode::FullyConsistent && self.config.wal_mode {
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        }

        let revision_filter = match consistency {
            ConsistencyMode::AtRevision(rev) => {
                let r = rev.as_u64() as i64;
                format!("revision_added <= {r} AND (revision_removed IS NULL OR revision_removed > {r})")
            }
            _ => "revision_removed IS NULL".to_string(),
        };
        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(rel) = relation {
            (
                format!(
                    "SELECT subject, relation, object, created_at, metadata, valid_until, condition FROM _aegis_tuples
                     WHERE object = ?1 AND relation = ?2 AND partition_id = ?3 AND {revision_filter}"
                ),
                vec![
                    Box::new(object.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(rel.as_str().to_string()),
                    Box::new(partition_id.as_str().to_string()),
                ],
            )
        } else {
            (
                format!(
                    "SELECT subject, relation, object, created_at, metadata, valid_until, condition FROM _aegis_tuples
                     WHERE object = ?1 AND partition_id = ?2 AND {revision_filter}"
                ),
                vec![
                    Box::new(object.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(partition_id.as_str().to_string()),
                ],
            )
        };

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let subject_str: String = row.get(0)?;
                let relation_str: String = row.get(1)?;
                let object_str: String = row.get(2)?;
                let created_at_str: String = row.get(3)?;
                let metadata_json: Option<String> = row.get(4)?;
                let valid_until_str: Option<String> = row.get(5)?;
                let condition_str: Option<String> = row.get(6)?;

                let subject = SubjectId::new(&subject_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let relation = Relation::new(&relation_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let object = ResourceId::new(&object_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata = metadata_json
                    .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());
                let valid_until = valid_until_str.and_then(|s| s.parse::<DateTime<Utc>>().ok());

                Ok(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at,
                    metadata,
                    valid_until,
                    condition: condition_str,
                })
            })
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(
                row.map_err(|e| AegisError::StorageQuery(e.to_string()))?,
            );
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
        let conn = self.conn()?;

        if *consistency == ConsistencyMode::FullyConsistent && self.config.wal_mode {
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        }

        let revision_filter = match consistency {
            ConsistencyMode::AtRevision(rev) => {
                let r = rev.as_u64() as i64;
                format!("revision_added <= {r} AND (revision_removed IS NULL OR revision_removed > {r})")
            }
            _ => "revision_removed IS NULL".to_string(),
        };
        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(rel) = relation {
            (
                format!(
                    "SELECT subject, relation, object, created_at, metadata, valid_until, condition FROM _aegis_tuples
                     WHERE subject = ?1 AND relation = ?2 AND partition_id = ?3 AND {revision_filter}"
                ),
                vec![
                    Box::new(subject.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(rel.as_str().to_string()),
                    Box::new(partition_id.as_str().to_string()),
                ],
            )
        } else {
            (
                format!(
                    "SELECT subject, relation, object, created_at, metadata, valid_until, condition FROM _aegis_tuples
                     WHERE subject = ?1 AND partition_id = ?2 AND {revision_filter}"
                ),
                vec![
                    Box::new(subject.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(partition_id.as_str().to_string()),
                ],
            )
        };

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let subject_str: String = row.get(0)?;
                let relation_str: String = row.get(1)?;
                let object_str: String = row.get(2)?;
                let created_at_str: String = row.get(3)?;
                let metadata_json: Option<String> = row.get(4)?;
                let valid_until_str: Option<String> = row.get(5)?;
                let condition_str: Option<String> = row.get(6)?;

                let subject = SubjectId::new(&subject_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let relation = Relation::new(&relation_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let object = ResourceId::new(&object_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata = metadata_json
                    .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());
                let valid_until = valid_until_str.and_then(|s| s.parse::<DateTime<Utc>>().ok());

                Ok(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at,
                    metadata,
                    valid_until,
                    condition: condition_str,
                })
            })
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(
                row.map_err(|e| AegisError::StorageQuery(e.to_string()))?,
            );
        }
        Ok(results)
    }

    fn list_by_relation(
        &self,
        partition_id: &PartitionId,
        object: &ResourceId,
        relation: &Relation,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT subject, relation, object, created_at, metadata, valid_until, condition FROM _aegis_tuples
                 WHERE object = ?1 AND relation = ?2 AND partition_id = ?3 AND revision_removed IS NULL",
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let rows = stmt
            .query_map(
                params![object.as_str(), relation.as_str(), partition_id.as_str()],
                |row| {
                    let subject_str: String = row.get(0)?;
                    let relation_str: String = row.get(1)?;
                    let object_str: String = row.get(2)?;
                    let created_at_str: String = row.get(3)?;
                    let metadata_json: Option<String> = row.get(4)?;
                    let valid_until_str: Option<String> = row.get(5)?;
                    let condition_str: Option<String> = row.get(6)?;

                    let subject = SubjectId::new(&subject_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let relation = Relation::new(&relation_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let object = ResourceId::new(&object_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                    let metadata = metadata_json
                        .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());
                    let valid_until = valid_until_str.and_then(|s| s.parse::<DateTime<Utc>>().ok());

                    Ok(RelationshipTuple {
                        subject,
                        relation,
                        object,
                        created_at,
                        metadata,
                        valid_until,
                        condition: condition_str,
                    })
                },
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(
                row.map_err(|e| AegisError::StorageQuery(e.to_string()))?,
            );
        }
        Ok(results)
    }

    fn query_tuples(
        &self,
        partition_id: &PartitionId,
        filter: &TupleFilter,
        pagination: &PaginationParams,
        consistency: &ConsistencyMode,
    ) -> AegisResult<PaginatedTuples> {
        let conn = self.conn()?;

        if *consistency == ConsistencyMode::FullyConsistent && self.config.wal_mode {
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        }
        let revision = Self::read_revision(&conn)?;

        let revision_filter = match consistency {
            ConsistencyMode::AtRevision(rev) => {
                let r = rev.as_u64() as i64;
                format!("revision_added <= {r} AND (revision_removed IS NULL OR revision_removed > {r})")
            }
            _ => "revision_removed IS NULL".to_string(),
        };
        let mut conditions = vec!["partition_id = ?1".to_string(), revision_filter];
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(partition_id.as_str().to_string())];

        if let Some(ref st) = filter.subject_type {
            params_vec.push(Box::new(format!("{st}:%")));
            conditions.push(format!("subject LIKE ?{}", params_vec.len()));
        }
        if let Some(ref rel) = filter.relation {
            params_vec.push(Box::new(rel.as_str().to_string()));
            conditions.push(format!("relation = ?{}", params_vec.len()));
        }
        if let Some(ref ot) = filter.object_type {
            params_vec.push(Box::new(format!("{ot}:%")));
            conditions.push(format!("object LIKE ?{}", params_vec.len()));
        }
        if let Some(ref mk) = filter.metadata_key {
            params_vec.push(Box::new(format!("%{mk}%")));
            conditions.push(format!("metadata LIKE ?{}", params_vec.len()));
        }

        let where_clause = conditions.join(" AND ");
        let offset = pagination
            .cursor
            .as_ref()
            .map(|c| c.offset)
            .unwrap_or(0);
        let limit = pagination.limit;

        let sql = format!(
            "SELECT subject, relation, object, created_at, metadata, valid_until, condition FROM _aegis_tuples
             WHERE {where_clause}
             ORDER BY subject, relation, object
             LIMIT ?{limit_idx} OFFSET ?{offset_idx}",
            limit_idx = params_vec.len() + 1,
            offset_idx = params_vec.len() + 2,
        );

        params_vec.push(Box::new(limit as i64));
        params_vec.push(Box::new(offset as i64));

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let subject_str: String = row.get(0)?;
                let relation_str: String = row.get(1)?;
                let object_str: String = row.get(2)?;
                let created_at_str: String = row.get(3)?;
                let metadata_json: Option<String> = row.get(4)?;
                let valid_until_str: Option<String> = row.get(5)?;
                let condition_str: Option<String> = row.get(6)?;

                let subject = SubjectId::new(&subject_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let relation = Relation::new(&relation_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let object = ResourceId::new(&object_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata = metadata_json
                    .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());
                let valid_until = valid_until_str.and_then(|s| s.parse::<DateTime<Utc>>().ok());

                Ok(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at,
                    metadata,
                    valid_until,
                    condition: condition_str,
                })
            })
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let mut tuples = Vec::new();
        for row in rows {
            tuples.push(row.map_err(|e| AegisError::StorageQuery(e.to_string()))?);
        }

        let next_cursor = if tuples.len() as u64 == limit {
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

    fn current_revision(&self, partition_id: &PartitionId) -> AegisResult<Revision> {
        let _ = partition_id;
        let conn = self.conn()?;
        Self::read_revision(&conn)
    }

    fn read_schema_version(&self) -> AegisResult<u32> {
        let conn = self.conn()?;
        let result: Result<u32, _> = conn.query_row(
            "SELECT version FROM _aegis_schema ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        );
        match result {
            Ok(v) => Ok(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(AegisError::StorageQuery(e.to_string())),
        }
    }

    fn write_schema_version(&self, version: u32) -> AegisResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO _aegis_schema (version, applied_at, checksum) VALUES (?1, ?2, ?3)",
            params![version as i64, Utc::now().to_rfc3339(), ""],
        )
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn current_token(&self) -> AegisResult<RevisionToken> {
        let revision = self.current_revision(&PartitionId::default())?;
        Ok(RevisionToken::new(revision, self.node_id))
    }

    fn begin_transaction(&self, partition_id: &PartitionId) -> AegisResult<Box<dyn StorageTransaction>> {
        let _ = partition_id;
        let conn = self.conn()?;
        let identity = self.actor_identity.lock().unwrap().clone();
        let tx = SqliteTransaction::new(conn, self.node_id, identity)?;
        Ok(Box::new(tx))
    }

    fn query_audit(
        &self,
        partition_id: &PartitionId,
        object: Option<&ResourceId>,
        from_revision: Option<Revision>,
        to_revision: Option<Revision>,
        pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>> {
        let conn = self.conn()?;
        let mut conditions: Vec<String> = vec!["partition_id = ?1".to_string()];
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(partition_id.as_str().to_string())];

        if let Some(obj) = object {
            params_vec.push(Box::new(obj.as_str().to_string()));
            conditions.push(format!("object = ?{}", params_vec.len()));
        }

        if let Some(from) = from_revision {
            params_vec.push(Box::new(from.as_u64() as i64));
            conditions.push(format!("revision >= ?{}", params_vec.len()));
        }
        if let Some(to) = to_revision {
            params_vec.push(Box::new(to.as_u64() as i64));
            conditions.push(format!("revision <= ?{}", params_vec.len()));
        }

        let where_clause = if conditions.is_empty() {
            "1=1".to_string()
        } else {
            conditions.join(" AND ")
        };
        let offset = pagination
            .cursor
            .as_ref()
            .map(|c| c.offset)
            .unwrap_or(0);
        let limit = pagination.limit;

        let sql = format!(
            "SELECT revision, action, subject, relation, object, timestamp, metadata, identity
             FROM _aegis_events
             WHERE {where_clause}
             ORDER BY revision ASC
             LIMIT ?{limit_idx} OFFSET ?{offset_idx}",
            limit_idx = params_vec.len() + 1,
            offset_idx = params_vec.len() + 2,
        );

        params_vec.push(Box::new(limit as i64));
        params_vec.push(Box::new(offset as i64));

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let revision: i64 = row.get(0)?;
                let action_str: String = row.get(1)?;
                let subject: String = row.get(2)?;
                let relation: String = row.get(3)?;
                let object: String = row.get(4)?;
                let timestamp_str: String = row.get(5)?;
                let metadata_json: Option<String> = row.get(6)?;
                let identity: Option<String> = row.get(7)?;

                let timestamp: DateTime<Utc> = timestamp_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata = metadata_json
                    .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());
                let action = if action_str == "add" {
                    crate::types::TupleMutation::Add
                } else {
                    crate::types::TupleMutation::Remove
                };

                Ok(AuditEntry {
                    revision: Revision::new(revision as u64),
                    action,
                    subject,
                    relation,
                    object,
                    timestamp,
                    metadata,
                    identity,
                })
            })
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| AegisError::StorageQuery(e.to_string()))?);
        }
        Ok(results)
    }

    fn integrity_check(&self) -> AegisResult<IntegrityReport> {
        let conn = self.conn()?;
        let result: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let mut details = Vec::new();
        for line in result.lines() {
            details.push(line.to_string());
        }

        let passed = details.iter().all(|d| d == "ok");

        Ok(IntegrityReport {
            passed,
            details,
            backend_type: BackendType::Sqlite,
            tenant_leakage_detected: false,
            leaked_crossings: vec![],
            orphaned_tuple_count: 0,
        })
    }

    fn storage_version(&self) -> Option<String> {
        let conn = self.conn().ok()?;
        conn.query_row("SELECT sqlite_version()", [], |row| row.get(0))
            .ok()
    }

    fn connection_stats(&self) -> ConnectionStats {
        let state = self.pool.state();
        crate::types::ConnectionStats {
            read_active: state.connections,
            read_idle: state.idle_connections,
            write_busy: false,
        }
    }

    fn wal_size_mb(&self) -> Option<f64> {
        if self.config.path == ":memory:" || self.config.path.is_empty() {
            return None;
        }
        let wal_path = format!("{}-wal", self.config.path);
        std::fs::metadata(&wal_path).ok().map(|m| m.len() as f64 / (1024.0 * 1024.0))
    }

    fn delete_events_before(&self, partition_id: &PartitionId, cutoff: DateTime<Utc>) -> AegisResult<usize> {
        let conn = self.conn()?;
        let cutoff_str = cutoff.to_rfc3339();
        let count = conn
            .execute(
                "DELETE FROM _aegis_events WHERE partition_id = ?1 AND timestamp < ?2",
                params![partition_id.as_str(), cutoff_str],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(count)
    }

    fn delete_soft_deleted_tuples_before(
        &self,
        partition_id: &PartitionId,
        cutoff: DateTime<Utc>,
    ) -> AegisResult<usize> {
        let conn = self.conn()?;
        let cutoff_str = cutoff.to_rfc3339();
        let count = conn
            .execute(
                "DELETE FROM _aegis_tuples
                 WHERE partition_id = ?1
                   AND revision_removed IS NOT NULL
                   AND revision_removed <= (
                     SELECT COALESCE(MAX(revision), 0) FROM _aegis_events
                     WHERE partition_id = ?1 AND timestamp < ?2
                   )",
                params![partition_id.as_str(), cutoff_str],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(count)
    }

    fn compact_events(&self, partition_id: &PartitionId) -> AegisResult<usize> {
        SqliteStorage::compact_events(self, partition_id)
    }

    fn close(&self) -> AegisResult<()> {
        if self.config.wal_mode && self.config.path != ":memory:" {
            if let Ok(conn) = self.pool.get() {
                let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            }
        }
        Ok(())
    }

    fn restore_backup(
        &self,
        partition_id: &PartitionId,
        tuples: &[RelationshipTuple],
        events: &[AuditEntry],
        revision: Revision,
    ) -> AegisResult<()> {
        self.with_write_tx(|conn| {
            conn.execute("DELETE FROM _aegis_tuples", [])
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            conn.execute("DELETE FROM _aegis_events", [])
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            for tuple in tuples {
                let metadata_json = tuple.metadata
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;
                let valid_until_str = tuple.valid_until.map(|v| v.to_rfc3339());
                let condition_str = tuple.condition.as_deref();
                conn.execute(
                    "INSERT INTO _aegis_tuples (subject, relation, object, partition_id, created_at, metadata, valid_until, condition, revision_added, revision_removed)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL)",
                    params![
                        tuple.subject.as_str(),
                        tuple.relation.as_str(),
                        tuple.object.as_str(),
                        partition_id.as_str(),
                        tuple.created_at.to_rfc3339(),
                        metadata_json,
                        valid_until_str,
                        condition_str,
                        revision.as_u64() as i64,
                    ],
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            }

            for event in events {
                let action_str = match event.action {
                    TupleMutation::Add => "add",
                    TupleMutation::Remove => "remove",
                };
                let metadata_json = event.metadata
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;
                conn.execute(
                    "INSERT INTO _aegis_events (revision, action, subject, relation, object, partition_id, metadata, timestamp, identity, previous_hash, event_hash)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '', '')",
                    params![
                        event.revision.as_u64() as i64,
                        action_str,
                        event.subject,
                        event.relation,
                        event.object,
                        partition_id.as_str(),
                        metadata_json,
                        event.timestamp.to_rfc3339(),
                        event.identity,
                    ],
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            }

            conn.execute(
                "UPDATE _aegis_meta SET value = ?1 WHERE key = 'revision'",
                params![revision.as_u64() as i64],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            Ok(())
        })
    }

    fn recover_from_events(&self, partition_id: &PartitionId, to_revision: Option<Revision>) -> AegisResult<Revision> {
        self.recover_from_events_impl(partition_id, to_revision)
    }

    fn verify_audit_chain(&self, partition_id: &PartitionId) -> AegisResult<Option<String>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT event_id, revision, action, subject, relation, object, partition_id, metadata, timestamp, identity, previous_hash, event_hash
                 FROM _aegis_events
                 WHERE partition_id = ?1
                 ORDER BY event_id ASC",
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let rows = stmt
            .query_map(params![partition_id.as_str()], |row| {
                let event_id: i64 = row.get(0)?;
                let revision: i64 = row.get(1)?;
                let action: String = row.get(2)?;
                let subject: String = row.get(3)?;
                let relation: String = row.get(4)?;
                let object: String = row.get(5)?;
                let pid: String = row.get(6)?;
                let metadata: Option<String> = row.get(7)?;
                let timestamp: String = row.get(8)?;
                let identity: Option<String> = row.get(9)?;
                let previous_hash: String = row.get(10)?;
                let event_hash: String = row.get(11)?;
                Ok((event_id, revision, action, subject, relation, object, pid, metadata, timestamp, identity, previous_hash, event_hash))
            })
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let mut last_event_hash = String::new();
        for row in rows {
            let (event_id, revision, action, subject, relation, object, pid, metadata, timestamp, identity, prev_hash, event_hash) =
                row.map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            if prev_hash != last_event_hash {
                return Ok(Some(format!(
                    "Chain break at event_id={}: expected previous_hash='{}', got '{}'",
                    event_id, last_event_hash, prev_hash
                )));
            }

            let expected = crate::storage::compute_event_hash(
                &last_event_hash,
                revision,
                &action,
                &subject,
                &relation,
                &object,
                &pid,
                metadata.as_deref(),
                &timestamp,
                identity.as_deref(),
            );

            if expected != event_hash {
                return Ok(Some(format!(
                    "Hash mismatch at event_id={}: expected '{}', got '{}'",
                    event_id, expected, event_hash
                )));
            }

            last_event_hash = event_hash;
        }

        Ok(None)
    }

    fn list_policy_versions(&self) -> AegisResult<Vec<PolicyVersion>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT version, schema, created_at, description
                 FROM _aegis_policy_versions
                 ORDER BY version ASC",
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                let version: i64 = row.get(0)?;
                let schema: String = row.get(1)?;
                let created_at: String = row.get(2)?;
                let description: String = row.get(3)?;
                Ok(PolicyVersion {
                    version: version as u32,
                    schema,
                    created_at,
                    description,
                })
            })
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let mut versions = Vec::new();
        for row in rows {
            versions.push(row.map_err(|e| AegisError::StorageQuery(e.to_string()))?);
        }
        Ok(versions)
    }

    fn save_policy_version(&self, version: &PolicyVersion) -> AegisResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO _aegis_policy_versions (version, schema, created_at, description) VALUES (?1, ?2, ?3, ?4)",
            params![version.version as i64, version.schema, version.created_at, version.description],
        )
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn load_policy_version(&self, version: u32) -> AegisResult<Option<String>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT schema FROM _aegis_policy_versions WHERE version = ?1")
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let result = stmt
            .query_row(params![version as i64], |row| row.get::<_, String>(0))
            .ok();
        Ok(result)
    }
}

// ── Event Log Recovery ─────────────────────────────────────────

impl SqliteStorage {
    /// Recover the tuple graph from the event log.
    /// Replays all events in revision order to reconstruct the current state.
    /// After recovery, verifies that the final revision matches.
    fn recover_from_events_impl(&self, partition_id: &PartitionId, to_revision: Option<Revision>) -> AegisResult<Revision> {
        self.with_write_tx(|conn| {
            conn.execute("DELETE FROM _aegis_tuples WHERE partition_id = ?1", [partition_id.as_str()])
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut stmt = conn
                .prepare(
                    "SELECT revision, action, subject, relation, object, metadata
                     FROM _aegis_events
                     WHERE partition_id = ?1
                     ORDER BY revision ASC, event_id ASC",
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let rows = stmt
                .query_map(params![partition_id.as_str()], |row| {
                    let rev: i64 = row.get(0)?;
                    let action: String = row.get(1)?;
                    let subject: String = row.get(2)?;
                    let relation: String = row.get(3)?;
                    let object: String = row.get(4)?;
                    let metadata: Option<String> = row.get(5)?;
                    Ok((rev, action, subject, relation, object, metadata))
                })
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut last_revision = Revision::ZERO;

            for row in rows {
                let (rev, action, subject, relation, object, metadata) =
                    row.map_err(|e| AegisError::StorageQuery(e.to_string()))?;

                let rev = Revision::new(rev as u64);
                if let Some(target) = to_revision {
                    if rev > target {
                        continue;
                    }
                }
                let now = Utc::now().to_rfc3339();

                match action.as_str() {
                    "add" => {
                        conn.execute(
                            "INSERT INTO _aegis_tuples (subject, relation, object, partition_id, created_at, metadata, valid_until, revision_added, revision_removed)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, NULL)",
                            rusqlite::params![subject, relation, object, partition_id.as_str(), now, metadata, rev.as_u64() as i64],
                        )
                        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                    }
                    "remove" => {
                        conn.execute(
                            "UPDATE _aegis_tuples SET revision_removed = ?1
                             WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND partition_id = ?5 AND revision_removed IS NULL",
                            rusqlite::params![rev.as_u64() as i64, subject, relation, object, partition_id.as_str()],
                        )
                        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                    }
                    _ => {}
                }

                last_revision = rev;
            }

            let current = Self::read_revision(conn)?;
            if current != last_revision && last_revision != Revision::ZERO {
                conn.execute(
                    "UPDATE _aegis_meta SET value = ?1 WHERE key = 'revision'",
                    rusqlite::params![last_revision.as_u64() as i64],
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            }

            Ok(Self::read_revision(conn)?)
        })
    }

    /// Recover to a specific revision (point-in-time recovery).
    pub fn recover_to_revision(&self, target: Revision) -> AegisResult<Revision> {
        let current = self.current_revision(&PartitionId::default())?;
        if target > current {
            return Err(AegisError::RevisionFromFuture(target.as_u64() as usize));
        }

        self.with_write_tx(|conn| {
            conn.execute("DELETE FROM _aegis_tuples", [])
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut stmt = conn
                .prepare(
                    "SELECT revision, action, subject, relation, object, metadata
                     FROM _aegis_events
                     WHERE partition_id = ?1 AND revision <= ?2
                     ORDER BY revision ASC, event_id ASC",
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let rows = stmt
                .query_map(rusqlite::params![PartitionId::default().as_str(), target.as_u64() as i64], |row| {
                    let rev: i64 = row.get(0)?;
                    let action: String = row.get(1)?;
                    let subject: String = row.get(2)?;
                    let relation: String = row.get(3)?;
                    let object: String = row.get(4)?;
                    let metadata: Option<String> = row.get(5)?;
                    Ok((rev, action, subject, relation, object, metadata))
                })
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            for row in rows {
                let (rev, action, subject, relation, object, metadata) =
                    row.map_err(|e| AegisError::StorageQuery(e.to_string()))?;

                let rev = Revision::new(rev as u64);
                let now = Utc::now().to_rfc3339();

                match action.as_str() {
                    "add" => {
                        conn.execute(
                            "INSERT INTO _aegis_tuples (subject, relation, object, partition_id, created_at, metadata, valid_until, revision_added, revision_removed)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, NULL)",
                            rusqlite::params![subject, relation, object, PartitionId::default().as_str(), now, metadata, rev.as_u64() as i64],
                        )
                        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                    }
                    "remove" => {
                        conn.execute(
                            "UPDATE _aegis_tuples SET revision_removed = ?1
                             WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND partition_id = ?5 AND revision_removed IS NULL",
                            rusqlite::params![rev.as_u64() as i64, subject, relation, object, PartitionId::default().as_str()],
                        )
                        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                    }
                    _ => {}
                }
            }

            conn.execute(
                "UPDATE _aegis_meta SET value = ?1 WHERE key = 'revision'",
                rusqlite::params![target.as_u64() as i64],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            Ok(Self::read_revision(conn)?)
        })
    }

    /// Compact the event log by merging add/remove pairs for the same tuple.
    /// Removes event pairs that are semantically no-ops (add then later remove
    /// with no intermediate add for the same tuple key).
    /// Returns the number of event rows removed.
    pub fn compact_events(&self, partition_id: &PartitionId) -> AegisResult<usize> {
        let conn = self.conn()?;

        let total = conn
            .execute(
                "DELETE FROM _aegis_events WHERE partition_id = ?1 AND event_id IN (
                    SELECT e1.event_id FROM _aegis_events e1
                    WHERE e1.action = 'add'
                        AND e1.partition_id = ?1
                        AND EXISTS (
                            SELECT 1 FROM _aegis_events e2
                            WHERE e2.action = 'remove'
                                AND e2.partition_id = ?1
                                AND e2.subject = e1.subject
                                AND e2.relation = e1.relation
                                AND e2.object = e1.object
                                AND e2.event_id > e1.event_id
                                AND NOT EXISTS (
                                    SELECT 1 FROM _aegis_events e3
                                    WHERE e3.event_id > e1.event_id
                                        AND e3.event_id < e2.event_id
                                        AND e3.partition_id = ?1
                                        AND e3.subject = e1.subject
                                        AND e3.relation = e1.relation
                                        AND e3.object = e1.object
                                        AND e3.action = 'add'
                                )
                        )
                    UNION ALL
                    SELECT e2.event_id FROM _aegis_events e1
                    INNER JOIN _aegis_events e2
                        ON e2.action = 'remove'
                        AND e2.partition_id = ?1
                        AND e2.subject = e1.subject
                        AND e2.relation = e1.relation
                        AND e2.object = e1.object
                    WHERE e1.action = 'add'
                        AND e1.partition_id = ?1
                        AND e1.event_id < e2.event_id
                        AND NOT EXISTS (
                            SELECT 1 FROM _aegis_events e3
                            WHERE e3.event_id > e1.event_id
                                AND e3.event_id < e2.event_id
                                AND e3.partition_id = ?1
                                AND e3.subject = e1.subject
                                AND e3.relation = e1.relation
                                AND e3.object = e1.object
                                AND e3.action = 'add'
                        )
                )",
                params![partition_id.as_str()],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Ok(total)
    }
}

// ── SqliteTransaction ──────────────────────────────────────────

pub struct SqliteTransaction {
    conn: Option<r2d2::PooledConnection<SqliteConnectionManager>>,
    committed: bool,
    _node_id: Uuid,
    actor_identity: Option<String>,
}

impl SqliteTransaction {
    pub fn new(conn: r2d2::PooledConnection<SqliteConnectionManager>, node_id: Uuid, actor_identity: Option<String>) -> AegisResult<Self> {
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(Self {
            conn: Some(conn),
            committed: false,
            _node_id: node_id,
            actor_identity,
        })
    }

    fn conn(&self) -> AegisResult<&r2d2::PooledConnection<SqliteConnectionManager>> {
        self.conn
            .as_ref()
            .ok_or_else(|| AegisError::Internal("transaction already consumed".into()))
    }

    fn bump_revision(&self) -> AegisResult<Revision> {
        let conn = self.conn()?;
        SqliteStorage::bump_revision(conn)
    }

    fn append_event(
        &self,
        revision: Revision,
        action: &str,
        subject: &str,
        relation: &str,
        object: &str,
        partition_id: &str,
        metadata: Option<&str>,
    ) -> AegisResult<()> {
        let conn = self.conn()?;
        SqliteStorage::append_event(conn, revision, action, subject, relation, object, partition_id, metadata, self.actor_identity.as_deref())
    }
}

fn validate_savepoint_name(name: &str) -> AegisResult<()> {
    if name.is_empty() {
        return Err(AegisError::Validation(crate::types::ValidationError::Empty));
    }
    if name.len() > 64 {
        return Err(AegisError::Validation(
            crate::types::ValidationError::TooLong {
                max: 64,
                actual: name.len(),
            },
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AegisError::Validation(
            crate::types::ValidationError::InvalidCharacters(format!(
                "invalid characters in savepoint name '{name}': must contain only alphanumeric, underscore, or hyphen"
            )),
        ));
    }
    Ok(())
}

impl StorageTransaction for SqliteTransaction {
    fn set_actor_identity(&mut self, identity: Option<String>) -> Option<String> {
        let prev = self.actor_identity.take();
        self.actor_identity = identity;
        prev
    }

    fn write(&mut self, partition_id: &PartitionId, tuple: &RelationshipTuple) -> AegisResult<()> {
        let conn = self.conn()?;
        let revision = self.bump_revision()?;
        let metadata_json = tuple
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;

        conn.execute(
            "UPDATE _aegis_tuples SET revision_removed = ?1
             WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND partition_id = ?5 AND revision_removed IS NULL",
            params![
                revision.as_u64() as i64,
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
                partition_id.as_str(),
            ],
        )
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let valid_until_str = tuple.valid_until.map(|v| v.to_rfc3339());
        conn.execute(
            "INSERT INTO _aegis_tuples (subject, relation, object, partition_id, created_at, metadata, valid_until, revision_added, revision_removed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)",
            params![
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
                partition_id.as_str(),
                tuple.created_at.to_rfc3339(),
                metadata_json,
                valid_until_str,
                revision.as_u64() as i64,
            ],
        )
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        self.append_event(
            revision,
            "add",
            tuple.subject.as_str(),
            tuple.relation.as_str(),
            tuple.object.as_str(),
            partition_id.as_str(),
            metadata_json.as_deref(),
        )?;

        Ok(())
    }

    fn delete(&mut self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<()> {
        let conn = self.conn()?;
        let revision = self.bump_revision()?;

        conn.execute(
            "UPDATE _aegis_tuples SET revision_removed = ?1
             WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND partition_id = ?5 AND revision_removed IS NULL",
            params![
                revision.as_u64() as i64,
                key.subject.as_str(),
                key.relation.as_str(),
                key.object.as_str(),
                partition_id.as_str(),
            ],
        )
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        self.append_event(
            revision,
            "remove",
            key.subject.as_str(),
            key.relation.as_str(),
            key.object.as_str(),
            partition_id.as_str(),
            None,
        )?;

        Ok(())
    }

    fn savepoint(&self, name: &str) -> AegisResult<()> {
        validate_savepoint_name(name)?;
        let conn = self.conn()?;
        conn.execute_batch(&format!("SAVEPOINT \"{}\"", name))
            .map_err(|e| AegisError::StorageQuery(e.to_string()))
    }

    fn rollback_to_savepoint(&self, name: &str) -> AegisResult<()> {
        validate_savepoint_name(name)?;
        let conn = self.conn()?;
        conn.execute_batch(&format!("ROLLBACK TO SAVEPOINT \"{}\"", name))
            .map_err(|e| AegisError::StorageQuery(e.to_string()))
    }

    fn release_savepoint(&self, name: &str) -> AegisResult<()> {
        validate_savepoint_name(name)?;
        let conn = self.conn()?;
        conn.execute_batch(&format!("RELEASE SAVEPOINT \"{}\"", name))
            .map_err(|e| AegisError::StorageQuery(e.to_string()))
    }

    fn commit(mut self: Box<Self>) -> AegisResult<Revision> {
        let conn = self.conn.as_ref().ok_or_else(|| {
            AegisError::Internal("transaction already consumed".into())
        })?;
        let revision = SqliteStorage::read_revision(conn)?;
        conn.execute_batch("COMMIT")
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        self.committed = true;
        drop(self.conn.take());
        Ok(revision)
    }

    fn rollback(mut self: Box<Self>) -> AegisResult<()> {
        if !self.committed {
            if let Some(conn) = self.conn.take() {
                conn.execute_batch("ROLLBACK")
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            }
        }
        Ok(())
    }
}

impl Drop for SqliteTransaction {
    fn drop(&mut self) {
        if !self.committed {
            if let Some(conn) = self.conn.take() {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RelationshipTuple;

    fn storage() -> SqliteStorage {
        SqliteStorage::new(SqliteConfig::in_memory()).unwrap()
    }

    fn test_tuple() -> RelationshipTuple {
        RelationshipTuple::new(
            SubjectId::new("user:123").unwrap(),
            Relation::new("editor").unwrap(),
            ResourceId::new("repo:fluxbus").unwrap(),
        )
    }

    fn tuple(s: &str, r: &str, o: &str) -> RelationshipTuple {
        RelationshipTuple::new(
            SubjectId::new(s).unwrap(),
            Relation::new(r).unwrap(),
            ResourceId::new(o).unwrap(),
        )
    }

    fn key(s: &str, r: &str, o: &str) -> TupleKey {
        TupleKey {
            subject: SubjectId::new(s).unwrap(),
            relation: Relation::new(r).unwrap(),
            object: ResourceId::new(o).unwrap(),
        }
    }

    // ── Write & Check ──

    #[test]
    fn test_write_tuple() {
        let mut store = storage();
        let meta = store.initialize().unwrap();
        assert!(meta.healthy);

        let rev = store.write_tuple(&PartitionId::default(), &test_tuple()).unwrap();
        assert!(rev.as_u64() > 0);

        let has = store.has_tuple(&PartitionId::default(), &test_tuple().key()).unwrap();
        assert!(has);
    }

    #[test]
    fn test_write_and_read() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &test_tuple()).unwrap();

        let read = store.read_tuple(&PartitionId::default(), &test_tuple().key()).unwrap();
        assert!(read.is_some());
        let t = read.unwrap();
        assert_eq!(t.subject.as_str(), "user:123");
        assert_eq!(t.relation.as_str(), "editor");
        assert_eq!(t.object.as_str(), "repo:fluxbus");
    }

    #[test]
    fn test_write_revision_increments() {
        let mut store = storage();
        store.initialize().unwrap();

        let r1 = store.write_tuple(&PartitionId::default(), &test_tuple()).unwrap();
        let r2 = store.write_tuple(&PartitionId::default(), &tuple("user:456", "viewer", "repo:other")).unwrap();
        assert_eq!(r1.as_u64() + 1, r2.as_u64());
    }

    #[test]
    fn test_idempotent_write() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &test_tuple()).unwrap();
        store.write_tuple(&PartitionId::default(), &test_tuple()).unwrap(); // same tuple again

        let count = store
            .conn()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM _aegis_tuples WHERE revision_removed IS NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(count, 1); // upsert, not duplicate
    }

    // ── Delete ──

    #[test]
    fn test_delete_tuple() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &test_tuple()).unwrap();
        assert!(store.has_tuple(&PartitionId::default(), &test_tuple().key()).unwrap());

        store.delete_tuple(&PartitionId::default(), &test_tuple().key()).unwrap();
        assert!(!store.has_tuple(&PartitionId::default(), &test_tuple().key()).unwrap());
    }

    #[test]
    fn test_delete_non_existent() {
        let mut store = storage();
        store.initialize().unwrap();

        let rev_before = store.current_revision(&PartitionId::default()).unwrap();
        let rev_after = store
            .delete_tuple(&PartitionId::default(), &key("user:999", "editor", "repo:nonexistent"))
            .unwrap();
        assert_eq!(rev_before, rev_after); // no bump
    }

    #[test]
    fn test_delete_subject() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&PartitionId::default(), &tuple("user:1", "viewer", "repo:b")).unwrap();

        assert_eq!(
            store
                .list_by_subject(&PartitionId::default(), &SubjectId::new("user:1").unwrap(), None, &ConsistencyMode::MinimizeLatency)
                .unwrap()
                .len(),
            2
        );

        store
            .delete_subject(&PartitionId::default(), &SubjectId::new("user:1").unwrap())
            .unwrap();

        assert_eq!(
            store
                .list_by_subject(&PartitionId::default(), &SubjectId::new("user:1").unwrap(), None, &ConsistencyMode::MinimizeLatency)
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn test_delete_object() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&PartitionId::default(), &tuple("user:2", "viewer", "repo:a")).unwrap();

        assert_eq!(
            store
                .list_by_object(&PartitionId::default(), &ResourceId::new("repo:a").unwrap(), None, &ConsistencyMode::MinimizeLatency)
                .unwrap()
                .len(),
            2
        );

        store
            .delete_object(&PartitionId::default(), &ResourceId::new("repo:a").unwrap())
            .unwrap();

        assert_eq!(
            store
                .list_by_object(&PartitionId::default(), &ResourceId::new("repo:a").unwrap(), None, &ConsistencyMode::MinimizeLatency)
                .unwrap()
                .len(),
            0
        );
    }

    // ── List ──

    #[test]
    fn test_list_by_object() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&PartitionId::default(), &tuple("user:2", "viewer", "repo:a")).unwrap();

        let results = store
            .list_by_object(&PartitionId::default(), &ResourceId::new("repo:a").unwrap(), None, &ConsistencyMode::MinimizeLatency)
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_list_by_object_with_relation() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&PartitionId::default(), &tuple("user:2", "viewer", "repo:a")).unwrap();

        let results = store
            .list_by_object(
                &PartitionId::default(),
                &ResourceId::new("repo:a").unwrap(),
                Some(&Relation::new("editor").unwrap()),
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].subject.as_str(), "user:1");
    }

    #[test]
    fn test_list_by_subject() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&PartitionId::default(), &tuple("user:1", "viewer", "repo:b")).unwrap();

        let results = store
            .list_by_subject(&PartitionId::default(), &SubjectId::new("user:1").unwrap(), None, &ConsistencyMode::MinimizeLatency)
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_list_by_relation() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&PartitionId::default(), &tuple("user:2", "editor", "repo:a")).unwrap();
        store.write_tuple(&PartitionId::default(), &tuple("user:3", "viewer", "repo:a")).unwrap();

        let results = store
            .list_by_relation(
                &PartitionId::default(),
                &ResourceId::new("repo:a").unwrap(),
                &Relation::new("editor").unwrap(),
            )
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    // ── Query / Pagination ──

    #[test]
    fn test_query_pagination() {
        let mut store = storage();
        store.initialize().unwrap();

        for i in 0..10 {
            store
                .write_tuple(&PartitionId::default(), &tuple(
                    &format!("user:{i}"),
                    "editor",
                    "repo:fluxbus",
                ))
                .unwrap();
        }

        let page1 = store
            .query_tuples(
                &PartitionId::default(),
                &TupleFilter::default(),
                &PaginationParams {
                    limit: 3,
                    cursor: None,
                },
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(page1.tuples.len(), 3);
        assert!(page1.next_cursor.is_some());

        let page2 = store
            .query_tuples(
                &PartitionId::default(),
                &TupleFilter::default(),
                &PaginationParams {
                    limit: 3,
                    cursor: page1.next_cursor,
                },
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(page2.tuples.len(), 3);

        let page3 = store
            .query_tuples(
                &PartitionId::default(),
                &TupleFilter::default(),
                &PaginationParams {
                    limit: 3,
                    cursor: page2.next_cursor,
                },
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(page3.tuples.len(), 3);

        let page4 = store
            .query_tuples(
                &PartitionId::default(),
                &TupleFilter::default(),
                &PaginationParams {
                    limit: 3,
                    cursor: page3.next_cursor,
                },
                &ConsistencyMode::MinimizeLatency,
            )
            .unwrap();
        assert_eq!(page4.tuples.len(), 1); // last page
        assert!(page4.next_cursor.is_none());
    }

    #[test]
    fn test_query_with_subject_filter() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&PartitionId::default(), &tuple("user:2", "editor", "repo:a")).unwrap();

        let filter = TupleFilter {
            subject_type: Some("user".to_string()),
            ..Default::default()
        };
        let results = store
            .query_tuples(&PartitionId::default(), &filter, &PaginationParams::default(), &ConsistencyMode::MinimizeLatency)
            .unwrap();
        assert_eq!(results.tuples.len(), 2);
    }

    // ── Revision & Token ──

    #[test]
    fn test_current_revision() {
        let mut store = storage();
        store.initialize().unwrap();

        assert_eq!(store.current_revision(&PartitionId::default()).unwrap().as_u64(), 0);

        store.write_tuple(&PartitionId::default(), &test_tuple()).unwrap();
        assert_eq!(store.current_revision(&PartitionId::default()).unwrap().as_u64(), 1);

        store.write_tuple(&PartitionId::default(), &tuple("user:456", "viewer", "repo:other")).unwrap();
        assert_eq!(store.current_revision(&PartitionId::default()).unwrap().as_u64(), 2);
    }

    #[test]
    fn test_current_token() {
        let mut store = storage();
        store.initialize().unwrap();

        let token = store.current_token().unwrap();
        assert_eq!(token.revision.as_u64(), 0);
        assert!(!token.node_id.is_nil());
    }

    // ── Transaction ──

    #[test]
    fn test_transaction_commit() {
        let mut store = storage();
        store.initialize().unwrap();

        let mut tx = store.begin_transaction(&PartitionId::default()).unwrap();
        tx.write(&PartitionId::default(), &test_tuple()).unwrap();
        tx.write(&PartitionId::default(), &tuple("user:456", "viewer", "repo:other")).unwrap();
        let rev = tx.commit().unwrap();

        assert!(rev.as_u64() > 0);
        assert!(store.has_tuple(&PartitionId::default(), &test_tuple().key()).unwrap());
    }

    #[test]
    fn test_transaction_rollback() {
        let mut store = storage();
        store.initialize().unwrap();

        let rev_before = store.current_revision(&PartitionId::default()).unwrap();

        let mut tx = store.begin_transaction(&PartitionId::default()).unwrap();
        tx.write(&PartitionId::default(), &test_tuple()).unwrap();
        tx.rollback().unwrap();

        assert_eq!(store.current_revision(&PartitionId::default()).unwrap(), rev_before);
        assert!(!store.has_tuple(&PartitionId::default(), &test_tuple().key()).unwrap());
    }

    #[test]
    fn test_savepoint_rollback() {
        let mut store = storage();
        store.initialize().unwrap();

        let mut tx = store.begin_transaction(&PartitionId::default()).unwrap();
        tx.write(&PartitionId::default(), &test_tuple()).unwrap();

        tx.savepoint("sp1").unwrap();
        tx.write(&PartitionId::default(), &tuple("user:savepoint", "test", "repo:sp")).unwrap();

        // Savepoint tuple should exist (it was written after the savepoint)
        tx.rollback_to_savepoint("sp1").unwrap();
        tx.release_savepoint("sp1").unwrap();

        let rev = tx.commit().unwrap();
        assert!(rev.as_u64() > 0);

        // After commit: only the original tuple exists, savepoint tuple was rolled back
        assert!(store.has_tuple(&PartitionId::default(), &test_tuple().key()).unwrap());
        assert!(!store.has_tuple(&PartitionId::default(), &key("user:savepoint", "test", "repo:sp")).unwrap());
    }

    #[test]
    fn test_transaction_rollback_on_drop() {
        let mut store = storage();
        store.initialize().unwrap();

        let rev_before = store.current_revision(&PartitionId::default()).unwrap();

        {
            let mut tx = store.begin_transaction(&PartitionId::default()).unwrap();
            tx.write(&PartitionId::default(), &test_tuple()).unwrap();
            // tx drops without commit
        }

        assert_eq!(store.current_revision(&PartitionId::default()).unwrap(), rev_before);
    }

    // ── Audit ──

    #[test]
    fn test_audit_log_writes() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &test_tuple()).unwrap();
        store
            .delete_tuple(&PartitionId::default(), &test_tuple().key())
            .unwrap();

        let audit = store
            .query_audit(
                &PartitionId::default(),
                Some(&ResourceId::new("repo:fluxbus").unwrap()),
                None,
                None,
                &PaginationParams::default(),
            )
            .unwrap();
        assert_eq!(audit.len(), 2);
        assert_eq!(audit[0].action, crate::types::TupleMutation::Add);
        assert_eq!(audit[1].action, crate::types::TupleMutation::Remove);
    }

    #[test]
    fn test_audit_log_filtered_by_revision() {
        let mut store = storage();
        store.initialize().unwrap();

        store
            .write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a"))
            .unwrap();
        let r2 = store
            .write_tuple(&PartitionId::default(), &tuple("user:2", "viewer", "repo:a"))
            .unwrap();

        let audit = store
            .query_audit(
                &PartitionId::default(),
                Some(&ResourceId::new("repo:a").unwrap()),
                Some(r2),
                Some(r2),
                &PaginationParams::default(),
            )
            .unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].subject, "user:2");
    }

    // ── Integrity ──

    #[test]
    fn test_integrity_check_passes() {
        let mut store = storage();
        store.initialize().unwrap();

        let report = store.integrity_check().unwrap();
        assert!(report.passed);
    }

    #[test]
    fn test_close_and_checkpoint() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &test_tuple()).unwrap();
        store.close().unwrap();

        // After close, can still read (pool connections may be live)
        assert!(store.has_tuple(&PartitionId::default(), &test_tuple().key()).unwrap());
    }

    // ── Revision Snapshots ──

    #[test]
    fn test_read_at_revision() {
        let mut store = storage();
        store.initialize().unwrap();

        // Write tuple at rev 1
        store.write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a")).unwrap();
        let rev_before_delete = store.current_revision(&PartitionId::default()).unwrap();

        // Delete and re-write at rev 2+
        store.write_tuple(&PartitionId::default(), &tuple("user:2", "viewer", "repo:a")).unwrap();
        store.delete_tuple(&PartitionId::default(), &key("user:1", "editor", "repo:a")).unwrap();

        // Read at rev 1 should see user:1 only (active at that point)
        let conn = store.conn().unwrap();
        let at_rev1 = SqliteStorage::read_tuples_at_revision(&conn, rev_before_delete.as_u64() as i64).unwrap();
        let subjects_at_rev1: Vec<String> = at_rev1.iter().map(|t| t.subject.as_str().to_string()).collect();
        assert!(subjects_at_rev1.contains(&"user:1".to_string()));
        assert!(!subjects_at_rev1.contains(&"user:2".to_string()));
    }

    // ── Batch Write ──

    #[test]
    fn test_write_batch() {
        let mut store = storage();
        store.initialize().unwrap();

        let tuples = vec![
            tuple("user:1", "editor", "repo:a"),
            tuple("user:2", "viewer", "repo:b"),
            tuple("team:eng", "owner", "workspace:core"),
        ];

        let rev = store.write_tuples_batch(&PartitionId::default(), &tuples).unwrap();
        assert!(rev.as_u64() > 0);

        assert!(store.has_tuple(&PartitionId::default(), &key("user:1", "editor", "repo:a")).unwrap());
        assert!(store.has_tuple(&PartitionId::default(), &key("user:2", "viewer", "repo:b")).unwrap());
        assert!(
            store
                .has_tuple(&PartitionId::default(), &key("team:eng", "owner", "workspace:core"))
                .unwrap()
        );
    }

    #[test]
    fn test_write_batch_empty() {
        let mut store = storage();
        store.initialize().unwrap();

        let rev = store.write_tuples_batch(&PartitionId::default(), &[]).unwrap();
        assert_eq!(rev.as_u64(), 0);
    }

    // ── Metadata ──

    #[test]
    fn test_write_with_metadata() {
        let mut store = storage();
        store.initialize().unwrap();

        let mut meta = HashMap::new();
        meta.insert("granted_by".to_string(), "admin:1".to_string());

        let tuple = RelationshipTuple::with_metadata(
            SubjectId::new("user:1").unwrap(),
            Relation::new("editor").unwrap(),
            ResourceId::new("repo:a").unwrap(),
            meta.clone(),
        )
        .unwrap();

        store.write_tuple(&PartitionId::default(), &tuple).unwrap();

        let read = store.read_tuple(&PartitionId::default(), &tuple.key()).unwrap().unwrap();
        assert_eq!(read.metadata.unwrap(), meta);
    }

    // ── WAL Mode ──

    #[test]
    fn test_wal_mode_enabled() {
        let config = SqliteConfig {
            path: ":memory:".to_string(),
            max_readers: 4,
            busy_timeout_ms: 5000,
            wal_mode: true,
            mmap_size: 0,
        };
        let store = SqliteStorage::new(config).unwrap();

        // In in-memory mode, WAL may not be used, but we verify no crash
        let mut store = store;
        store.initialize().unwrap();
        store.write_tuple(&PartitionId::default(), &test_tuple()).unwrap();
        assert!(store.has_tuple(&PartitionId::default(), &test_tuple().key()).unwrap());
    }

    // ── Initialize ──

    #[test]
    fn test_initialize_returns_meta() {
        let mut store = storage();
        let meta = store.initialize().unwrap();
        assert!(meta.healthy);
        assert_eq!(meta.current_revision.as_u64(), 0);
        assert_eq!(meta.backend_type, BackendType::Sqlite);
        assert_eq!(meta.schema_version, 1);
    }

    // ── Empty Store ──

    #[test]
    fn test_empty_store_no_tuples() {
        let store = storage();
        assert!(
            store
                .list_by_object(&PartitionId::default(), &ResourceId::new("nonexistent").unwrap(), None, &ConsistencyMode::MinimizeLatency)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .has_tuple(&PartitionId::default(), &key("user:1", "editor", "repo:a"))
                .unwrap()
                == false
        );
    }

    // ── Event Recovery ──

    #[test]
    fn test_event_recover_from_events() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&PartitionId::default(), &tuple("user:2", "viewer", "repo:b")).unwrap();
        let rev_before = store.current_revision(&PartitionId::default()).unwrap();

        let recovered = store.recover_from_events(&PartitionId::default(), None).unwrap();
        assert_eq!(recovered, rev_before);

        assert!(store.has_tuple(&PartitionId::default(), &key("user:1", "editor", "repo:a")).unwrap());
        assert!(store.has_tuple(&PartitionId::default(), &key("user:2", "viewer", "repo:b")).unwrap());
    }

    #[test]
    fn test_event_recover_point_in_time() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&PartitionId::default(), &tuple("user:2", "viewer", "repo:b")).unwrap();

        let recovered = store.recover_to_revision(Revision::new(1)).unwrap();
        assert_eq!(recovered.as_u64(), 1);

        assert!(store.has_tuple(&PartitionId::default(), &key("user:1", "editor", "repo:a")).unwrap());
        assert!(!store.has_tuple(&PartitionId::default(), &key("user:2", "viewer", "repo:b")).unwrap());
    }

    #[test]
    fn test_event_compaction() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&PartitionId::default(), &tuple("user:1", "editor", "repo:a")).unwrap();
        store.delete_tuple(&PartitionId::default(), &key("user:1", "editor", "repo:a")).unwrap();

        let before_events = store.conn().unwrap()
            .query_row("SELECT COUNT(*) FROM _aegis_events", [], |row| row.get::<_, i64>(0))
            .unwrap();

        assert_eq!(before_events, 2);

        let removed = store.compact_events(&PartitionId::default()).unwrap();
        assert_eq!(removed, 2);

        let after_events = store.conn().unwrap()
            .query_row("SELECT COUNT(*) FROM _aegis_events", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(after_events, 0);
    }

    #[test]
    fn test_recover_empty_events() {
        let mut store = storage();
        store.initialize().unwrap();

        let recovered = store.recover_from_events(&PartitionId::default(), None).unwrap();
        assert_eq!(recovered.as_u64(), 0);
    }

    #[test]
    fn test_savepoint_name_validation() {
        let mut store = storage();
        store.initialize().unwrap();

        // Valid names
        let tx = store.begin_transaction(&PartitionId::default()).unwrap();
        tx.savepoint("sp1").unwrap();
        tx.savepoint("my_savepoint_42").unwrap();
        tx.savepoint("a").unwrap();
        tx.rollback().ok();

        // Empty name
        let tx = store.begin_transaction(&PartitionId::default()).unwrap();
        let err = tx.savepoint("").unwrap_err();
        assert!(matches!(err, AegisError::Validation(crate::types::ValidationError::Empty)), "empty name should fail: {err}");
        tx.rollback().ok();

        // Too long name (65 chars)
        let tx = store.begin_transaction(&PartitionId::default()).unwrap();
        let long_name = "a".repeat(65);
        let err = tx.savepoint(&long_name).unwrap_err();
        assert!(matches!(err, AegisError::Validation(crate::types::ValidationError::TooLong { .. })), "long name should fail: {err}");
        tx.rollback().ok();

        // Invalid characters (SQL injection attempt)
        let tx = store.begin_transaction(&PartitionId::default()).unwrap();
        let err = tx.savepoint("\"; DROP TABLE _aegis_tuples; --").unwrap_err();
        assert!(matches!(err, AegisError::Validation(crate::types::ValidationError::InvalidCharacters(_))), "injection attempt should fail: {err}");
        tx.rollback().ok();

        // Same validation applies to rollback_to_savepoint and release_savepoint
        let tx = store.begin_transaction(&PartitionId::default()).unwrap();
        tx.savepoint("valid").unwrap();
        let err = tx.rollback_to_savepoint("invalid!").unwrap_err();
        assert!(matches!(err, AegisError::Validation(crate::types::ValidationError::InvalidCharacters(_))), "rollback_to_savepoint should validate name: {err}");
        let err = tx.release_savepoint("no space").unwrap_err();
        assert!(matches!(err, AegisError::Validation(crate::types::ValidationError::InvalidCharacters(_))), "release_savepoint should validate name: {err}");
        tx.rollback().ok();
    }
}
