use crate::error::{AegisError, AegisResult};
use crate::storage::traits::{
    BackendType, IntegrityReport, StorageBackend, StorageMeta, StorageTransaction, TupleFilter,
};
use crate::types::{
    AuditEntry, ConsistencyMode, PaginatedTuples, PaginationCursor, PaginationParams, Relation,
    RelationshipTuple, ResourceId, Revision, RevisionToken, SubjectId, TupleKey,
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
        created_at       TEXT NOT NULL,
        metadata         TEXT,
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

const EVENTS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS _aegis_events (
        event_id   INTEGER PRIMARY KEY AUTOINCREMENT,
        revision   INTEGER NOT NULL,
        action     TEXT NOT NULL,
        subject    TEXT NOT NULL,
        relation   TEXT NOT NULL,
        object     TEXT NOT NULL,
        metadata   TEXT,
        timestamp  TEXT NOT NULL,
        identity   TEXT
    )";

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
}

impl Default for SqliteConfig {
    fn default() -> Self {
        Self {
            path: "aegis.db".to_string(),
            max_readers: 4,
            busy_timeout_ms: 5000,
            wal_mode: true,
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
        }
    }
}

// ── Storage Adapter ────────────────────────────────────────────

pub struct SqliteStorage {
    pool: Pool<SqliteConnectionManager>,
    config: SqliteConfig,
    node_id: Uuid,
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

        let pool = Pool::builder()
            .max_size(config.max_readers + 1) // +1 for potential write connection
            .build(manager)
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        let node_id = Uuid::new_v4();

        let storage = Self {
            pool,
            config,
            node_id,
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
        conn.execute_batch(EVENTS_TABLE)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        conn.execute_batch(SCHEMA_TABLE)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    /// Configure PRAGMA settings on a connection.
    fn configure_connection(conn: &Connection, config: &SqliteConfig) -> AegisResult<()> {
        conn.execute_batch(&format!(
            "PRAGMA busy_timeout = {};",
            config.busy_timeout_ms
        ))
        .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        conn.execute_batch("PRAGMA journal_mode = WAL;")
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        conn.execute_batch("PRAGMA synchronous = NORMAL;")
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

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

    /// Append an event to the event log.
    fn append_event(
        conn: &Connection,
        revision: Revision,
        action: &str,
        subject: &str,
        relation: &str,
        object: &str,
        metadata: Option<&str>,
        identity: Option<&str>,
    ) -> AegisResult<()> {
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO _aegis_events (revision, action, subject, relation, object, metadata, timestamp, identity)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                revision.as_u64() as i64,
                action,
                subject,
                relation,
                object,
                metadata,
                now,
                identity,
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
                "SELECT subject, relation, object, created_at, metadata
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

                let subject = SubjectId::new(&subject_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let relation = Relation::new(&relation_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let object = ResourceId::new(&object_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata = metadata_json
                    .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());

                Ok(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at,
                    metadata,
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

    fn write_tuple(&self, tuple: &RelationshipTuple) -> AegisResult<Revision> {
        self.with_write_tx(|conn| {
            let revision = Self::bump_revision(conn)?;
            let metadata_json = tuple
                .metadata
                .as_ref()
                .map(|m| serde_json::to_string(m).unwrap_or_default());

            conn.execute(
                "UPDATE _aegis_tuples SET revision_removed = ?1
                 WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND revision_removed IS NULL",
                params![
                    revision.as_u64() as i64,
                    tuple.subject.as_str(),
                    tuple.relation.as_str(),
                    tuple.object.as_str(),
                ],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            conn.execute(
                "INSERT INTO _aegis_tuples (subject, relation, object, created_at, metadata, revision_added, revision_removed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
                params![
                    tuple.subject.as_str(),
                    tuple.relation.as_str(),
                    tuple.object.as_str(),
                    tuple.created_at.to_rfc3339(),
                    metadata_json,
                    revision.as_u64() as i64,
                ],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            Self::append_event(
                conn,
                revision,
                "add",
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
                metadata_json.as_deref(),
                None,
            )?;

            Ok(revision)
        })
    }

    fn write_tuples_batch(&self, tuples: &[RelationshipTuple]) -> AegisResult<Revision> {
        if tuples.is_empty() {
            return self.current_revision();
        }

        self.with_write_tx(|conn| {
            let revision = Self::bump_revision(conn)?;

            for tuple in tuples {
                let metadata_json = tuple
                    .metadata
                    .as_ref()
                    .map(|m| serde_json::to_string(m).unwrap_or_default());

                conn.execute(
                    "UPDATE _aegis_tuples SET revision_removed = ?1
                     WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND revision_removed IS NULL",
                    params![
                        revision.as_u64() as i64,
                        tuple.subject.as_str(),
                        tuple.relation.as_str(),
                        tuple.object.as_str(),
                    ],
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

                conn.execute(
                    "INSERT INTO _aegis_tuples (subject, relation, object, created_at, metadata, revision_added, revision_removed)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
                    params![
                        tuple.subject.as_str(),
                        tuple.relation.as_str(),
                        tuple.object.as_str(),
                        tuple.created_at.to_rfc3339(),
                        metadata_json,
                        revision.as_u64() as i64,
                    ],
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

                Self::append_event(
                    conn,
                    revision,
                    "add",
                    tuple.subject.as_str(),
                    tuple.relation.as_str(),
                    tuple.object.as_str(),
                    metadata_json.as_deref(),
                    None,
                )?;
            }

            Ok(revision)
        })
    }

    fn delete_tuple(&self, key: &TupleKey) -> AegisResult<Revision> {
        self.with_write_tx(|conn| {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM _aegis_tuples
                     WHERE subject = ?1 AND relation = ?2 AND object = ?3 AND revision_removed IS NULL",
                    params![key.subject.as_str(), key.relation.as_str(), key.object.as_str()],
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
                 WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND revision_removed IS NULL",
                params![
                    revision.as_u64() as i64,
                    key.subject.as_str(),
                    key.relation.as_str(),
                    key.object.as_str(),
                ],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            Self::append_event(
                conn,
                revision,
                "remove",
                key.subject.as_str(),
                key.relation.as_str(),
                key.object.as_str(),
                None,
                None,
            )?;

            Ok(revision)
        })
    }

    fn delete_subject(&self, subject: &SubjectId) -> AegisResult<Revision> {
        self.with_write_tx(|conn| {
            let revision = Self::bump_revision(conn)?;

            let mut stmt = conn
                .prepare(
                    "SELECT relation, object FROM _aegis_tuples
                     WHERE subject = ?1 AND revision_removed IS NULL",
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let tuples: Vec<(String, String)> = stmt
                .query_map(params![subject.as_str()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();

            drop(stmt);

            conn.execute(
                "UPDATE _aegis_tuples SET revision_removed = ?1
                 WHERE subject = ?2 AND revision_removed IS NULL",
                params![revision.as_u64() as i64, subject.as_str()],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            for (relation, object) in &tuples {
                Self::append_event(
                    conn,
                    revision,
                    "remove",
                    subject.as_str(),
                    relation,
                    object,
                    None,
                    None,
                )?;
            }

            Ok(revision)
        })
    }

    fn delete_object(&self, object: &ResourceId) -> AegisResult<Revision> {
        self.with_write_tx(|conn| {
            let revision = Self::bump_revision(conn)?;

            let mut stmt = conn
                .prepare(
                    "SELECT subject, relation FROM _aegis_tuples
                     WHERE object = ?1 AND revision_removed IS NULL",
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let tuples: Vec<(String, String)> = stmt
                .query_map(params![object.as_str()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();

            drop(stmt);

            conn.execute(
                "UPDATE _aegis_tuples SET revision_removed = ?1
                 WHERE object = ?2 AND revision_removed IS NULL",
                params![revision.as_u64() as i64, object.as_str()],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            for (subject, relation) in &tuples {
                Self::append_event(
                    conn,
                    revision,
                    "remove",
                    subject,
                    relation,
                    object.as_str(),
                    None,
                    None,
                )?;
            }

            Ok(revision)
        })
    }

    fn has_tuple(&self, key: &TupleKey) -> AegisResult<bool> {
        let conn = self.conn()?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM _aegis_tuples
                 WHERE subject = ?1 AND relation = ?2 AND object = ?3 AND revision_removed IS NULL",
                params![key.subject.as_str(), key.relation.as_str(), key.object.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(count > 0)
    }

    fn read_tuple(&self, key: &TupleKey) -> AegisResult<Option<RelationshipTuple>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT subject, relation, object, created_at, metadata, revision_added
                 FROM _aegis_tuples
                 WHERE subject = ?1 AND relation = ?2 AND object = ?3 AND revision_removed IS NULL",
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let result = stmt
            .query_row(
                params![key.subject.as_str(), key.relation.as_str(), key.object.as_str()],
                |row| {
                    let subject_str: String = row.get(0)?;
                    let relation_str: String = row.get(1)?;
                    let object_str: String = row.get(2)?;
                    let created_at_str: String = row.get(3)?;
                    let metadata_json: Option<String> = row.get(4)?;

                    let subject = SubjectId::new(&subject_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let relation = Relation::new(&relation_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let object = ResourceId::new(&object_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                    let metadata = metadata_json
                        .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());

                    Ok(RelationshipTuple {
                        subject,
                        relation,
                        object,
                        created_at,
                        metadata,
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
        object: &ResourceId,
        relation: Option<&Relation>,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let conn = self.conn()?;
        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(rel) = relation {
            (
                "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                 WHERE object = ?1 AND relation = ?2 AND revision_removed IS NULL"
                    .to_string(),
                vec![
                    Box::new(object.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(rel.as_str().to_string()),
                ],
            )
        } else {
            (
                "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                 WHERE object = ?1 AND revision_removed IS NULL"
                    .to_string(),
                vec![Box::new(object.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>],
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

                let subject = SubjectId::new(&subject_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let relation = Relation::new(&relation_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let object = ResourceId::new(&object_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata = metadata_json
                    .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());

                Ok(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at,
                    metadata,
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
        subject: &SubjectId,
        relation: Option<&Relation>,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let conn = self.conn()?;
        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(rel) = relation {
            (
                "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                 WHERE subject = ?1 AND relation = ?2 AND revision_removed IS NULL"
                    .to_string(),
                vec![
                    Box::new(subject.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(rel.as_str().to_string()),
                ],
            )
        } else {
            (
                "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                 WHERE subject = ?1 AND revision_removed IS NULL"
                    .to_string(),
                vec![Box::new(subject.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>],
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

                let subject = SubjectId::new(&subject_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let relation = Relation::new(&relation_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let object = ResourceId::new(&object_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata = metadata_json
                    .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());

                Ok(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at,
                    metadata,
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
        object: &ResourceId,
        relation: &Relation,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                 WHERE object = ?1 AND relation = ?2 AND revision_removed IS NULL",
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        let rows = stmt
            .query_map(
                params![object.as_str(), relation.as_str()],
                |row| {
                    let subject_str: String = row.get(0)?;
                    let relation_str: String = row.get(1)?;
                    let object_str: String = row.get(2)?;
                    let created_at_str: String = row.get(3)?;
                    let metadata_json: Option<String> = row.get(4)?;

                    let subject = SubjectId::new(&subject_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let relation = Relation::new(&relation_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let object = ResourceId::new(&object_str)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                    let metadata = metadata_json
                        .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());

                    Ok(RelationshipTuple {
                        subject,
                        relation,
                        object,
                        created_at,
                        metadata,
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

        let mut conditions = vec!["revision_removed IS NULL".to_string()];
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

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
            "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
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

                let subject = SubjectId::new(&subject_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let relation = Relation::new(&relation_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let object = ResourceId::new(&object_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata = metadata_json
                    .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());

                Ok(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at,
                    metadata,
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

    fn current_revision(&self) -> AegisResult<Revision> {
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
        let revision = self.current_revision()?;
        Ok(RevisionToken::new(revision, self.node_id))
    }

    fn begin_transaction(&self) -> AegisResult<Box<dyn StorageTransaction>> {
        let conn = self.conn()?;
        let tx = SqliteTransaction::new(conn, self.node_id)?;
        Ok(Box::new(tx))
    }

    fn query_audit(
        &self,
        object: &ResourceId,
        from_revision: Option<Revision>,
        to_revision: Option<Revision>,
        pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>> {
        let conn = self.conn()?;
        let mut conditions = vec!["object = ?1".to_string()];
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(object.as_str().to_string())];

        if let Some(from) = from_revision {
            params_vec.push(Box::new(from.as_u64() as i64));
            conditions.push(format!("revision >= ?{}", params_vec.len()));
        }
        if let Some(to) = to_revision {
            params_vec.push(Box::new(to.as_u64() as i64));
            conditions.push(format!("revision <= ?{}", params_vec.len()));
        }

        let where_clause = conditions.join(" AND ");
        let offset = pagination
            .cursor
            .as_ref()
            .map(|c| c.offset)
            .unwrap_or(0);
        let limit = pagination.limit;

        let sql = format!(
            "SELECT revision, action, subject, relation, object, timestamp, metadata
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
        })
    }

    fn delete_events_before(&self, cutoff: DateTime<Utc>) -> AegisResult<usize> {
        let conn = self.conn()?;
        let cutoff_str = cutoff.to_rfc3339();
        let count = conn
            .execute(
                "DELETE FROM _aegis_events WHERE timestamp < ?1",
                params![cutoff_str],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(count)
    }

    fn delete_soft_deleted_tuples_before(
        &self,
        cutoff: DateTime<Utc>,
    ) -> AegisResult<usize> {
        let conn = self.conn()?;
        let cutoff_str = cutoff.to_rfc3339();
        let count = conn
            .execute(
                "DELETE FROM _aegis_tuples
                 WHERE revision_removed IS NOT NULL
                   AND revision_removed <= (
                     SELECT COALESCE(MAX(revision), 0) FROM _aegis_events WHERE timestamp < ?1
                   )",
                params![cutoff_str],
            )
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(count)
    }

    fn compact_events(&self) -> AegisResult<usize> {
        SqliteStorage::compact_events(self)
    }

    fn close(&self) -> AegisResult<()> {
        if self.config.wal_mode && self.config.path != ":memory:" {
            if let Ok(conn) = self.pool.get() {
                let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            }
        }
        Ok(())
    }
}

// ── Event Log Recovery ─────────────────────────────────────────

impl SqliteStorage {
    /// Recover the tuple graph from the event log.
    /// Replays all events in revision order to reconstruct the current state.
    /// After recovery, verifies that the final revision matches.
    pub fn recover_from_events(&self) -> AegisResult<Revision> {
        self.with_write_tx(|conn| {
            conn.execute("DELETE FROM _aegis_tuples WHERE revision_removed IS NOT NULL OR 1=1", [])
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut stmt = conn
                .prepare(
                    "SELECT revision, action, subject, relation, object, metadata
                     FROM _aegis_events
                     ORDER BY revision ASC, event_id ASC",
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let rows = stmt
                .query_map([], |row| {
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
                let now = Utc::now().to_rfc3339();

                match action.as_str() {
                    "add" => {
                        conn.execute(
                            "INSERT INTO _aegis_tuples (subject, relation, object, created_at, metadata, revision_added, revision_removed)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
                            rusqlite::params![subject, relation, object, now, metadata, rev.as_u64() as i64],
                        )
                        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                    }
                    "remove" => {
                        conn.execute(
                            "UPDATE _aegis_tuples SET revision_removed = ?1
                             WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND revision_removed IS NULL",
                            rusqlite::params![rev.as_u64() as i64, subject, relation, object],
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
        let current = self.current_revision()?;
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
                     WHERE revision <= ?1
                     ORDER BY revision ASC, event_id ASC",
                )
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let rows = stmt
                .query_map(rusqlite::params![target.as_u64() as i64], |row| {
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
                            "INSERT INTO _aegis_tuples (subject, relation, object, created_at, metadata, revision_added, revision_removed)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
                            rusqlite::params![subject, relation, object, now, metadata, rev.as_u64() as i64],
                        )
                        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                    }
                    "remove" => {
                        conn.execute(
                            "UPDATE _aegis_tuples SET revision_removed = ?1
                             WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND revision_removed IS NULL",
                            rusqlite::params![rev.as_u64() as i64, subject, relation, object],
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
    pub fn compact_events(&self) -> AegisResult<usize> {
        let conn = self.conn()?;

        let total = conn
            .execute(
                "DELETE FROM _aegis_events WHERE event_id IN (
                    SELECT e1.event_id FROM _aegis_events e1
                    WHERE e1.action = 'add'
                        AND EXISTS (
                            SELECT 1 FROM _aegis_events e2
                            WHERE e2.action = 'remove'
                                AND e2.subject = e1.subject
                                AND e2.relation = e1.relation
                                AND e2.object = e1.object
                                AND e2.event_id > e1.event_id
                                AND NOT EXISTS (
                                    SELECT 1 FROM _aegis_events e3
                                    WHERE e3.event_id > e1.event_id
                                        AND e3.event_id < e2.event_id
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
                        AND e2.subject = e1.subject
                        AND e2.relation = e1.relation
                        AND e2.object = e1.object
                    WHERE e1.action = 'add'
                        AND e1.event_id < e2.event_id
                        AND NOT EXISTS (
                            SELECT 1 FROM _aegis_events e3
                            WHERE e3.event_id > e1.event_id
                                AND e3.event_id < e2.event_id
                                AND e3.subject = e1.subject
                                AND e3.relation = e1.relation
                                AND e3.object = e1.object
                                AND e3.action = 'add'
                        )
                )",
                [],
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
}

impl SqliteTransaction {
    pub fn new(conn: r2d2::PooledConnection<SqliteConnectionManager>, node_id: Uuid) -> AegisResult<Self> {
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(Self {
            conn: Some(conn),
            committed: false,
            _node_id: node_id,
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
        metadata: Option<&str>,
    ) -> AegisResult<()> {
        let conn = self.conn()?;
        SqliteStorage::append_event(conn, revision, action, subject, relation, object, metadata, None)
    }
}

impl StorageTransaction for SqliteTransaction {
    fn write(&mut self, tuple: &RelationshipTuple) -> AegisResult<()> {
        let conn = self.conn()?;
        let revision = self.bump_revision()?;
        let metadata_json = tuple
            .metadata
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default());

        conn.execute(
            "UPDATE _aegis_tuples SET revision_removed = ?1
             WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND revision_removed IS NULL",
            params![
                revision.as_u64() as i64,
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
            ],
        )
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        conn.execute(
            "INSERT INTO _aegis_tuples (subject, relation, object, created_at, metadata, revision_added, revision_removed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            params![
                tuple.subject.as_str(),
                tuple.relation.as_str(),
                tuple.object.as_str(),
                tuple.created_at.to_rfc3339(),
                metadata_json,
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
            metadata_json.as_deref(),
        )?;

        Ok(())
    }

    fn delete(&mut self, key: &TupleKey) -> AegisResult<()> {
        let conn = self.conn()?;
        let revision = self.bump_revision()?;

        conn.execute(
            "UPDATE _aegis_tuples SET revision_removed = ?1
             WHERE subject = ?2 AND relation = ?3 AND object = ?4 AND revision_removed IS NULL",
            params![
                revision.as_u64() as i64,
                key.subject.as_str(),
                key.relation.as_str(),
                key.object.as_str(),
            ],
        )
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        self.append_event(
            revision,
            "remove",
            key.subject.as_str(),
            key.relation.as_str(),
            key.object.as_str(),
            None,
        )?;

        Ok(())
    }

    fn savepoint(&self, name: &str) -> AegisResult<()> {
        let conn = self.conn()?;
        conn.execute_batch(&format!("SAVEPOINT \"{}\"", name))
            .map_err(|e| AegisError::StorageQuery(e.to_string()))
    }

    fn rollback_to_savepoint(&self, name: &str) -> AegisResult<()> {
        let conn = self.conn()?;
        conn.execute_batch(&format!("ROLLBACK TO SAVEPOINT \"{}\"", name))
            .map_err(|e| AegisError::StorageQuery(e.to_string()))
    }

    fn release_savepoint(&self, name: &str) -> AegisResult<()> {
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

        let rev = store.write_tuple(&test_tuple()).unwrap();
        assert!(rev.as_u64() > 0);

        let has = store.has_tuple(&test_tuple().key()).unwrap();
        assert!(has);
    }

    #[test]
    fn test_write_and_read() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&test_tuple()).unwrap();

        let read = store.read_tuple(&test_tuple().key()).unwrap();
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

        let r1 = store.write_tuple(&test_tuple()).unwrap();
        let r2 = store.write_tuple(&tuple("user:456", "viewer", "repo:other")).unwrap();
        assert_eq!(r1.as_u64() + 1, r2.as_u64());
    }

    #[test]
    fn test_idempotent_write() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&test_tuple()).unwrap();
        store.write_tuple(&test_tuple()).unwrap(); // same tuple again

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

        store.write_tuple(&test_tuple()).unwrap();
        assert!(store.has_tuple(&test_tuple().key()).unwrap());

        store.delete_tuple(&test_tuple().key()).unwrap();
        assert!(!store.has_tuple(&test_tuple().key()).unwrap());
    }

    #[test]
    fn test_delete_non_existent() {
        let mut store = storage();
        store.initialize().unwrap();

        let rev_before = store.current_revision().unwrap();
        let rev_after = store
            .delete_tuple(&key("user:999", "editor", "repo:nonexistent"))
            .unwrap();
        assert_eq!(rev_before, rev_after); // no bump
    }

    #[test]
    fn test_delete_subject() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&tuple("user:1", "viewer", "repo:b")).unwrap();

        assert_eq!(
            store
                .list_by_subject(&SubjectId::new("user:1").unwrap(), None)
                .unwrap()
                .len(),
            2
        );

        store
            .delete_subject(&SubjectId::new("user:1").unwrap())
            .unwrap();

        assert_eq!(
            store
                .list_by_subject(&SubjectId::new("user:1").unwrap(), None)
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn test_delete_object() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&tuple("user:2", "viewer", "repo:a")).unwrap();

        assert_eq!(
            store
                .list_by_object(&ResourceId::new("repo:a").unwrap(), None)
                .unwrap()
                .len(),
            2
        );

        store
            .delete_object(&ResourceId::new("repo:a").unwrap())
            .unwrap();

        assert_eq!(
            store
                .list_by_object(&ResourceId::new("repo:a").unwrap(), None)
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

        store.write_tuple(&tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&tuple("user:2", "viewer", "repo:a")).unwrap();

        let results = store
            .list_by_object(&ResourceId::new("repo:a").unwrap(), None)
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_list_by_object_with_relation() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&tuple("user:2", "viewer", "repo:a")).unwrap();

        let results = store
            .list_by_object(
                &ResourceId::new("repo:a").unwrap(),
                Some(&Relation::new("editor").unwrap()),
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].subject.as_str(), "user:1");
    }

    #[test]
    fn test_list_by_subject() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&tuple("user:1", "viewer", "repo:b")).unwrap();

        let results = store
            .list_by_subject(&SubjectId::new("user:1").unwrap(), None)
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_list_by_relation() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&tuple("user:2", "editor", "repo:a")).unwrap();
        store.write_tuple(&tuple("user:3", "viewer", "repo:a")).unwrap();

        let results = store
            .list_by_relation(
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
                .write_tuple(&tuple(
                    &format!("user:{i}"),
                    "editor",
                    "repo:fluxbus",
                ))
                .unwrap();
        }

        let page1 = store
            .query_tuples(
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

        store.write_tuple(&tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&tuple("user:2", "editor", "repo:a")).unwrap();

        let filter = TupleFilter {
            subject_type: Some("user".to_string()),
            ..Default::default()
        };
        let results = store
            .query_tuples(&filter, &PaginationParams::default(), &ConsistencyMode::MinimizeLatency)
            .unwrap();
        assert_eq!(results.tuples.len(), 2);
    }

    // ── Revision & Token ──

    #[test]
    fn test_current_revision() {
        let mut store = storage();
        store.initialize().unwrap();

        assert_eq!(store.current_revision().unwrap().as_u64(), 0);

        store.write_tuple(&test_tuple()).unwrap();
        assert_eq!(store.current_revision().unwrap().as_u64(), 1);

        store.write_tuple(&tuple("user:456", "viewer", "repo:other")).unwrap();
        assert_eq!(store.current_revision().unwrap().as_u64(), 2);
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

        let mut tx = store.begin_transaction().unwrap();
        tx.write(&test_tuple()).unwrap();
        tx.write(&tuple("user:456", "viewer", "repo:other")).unwrap();
        let rev = tx.commit().unwrap();

        assert!(rev.as_u64() > 0);
        assert!(store.has_tuple(&test_tuple().key()).unwrap());
    }

    #[test]
    fn test_transaction_rollback() {
        let mut store = storage();
        store.initialize().unwrap();

        let rev_before = store.current_revision().unwrap();

        let mut tx = store.begin_transaction().unwrap();
        tx.write(&test_tuple()).unwrap();
        tx.rollback().unwrap();

        assert_eq!(store.current_revision().unwrap(), rev_before);
        assert!(!store.has_tuple(&test_tuple().key()).unwrap());
    }

    #[test]
    fn test_savepoint_rollback() {
        let mut store = storage();
        store.initialize().unwrap();

        let mut tx = store.begin_transaction().unwrap();
        tx.write(&test_tuple()).unwrap();

        tx.savepoint("sp1").unwrap();
        tx.write(&tuple("user:savepoint", "test", "repo:sp")).unwrap();

        // Savepoint tuple should exist (it was written after the savepoint)
        tx.rollback_to_savepoint("sp1").unwrap();
        tx.release_savepoint("sp1").unwrap();

        let rev = tx.commit().unwrap();
        assert!(rev.as_u64() > 0);

        // After commit: only the original tuple exists, savepoint tuple was rolled back
        assert!(store.has_tuple(&test_tuple().key()).unwrap());
        assert!(!store.has_tuple(&key("user:savepoint", "test", "repo:sp")).unwrap());
    }

    #[test]
    fn test_transaction_rollback_on_drop() {
        let mut store = storage();
        store.initialize().unwrap();

        let rev_before = store.current_revision().unwrap();

        {
            let mut tx = store.begin_transaction().unwrap();
            tx.write(&test_tuple()).unwrap();
            // tx drops without commit
        }

        assert_eq!(store.current_revision().unwrap(), rev_before);
    }

    // ── Audit ──

    #[test]
    fn test_audit_log_writes() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&test_tuple()).unwrap();
        store
            .delete_tuple(&test_tuple().key())
            .unwrap();

        let audit = store
            .query_audit(
                &ResourceId::new("repo:fluxbus").unwrap(),
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
            .write_tuple(&tuple("user:1", "editor", "repo:a"))
            .unwrap();
        let r2 = store
            .write_tuple(&tuple("user:2", "viewer", "repo:a"))
            .unwrap();

        let audit = store
            .query_audit(
                &ResourceId::new("repo:a").unwrap(),
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

        store.write_tuple(&test_tuple()).unwrap();
        store.close().unwrap();

        // After close, can still read (pool connections may be live)
        assert!(store.has_tuple(&test_tuple().key()).unwrap());
    }

    // ── Revision Snapshots ──

    #[test]
    fn test_read_at_revision() {
        let mut store = storage();
        store.initialize().unwrap();

        // Write tuple at rev 1
        store.write_tuple(&tuple("user:1", "editor", "repo:a")).unwrap();
        let rev_before_delete = store.current_revision().unwrap();

        // Delete and re-write at rev 2+
        store.write_tuple(&tuple("user:2", "viewer", "repo:a")).unwrap();
        store.delete_tuple(&key("user:1", "editor", "repo:a")).unwrap();

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

        let rev = store.write_tuples_batch(&tuples).unwrap();
        assert!(rev.as_u64() > 0);

        assert!(store.has_tuple(&key("user:1", "editor", "repo:a")).unwrap());
        assert!(store.has_tuple(&key("user:2", "viewer", "repo:b")).unwrap());
        assert!(
            store
                .has_tuple(&key("team:eng", "owner", "workspace:core"))
                .unwrap()
        );
    }

    #[test]
    fn test_write_batch_empty() {
        let mut store = storage();
        store.initialize().unwrap();

        let rev = store.write_tuples_batch(&[]).unwrap();
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

        store.write_tuple(&tuple).unwrap();

        let read = store.read_tuple(&tuple.key()).unwrap().unwrap();
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
        };
        let store = SqliteStorage::new(config).unwrap();

        // In in-memory mode, WAL may not be used, but we verify no crash
        let mut store = store;
        store.initialize().unwrap();
        store.write_tuple(&test_tuple()).unwrap();
        assert!(store.has_tuple(&test_tuple().key()).unwrap());
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
                .list_by_object(&ResourceId::new("nonexistent").unwrap(), None)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .has_tuple(&key("user:1", "editor", "repo:a"))
                .unwrap()
                == false
        );
    }

    // ── Event Recovery ──

    #[test]
    fn test_event_recover_from_events() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&tuple("user:2", "viewer", "repo:b")).unwrap();
        let rev_before = store.current_revision().unwrap();

        let recovered = store.recover_from_events().unwrap();
        assert_eq!(recovered, rev_before);

        assert!(store.has_tuple(&key("user:1", "editor", "repo:a")).unwrap());
        assert!(store.has_tuple(&key("user:2", "viewer", "repo:b")).unwrap());
    }

    #[test]
    fn test_event_recover_point_in_time() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&tuple("user:1", "editor", "repo:a")).unwrap();
        store.write_tuple(&tuple("user:2", "viewer", "repo:b")).unwrap();

        let recovered = store.recover_to_revision(Revision::new(1)).unwrap();
        assert_eq!(recovered.as_u64(), 1);

        assert!(store.has_tuple(&key("user:1", "editor", "repo:a")).unwrap());
        assert!(!store.has_tuple(&key("user:2", "viewer", "repo:b")).unwrap());
    }

    #[test]
    fn test_event_compaction() {
        let mut store = storage();
        store.initialize().unwrap();

        store.write_tuple(&tuple("user:1", "editor", "repo:a")).unwrap();
        store.delete_tuple(&key("user:1", "editor", "repo:a")).unwrap();

        let before_events = store.conn().unwrap()
            .query_row("SELECT COUNT(*) FROM _aegis_events", [], |row| row.get::<_, i64>(0))
            .unwrap();

        assert_eq!(before_events, 2);

        let removed = store.compact_events().unwrap();
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

        let recovered = store.recover_from_events().unwrap();
        assert_eq!(recovered.as_u64(), 0);
    }
}
