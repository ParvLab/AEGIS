use crate::error::{AegisError, AegisResult};
use crate::storage::traits::{
    BackendType, IntegrityReport, StorageBackend, StorageMeta, StorageTransaction, TupleFilter,
};
use crate::types::{
    AuditEntry, ConsistencyMode, PaginatedTuples, PaginationParams, Relation, RelationshipTuple,
    ResourceId, Revision, RevisionToken, SubjectId, TupleKey, TupleMutation,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

/// PostgreSQL-backed storage adapter.
#[cfg(feature = "postgres")]
pub struct PostgresStorage {
    pool: deadpool_postgres::Pool,
    node_id: Uuid,
    runtime: tokio::runtime::Runtime,
}

#[cfg(feature = "postgres")]
impl PostgresStorage {
    pub fn new(connection_string: &str) -> AegisResult<Self> {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        let config = tokio_postgres::Config::from_str(connection_string)
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        let mgr = deadpool_postgres::Manager::new(config, deadpool_postgres::Runtime::Tokio1);
        let pool = deadpool_postgres::Pool::builder(mgr)
            .max_size(10)
            .build()
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        Ok(Self {
            pool,
            node_id: Uuid::new_v4(),
            runtime,
        })
    }

    async fn get_client(&self) -> AegisResult<deadpool_postgres::Object> {
        self.pool
            .get()
            .await
            .map_err(|e| AegisError::StorageConnection(e.to_string()))
    }

    async fn run_ddl_async(client: &tokio_postgres::Client) -> AegisResult<()> {
        let statements = [
            "CREATE TABLE IF NOT EXISTS _aegis_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            "INSERT INTO _aegis_meta (key, value) VALUES ('revision', '0') ON CONFLICT (key) DO NOTHING",
            "CREATE TABLE IF NOT EXISTS _aegis_tuples (
                row_id           BIGSERIAL PRIMARY KEY,
                subject          TEXT NOT NULL,
                relation         TEXT NOT NULL,
                object           TEXT NOT NULL,
                created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                metadata         JSONB,
                revision_added   BIGINT NOT NULL,
                revision_removed BIGINT DEFAULT NULL
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_tuples_active
             ON _aegis_tuples(subject, relation, object) WHERE revision_removed IS NULL",
            "CREATE INDEX IF NOT EXISTS idx_tuples_object ON _aegis_tuples(object)",
            "CREATE INDEX IF NOT EXISTS idx_tuples_subject ON _aegis_tuples(subject)",
            "CREATE INDEX IF NOT EXISTS idx_tuples_object_relation ON _aegis_tuples(object, relation)",
            "CREATE INDEX IF NOT EXISTS idx_tuples_subject_relation ON _aegis_tuples(subject, relation)",
            "CREATE TABLE IF NOT EXISTS _aegis_events (
                event_id   BIGSERIAL PRIMARY KEY,
                revision   BIGINT NOT NULL,
                action     TEXT NOT NULL,
                subject    TEXT NOT NULL,
                relation   TEXT NOT NULL,
                object     TEXT NOT NULL,
                metadata   JSONB,
                timestamp  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                identity   TEXT
            )",
            "CREATE TABLE IF NOT EXISTS _aegis_schema (
                version    INTEGER NOT NULL,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                checksum   TEXT NOT NULL
            )",
        ];
        for stmt in &statements {
            client
                .execute(stmt, &[])
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        }
        Ok(())
    }

    async fn current_revision_async(client: &tokio_postgres::Client) -> AegisResult<Revision> {
        let row = client
            .query_one(
                "SELECT COALESCE(CAST(value AS BIGINT), 0) FROM _aegis_meta WHERE key = 'revision'",
                &[],
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        let rev: i64 = row.get(0);
        Ok(Revision::new(rev as u64))
    }

    async fn bump_revision_async(client: &tokio_postgres::Client) -> AegisResult<Revision> {
        client
            .execute(
                "UPDATE _aegis_meta SET value = CAST(CAST(value AS BIGINT) + 1 AS TEXT) WHERE key = 'revision'",
                &[],
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Self::current_revision_async(client).await
    }

    async fn append_event_async(
        client: &tokio_postgres::Client,
        revision: Revision,
        action: &str,
        subject: &str,
        relation: &str,
        object: &str,
        metadata: Option<&serde_json::Value>,
    ) -> AegisResult<()> {
        client
            .execute(
                "INSERT INTO _aegis_events (revision, action, subject, relation, object, metadata)
                 VALUES ($1, $2, $3, $4, $5, $6)",
                &[&(revision.as_u64() as i64), &action, &subject, &relation, &object, &metadata],
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
        Ok(())
    }
}

#[cfg(feature = "postgres")]
impl StorageBackend for PostgresStorage {
    fn initialize(&mut self) -> AegisResult<StorageMeta> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            Self::run_ddl_async(&client).await?;
            let rev = Self::current_revision_async(&client).await?;
            Ok(StorageMeta {
                schema_version: 1,
                current_revision: rev,
                backend_type: BackendType::Postgres,
                healthy: true,
            })
        })
    }

    fn backend_type(&self) -> BackendType {
        BackendType::Postgres
    }

    fn write_tuple(&self, tuple: &RelationshipTuple) -> AegisResult<Revision> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let revision = Self::bump_revision_async(&client).await?;
            let meta_val = tuple
                .metadata
                .as_ref()
                .map(|m| serde_json::to_value(m).unwrap_or_default());

            client
                .execute(
                    "UPDATE _aegis_tuples SET revision_removed = $1
                     WHERE subject = $2 AND relation = $3 AND object = $4 AND revision_removed IS NULL",
                    &[&(revision.as_u64() as i64), &tuple.subject.as_str(), &tuple.relation.as_str(), &tuple.object.as_str()],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            client
                .execute(
                    "INSERT INTO _aegis_tuples (subject, relation, object, created_at, metadata, revision_added)
                     VALUES ($1, $2, $3, $4, $5, $6)",
                    &[&tuple.subject.as_str(), &tuple.relation.as_str(), &tuple.object.as_str(), &tuple.created_at, &meta_val, &(revision.as_u64() as i64)],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            Self::append_event_async(
                &client, revision, "add", tuple.subject.as_str(), tuple.relation.as_str(),
                tuple.object.as_str(), meta_val.as_ref(),
            )
            .await?;

            Ok(revision)
        })
    }

    fn write_tuples_batch(&self, _tuples: &[RelationshipTuple]) -> AegisResult<Revision> {
        Err(AegisError::NotImplemented("PostgreSQL batch writes".into()))
    }

    fn delete_tuple(&self, key: &TupleKey) -> AegisResult<Revision> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let row = client
                .query_one(
                    "SELECT COUNT(*)::bigint FROM _aegis_tuples
                     WHERE subject = $1 AND relation = $2 AND object = $3 AND revision_removed IS NULL",
                    &[&key.subject.as_str(), &key.relation.as_str(), &key.object.as_str()],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            let count: i64 = row.get(0);
            if count == 0 {
                return Self::current_revision_async(&client).await;
            }

            let revision = Self::bump_revision_async(&client).await?;
            client
                .execute(
                    "UPDATE _aegis_tuples SET revision_removed = $1
                     WHERE subject = $2 AND relation = $3 AND object = $4 AND revision_removed IS NULL",
                    &[&(revision.as_u64() as i64), &key.subject.as_str(), &key.relation.as_str(), &key.object.as_str()],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            Self::append_event_async(
                &client, revision, "remove", key.subject.as_str(), key.relation.as_str(),
                key.object.as_str(), None,
            )
            .await?;

            Ok(revision)
        })
    }

    fn delete_subject(&self, _subject: &SubjectId) -> AegisResult<Revision> {
        Err(AegisError::NotImplemented("PostgreSQL delete subject".into()))
    }

    fn delete_object(&self, _object: &ResourceId) -> AegisResult<Revision> {
        Err(AegisError::NotImplemented("PostgreSQL delete object".into()))
    }

    fn has_tuple(&self, key: &TupleKey) -> AegisResult<bool> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let row = client
                .query_one(
                    "SELECT COUNT(*)::bigint FROM _aegis_tuples
                     WHERE subject = $1 AND relation = $2 AND object = $3 AND revision_removed IS NULL",
                    &[&key.subject.as_str(), &key.relation.as_str(), &key.object.as_str()],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            let count: i64 = row.get(0);
            Ok(count > 0)
        })
    }

    fn read_tuple(&self, key: &TupleKey) -> AegisResult<Option<RelationshipTuple>> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let rows = client
                .query(
                    "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                     WHERE subject = $1 AND relation = $2 AND object = $3 AND revision_removed IS NULL",
                    &[&key.subject.as_str(), &key.relation.as_str(), &key.object.as_str()],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            if rows.is_empty() {
                return Ok(None);
            }

            let row = &rows[0];
            let subj: String = row.get("subject");
            let rel: String = row.get("relation");
            let obj: String = row.get("object");
            let created: DateTime<Utc> = row.get("created_at");
            let meta_val: Option<serde_json::Value> = row.get("metadata");
            let metadata = meta_val.and_then(|v| serde_json::from_value::<HashMap<String, String>>(v).ok());

            let subject = SubjectId::new(&subj)
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            let relation = Relation::new(&rel)
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            let object = ResourceId::new(&obj)
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            Ok(Some(RelationshipTuple {
                subject,
                relation,
                object,
                created_at: created,
                metadata,
            }))
        })
    }

    fn list_by_object(
        &self, object: &ResourceId, relation: Option<&Relation>,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let obj = object.as_str().to_string();
        let rel = relation.map(|r| r.as_str().to_string());
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let rows = if let Some(ref r) = rel {
                client
                    .query(
                        "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                         WHERE object = $1 AND relation = $2 AND revision_removed IS NULL",
                        &[&obj, r],
                    )
                    .await
            } else {
                client
                    .query(
                        "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                         WHERE object = $1 AND revision_removed IS NULL",
                        &[&obj],
                    )
                    .await
            }
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut results = Vec::with_capacity(rows.len());
            for row in &rows {
                let subj: String = row.get("subject");
                let r: String = row.get("relation");
                let o: String = row.get("object");
                let created: DateTime<Utc> = row.get("created_at");
                let meta_val: Option<serde_json::Value> = row.get("metadata");
                let metadata = meta_val.and_then(|v| serde_json::from_value::<HashMap<String, String>>(v).ok());
                let subject = SubjectId::new(&subj)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let relation = Relation::new(&r)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let object = ResourceId::new(&o)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                results.push(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at: created,
                    metadata,
                });
            }
            Ok(results)
        })
    }

    fn list_by_subject(
        &self, subject: &SubjectId, relation: Option<&Relation>,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let subj = subject.as_str().to_string();
        let rel = relation.map(|r| r.as_str().to_string());
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let rows = if let Some(ref r) = rel {
                client
                    .query(
                        "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                         WHERE subject = $1 AND relation = $2 AND revision_removed IS NULL",
                        &[&subj, r],
                    )
                    .await
            } else {
                client
                    .query(
                        "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                         WHERE subject = $1 AND revision_removed IS NULL",
                        &[&subj],
                    )
                    .await
            }
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut results = Vec::with_capacity(rows.len());
            for row in &rows {
                let s: String = row.get("subject");
                let r: String = row.get("relation");
                let o: String = row.get("object");
                let created: DateTime<Utc> = row.get("created_at");
                let meta_val: Option<serde_json::Value> = row.get("metadata");
                let metadata = meta_val.and_then(|v| serde_json::from_value::<HashMap<String, String>>(v).ok());
                let subject = SubjectId::new(&s)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let relation = Relation::new(&r)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let object = ResourceId::new(&o)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                results.push(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at: created,
                    metadata,
                });
            }
            Ok(results)
        })
    }

    fn list_by_relation(
        &self, object: &ResourceId, relation: &Relation,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let obj = object.as_str().to_string();
        let rel = relation.as_str().to_string();
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let rows = client
                .query(
                    "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                     WHERE object = $1 AND relation = $2 AND revision_removed IS NULL",
                    &[&obj, &rel],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut results = Vec::with_capacity(rows.len());
            for row in &rows {
                let s: String = row.get("subject");
                let r: String = row.get("relation");
                let o: String = row.get("object");
                let created: DateTime<Utc> = row.get("created_at");
                let meta_val: Option<serde_json::Value> = row.get("metadata");
                let metadata = meta_val.and_then(|v| serde_json::from_value::<HashMap<String, String>>(v).ok());
                let subject = SubjectId::new(&s)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let relation = Relation::new(&r)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let object = ResourceId::new(&o)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                results.push(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at: created,
                    metadata,
                });
            }
            Ok(results)
        })
    }

    fn query_tuples(
        &self, _filter: &TupleFilter, _pagination: &PaginationParams, _consistency: &ConsistencyMode,
    ) -> AegisResult<PaginatedTuples> {
        Err(AegisError::NotImplemented("PostgreSQL query tuples".into()))
    }

    fn current_revision(&self) -> AegisResult<Revision> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            Self::current_revision_async(&client).await
        })
    }

    fn current_token(&self) -> AegisResult<RevisionToken> {
        let revision = self.current_revision()?;
        Ok(RevisionToken::new(revision, self.node_id))
    }

    fn begin_transaction(&self) -> AegisResult<Box<dyn StorageTransaction>> {
        Err(AegisError::NotImplemented("PostgreSQL transactions".into()))
    }

    fn query_audit(
        &self, object: &ResourceId, from_revision: Option<Revision>,
        to_revision: Option<Revision>, pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>> {
        let obj = object.as_str().to_string();
        let from = from_revision.map(|r| r.as_u64() as i64);
        let to = to_revision.map(|r| r.as_u64() as i64);
        let offset = pagination.cursor.as_ref().map(|c| c.offset).unwrap_or(0);
        let limit_val = pagination.limit;

        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let mut sql = String::from(
                "SELECT revision, action, subject, relation, object, timestamp, metadata
                 FROM _aegis_events WHERE object = $1",
            );
            let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> =
                vec![Box::new(obj)];
            let mut idx = 2;
            if let Some(f) = from {
                sql.push_str(&format!(" AND revision >= ${idx}"));
                params.push(Box::new(f));
                idx += 1;
            }
            if let Some(t) = to {
                sql.push_str(&format!(" AND revision <= ${idx}"));
                params.push(Box::new(t));
                idx += 1;
            }
            sql.push_str(" ORDER BY revision ASC");

            let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                params.iter().map(|p| p.as_ref()).collect();

            let rows = client
                .query(&sql, &param_refs)
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let results: Vec<AuditEntry> = rows
                .into_iter()
                .skip(offset as usize)
                .take(limit_val as usize)
                .map(|row| {
                    let rev: i64 = row.get("revision");
                    let action_str: String = row.get("action");
                    let subject: String = row.get("subject");
                    let relation: String = row.get("relation");
                    let obj: String = row.get("object");
                    let ts: DateTime<Utc> = row.get("timestamp");
                    let meta_val: Option<serde_json::Value> = row.get("metadata");
                    let metadata =
                        meta_val.and_then(|v| serde_json::from_value::<HashMap<String, String>>(v).ok());
                    let action = if action_str == "add" {
                        TupleMutation::Add
                    } else {
                        TupleMutation::Remove
                    };
                    AuditEntry {
                        revision: Revision::new(rev as u64),
                        action,
                        subject,
                        relation,
                        object: obj,
                        timestamp: ts,
                        metadata,
                    }
                })
                .collect();

            Ok(results)
        })
    }

    fn integrity_check(&self) -> AegisResult<IntegrityReport> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            match client.query_one("SELECT 1 FROM _aegis_meta LIMIT 1", &[]).await {
                Ok(_) => Ok(IntegrityReport {
                    passed: true,
                    details: vec!["ok".to_string()],
                    backend_type: BackendType::Postgres,
                }),
                Err(e) => Ok(IntegrityReport {
                    passed: false,
                    details: vec![e.to_string()],
                    backend_type: BackendType::Postgres,
                }),
            }
        })
    }

    fn close(&self) -> AegisResult<()> {
        Ok(())
    }
}
