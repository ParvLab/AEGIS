use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde_json::json;

use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::StorageBackend;
use aegis_core::storage::TupleFilter;
use aegis_core::types::*;

#[cfg(feature = "postgres")]
use aegis_core::storage::PostgresStorage;
#[cfg(feature = "rocksdb")]
use aegis_core::storage::RocksDbStorage;
#[cfg(feature = "mysql")]
use aegis_core::storage::mysql::MysqlConfig;
#[cfg(feature = "mysql")]
use aegis_core::storage::MysqlStorage;

mod repl;

#[derive(Parser)]
#[command(name = "aegis", version, about = "Aegis authorization engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Storage backend type: sqlite, postgres, rocksdb, mysql
    #[arg(long, default_value = "sqlite", global = true)]
    storage: String,

    /// Connection string for database backends (postgresql://user:pass@host/db, mysql://user:pass@host/db)
    #[arg(long, global = true)]
    connection_string: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Check whether a subject has a permission on a resource
    Check {
        subject: String,
        permission: String,
        resource: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        schema: Option<String>,
    },
    /// Write a relationship tuple
    Write {
        subject: String,
        relation: String,
        resource: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        schema: Option<String>,
    },
    /// Delete a relationship tuple
    Delete {
        subject: String,
        relation: String,
        resource: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// List tuples for an object
    List {
        object: String,
        #[arg(long)]
        relation: Option<String>,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Explain why a check returned its result
    Explain {
        subject: String,
        permission: String,
        resource: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        schema: Option<String>,
    },
    /// Enter an interactive REPL shell
    Repl {
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        schema: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Query tuples with filters
    Query {
        #[arg(long)]
        subject_type: Option<String>,
        #[arg(long)]
        relation: Option<String>,
        #[arg(long)]
        object_type: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: u64,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Health check
    Health {
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        schema: Option<String>,
    },
    /// Dry-run check (evaluate without caching)
    CheckDryRun {
        subject: String,
        permission: String,
        resource: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        schema: Option<String>,
    },
    /// Dry-run write (validate without persisting)
    WriteDryRun {
        subject: String,
        relation: String,
        resource: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        schema: Option<String>,
    },
    /// Query audit log for an object
    Audit {
        object: String,
        #[arg(long)]
        from: Option<u64>,
        #[arg(long)]
        to: Option<u64>,
        #[arg(long, default_value_t = 100)]
        limit: u64,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Export all tuples for a subject (GDPR)
    ExportSubject {
        subject: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Backup tuples and events to a file
    BackupCreate {
        path: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        schema: Option<String>,
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Restore tuples and events from a backup file
    BackupRestore {
        path: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Export tuples in JSON format (optionally filtered by subject)
    Export {
        #[arg(long)]
        subject: Option<String>,
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        schema: Option<String>,
    },
    /// Import tuples from a JSON file
    Import {
        path: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        schema: Option<String>,
    },
    /// Lint a schema file for compatibility issues
    SchemaLint {
        path: String,
    },
    /// Run event log recovery and compaction
    Recover {
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        to_revision: Option<i64>,
    },
    /// Delete a subject with an ownership policy (GDPR right to erasure)
    DeleteSubject {
        subject: String,
        #[arg(long, default_value = "fail")]
        policy: String,
        #[arg(long)]
        transfer_to: Option<String>,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
}

fn load_storage(
    db_path: &str,
    storage_type: &str,
    _conn_str: Option<&str>,
) -> Result<Box<dyn StorageBackend>> {
    match storage_type {
        "sqlite" => {
            let config = SqliteConfig {
                path: db_path.to_string(),
                ..Default::default()
            };
            let mut storage = SqliteStorage::new(config)
                .with_context(|| format!("failed to create SQLite storage at {db_path}"))?;
            storage
                .initialize()
                .context("failed to initialize storage")?;
            Ok(Box::new(storage))
        }
        #[cfg(feature = "postgres")]
        "postgres" | "pg" => {
            let cs = _conn_str.context("--connection-string is required for postgres backend")?;
            let mut storage = PostgresStorage::new(cs)
                .context("failed to create Postgres storage")?;
            storage
                .initialize()
                .context("failed to initialize storage")?;
            Ok(Box::new(storage))
        }
        #[cfg(not(feature = "postgres"))]
        "postgres" | "pg" => {
            anyhow::bail!("postgres backend is not enabled. Rebuild aegis-cli with --features postgres");
        }
        #[cfg(feature = "rocksdb")]
        "rocksdb" => {
            let mut storage = RocksDbStorage::new(db_path)
                .with_context(|| format!("failed to create RocksDB storage at {db_path}"))?;
            storage
                .initialize()
                .context("failed to initialize storage")?;
            Ok(Box::new(storage))
        }
        #[cfg(not(feature = "rocksdb"))]
        "rocksdb" => {
            anyhow::bail!("rocksdb backend is not enabled. Rebuild aegis-cli with --features rocksdb");
        }
        #[cfg(feature = "mysql")]
        "mysql" => {
            let cs = _conn_str.context("--connection-string is required for mysql backend")?;
            let config = parse_mysql_connection_string(cs)?;
            let mut storage = MysqlStorage::new(config)
                .context("failed to create MySQL storage")?;
            storage
                .initialize()
                .context("failed to initialize storage")?;
            Ok(Box::new(storage))
        }
        #[cfg(not(feature = "mysql"))]
        "mysql" => {
            anyhow::bail!("mysql backend is not enabled. Rebuild aegis-cli with --features mysql");
        }
        _ => anyhow::bail!(
            "unknown storage backend: {storage_type}. Supported: sqlite, postgres, rocksdb, mysql"
        ),
    }
}

#[cfg(feature = "mysql")]
fn parse_mysql_connection_string(cs: &str) -> Result<MysqlConfig> {
    let remainder = cs
        .strip_prefix("mysql://")
        .with_context(|| "mysql connection string must start with mysql://")?;
    let (userinfo, rest) = remainder
        .split_once('@')
        .with_context(|| "invalid mysql connection string: expected user:pass@host/db")?;
    let (user, pass) = userinfo
        .split_once(':')
        .with_context(|| "invalid mysql connection string: expected user:pass")?;
    let (hostinfo, database) = rest
        .split_once('/')
        .with_context(|| "invalid mysql connection string: expected host/db")?;
    let (host, port) = if let Some((h, p)) = hostinfo.split_once(':') {
        (h.to_string(), p.parse::<u16>().with_context(|| "invalid port in connection string")?)
    } else {
        (hostinfo.to_string(), 3306u16)
    };
    Ok(MysqlConfig {
        host,
        port,
        user: user.to_string(),
        password: pass.to_string(),
        database: database.to_string(),
        pool_size: 10,
    })
}

fn load_db(path: &str, schema_path: Option<&str>, storage_type: &str, conn_str: Option<&str>) -> Result<GraphEngine> {
    let storage = load_storage(path, storage_type, conn_str)?;

    let schema = if let Some(sp) = schema_path {
        let yaml = std::fs::read_to_string(sp)
            .with_context(|| format!("failed to read schema file {sp}"))?;
        parse_schema(&yaml).context("failed to parse schema")?
    } else {
        Schema {
            schema_version: 1,
            namespace: "default".to_string(),
            types: HashMap::new(),
        }
    };

    Ok(GraphEngine::new(storage, schema))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mk_engine = |db: &str, schema: Option<&str>| -> Result<GraphEngine> {
        load_db(db, schema, &cli.storage, cli.connection_string.as_deref())
    };

    match &cli.command {
        Commands::Check {
            subject,
            permission,
            resource,
            db,
            schema,
        } => {
            let engine = mk_engine(db, schema.as_deref())?;
            let subject_id = SubjectId::new(subject.as_str())
                .with_context(|| format!("invalid subject: {subject}"))?;
            let resource_id = ResourceId::new(resource.as_str())
                .with_context(|| format!("invalid resource: {resource}"))?;
            let result = engine.check(&subject_id, permission, &resource_id, None)?;
            let output = json!({
                "allowed": result.allowed,
                "revision": result.revision.as_u64(),
            });
            println!("{}", serde_json::to_string(&output)?);
        }
        Commands::Write {
            subject,
            relation,
            resource,
            db,
            schema,
        } => {
            let engine = mk_engine(db, schema.as_deref())?;
            let subject_id = SubjectId::new(subject.as_str())
                .with_context(|| format!("invalid subject: {subject}"))?;
            let relation_val = Relation::new(relation.as_str())
                .with_context(|| format!("invalid relation: {relation}"))?;
            let resource_id = ResourceId::new(resource.as_str())
                .with_context(|| format!("invalid resource: {resource}"))?;
            let tuple = RelationshipTuple::new(subject_id, relation_val, resource_id);
            let token = engine.write(&tuple)?;
            let output = json!({ "revision": token.revision.as_u64() });
            println!("{}", serde_json::to_string(&output)?);
        }
        Commands::Delete {
            subject,
            relation,
            resource,
            db,
        } => {
            let engine = mk_engine(db, None)?;
            let subject_id = SubjectId::new(subject.as_str())
                .with_context(|| format!("invalid subject: {subject}"))?;
            let relation_val = Relation::new(relation.as_str())
                .with_context(|| format!("invalid relation: {relation}"))?;
            let resource_id = ResourceId::new(resource.as_str())
                .with_context(|| format!("invalid resource: {resource}"))?;
            let key = TupleKey {
                subject: subject_id,
                relation: relation_val,
                object: resource_id,
            };
            let token = engine.delete(&key)?;
            let output = json!({ "revision": token.revision.as_u64() });
            println!("{}", serde_json::to_string(&output)?);
        }
        Commands::List {
            object,
            relation,
            db,
        } => {
            let engine = mk_engine(db, None)?;
            let resource_id = ResourceId::new(object.as_str())
                .with_context(|| format!("invalid object: {object}"))?;
            let relation_filter = relation
                .as_ref()
                .map(|r| Relation::new(r.as_str()))
                .transpose()
                .with_context(|| "invalid relation filter")?;
            let tuples = engine
                .storage()
                .list_by_object(&resource_id, relation_filter.as_ref(), &ConsistencyMode::MinimizeLatency)?;
            println!("{}", serde_json::to_string(&tuples)?);
        }
        Commands::Explain {
            subject,
            permission,
            resource,
            db,
            schema,
        } => {
            let engine = mk_engine(db, schema.as_deref())?;
            let subject_id = SubjectId::new(subject.as_str())
                .with_context(|| format!("invalid subject: {subject}"))?;
            let resource_id = ResourceId::new(resource.as_str())
                .with_context(|| format!("invalid resource: {resource}"))?;
            let result = engine.explain(&subject_id, permission, &resource_id, None)?;
            let output = json!({
                "allowed": result.allowed,
                "revision": result.revision.as_u64(),
                "resolved_via": result.resolved_via,
                "trace": result.trace,
            });
            println!("{}", serde_json::to_string(&output)?);
        }
        Commands::Health { db, schema } => {
            let engine = mk_engine(db, schema.as_deref())?;
            let report = engine.health();
            let output = serde_json::to_value(&report)?;
            println!("{}", serde_json::to_string(&output)?);
        }
        Commands::CheckDryRun {
            subject,
            permission,
            resource,
            db,
            schema,
        } => {
            let engine = mk_engine(db, schema.as_deref())?;
            let subject_id = SubjectId::new(subject.as_str())
                .with_context(|| format!("invalid subject: {subject}"))?;
            let resource_id = ResourceId::new(resource.as_str())
                .with_context(|| format!("invalid resource: {resource}"))?;
            let result = engine.check_dry_run(&subject_id, permission, &resource_id, None)?;
            let output = json!({
                "allowed": result.allowed,
                "revision": result.revision.as_u64(),
                "dry_run": true,
            });
            println!("{}", serde_json::to_string(&output)?);
        }
        Commands::WriteDryRun {
            subject,
            relation,
            resource,
            db,
            schema,
        } => {
            let engine = mk_engine(db, schema.as_deref())?;
            let subject_id = SubjectId::new(subject.as_str())
                .with_context(|| format!("invalid subject: {subject}"))?;
            let relation_val = Relation::new(relation.as_str())
                .with_context(|| format!("invalid relation: {relation}"))?;
            let resource_id = ResourceId::new(resource.as_str())
                .with_context(|| format!("invalid resource: {resource}"))?;
            let tuple = RelationshipTuple::new(subject_id, relation_val, resource_id);
            let token = engine.write_dry_run(&tuple)?;
            let output = json!({
                "revision": token.revision.as_u64(),
                "dry_run": true,
                "valid": true,
            });
            println!("{}", serde_json::to_string(&output)?);
        }
        Commands::Audit {
            object,
            from,
            to,
            limit,
            db,
        } => {
            let engine = mk_engine(db, None)?;
            let resource_id = ResourceId::new(object.as_str())
                .with_context(|| format!("invalid object: {object}"))?;
            let from_rev = from.map(Revision::new);
            let to_rev = to.map(Revision::new);
            let pagination = PaginationParams {
                limit: *limit,
                cursor: None,
            };
            let entries = engine.query_audit(&resource_id, from_rev, to_rev, &pagination)?;
            println!("{}", serde_json::to_string(&entries)?);
        }
        Commands::ExportSubject { subject, db } => {
            let engine = mk_engine(db, None)?;
            let subject_id = SubjectId::new(subject.as_str())
                .with_context(|| format!("invalid subject: {subject}"))?;
            let tuples = engine.export_subject(&subject_id)?;
            println!("{}", serde_json::to_string(&tuples)?);
        }
        Commands::Repl { db, schema, json } => {
            repl::run_repl(db, schema.as_deref(), &cli.storage, cli.connection_string.as_deref(), *json)?;
        }
        Commands::Query {
            subject_type,
            relation,
            object_type,
            limit,
            db,
        } => {
            let engine = mk_engine(db, None)?;
            let filter = TupleFilter {
                subject_type: subject_type.clone(),
                relation: relation
                    .as_ref()
                    .map(|r| Relation::new(r.as_str()))
                    .transpose()
                    .with_context(|| "invalid relation filter")?,
                object_type: object_type.clone(),
                metadata_key: None,
                metadata_value: None,
            };
            let pagination = PaginationParams {
                limit: *limit,
                cursor: None,
            };
            let result = engine.storage().query_tuples(
                &filter,
                &pagination,
                &ConsistencyMode::MinimizeLatency,
            )?;
            println!("{}", serde_json::to_string(&result)?);
        }
        Commands::BackupCreate {
            path,
            db,
            schema,
            format: _format,
        } => {
            let engine = mk_engine(db, None)?;
            let all_tuples = engine
                .storage()
                .query_tuples(
                    &TupleFilter::default(),
                    &PaginationParams {
                        limit: u64::MAX,
                        cursor: None,
                    },
                    &ConsistencyMode::MinimizeLatency,
                )?
                .tuples;
            let schema_yaml = if let Some(sp) = schema {
                std::fs::read_to_string(sp)
                    .with_context(|| format!("failed to read schema file {sp}"))?
            } else {
                String::new()
            };
            let events = engine.query_audit_all(
                None,
                None,
                &PaginationParams {
                    limit: u64::MAX,
                    cursor: None,
                },
            )?;
            let revision = engine.storage().current_revision()?;
            let backend_type = engine.storage().backend_type().to_string();
            let exported_at = Utc::now().to_rfc3339();
            let backup = serde_json::json!({
                "version": 2,
                "schema_yaml": schema_yaml,
                "tuples": all_tuples,
                "events": events,
                "metadata": {
                    "backend_type": backend_type,
                    "revision": revision.as_u64(),
                    "exported_at": exported_at,
                },
            });
            let output = serde_json::to_string_pretty(&backup)?;
            std::fs::write(path, output)
                .with_context(|| format!("failed to write backup to {path}"))?;
            println!(r#"{{"status":"ok","tuples":{},"events":{},"revision":{}}}"#,
                all_tuples.len(), events.len(), revision.as_u64());
        }
        Commands::BackupRestore { path, db } => {
            let engine = mk_engine(db, None)?;
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read backup from {path}"))?;
            let backup: serde_json::Value = serde_json::from_str(&content)?;
            let version = backup.get("version").and_then(|v| v.as_i64()).unwrap_or(1);
            if version >= 2 {
                if let Some(sy) = backup.get("schema_yaml").and_then(|s| s.as_str()) {
                    if !sy.is_empty() {
                        let schema = parse_schema(sy)
                            .context("failed to parse schema from backup")?;
                        engine.reload_schema(schema)?;
                    }
                }
            }
            let tuples: Vec<TupleImport> = serde_json::from_value(
                backup.get("tuples").cloned().unwrap_or(serde_json::Value::Null),
            )
            .context("invalid backup format: missing or invalid 'tuples' field")?;
            let mut count = 0usize;
            for chunk in tuples.chunks(100) {
                let batch: Vec<RelationshipTuple> = chunk
                    .iter()
                    .map(|t| {
                        let subject_id = SubjectId::new(&t.subject)
                            .with_context(|| format!("invalid subject: {}", t.subject))?;
                        let relation_val = Relation::new(&t.relation)
                            .with_context(|| format!("invalid relation: {}", t.relation))?;
                        let object_id = ResourceId::new(&t.object)
                            .with_context(|| format!("invalid object: {}", t.object))?;
                        Ok(RelationshipTuple::new(subject_id, relation_val, object_id))
                    })
                    .collect::<Result<Vec<_>>>()?;
                engine.write_batch(&batch)?;
                count += batch.len();
            }
            println!(r#"{{"status":"ok","restored":{count}}}"#);
        }
        Commands::Export {
            subject,
            db,
            schema,
        } => {
            let engine = mk_engine(db, schema.as_deref())?;
            let tuples = if let Some(s) = subject {
                let subject_id = SubjectId::new(s.as_str())
                    .with_context(|| format!("invalid subject: {s}"))?;
                engine.export_subject(&subject_id)?
            } else {
                engine
                    .storage()
                    .query_tuples(
                        &TupleFilter::default(),
                        &PaginationParams {
                            limit: u64::MAX,
                            cursor: None,
                        },
                        &ConsistencyMode::MinimizeLatency,
                    )?
                    .tuples
            };
            println!("{}", serde_json::to_string_pretty(&tuples)?);
        }
        Commands::Import {
            path,
            db,
            schema,
        } => {
            let engine = mk_engine(db, schema.as_deref())?;
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read import file {path}"))?;
            let tuples: Vec<TupleImport> = serde_json::from_str(&content)
                .context("invalid import format: expected array of tuples")?;
            let mut count = 0usize;
            for t in &tuples {
                let subject_id = SubjectId::new(&t.subject)
                    .with_context(|| format!("invalid subject: {}", t.subject))?;
                let relation_val = Relation::new(&t.relation)
                    .with_context(|| format!("invalid relation: {}", t.relation))?;
                let object_id = ResourceId::new(&t.object)
                    .with_context(|| format!("invalid object: {}", t.object))?;
                let tuple = RelationshipTuple::new(subject_id, relation_val, object_id);
                engine.write(&tuple)?;
                count += 1;
            }
            println!(r#"{{"status":"ok","imported":{count}}}"#);
        }
        Commands::SchemaLint { path } => {
            let yaml = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read schema file {path}"))?;
            match parse_schema(&yaml) {
                Ok(schema) => {
                    let report = aegis_core::schema::check_schema_compatibility(&schema, &schema);
                    if report.breaking.is_empty() && report.warnings.is_empty() {
                        println!(r#"{{"status":"ok","types":{},"version":{}}}"#,
                            schema.types.len(), schema.schema_version);
                    } else {
                        let output = serde_json::json!({
                            "status": report.breaking.is_empty().then(|| "warning").unwrap_or("error"),
                            "breaking": report.breaking,
                            "warnings": report.warnings,
                        });
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    }
                }
                Err(e) => {
                    let output = serde_json::json!({
                        "status": "error",
                        "error": e.to_string(),
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
            }
        }
        Commands::Recover { db, to_revision } => {
            let engine = mk_engine(db, None)?;
            let to_rev = to_revision.map(|r| Revision::new(r as u64));
            let revision = engine.recover_from_events(to_rev)?;
            println!(
                "{}",
                serde_json::json!({
                    "status": "ok",
                    "revision": revision.as_u64(),
                })
            );
        }
        Commands::DeleteSubject {
            subject,
            policy,
            transfer_to,
            db,
        } => {
            let engine = mk_engine(db, None)?;
            let subject_id = SubjectId::new(subject.as_str())
                .with_context(|| format!("invalid subject: {subject}"))?;
            let transfer = if let Some(t) = transfer_to {
                Some(
                    SubjectId::new(t.as_str())
                        .with_context(|| format!("invalid transfer_to subject: {t}"))?,
                )
            } else {
                None
            };
            let token = engine.delete_subject_with_policy(
                &subject_id,
                policy,
                transfer.as_ref(),
            )?;
            println!(
                "{}",
                serde_json::json!({
                    "status": "ok",
                    "revision": token.revision.as_u64(),
                })
            );
        }
    }

    Ok(())
}

#[derive(serde::Deserialize)]
struct TupleImport {
    subject: String,
    relation: String,
    object: String,
}
