use crate::error::{AegisError, AegisResult};
use crate::storage::traits::{
    BackendType, IntegrityReport, StorageBackend, StorageMeta, StorageTransaction, TupleFilter,
};
use crate::types::{
    AuditEntry, ConsistencyMode, PaginatedTuples, PaginationCursor, PaginationParams, PartitionId,
    Relation, RelationshipTuple, ResourceId, Revision, RevisionToken, SubjectId, TupleKey,
};
use chrono::{DateTime, Utc};
use mysql_async::prelude::Queryable;
use std::collections::HashMap;
use uuid::Uuid;

/// MySQL configuration.
#[derive(Debug, Clone)]
pub struct MysqlConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
    pub pool_size: usize,
}

impl Default for MysqlConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: String::new(),
            database: "aegis".to_string(),
            pool_size: 10,
        }
    }
}

/// MySQL-backed storage adapter.
pub struct MysqlStorage {
    pool: mysql_async::Pool,
    node_id: Uuid,
    runtime: tokio::runtime::Runtime,
}

impl MysqlStorage {
    pub fn new(config: MysqlConfig) -> AegisResult<Self> {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        let url = format!(
            "mysql://{}:{}@{}:{}/{}",
            config.user, config.password, config.host, config.port, config.database
        );
        let opts = mysql_async::Opts::from_url(&url)
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;
        let pool = mysql_async::Pool::new(opts);

        Ok(Self {
            pool,
            node_id: Uuid::new_v4(),
            runtime,
        })
    }

    async fn get_conn(&self) -> AegisResult<mysql_async::Conn> {
        self.pool
            .get_conn()
            .await
            .map_err(|e| AegisError::StorageConnection(e.to_string()))
    }

    async fn run_ddl_async(conn: &mut mysql_async::Conn) -> AegisResult<()> {
        let statements = [
            "CREATE TABLE IF NOT EXISTS _aegis_meta (
                `key`   VARCHAR(255) PRIMARY KEY,
                `value` TEXT NOT NULL
            )",
            "INSERT IGNORE INTO _aegis_meta (`key`, `value`) VALUES ('revision', '0')",
            "CREATE TABLE IF NOT EXISTS _aegis_tuples (
                `row_id`           BIGINT AUTO_INCREMENT PRIMARY KEY,
                `subject`          VARCHAR(512) NOT NULL,
                `relation`         VARCHAR(255) NOT NULL,
                `object`           VARCHAR(512) NOT NULL,
                `created_at`       VARCHAR(64) NOT NULL,
                `metadata`         TEXT,
                `revision_added`   BIGINT NOT NULL,
                `revision_removed` BIGINT DEFAULT NULL
            )",
            "CREATE INDEX IF NOT EXISTS idx_tuples_object
             ON _aegis_tuples(`object`(255))",
            "CREATE INDEX IF NOT EXISTS idx_tuples_subject
             ON _aegis_tuples(`subject`(255))",
            "CREATE INDEX IF NOT EXISTS idx_tuples_object_relation
             ON _aegis_tuples(`object`(255), `relation`(255))",
            "CREATE INDEX IF NOT EXISTS idx_tuples_subject_relation
             ON _aegis_tuples(`subject`(255), `relation`(255))",
            "CREATE TABLE IF NOT EXISTS _aegis_events (
                `event_id`  BIGINT AUTO_INCREMENT PRIMARY KEY,
                `revision`  BIGINT NOT NULL,
                `action`    VARCHAR(16) NOT NULL,
                `subject`   VARCHAR(512) NOT NULL,
                `relation`  VARCHAR(255) NOT NULL,
                `object`    VARCHAR(512) NOT NULL,
                `metadata`  TEXT,
                `timestamp` VARCHAR(64) NOT NULL,
                `identity`  VARCHAR(255)
            )",
            "CREATE INDEX IF NOT EXISTS idx_events_revision
             ON _aegis_events(`revision`)",
            "CREATE TABLE IF NOT EXISTS _aegis_schema (
                `version`    INTEGER NOT NULL,
                `applied_at` VARCHAR(64) NOT NULL,
                `checksum`   TEXT NOT NULL
            )",
        ];
        for stmt in &statements {
            conn.exec_drop(*stmt, ())
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        }
        Ok(())
    }

    async fn current_revision_async(conn: &mut mysql_async::Conn) -> AegisResult<Revision> {
        let row: Option<(i64,)> = conn
            .exec_first(
                "SELECT CAST(`value` AS SIGNED INTEGER) FROM _aegis_meta WHERE `key` = 'revision'",
                (),
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        let rev = row.map(|r| r.0).unwrap_or(0);
        Ok(Revision::new(rev as u64))
    }

    async fn bump_revision_async(conn: &mut mysql_async::Conn) -> AegisResult<Revision> {
        conn.exec_drop(
            "UPDATE _aegis_meta SET `value` = CAST(CAST(`value` AS SIGNED INTEGER) + 1 AS CHAR) WHERE `key` = 'revision'",
            (),
        )
        .await
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Self::current_revision_async(conn).await
    }

    async fn append_event_async(
        conn: &mut mysql_async::Conn,
        revision: Revision,
        action: &str,
        subject: &str,
        relation: &str,
        object: &str,
        metadata: Option<&str>,
        identity: Option<&str>,
    ) -> AegisResult<()> {
        let now = Utc::now().to_rfc3339();
        conn.exec_drop(
            "INSERT INTO _aegis_events (`revision`, `action`, `subject`, `relation`, `object`, `metadata`, `timestamp`, `identity`)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            (revision.as_u64() as i64, action, subject, relation, object, metadata, &now, identity),
        )
        .await
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }

    fn row_to_tuple(
        subject: String,
        relation: String,
        object: String,
        created_at: String,
        metadata_json: Option<String>,
    ) -> AegisResult<RelationshipTuple> {
        let subject = SubjectId::new(&subject)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        let relation = Relation::new(&relation)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        let object = ResourceId::new(&object)
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        let created_at: DateTime<Utc> = created_at.parse().unwrap_or_else(|_| Utc::now());
        let metadata = metadata_json
            .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());
        Ok(RelationshipTuple {
            subject,
            relation,
            object,
            created_at,
            metadata,
            valid_until: None,
            condition: None,
        })
    }
}

impl StorageBackend for MysqlStorage {
    fn initialize(&mut self) -> AegisResult<StorageMeta> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            Self::run_ddl_async(&mut conn).await?;
            let rev = Self::current_revision_async(&mut conn).await?;
            Ok(StorageMeta {
                schema_version: 1,
                current_revision: rev,
                backend_type: BackendType::Mysql,
                healthy: true,
            })
        })
    }

    fn backend_type(&self) -> BackendType {
        BackendType::Mysql
    }

    fn write_tuple(&self, partition_id: &PartitionId, tuple: &RelationshipTuple) -> AegisResult<Revision> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            let revision = Self::bump_revision_async(&mut conn).await?;
            let metadata_json = tuple
                .metadata
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;

            conn.exec_drop(
                "UPDATE _aegis_tuples SET `revision_removed` = ?
                 WHERE `subject` = ? AND `relation` = ? AND `object` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                (revision.as_u64() as i64, tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str(), partition_id.as_str()),
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            conn.exec_drop(
                "INSERT INTO _aegis_tuples (`subject`, `relation`, `object`, `created_at`, `metadata`, `revision_added`, `revision_removed`)
                 VALUES (?, ?, ?, ?, ?, ?, NULL)",
                (tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str(), tuple.created_at.to_rfc3339(), metadata_json.as_deref(), revision.as_u64() as i64),
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            Self::append_event_async(
                &mut conn, revision, "add", tuple.subject.as_str(),
                tuple.relation.as_str(), tuple.object.as_str(),
                metadata_json.as_deref(), None,
            )
            .await?;

            Ok(revision)
        })
    }

    fn write_tuples_batch(&self, partition_id: &PartitionId, tuples: &[RelationshipTuple]) -> AegisResult<Revision> {
        if tuples.is_empty() {
            return self.current_revision(partition_id);
        }
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            let revision = Self::bump_revision_async(&mut conn).await?;

            for tuple in tuples {
                let metadata_json = tuple
                    .metadata
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;

                conn.exec_drop(
                    "UPDATE _aegis_tuples SET `revision_removed` = ?
                     WHERE `subject` = ? AND `relation` = ? AND `object` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                    (revision.as_u64() as i64, tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str(), partition_id.as_str()),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

                conn.exec_drop(
                    "INSERT INTO _aegis_tuples (`subject`, `relation`, `object`, `created_at`, `metadata`, `revision_added`, `revision_removed`)
                     VALUES (?, ?, ?, ?, ?, ?, NULL)",
                    (tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str(), tuple.created_at.to_rfc3339(), metadata_json.as_deref(), revision.as_u64() as i64),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

                Self::append_event_async(
                    &mut conn, revision, "add", tuple.subject.as_str(),
                    tuple.relation.as_str(), tuple.object.as_str(),
                    metadata_json.as_deref(), None,
                )
                .await?;
            }

            Ok(revision)
        })
    }

    fn delete_tuple(&self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<Revision> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            let row: Option<(i64,)> = conn
                .exec_first(
                    "SELECT COUNT(*) FROM _aegis_tuples
                     WHERE `subject` = ? AND `relation` = ? AND `object` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                    (key.subject.as_str(), key.relation.as_str(), key.object.as_str(), partition_id.as_str()),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            let count = row.map(|r| r.0).unwrap_or(0);
            if count == 0 {
                return Self::current_revision_async(&mut conn).await;
            }

            let revision = Self::bump_revision_async(&mut conn).await?;
            conn.exec_drop(
                "UPDATE _aegis_tuples SET `revision_removed` = ?
                 WHERE `subject` = ? AND `relation` = ? AND `object` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                (revision.as_u64() as i64, key.subject.as_str(), key.relation.as_str(), key.object.as_str(), partition_id.as_str()),
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            Self::append_event_async(
                &mut conn, revision, "remove", key.subject.as_str(),
                key.relation.as_str(), key.object.as_str(), None, None,
            )
            .await?;

            Ok(revision)
        })
    }

    fn delete_subject(&self, partition_id: &PartitionId, subject: &SubjectId) -> AegisResult<Revision> {
        let subj = subject.as_str().to_string();
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;

            let rows: Vec<(String, String)> = conn
                .exec(
                    "SELECT `relation`, `object` FROM _aegis_tuples
                     WHERE `subject` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                    (&subj, partition_id.as_str()),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            if rows.is_empty() {
                return Self::current_revision_async(&mut conn).await;
            }

            let revision = Self::bump_revision_async(&mut conn).await?;

            conn.exec_drop(
                "UPDATE _aegis_tuples SET `revision_removed` = ?
                 WHERE `subject` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                (revision.as_u64() as i64, &subj, partition_id.as_str()),
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            for (relation, object) in &rows {
                Self::append_event_async(
                    &mut conn, revision, "remove", subject.as_str(),
                    relation, object, None, None,
                )
                .await?;
            }

            Ok(revision)
        })
    }

    fn delete_object(&self, partition_id: &PartitionId, object: &ResourceId) -> AegisResult<Revision> {
        let obj = object.as_str().to_string();
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;

            let rows: Vec<(String, String)> = conn
                .exec(
                    "SELECT `subject`, `relation` FROM _aegis_tuples
                     WHERE `object` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                    (&obj, partition_id.as_str()),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            if rows.is_empty() {
                return Self::current_revision_async(&mut conn).await;
            }

            let revision = Self::bump_revision_async(&mut conn).await?;

            conn.exec_drop(
                "UPDATE _aegis_tuples SET `revision_removed` = ?
                 WHERE `object` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                (revision.as_u64() as i64, &obj, partition_id.as_str()),
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            for (subject, relation) in &rows {
                Self::append_event_async(
                    &mut conn, revision, "remove", subject,
                    relation, object.as_str(), None, None,
                )
                .await?;
            }

            Ok(revision)
        })
    }

    fn has_tuple(&self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<bool> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            let row: Option<(i64,)> = conn
                .exec_first(
                    "SELECT COUNT(*) FROM _aegis_tuples
                     WHERE `subject` = ? AND `relation` = ? AND `object` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                    (key.subject.as_str(), key.relation.as_str(), key.object.as_str(), partition_id.as_str()),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(row.map(|r| r.0 > 0).unwrap_or(false))
        })
    }

    fn read_tuple(&self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<Option<RelationshipTuple>> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            let rows: Vec<(String, String, String, String, Option<String>)> = conn
                .exec(
                    "SELECT `subject`, `relation`, `object`, `created_at`, `metadata` FROM _aegis_tuples
                     WHERE `subject` = ? AND `relation` = ? AND `object` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                    (key.subject.as_str(), key.relation.as_str(), key.object.as_str(), partition_id.as_str()),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            if rows.is_empty() {
                return Ok(None);
            }

            let (subject, relation, object, created_at, metadata_json) = rows.into_iter().next().unwrap();
            Self::row_to_tuple(subject, relation, object, created_at, metadata_json).map(Some)
        })
    }

    fn list_by_object(
        &self, partition_id: &PartitionId, object: &ResourceId, relation: Option<&Relation>, consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let obj = object.as_str().to_string();
        let rel = relation.map(|r| r.as_str().to_string());
        let rev_filter = match consistency {
            ConsistencyMode::AtRevision(rev) => {
                let r = rev.as_u64() as i64;
                format!("`revision_added` <= {r} AND (`revision_removed` IS NULL OR `revision_removed` > {r})")
            }
            _ => "`revision_removed` IS NULL".to_string(),
        };

        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;

            let rows: Vec<(String, String, String, String, Option<String>)> = if let Some(ref r) = rel {
                conn.exec(
                    &format!(
                        "SELECT `subject`, `relation`, `object`, `created_at`, `metadata` FROM _aegis_tuples
                         WHERE `object` = ? AND `relation` = ? AND `partition_id` = ? AND {rev_filter}"
                    ),
                    (&obj, r.as_str(), partition_id.as_str()),
                )
                .await
            } else {
                conn.exec(
                    &format!(
                        "SELECT `subject`, `relation`, `object`, `created_at`, `metadata` FROM _aegis_tuples
                         WHERE `object` = ? AND `partition_id` = ? AND {rev_filter}"
                    ),
                    (&obj, partition_id.as_str()),
                )
                .await
            }
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut results = Vec::with_capacity(rows.len());
            for (subject, relation, object, created_at, metadata_json) in rows {
                results.push(Self::row_to_tuple(subject, relation, object, created_at, metadata_json)?);
            }
            Ok(results)
        })
    }

    fn list_by_subject(
        &self, partition_id: &PartitionId, subject: &SubjectId, relation: Option<&Relation>, consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let subj = subject.as_str().to_string();
        let rel = relation.map(|r| r.as_str().to_string());
        let rev_filter = match consistency {
            ConsistencyMode::AtRevision(rev) => {
                let r = rev.as_u64() as i64;
                format!("`revision_added` <= {r} AND (`revision_removed` IS NULL OR `revision_removed` > {r})")
            }
            _ => "`revision_removed` IS NULL".to_string(),
        };

        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;

            let rows: Vec<(String, String, String, String, Option<String>)> = if let Some(ref r) = rel {
                conn.exec(
                    &format!(
                        "SELECT `subject`, `relation`, `object`, `created_at`, `metadata` FROM _aegis_tuples
                         WHERE `subject` = ? AND `relation` = ? AND `partition_id` = ? AND {rev_filter}"
                    ),
                    (&subj, r.as_str(), partition_id.as_str()),
                )
                .await
            } else {
                conn.exec(
                    &format!(
                        "SELECT `subject`, `relation`, `object`, `created_at`, `metadata` FROM _aegis_tuples
                         WHERE `subject` = ? AND `partition_id` = ? AND {rev_filter}"
                    ),
                    (&subj, partition_id.as_str()),
                )
                .await
            }
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut results = Vec::with_capacity(rows.len());
            for (subject, relation, object, created_at, metadata_json) in rows {
                results.push(Self::row_to_tuple(subject, relation, object, created_at, metadata_json)?);
            }
            Ok(results)
        })
    }

    fn list_by_relation(
        &self, partition_id: &PartitionId, object: &ResourceId, relation: &Relation,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let obj = object.as_str().to_string();
        let rel = relation.as_str().to_string();
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;

            let rows: Vec<(String, String, String, String, Option<String>)> = conn
                .exec(
                    "SELECT `subject`, `relation`, `object`, `created_at`, `metadata` FROM _aegis_tuples
                     WHERE `object` = ? AND `relation` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                    (&obj, &rel, partition_id.as_str()),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut results = Vec::with_capacity(rows.len());
            for (subject, relation, object, created_at, metadata_json) in rows {
                results.push(Self::row_to_tuple(subject, relation, object, created_at, metadata_json)?);
            }
            Ok(results)
        })
    }

    fn query_tuples(
        &self, partition_id: &PartitionId, filter: &TupleFilter, pagination: &PaginationParams, consistency: &ConsistencyMode,
    ) -> AegisResult<PaginatedTuples> {
        let subj_type = filter.subject_type.clone();
        let rel = filter.relation.as_ref().map(|r| r.as_str().to_string());
        let obj_type = filter.object_type.clone();
        let meta_key = filter.metadata_key.clone();
        let rev_filter = match consistency {
            ConsistencyMode::AtRevision(rev) => {
                let r = rev.as_u64() as i64;
                format!("`revision_added` <= {r} AND (`revision_removed` IS NULL OR `revision_removed` > {r})")
            }
            _ => "`revision_removed` IS NULL".to_string(),
        };

        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            let revision = Self::current_revision_async(&mut conn).await?;

            let mut conditions = vec![format!("`partition_id` = ?1"), rev_filter];
            let mut values: Vec<mysql_async::Value> = vec![partition_id.as_str().into()];

            if let Some(st) = subj_type {
                values.push(format!("{st}:%").into());
                conditions.push(format!("`subject` LIKE ?{}", values.len()));
            }
            if let Some(r) = rel {
                values.push(r.into());
                conditions.push(format!("`relation` = ?{}", values.len()));
            }
            if let Some(ot) = obj_type {
                values.push(format!("{ot}:%").into());
                conditions.push(format!("`object` LIKE ?{}", values.len()));
            }
            if let Some(mk) = meta_key {
                values.push(format!("%{mk}%").into());
                conditions.push(format!("`metadata` LIKE ?{}", values.len()));
            }

            let where_clause = conditions.join(" AND ");
            let offset = pagination.cursor.as_ref().map(|c| c.offset).unwrap_or(0);
            let limit = pagination.limit;

            values.push((limit as i64).into());
            values.push((offset as i64).into());

            let sql = format!(
                "SELECT `subject`, `relation`, `object`, `created_at`, `metadata` FROM _aegis_tuples
                 WHERE {where_clause}
                 ORDER BY `subject`, `relation`, `object`
                 LIMIT ?{} OFFSET ?{}",
                values.len() - 1,
                values.len(),
            );

            let params = mysql_async::Params::Positional(values);
            let rows: Vec<(String, String, String, String, Option<String>)> = conn
                .exec(&sql, params)
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut tuples = Vec::with_capacity(rows.len());
            for (subject_str, relation_str, object_str, created_at, metadata_json) in rows {
                tuples.push(Self::row_to_tuple(subject_str, relation_str, object_str, created_at, metadata_json)?);
            }

            let next_cursor = if tuples.len() as u64 == limit {
                Some(PaginationCursor {
                    offset: offset + limit,
                    revision,
                })
            } else {
                None
            };

            Ok(PaginatedTuples { tuples, next_cursor, revision })
        })
    }

    fn current_revision(&self, partition_id: &PartitionId) -> AegisResult<Revision> {
        let _ = partition_id;
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            Self::current_revision_async(&mut conn).await
        })
    }

    fn current_token(&self) -> AegisResult<RevisionToken> {
        let revision = self.current_revision(&PartitionId::default())?;
        Ok(RevisionToken::new(revision, self.node_id))
    }

    fn begin_transaction(&self, partition_id: &PartitionId) -> AegisResult<Box<dyn StorageTransaction>> {
        let _ = partition_id;
        let node_id = self.node_id;
        let handle = self.runtime.handle().clone();
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            conn.exec_drop("BEGIN", ())
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(Box::new(MysqlTransaction::new(conn, handle, node_id)) as Box<dyn StorageTransaction>)
        })
    }

    fn query_audit(
        &self, partition_id: &PartitionId, object: Option<&ResourceId>, from_revision: Option<Revision>,
        to_revision: Option<Revision>, pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>> {
        let from = from_revision.map(|r| r.as_u64() as i64);
        let to = to_revision.map(|r| r.as_u64() as i64);
        let offset = pagination.cursor.as_ref().map(|c| c.offset).unwrap_or(0);
        let limit_val = pagination.limit;

        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            let mut sql = String::from(
                "SELECT `revision`, `action`, `subject`, `relation`, `object`, `timestamp`, `metadata`
                 FROM _aegis_events",
            );
            let mut conditions: Vec<String> = vec!["`partition_id` = ?1".to_string()];
            let mut values: Vec<mysql_async::Value> = vec![partition_id.as_str().into()];

            if let Some(obj) = object {
                values.push(obj.as_str().to_string().into());
                conditions.push(format!("`object` = ?{}", values.len()));
            }
            if let Some(f) = from {
                values.push(f.into());
                conditions.push(format!("`revision` >= ?{}", values.len()));
            }
            if let Some(t) = to {
                values.push(t.into());
                conditions.push(format!("`revision` <= ?{}", values.len()));
            }

            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));

            values.push((limit_val as i64).into());
            values.push((offset as i64).into());

            sql.push_str(&format!(
                " ORDER BY `revision` ASC LIMIT ?{} OFFSET ?{}",
                values.len() - 1,
                values.len(),
            ));

            let params = mysql_async::Params::Positional(values);
            let rows: Vec<(i64, String, String, String, String, String, Option<String>)> = conn
                .exec(&sql, params)
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let results: Vec<AuditEntry> = rows.into_iter().map(|(rev, action_str, subject, relation, obj, timestamp_str, meta_val)| {
                let timestamp: DateTime<Utc> = timestamp_str.parse().unwrap_or_else(|_| Utc::now());
                let metadata = meta_val
                    .filter(|s| !s.is_empty())
                    .and_then(|m| serde_json::from_str::<HashMap<String, String>>(&m).ok());
                let action = if action_str == "add" {
                    crate::types::TupleMutation::Add
                } else {
                    crate::types::TupleMutation::Remove
                };
                AuditEntry {
                    revision: Revision::new(rev as u64),
                    action,
                    subject,
                    relation,
                    object: obj,
                    timestamp,
                    metadata,
                }
            }).collect();

            Ok(results)
        })
    }

    fn read_schema_version(&self) -> AegisResult<u32> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            let result: Option<(i32,)> = conn
                .exec_first(
                    "SELECT `version` FROM _aegis_schema ORDER BY `version` DESC LIMIT 1",
                    (),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            match result {
                Some((v,)) => Ok(v as u32),
                None => Ok(0),
            }
        })
    }

    fn write_schema_version(&self, version: u32) -> AegisResult<()> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            conn.exec_drop(
                "INSERT INTO _aegis_schema (`version`, `applied_at`, `checksum`) VALUES (?, ?, ?)",
                (version as i32, Utc::now().to_rfc3339(), ""),
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(())
        })
    }

    fn integrity_check(&self) -> AegisResult<IntegrityReport> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            match conn.exec_drop("SELECT 1 FROM _aegis_meta LIMIT 1", ()).await {
                Ok(_) => Ok(IntegrityReport {
                    passed: true,
                    details: vec!["ok".to_string()],
                    backend_type: BackendType::Mysql,
                }),
                Err(e) => Ok(IntegrityReport {
                    passed: false,
                    details: vec![e.to_string()],
                    backend_type: BackendType::Mysql,
                }),
            }
        })
    }

    fn delete_events_before(&self, partition_id: &PartitionId, cutoff: DateTime<Utc>) -> AegisResult<usize> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            let cutoff_str = cutoff.to_rfc3339();
            let result = conn
                .exec_iter(
                    "DELETE FROM _aegis_events WHERE `partition_id` = ? AND `timestamp` < ?",
                    (partition_id.as_str(), &cutoff_str),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(result.affected_rows() as usize)
        })
    }

    fn delete_soft_deleted_tuples_before(&self, partition_id: &PartitionId, cutoff: DateTime<Utc>) -> AegisResult<usize> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;
            let cutoff_str = cutoff.to_rfc3339();
            let result = conn
                .exec_iter(
                    "DELETE FROM _aegis_tuples
                     WHERE `partition_id` = ?
                       AND `revision_removed` IS NOT NULL
                       AND `revision_removed` <= (
                         SELECT COALESCE(MAX(`revision`), 0) FROM _aegis_events
                         WHERE `partition_id` = ? AND `timestamp` < ?
                       )",
                    (partition_id.as_str(), partition_id.as_str(), &cutoff_str),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(result.affected_rows() as usize)
        })
    }

    fn recover_from_events(&self, partition_id: &PartitionId, to_revision: Option<Revision>) -> AegisResult<Revision> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;

            conn.exec_drop("DELETE FROM _aegis_tuples", ())
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let rows: Vec<(i64, String, String, String, String, Option<String>)> = conn
                .exec(
                    "SELECT `revision`, `action`, `subject`, `relation`, `object`, `metadata`
                     FROM _aegis_events
                     WHERE `partition_id` = ?
                     ORDER BY `revision` ASC, `event_id` ASC",
                    (partition_id.as_str(),),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut last_revision = Revision::ZERO;

            for (rev, action, subject, relation, object, metadata) in &rows {
                let revision = Revision::new(*rev as u64);
                if let Some(target) = to_revision {
                    if revision > target {
                        continue;
                    }
                }
                let now = Utc::now().to_rfc3339();

                match action.as_str() {
                    "add" => {
                        conn.exec_drop(
                            "INSERT INTO _aegis_tuples (`subject`, `relation`, `object`, `created_at`, `metadata`, `revision_added`, `revision_removed`)
                             VALUES (?, ?, ?, ?, ?, ?, NULL)",
                            (subject.as_str(), relation.as_str(), object.as_str(), &now, metadata.as_deref(), *rev),
                        )
                        .await
                        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                    }
                    "remove" => {
                        conn.exec_drop(
                            "UPDATE _aegis_tuples SET `revision_removed` = ?
                             WHERE `subject` = ? AND `relation` = ? AND `object` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
                            (*rev, subject.as_str(), relation.as_str(), object.as_str(), partition_id.as_str()),
                        )
                        .await
                        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                    }
                    _ => {}
                }

                last_revision = revision;
            }

            if last_revision != Revision::ZERO {
                let current = Self::current_revision_async(&mut conn).await?;
                if current != last_revision {
                    conn.exec_drop(
                        "UPDATE _aegis_meta SET `value` = ? WHERE `key` = 'revision'",
                        (last_revision.as_u64() as i64,),
                    )
                    .await
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                }
            }

            Ok(last_revision)
        })
    }

    fn compact_events(&self, partition_id: &PartitionId) -> AegisResult<usize> {
        self.runtime.block_on(async {
            let mut conn = self.get_conn().await?;

            let rows: Vec<(i64, String, String, String, String)> = conn
                .exec(
                    "SELECT `event_id`, `action`, `subject`, `relation`, `object` FROM _aegis_events WHERE `partition_id` = ? ORDER BY `revision` ASC, `event_id` ASC",
                    (partition_id.as_str(),),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut adds: HashMap<(String, String, String), i64> = HashMap::new();
            let mut to_delete: Vec<i64> = Vec::new();

            for (event_id, action, subject, relation, object) in &rows {
                let key = (subject.clone(), relation.clone(), object.clone());
                match action.as_str() {
                    "add" => {
                        adds.insert(key, *event_id);
                    }
                    "remove" => {
                        if let Some(add_id) = adds.remove(&key) {
                            to_delete.push(add_id);
                            to_delete.push(*event_id);
                        }
                    }
                    _ => {}
                }
            }

            if to_delete.is_empty() {
                return Ok(0);
            }

            let total = to_delete.len();
            for id in &to_delete {
                conn.exec_drop(
                    "DELETE FROM _aegis_events WHERE `event_id` = ? AND `partition_id` = ?",
                    (*id, partition_id.as_str()),
                )
                    .await
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            }

            Ok(total)
        })
    }

    fn close(&self) -> AegisResult<()> {
        self.runtime
            .block_on(self.pool.clone().disconnect())
            .map_err(|e| AegisError::StorageConnection(e.to_string()))
    }
}

/// A MySQL transaction wrapping a pooled connection.
pub struct MysqlTransaction {
    conn: Option<tokio::sync::Mutex<mysql_async::Conn>>,
    runtime: tokio::runtime::Handle,
    _node_id: Uuid,
}

impl MysqlTransaction {
    fn new(
        conn: mysql_async::Conn,
        runtime: tokio::runtime::Handle,
        _node_id: Uuid,
    ) -> Self {
        Self {
            conn: Some(tokio::sync::Mutex::new(conn)),
            runtime,
            _node_id,
        }
    }

    fn conn_ref(&self) -> AegisResult<&tokio::sync::Mutex<mysql_async::Conn>> {
        self.conn
            .as_ref()
            .ok_or_else(|| AegisError::Internal("transaction already consumed".into()))
    }

    fn take_conn(&mut self) -> AegisResult<tokio::sync::Mutex<mysql_async::Conn>> {
        self.conn
            .take()
            .ok_or_else(|| AegisError::Internal("transaction already consumed".into()))
    }

    fn block_on<T>(&self, fut: impl std::future::Future<Output = AegisResult<T>>) -> AegisResult<T> {
        self.runtime.block_on(fut)
    }

    async fn bump_revision_async(conn: &mut mysql_async::Conn) -> AegisResult<Revision> {
        conn.exec_drop(
            "UPDATE _aegis_meta SET `value` = CAST(CAST(`value` AS SIGNED INTEGER) + 1 AS CHAR) WHERE `key` = 'revision'",
            (),
        )
        .await
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        let row: Option<(i64,)> = conn
            .exec_first(
                "SELECT CAST(`value` AS SIGNED INTEGER) FROM _aegis_meta WHERE `key` = 'revision'",
                (),
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        let rev = row.map(|r| r.0).unwrap_or(0);
        Ok(Revision::new(rev as u64))
    }

    async fn append_event_async(
        conn: &mut mysql_async::Conn,
        revision: Revision,
        action: &str,
        subject: &str,
        relation: &str,
        object: &str,
        metadata: Option<&str>,
    ) -> AegisResult<()> {
        let now = Utc::now().to_rfc3339();
        conn.exec_drop(
            "INSERT INTO _aegis_events (`revision`, `action`, `subject`, `relation`, `object`, `metadata`, `timestamp`, `identity`)
             VALUES (?, ?, ?, ?, ?, ?, ?, NULL)",
            (revision.as_u64() as i64, action, subject, relation, object, metadata, &now),
        )
        .await
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
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
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(AegisError::Validation(
                crate::types::ValidationError::InvalidCharacters(name.to_string()),
            ));
        }
        Ok(())
    }

    async fn write_impl(
        conn: &mut mysql_async::Conn,
        partition_id: &PartitionId,
        tuple: &RelationshipTuple,
    ) -> AegisResult<()> {
        let revision = Self::bump_revision_async(conn).await?;
        let metadata_json = tuple
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;

        conn.exec_drop(
            "UPDATE _aegis_tuples SET `revision_removed` = ?
             WHERE `subject` = ? AND `relation` = ? AND `object` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
            (revision.as_u64() as i64, tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str(), partition_id.as_str()),
        )
        .await
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        conn.exec_drop(
            "INSERT INTO _aegis_tuples (`subject`, `relation`, `object`, `created_at`, `metadata`, `revision_added`, `revision_removed`)
             VALUES (?, ?, ?, ?, ?, ?, NULL)",
            (tuple.subject.as_str(), tuple.relation.as_str(), tuple.object.as_str(), tuple.created_at.to_rfc3339(), metadata_json.as_deref(), revision.as_u64() as i64),
        )
        .await
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Self::append_event_async(
            conn, revision, "add", tuple.subject.as_str(),
            tuple.relation.as_str(), tuple.object.as_str(),
            metadata_json.as_deref(),
        )
        .await?;

        Ok(())
    }

    async fn delete_impl(
        conn: &mut mysql_async::Conn,
        partition_id: &PartitionId,
        key: &TupleKey,
    ) -> AegisResult<()> {
        let revision = Self::bump_revision_async(conn).await?;

        conn.exec_drop(
            "UPDATE _aegis_tuples SET `revision_removed` = ?
             WHERE `subject` = ? AND `relation` = ? AND `object` = ? AND `partition_id` = ? AND `revision_removed` IS NULL",
            (revision.as_u64() as i64, key.subject.as_str(), key.relation.as_str(), key.object.as_str(), partition_id.as_str()),
        )
        .await
        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

        Self::append_event_async(
            conn, revision, "remove", key.subject.as_str(),
            key.relation.as_str(), key.object.as_str(), None,
        )
        .await?;

        Ok(())
    }
}

impl StorageTransaction for MysqlTransaction {
    fn write(&mut self, partition_id: &PartitionId, tuple: &RelationshipTuple) -> AegisResult<()> {
        let tuple_clone = tuple.clone();
        let mutex = self.take_conn()?;
        let handle = self.runtime.clone();
        let result = handle.block_on(async {
            let mut conn = mutex.lock().await;
            Self::write_impl(&mut conn, partition_id, &tuple_clone).await
        });
        self.conn = Some(mutex);
        result
    }

    fn delete(&mut self, partition_id: &PartitionId, key: &TupleKey) -> AegisResult<()> {
        let key_clone = key.clone();
        let mutex = self.take_conn()?;
        let handle = self.runtime.clone();
        let result = handle.block_on(async {
            let mut conn = mutex.lock().await;
            Self::delete_impl(&mut conn, partition_id, &key_clone).await
        });
        self.conn = Some(mutex);
        result
    }

    fn savepoint(&self, name: &str) -> AegisResult<()> {
        Self::validate_savepoint_name(name)?;
        let name_owned = name.to_string();
        let mutex = self.conn_ref()?;
        self.block_on(async {
            let mut conn = mutex.lock().await;
            conn.exec_drop(&format!("SAVEPOINT \"{}\"", name_owned), ())
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(())
        })
    }

    fn rollback_to_savepoint(&self, name: &str) -> AegisResult<()> {
        Self::validate_savepoint_name(name)?;
        let name_owned = name.to_string();
        let mutex = self.conn_ref()?;
        self.block_on(async {
            let mut conn = mutex.lock().await;
            conn.exec_drop(&format!("ROLLBACK TO SAVEPOINT \"{}\"", name_owned), ())
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(())
        })
    }

    fn release_savepoint(&self, name: &str) -> AegisResult<()> {
        Self::validate_savepoint_name(name)?;
        let name_owned = name.to_string();
        let mutex = self.conn_ref()?;
        self.block_on(async {
            let mut conn = mutex.lock().await;
            conn.exec_drop(&format!("RELEASE SAVEPOINT \"{}\"", name_owned), ())
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(())
        })
    }

    fn commit(self: Box<Self>) -> AegisResult<Revision> {
        let s = *self;
        let mutex = s.conn
            .ok_or_else(|| AegisError::Internal("transaction already consumed".into()))?;
        let handle = s.runtime;
        handle.block_on(async {
            let mut conn = mutex.lock().await;
            conn.exec_drop("COMMIT", ())
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            let row: Option<(i64,)> = conn
                .exec_first(
                    "SELECT CAST(`value` AS SIGNED INTEGER) FROM _aegis_meta WHERE `key` = 'revision'",
                    (),
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            let rev = row.map(|r| r.0).unwrap_or(0);
            Ok(Revision::new(rev as u64))
        })
    }

    fn rollback(self: Box<Self>) -> AegisResult<()> {
        let s = *self;
        let mutex = s.conn
            .ok_or_else(|| AegisError::Internal("transaction already consumed".into()))?;
        let handle = s.runtime;
        handle.block_on(async {
            let mut conn = mutex.lock().await;
            conn.exec_drop("ROLLBACK", ())
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(())
        })
    }
}
