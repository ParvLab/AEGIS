use crate::error::{AegisError, AegisResult};
use crate::storage::traits::{
    BackendType, IntegrityReport, StorageBackend, StorageMeta, StorageTransaction, TupleFilter,
};
use crate::types::{
    AuditEntry, ConsistencyMode, PaginatedTuples, PaginationCursor, PaginationParams, Relation,
    RelationshipTuple, ResourceId, Revision, RevisionToken, SubjectId, TupleKey, TupleMutation,
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
        Self::with_pool_config(connection_string, 10)
    }

    /// Create storage with a configurable pool size.
    pub fn with_pool_config(connection_string: &str, max_pool_size: usize) -> AegisResult<Self> {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        let config = tokio_postgres::Config::from_str(connection_string)
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;

        // Detect TLS requirement from connection string sslmode parameter
        let conn_lower = connection_string.to_lowercase();
        let use_tls = conn_lower.contains("sslmode=require")
            || conn_lower.contains("sslmode=verify-ca")
            || conn_lower.contains("sslmode=verify-full");

        let pool: deadpool_postgres::Pool = if use_tls {
            let tls_config = rustls::ClientConfig::builder()
                .with_root_certificates({
                    let mut roots = rustls::RootCertStore::empty();
                    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                    roots
                })
                .with_no_client_auth();
            let tls = tokio_postgres_rustls::MakeRustlsConnect::new(tls_config);
            let mgr = deadpool_postgres::Manager::new(config, tls);
            deadpool_postgres::Pool::builder(mgr)
                .max_size(max_pool_size)
                .build()
                .map_err(|e| AegisError::StorageConnection(e.to_string()))?
        } else {
            let mgr = deadpool_postgres::Manager::new(config, tokio_postgres::NoTls);
            deadpool_postgres::Pool::builder(mgr)
                .max_size(max_pool_size)
                .build()
                .map_err(|e| AegisError::StorageConnection(e.to_string()))?
        };

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
                .execute(*stmt, &[])
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
                .map(|m| serde_json::to_value(m))
                .transpose()
                .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;

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

    fn write_tuples_batch(&self, tuples: &[RelationshipTuple]) -> AegisResult<Revision> {
        if tuples.is_empty() {
            return self.current_revision();
        }
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let revision = Self::bump_revision_async(&client).await?;

            for tuple in tuples {
                let meta_val = tuple
                    .metadata
                    .as_ref()
                    .map(|m| serde_json::to_value(m))
                    .transpose()
                    .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;

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
            }

            Ok(revision)
        })
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

    fn delete_subject(&self, subject: &SubjectId) -> AegisResult<Revision> {
        let subj = subject.as_str().to_string();
        self.runtime.block_on(async {
            let client = self.get_client().await?;

            let rows = client
                .query(
                    "SELECT relation, object FROM _aegis_tuples
                     WHERE subject = $1 AND revision_removed IS NULL",
                    &[&subj],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            if rows.is_empty() {
                return Self::current_revision_async(&client).await;
            }

            let tuples: Vec<(String, String)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();

            let revision = Self::bump_revision_async(&client).await?;

            client
                .execute(
                    "UPDATE _aegis_tuples SET revision_removed = $1
                     WHERE subject = $2 AND revision_removed IS NULL",
                    &[&(revision.as_u64() as i64), &subj],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            for (relation, object) in &tuples {
                Self::append_event_async(
                    &client, revision, "remove", subject.as_str(), relation, object, None,
                )
                .await?;
            }

            Ok(revision)
        })
    }

    fn delete_object(&self, object: &ResourceId) -> AegisResult<Revision> {
        let obj = object.as_str().to_string();
        self.runtime.block_on(async {
            let client = self.get_client().await?;

            let rows = client
                .query(
                    "SELECT subject, relation FROM _aegis_tuples
                     WHERE object = $1 AND revision_removed IS NULL",
                    &[&obj],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            if rows.is_empty() {
                return Self::current_revision_async(&client).await;
            }

            let tuples: Vec<(String, String)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();

            let revision = Self::bump_revision_async(&client).await?;

            client
                .execute(
                    "UPDATE _aegis_tuples SET revision_removed = $1
                     WHERE object = $2 AND revision_removed IS NULL",
                    &[&(revision.as_u64() as i64), &obj],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            for (subject, relation) in &tuples {
                Self::append_event_async(
                    &client, revision, "remove", subject, relation, object.as_str(), None,
                )
                .await?;
            }

            Ok(revision)
        })
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
        &self, object: &ResourceId, relation: Option<&Relation>, consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let obj = object.as_str().to_string();
        let rel = relation.map(|r| r.as_str().to_string());
        let rev_filter = match consistency {
            ConsistencyMode::AtRevision(rev) => {
                let r = rev.as_u64() as i64;
                format!("revision_added <= {r} AND (revision_removed IS NULL OR revision_removed > {r})")
            }
            _ => "revision_removed IS NULL".to_string(),
        };
        let is_serializable = *consistency == ConsistencyMode::FullyConsistent;
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let rows = if is_serializable {
                let tx = client
                    .build_transaction()
                    .isolation_level(tokio_postgres::IsolationLevel::Serializable)
                    .start()
                    .await
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let result = if let Some(ref r) = rel {
                    tx.query(
                        &format!(
                            "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                             WHERE object = $1 AND relation = $2 AND {rev_filter}"
                        ),
                        &[&obj, r],
                    )
                    .await
                } else {
                    tx.query(
                        &format!(
                            "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                             WHERE object = $1 AND {rev_filter}"
                        ),
                        &[&obj],
                    )
                    .await
                }
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                tx.commit().await.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                result
            } else {
                if let Some(ref r) = rel {
                    client
                        .query(
                            &format!(
                                "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                                 WHERE object = $1 AND relation = $2 AND {rev_filter}"
                            ),
                            &[&obj, r],
                        )
                        .await
                } else {
                    client
                        .query(
                            &format!(
                                "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                                 WHERE object = $1 AND {rev_filter}"
                            ),
                            &[&obj],
                        )
                        .await
                }
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            };

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
        &self, subject: &SubjectId, relation: Option<&Relation>, consistency: &ConsistencyMode,
    ) -> AegisResult<Vec<RelationshipTuple>> {
        let subj = subject.as_str().to_string();
        let rel = relation.map(|r| r.as_str().to_string());
        let rev_filter = match consistency {
            ConsistencyMode::AtRevision(rev) => {
                let r = rev.as_u64() as i64;
                format!("revision_added <= {r} AND (revision_removed IS NULL OR revision_removed > {r})")
            }
            _ => "revision_removed IS NULL".to_string(),
        };
        let is_serializable = *consistency == ConsistencyMode::FullyConsistent;
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let rows = if is_serializable {
                let tx = client
                    .build_transaction()
                    .isolation_level(tokio_postgres::IsolationLevel::Serializable)
                    .start()
                    .await
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let result = if let Some(ref r) = rel {
                    tx.query(
                        &format!(
                            "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                             WHERE subject = $1 AND relation = $2 AND {rev_filter}"
                        ),
                        &[&subj, r],
                    )
                    .await
                } else {
                    tx.query(
                        &format!(
                            "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                             WHERE subject = $1 AND {rev_filter}"
                        ),
                        &[&subj],
                    )
                    .await
                }
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                tx.commit().await.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                result
            } else {
                if let Some(ref r) = rel {
                    client
                        .query(
                            &format!(
                                "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                                 WHERE subject = $1 AND relation = $2 AND {rev_filter}"
                            ),
                            &[&subj, r],
                        )
                        .await
                } else {
                    client
                        .query(
                            &format!(
                                "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                                 WHERE subject = $1 AND {rev_filter}"
                            ),
                            &[&subj],
                        )
                        .await
                }
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            };

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
        &self, filter: &TupleFilter, pagination: &PaginationParams, consistency: &ConsistencyMode,
    ) -> AegisResult<PaginatedTuples> {
        let subj_type = filter.subject_type.clone();
        let rel = filter.relation.as_ref().map(|r| r.as_str().to_string());
        let obj_type = filter.object_type.clone();
        let meta_key = filter.metadata_key.clone();
        let rev_filter = match consistency {
            ConsistencyMode::AtRevision(rev) => {
                let r = rev.as_u64() as i64;
                format!("revision_added <= {r} AND (revision_removed IS NULL OR revision_removed > {r})")
            }
            _ => "revision_removed IS NULL".to_string(),
        };
        let is_serializable = *consistency == ConsistencyMode::FullyConsistent;

        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let revision = Self::current_revision_async(&client).await?;

            let mut conditions = vec![rev_filter];
            let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();
            let mut idx = 1u32;

            if let Some(st) = subj_type {
                params.push(Box::new(format!("{st}:%")));
                conditions.push(format!("subject LIKE ${idx}"));
                idx += 1;
            }
            if let Some(r) = rel {
                params.push(Box::new(r));
                conditions.push(format!("relation = ${idx}"));
                idx += 1;
            }
            if let Some(ot) = obj_type {
                params.push(Box::new(format!("{ot}:%")));
                conditions.push(format!("object LIKE ${idx}"));
                idx += 1;
            }
            if let Some(mk) = meta_key {
                params.push(Box::new(format!("%{mk}%")));
                conditions.push(format!("metadata::text LIKE ${idx}"));
                idx += 1;
            }

            let where_clause = conditions.join(" AND ");
            let offset = pagination.cursor.as_ref().map(|c| c.offset).unwrap_or(0);
            let limit = pagination.limit;

            let sql = format!(
                "SELECT subject, relation, object, created_at, metadata FROM _aegis_tuples
                 WHERE {where_clause}
                 ORDER BY subject, relation, object
                 LIMIT ${idx} OFFSET ${}",
                idx + 1,
            );
            params.push(Box::new(limit as i64));
            params.push(Box::new(offset as i64));

            let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                params.iter().map(|p| p.as_ref()).collect();

            let rows = if is_serializable {
                let tx = client
                    .build_transaction()
                    .isolation_level(tokio_postgres::IsolationLevel::Serializable)
                    .start()
                    .await
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let result = tx
                    .query(&sql, &param_refs)
                    .await
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                tx.commit().await.map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                result
            } else {
                client
                    .query(&sql, &param_refs)
                    .await
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?
            };

            let mut tuples = Vec::with_capacity(rows.len());
            for row in &rows {
                let subject_str: String = row.get("subject");
                let relation_str: String = row.get("relation");
                let object_str: String = row.get("object");
                let created: DateTime<Utc> = row.get("created_at");
                let meta_val: Option<serde_json::Value> = row.get("metadata");
                let metadata = meta_val.and_then(|v| serde_json::from_value::<HashMap<String, String>>(v).ok());

                let subject = SubjectId::new(&subject_str)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let relation = Relation::new(&relation_str)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                let object = ResourceId::new(&object_str)
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

                tuples.push(RelationshipTuple {
                    subject,
                    relation,
                    object,
                    created_at: created,
                    metadata,
                });
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
        let node_id = self.node_id;
        let handle = self.runtime.handle().clone();
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            client
                .execute("BEGIN", &[])
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(Box::new(PostgresTransaction::new(client, handle, node_id)) as Box<dyn StorageTransaction>)
        })
    }

    fn query_audit(
        &self, object: Option<&ResourceId>, from_revision: Option<Revision>,
        to_revision: Option<Revision>, pagination: &PaginationParams,
    ) -> AegisResult<Vec<AuditEntry>> {
        let from = from_revision.map(|r| r.as_u64() as i64);
        let to = to_revision.map(|r| r.as_u64() as i64);
        let offset = pagination.cursor.as_ref().map(|c| c.offset).unwrap_or(0);
        let limit_val = pagination.limit;

        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let mut sql = String::from(
                "SELECT revision, action, subject, relation, object, timestamp, metadata
                 FROM _aegis_events",
            );
            let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();
            let mut conditions: Vec<String> = Vec::new();
            let mut idx = 1;

            if let Some(obj) = object {
                conditions.push(format!("object = ${idx}"));
                params.push(Box::new(obj.as_str().to_string()));
                idx += 1;
            }
            if let Some(f) = from {
                conditions.push(format!("revision >= ${idx}"));
                params.push(Box::new(f));
                idx += 1;
            }
            if let Some(t) = to {
                conditions.push(format!("revision <= ${idx}"));
                params.push(Box::new(t));
                idx += 1;
            }

            if !conditions.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&conditions.join(" AND "));
            }
            sql.push_str(&format!(
                " ORDER BY revision ASC LIMIT ${idx} OFFSET ${}",
                idx + 1
            ));
            params.push(Box::new(limit_val as i64));
            params.push(Box::new(offset as i64));

            let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                params.iter().map(|p| p.as_ref()).collect();

            let rows = client
                .query(&sql, &param_refs)
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let results: Vec<AuditEntry> = rows
                .into_iter()
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

    fn read_schema_version(&self) -> AegisResult<u32> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            match client
                .query_opt("SELECT version FROM _aegis_schema ORDER BY version DESC LIMIT 1", &[])
                .await
            {
                Ok(Some(row)) => {
                    let v: i32 = row.get(0);
                    Ok(v as u32)
                }
                Ok(None) => Ok(0),
                Err(ref e) if e.code() == Some(&tokio_postgres::error::SqlState::UNDEFINED_TABLE) => {
                    Ok(0)
                }
                Err(e) => Err(AegisError::StorageQuery(e.to_string())),
            }
        })
    }

    fn write_schema_version(&self, version: u32) -> AegisResult<()> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            client
                .execute(
                    "INSERT INTO _aegis_schema (version, applied_at, checksum) VALUES ($1, NOW(), '')",
                    &[&(version as i32)],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(())
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

    fn delete_events_before(&self, cutoff: DateTime<Utc>) -> AegisResult<usize> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let rows = client
                .execute("DELETE FROM _aegis_events WHERE timestamp < $1", &[&cutoff])
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(rows as usize)
        })
    }

    fn delete_soft_deleted_tuples_before(&self, cutoff: DateTime<Utc>) -> AegisResult<usize> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let rows = client
                .execute(
                    "DELETE FROM _aegis_tuples
                     WHERE revision_removed IS NOT NULL
                       AND revision_removed <= (
                         SELECT COALESCE(MAX(revision), 0) FROM _aegis_events WHERE timestamp < $1
                       )",
                    &[&cutoff],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(rows as usize)
        })
    }

    fn recover_from_events(&self, to_revision: Option<Revision>) -> AegisResult<Revision> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;

            client
                .execute("DELETE FROM _aegis_tuples", &[])
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let rows = client
                .query(
                    "SELECT revision, action, subject, relation, object, metadata
                     FROM _aegis_events
                     ORDER BY revision ASC, event_id ASC",
                    &[],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut last_revision = Revision::ZERO;

            for row in &rows {
                let rev: i64 = row.get(0);
                let action: String = row.get(1);
                let subject: String = row.get(2);
                let relation: String = row.get(3);
                let object: String = row.get(4);
                let meta_val: Option<serde_json::Value> = row.get(5);

                let revision = Revision::new(rev as u64);
                if let Some(target) = to_revision {
                    if revision > target {
                        continue;
                    }
                }

                match action.as_str() {
                    "add" => {
                        client
                            .execute(
                                "INSERT INTO _aegis_tuples (subject, relation, object, created_at, metadata, revision_added)
                                 VALUES ($1, $2, $3, NOW(), $4, $5)",
                                &[&subject, &relation, &object, &meta_val, &rev],
                            )
                            .await
                            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                    }
                    "remove" => {
                        client
                            .execute(
                                "UPDATE _aegis_tuples SET revision_removed = $1
                                 WHERE subject = $2 AND relation = $3 AND object = $4 AND revision_removed IS NULL",
                                &[&rev, &subject, &relation, &object],
                            )
                            .await
                            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                    }
                    _ => {}
                }

                last_revision = revision;
            }

            if last_revision != Revision::ZERO {
                let current = Self::current_revision_async(&client).await?;
                if current != last_revision {
                    client
                        .execute(
                            "UPDATE _aegis_meta SET value = $1 WHERE key = 'revision'",
                            &[&(last_revision.as_u64() as i64)],
                        )
                        .await
                        .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
                }
            }

            Ok(last_revision)
        })
    }

    fn compact_events(&self) -> AegisResult<usize> {
        self.runtime.block_on(async {
            let client = self.get_client().await?;
            let rows = client
                .query(
                    "SELECT event_id, action, subject, relation, object FROM _aegis_events ORDER BY revision ASC, event_id ASC",
                    &[],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            let mut adds: HashMap<(String, String, String), i64> = HashMap::new();
            let mut to_delete: Vec<i64> = Vec::new();

            for row in &rows {
                let event_id: i64 = row.get(0);
                let action: String = row.get(1);
                let subject: String = row.get(2);
                let relation: String = row.get(3);
                let object: String = row.get(4);
                let key = (subject, relation, object);

                match action.as_str() {
                    "add" => {
                        adds.insert(key, event_id);
                    }
                    "remove" => {
                        if let Some(add_id) = adds.remove(&key) {
                            to_delete.push(add_id);
                            to_delete.push(event_id);
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
                client
                    .execute("DELETE FROM _aegis_events WHERE event_id = $1", &[id])
                    .await
                    .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            }

            Ok(total)
        })
    }

    fn close(&self) -> AegisResult<()> {
        self.runtime
            .block_on(self.pool.close())
            .map_err(|e| AegisError::StorageConnection(e.to_string()))
    }
}

/// A PostgreSQL transaction wrapping a pooled connection.
#[cfg(feature = "postgres")]
pub struct PostgresTransaction {
    conn: Option<deadpool_postgres::Object>,
    runtime: tokio::runtime::Handle,
    _node_id: Uuid,
}

#[cfg(feature = "postgres")]
impl PostgresTransaction {
    fn new(
        conn: deadpool_postgres::Object,
        runtime: tokio::runtime::Handle,
        _node_id: Uuid,
    ) -> Self {
        Self {
            conn: Some(conn),
            runtime,
            _node_id,
        }
    }

    fn conn(&self) -> AegisResult<&deadpool_postgres::Object> {
        self.conn
            .as_ref()
            .ok_or_else(|| AegisError::Internal("transaction already consumed".into()))
    }

    fn block_on<T>(&self, fut: impl std::future::Future<Output = AegisResult<T>>) -> AegisResult<T> {
        self.runtime.block_on(fut)
    }

    async fn bump_revision_async(client: &tokio_postgres::Client) -> AegisResult<Revision> {
        client
            .execute(
                "UPDATE _aegis_meta SET value = CAST(CAST(value AS BIGINT) + 1 AS TEXT) WHERE key = 'revision'",
                &[],
            )
            .await
            .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
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
impl StorageTransaction for PostgresTransaction {
    fn write(&mut self, tuple: &RelationshipTuple) -> AegisResult<()> {
        self.conn()?; // validate connection
        let tuple_clone = tuple.clone();
        self.block_on(async {
            let conn = self.conn.as_ref().unwrap();
            let revision = Self::bump_revision_async(conn).await?;
            let meta_val = tuple_clone
                .metadata
                .as_ref()
                .map(|m| serde_json::to_value(m))
                .transpose()
                .map_err(|e| AegisError::MetadataValidation(e.to_string()))?;

            conn
                .execute(
                    "UPDATE _aegis_tuples SET revision_removed = $1
                     WHERE subject = $2 AND relation = $3 AND object = $4 AND revision_removed IS NULL",
                    &[&(revision.as_u64() as i64), &tuple_clone.subject.as_str(), &tuple_clone.relation.as_str(), &tuple_clone.object.as_str()],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            conn
                .execute(
                    "INSERT INTO _aegis_tuples (subject, relation, object, created_at, metadata, revision_added)
                     VALUES ($1, $2, $3, $4, $5, $6)",
                    &[&tuple_clone.subject.as_str(), &tuple_clone.relation.as_str(), &tuple_clone.object.as_str(), &tuple_clone.created_at, &meta_val, &(revision.as_u64() as i64)],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            Self::append_event_async(
                conn, revision, "add", tuple_clone.subject.as_str(),
                tuple_clone.relation.as_str(), tuple_clone.object.as_str(), meta_val.as_ref(),
            )
            .await?;

            Ok(())
        })
    }

    fn delete(&mut self, key: &TupleKey) -> AegisResult<()> {
        let key_clone = key.clone();
        self.block_on(async {
            let conn = self.conn.as_ref().unwrap();
            let revision = Self::bump_revision_async(conn).await?;

            conn
                .execute(
                    "UPDATE _aegis_tuples SET revision_removed = $1
                     WHERE subject = $2 AND relation = $3 AND object = $4 AND revision_removed IS NULL",
                    &[&(revision.as_u64() as i64), &key_clone.subject.as_str(), &key_clone.relation.as_str(), &key_clone.object.as_str()],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;

            Self::append_event_async(
                conn, revision, "remove", key_clone.subject.as_str(),
                key_clone.relation.as_str(), key_clone.object.as_str(), None,
            )
            .await?;

            Ok(())
        })
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

    fn savepoint(&self, name: &str) -> AegisResult<()> {
        Self::validate_savepoint_name(name)?;
        let name_owned = name.to_string();
        self.block_on(async {
            let conn = self.conn.as_ref().unwrap();
            conn
                .execute(&format!("SAVEPOINT \"{}\"", name_owned), &[])
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(())
        })
    }

    fn rollback_to_savepoint(&self, name: &str) -> AegisResult<()> {
        Self::validate_savepoint_name(name)?;
        let name_owned = name.to_string();
        self.block_on(async {
            let conn = self.conn.as_ref().unwrap();
            conn
                .execute(&format!("ROLLBACK TO SAVEPOINT \"{}\"", name_owned), &[])
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(())
        })
    }

    fn release_savepoint(&self, name: &str) -> AegisResult<()> {
        Self::validate_savepoint_name(name)?;
        let name_owned = name.to_string();
        self.block_on(async {
            let conn = self.conn.as_ref().unwrap();
            conn
                .execute(&format!("RELEASE SAVEPOINT \"{}\"", name_owned), &[])
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(())
        })
    }

    fn commit(self: Box<Self>) -> AegisResult<Revision> {
        let s = *self;
        let conn = s.conn
            .ok_or_else(|| AegisError::Internal("transaction already consumed".into()))?;
        let handle = s.runtime;
        handle.block_on(async {
            conn
                .execute("COMMIT", &[])
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            let row = conn
                .query_one(
                    "SELECT COALESCE(CAST(value AS BIGINT), 0) FROM _aegis_meta WHERE key = 'revision'",
                    &[],
                )
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            let rev: i64 = row.get(0);
            Ok(Revision::new(rev as u64))
        })
    }

    fn rollback(self: Box<Self>) -> AegisResult<()> {
        let s = *self;
        let conn = s.conn
            .ok_or_else(|| AegisError::Internal("transaction already consumed".into()))?;
        let handle = s.runtime;
        handle.block_on(async {
            conn
                .execute("ROLLBACK", &[])
                .await
                .map_err(|e| AegisError::StorageQuery(e.to_string()))?;
            Ok(())
        })
    }
}
