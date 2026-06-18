use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde_json::json;
use sha2::{Digest, Sha256};

use aegis_core::engine::GraphEngine;
use aegis_core::engine::policy_lifecycle::DraftStatus;
use aegis_core::engine::watch::WatchEventType;
use aegis_core::schema::parse_schema;
use aegis_core::storage::StorageBackend;
use aegis_core::storage::TupleFilter;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::types::*;
use std::time::Duration;

#[cfg(feature = "rocksdb")]
use aegis_core::storage::RocksDbStorage;

mod repl;

#[derive(Parser)]
#[command(name = "aegis", version, about = "Aegis authorization engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Storage backend type: sqlite, rocksdb
    #[arg(long, default_value = "sqlite", global = true)]
    storage: String,
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
        /// Enable strict mode (promote warnings to errors)
        #[arg(long)]
        strict: bool,
    },
    /// Show a structured diff between the current schema and a new schema
    PolicyDiff {
        /// Path to the new schema file
        schema_file: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Run event log recovery and compaction
    Recover {
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        to_revision: Option<i64>,
        /// Dry-run: show what would be recovered without executing
        #[arg(long)]
        dry_run: bool,
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

    /// Manage policy lifecycle drafts
    #[command(name = "policy-draft", subcommand)]
    PolicyDraft(PolicyDraftAction),

    /// Manage analysis schedules
    #[command(name = "schedule", subcommand)]
    Schedule(ScheduleAction),

    /// Configure and query enforcement history
    #[command(name = "enforcement", subcommand)]
    Enforcement(EnforcementAction),

    /// Subscribe to engine events (interactive)
    Subscribe {
        /// Comma-separated event types
        event_types: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
        #[arg(long)]
        schema: Option<String>,
    },
}

#[derive(Subcommand)]
enum PolicyDraftAction {
    /// Create a new policy draft
    Create {
        name: String,
        description: String,
        #[arg(long)]
        /// Path to schema YAML file
        schema: Option<String>,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Validate a policy draft
    Validate {
        id: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Diff current schema against a draft's schema
    Diff {
        id: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Submit a draft for review
    Submit {
        id: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Approve a submitted draft
    Approve {
        id: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Reject a draft
    Reject {
        id: String,
        reason: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Publish an approved draft
    Publish {
        id: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Archive a draft
    Archive {
        id: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// List all drafts, optionally filtered by status
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
}

#[derive(Subcommand)]
enum ScheduleAction {
    /// Create a new analysis schedule
    Create {
        /// Path to schedule config JSON file
        config: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// List all schedules
    List {
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Delete a schedule
    Delete {
        id: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Run analysis now (optionally for a specific schedule)
    Run {
        #[arg(long)]
        schedule_id: Option<String>,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Get analysis run history
    Runs {
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
}

#[derive(Subcommand)]
enum EnforcementAction {
    /// Set enforcement history config (JSON)
    Set {
        /// Path to config JSON file
        config: String,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Get current enforcement history config
    Get {
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
    /// Get enforcement trends
    Trends {
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long, default_value = "aegis.db")]
        db: String,
    },
}

fn load_storage(db_path: &str, storage_type: &str) -> Result<Box<dyn StorageBackend>> {
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
            anyhow::bail!(
                "rocksdb backend is not enabled. Rebuild aegis-cli with --features rocksdb"
            );
        }
        _ => anyhow::bail!("unknown storage backend: {storage_type}. Supported: sqlite, rocksdb"),
    }
}

fn load_db(path: &str, schema_path: Option<&str>, storage_type: &str) -> Result<GraphEngine> {
    let storage = load_storage(path, storage_type)?;

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
        load_db(db, schema, &cli.storage)
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
            let tuples = engine.storage().list_by_object(
                &PartitionId::default(),
                &resource_id,
                relation_filter.as_ref(),
                &ConsistencyMode::MinimizeLatency,
            )?;
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
            repl::run_repl(db, schema.as_deref(), &cli.storage, *json)?;
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
                ..Default::default()
            };
            let pagination = PaginationParams {
                limit: *limit,
                cursor: None,
            };
            let result = engine.storage().query_tuples(
                &PartitionId::default(),
                &filter,
                &pagination,
                &ConsistencyMode::MinimizeLatency,
            )?;
            println!("{}", serde_json::to_string(&result)?);
        }
        Commands::BackupCreate { path, db, schema } => {
            let engine = mk_engine(db, None)?;
            let all_tuples = engine
                .storage()
                .query_tuples(
                    &PartitionId::default(),
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
            let revision = engine.storage().current_revision(&PartitionId::default())?;
            let backend_type = engine.storage().backend_type().to_string();
            let exported_at = Utc::now().to_rfc3339();
            let mut backup = serde_json::json!({
                "version": 3,
                "schema_yaml": schema_yaml,
                "tuples": all_tuples,
                "events": events,
                "metadata": {
                    "backend_type": backend_type,
                    "revision": revision.as_u64(),
                    "exported_at": exported_at,
                },
            });
            let canonical = serde_json::to_string(&backup)?;
            let mut hasher = Sha256::new();
            hasher.update(canonical.as_bytes());
            let hash = hasher.finalize();
            let checksum = hash
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            backup.as_object_mut().unwrap().insert(
                "checksum".to_string(),
                serde_json::Value::String(format!("sha256:{}", checksum)),
            );
            let output = serde_json::to_string_pretty(&backup)?;
            std::fs::write(path, output)
                .with_context(|| format!("failed to write backup to {path}"))?;
            println!(
                r#"{{"status":"ok","tuples":{},"events":{},"revision":{}}}"#,
                all_tuples.len(),
                events.len(),
                revision.as_u64()
            );
        }
        Commands::BackupRestore { path, db } => {
            let engine = mk_engine(db, None)?;
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read backup from {path}"))?;
            let mut backup: serde_json::Value = serde_json::from_str(&content)?;
            let stored_checksum = backup
                .get("checksum")
                .and_then(|v| v.as_str())
                .map(|s| s.strip_prefix("sha256:").unwrap_or(s))
                .unwrap_or("")
                .to_string();
            if let Some(obj) = backup.as_object_mut() {
                obj.remove("checksum");
            }
            if !stored_checksum.is_empty() {
                let canonical = serde_json::to_string(&backup)?;
                let mut hasher = Sha256::new();
                hasher.update(canonical.as_bytes());
                let hash = hasher.finalize();
                let computed = hash
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                if stored_checksum != computed {
                    anyhow::bail!("checksum mismatch: backup may be corrupted");
                }
            }
            let version = backup.get("version").and_then(|v| v.as_i64()).unwrap_or(1);
            if version >= 2 {
                if let Some(sy) = backup.get("schema_yaml").and_then(|s| s.as_str()) {
                    if !sy.is_empty() {
                        let schema =
                            parse_schema(sy).context("failed to parse schema from backup")?;
                        engine.reload_schema(schema)?;
                    }
                }
            }
            let tuples: Vec<RelationshipTuple> = serde_json::from_value(
                backup
                    .get("tuples")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            )
            .context("invalid backup format: missing or invalid 'tuples' field")?;
            let events: Vec<AuditEntry> = serde_json::from_value(
                backup
                    .get("events")
                    .cloned()
                    .unwrap_or(serde_json::Value::Array(vec![])),
            )
            .context("invalid backup format: missing or invalid 'events' field")?;
            let revision = backup
                .get("metadata")
                .and_then(|m| m.get("revision"))
                .and_then(|r| r.as_u64())
                .map(Revision::new)
                .unwrap_or(Revision::ZERO);
            let count = tuples.len();
            engine
                .storage()
                .restore_backup(&PartitionId::default(), &tuples, &events, revision)
                .context("failed to restore backup")?;
            println!(r#"{{"status":"ok","restored":{count}}}"#);
        }
        Commands::Export {
            subject,
            db,
            schema,
        } => {
            let engine = mk_engine(db, schema.as_deref())?;
            let tuples = if let Some(s) = subject {
                let subject_id =
                    SubjectId::new(s.as_str()).with_context(|| format!("invalid subject: {s}"))?;
                engine.export_subject(&subject_id)?
            } else {
                engine
                    .storage()
                    .query_tuples(
                        &PartitionId::default(),
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
        Commands::Import { path, db, schema } => {
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
        Commands::SchemaLint { path, strict } => {
            let yaml = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read schema file {path}"))?;
            match parse_schema(&yaml) {
                Ok(schema) => {
                    let report = aegis_core::schema::lint_schema(&schema, *strict);
                    if report.errors.is_empty() && report.warnings.is_empty() {
                        println!(
                            r#"{{"status":"ok","types":{},"version":{}}}"#,
                            schema.types.len(),
                            schema.schema_version
                        );
                    } else {
                        let status = if !report.errors.is_empty() {
                            "error"
                        } else {
                            "warning"
                        };
                        let output = serde_json::json!({
                            "status": status,
                            "errors": report.errors,
                            "warnings": report.warnings,
                            "passed": report.passed,
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
        Commands::PolicyDiff { schema_file, db } => {
            let engine = mk_engine(db, None)?;
            let yaml = std::fs::read_to_string(schema_file)
                .with_context(|| format!("failed to read schema file {schema_file}"))?;
            let new_schema = parse_schema(&yaml)
                .with_context(|| format!("failed to parse schema from {schema_file}"))?;
            let report = engine.check_schema(&new_schema);

            // Compute added/removed types, relations, permissions
            let current_schema = engine.schema();
            let current_types: std::collections::HashSet<&str> =
                current_schema.types.keys().map(|s| s.as_str()).collect();
            let new_types_set: std::collections::HashSet<&str> =
                new_schema.types.keys().map(|s| s.as_str()).collect();

            let mut types_added = Vec::new();
            let mut types_removed = Vec::new();
            let mut relations_added = Vec::new();
            let mut relations_removed = Vec::new();
            let mut permissions_added = Vec::new();
            let mut permissions_removed = Vec::new();

            for t in new_types_set.difference(&current_types) {
                types_added.push(t.to_string());
            }
            for t in current_types.difference(&new_types_set) {
                types_removed.push(t.to_string());
            }
            for type_name in current_types.intersection(&new_types_set) {
                let cur_type = &current_schema.types[*type_name];
                let new_type = &new_schema.types[*type_name];
                let cur_rels: std::collections::HashSet<&str> =
                    cur_type.relations.keys().map(|s| s.as_str()).collect();
                let new_rels: std::collections::HashSet<&str> =
                    new_type.relations.keys().map(|s| s.as_str()).collect();
                for r in new_rels.difference(&cur_rels) {
                    relations_added.push(format!("{}:{}", type_name, r));
                }
                for r in cur_rels.difference(&new_rels) {
                    relations_removed.push(format!("{}:{}", type_name, r));
                }
                let cur_perms: std::collections::HashSet<&str> =
                    cur_type.permissions.keys().map(|s| s.as_str()).collect();
                let new_perms: std::collections::HashSet<&str> =
                    new_type.permissions.keys().map(|s| s.as_str()).collect();
                for p in new_perms.difference(&cur_perms) {
                    permissions_added.push(format!("{}:{}", type_name, p));
                }
                for p in cur_perms.difference(&new_perms) {
                    permissions_removed.push(format!("{}:{}", type_name, p));
                }
            }

            println!("Policy Diff");
            println!("===========");
            println!(
                "Types Added:    {}",
                if types_added.is_empty() {
                    "(none)".to_string()
                } else {
                    types_added.join(", ")
                }
            );
            println!(
                "Types Removed:  {}",
                if types_removed.is_empty() {
                    "(none)".to_string()
                } else {
                    types_removed.join(", ")
                }
            );
            println!(
                "Relations Added:  {}",
                if relations_added.is_empty() {
                    "(none)".to_string()
                } else {
                    relations_added.join(", ")
                }
            );
            println!(
                "Relations Removed: {}",
                if relations_removed.is_empty() {
                    "(none)".to_string()
                } else {
                    relations_removed.join(", ")
                }
            );
            println!(
                "Permissions Added: {}",
                if permissions_added.is_empty() {
                    "(none)".to_string()
                } else {
                    permissions_added.join(", ")
                }
            );
            println!(
                "Permissions Removed: {}",
                if permissions_removed.is_empty() {
                    "(none)".to_string()
                } else {
                    permissions_removed.join(", ")
                }
            );
            println!("Warnings:");
            if report.warnings.is_empty() {
                println!("  (none)");
            } else {
                for w in &report.warnings {
                    println!("  - {w}");
                }
            }
            println!(
                "Breaking: {}",
                if report.breaking.is_empty() {
                    "No"
                } else {
                    "Yes"
                }
            );
            for b in &report.breaking {
                println!("  - {b}");
            }
        }
        Commands::Recover {
            db,
            to_revision,
            dry_run,
        } => {
            let engine = mk_engine(db, None)?;
            let to_rev = to_revision.map(|r| Revision::new(r as u64));
            if *dry_run {
                let current_rev = engine.storage().current_revision(&PartitionId::default())?;
                let target_rev = to_rev.unwrap_or(current_rev);
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "dry_run",
                        "current_revision": current_rev.as_u64(),
                        "target_revision": target_rev.as_u64(),
                        "message": format!("Would recover events up to revision {target_rev} (current: {current_rev})"),
                    })
                );
            } else {
                let revision = engine.recover_from_events(to_rev)?;
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "ok",
                        "revision": revision.as_u64(),
                    })
                );
            }
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
            let token =
                engine.delete_subject_with_policy(&subject_id, policy, transfer.as_ref())?;
            println!(
                "{}",
                serde_json::json!({
                    "status": "ok",
                    "revision": token.revision.as_u64(),
                })
            );
        }
        Commands::PolicyDraft(action) => {
            let (db, schema_path) = match action {
                PolicyDraftAction::Create { db, schema, .. } => (db, schema.as_deref()),
                PolicyDraftAction::Validate { db, .. } => (db, None),
                PolicyDraftAction::Diff { db, .. } => (db, None),
                PolicyDraftAction::Submit { db, .. } => (db, None),
                PolicyDraftAction::Approve { db, .. } => (db, None),
                PolicyDraftAction::Reject { db, .. } => (db, None),
                PolicyDraftAction::Publish { db, .. } => (db, None),
                PolicyDraftAction::Archive { db, .. } => (db, None),
                PolicyDraftAction::List { db, .. } => (db, None),
            };
            let engine = mk_engine(db, schema_path)?;
            match action {
                PolicyDraftAction::Create {
                    name,
                    description,
                    schema,
                    ..
                } => {
                    let draft = engine.create_policy_draft(name, description)?;
                    if let Some(schema_path) = schema.as_ref() {
                        let yaml = std::fs::read_to_string(schema_path)
                            .with_context(|| format!("failed to read schema file {schema_path}"))?;
                        let schema_obj = aegis_core::schema::parse_schema(&yaml)
                            .context("failed to parse schema")?;
                        let _ = engine.update_policy_draft(draft.id, schema_obj)?;
                    }
                    println!("{}", serde_json::to_string_pretty(&draft)?);
                }
                PolicyDraftAction::Validate { id, .. } => {
                    let uid =
                        uuid::Uuid::parse_str(id).with_context(|| format!("invalid id: {id}"))?;
                    let report = engine.validate_policy_draft(uid)?;
                    println!("{}", serde_json::to_string_pretty(&report)?);
                }
                PolicyDraftAction::Diff { id, .. } => {
                    let uid =
                        uuid::Uuid::parse_str(id).with_context(|| format!("invalid id: {id}"))?;
                    let drafts = engine.list_policy_drafts(None)?;
                    let draft = drafts
                        .into_iter()
                        .find(|d| d.id == uid)
                        .ok_or_else(|| anyhow::anyhow!("draft {id} not found"))?;
                    let report =
                        engine.access_diff(&*engine.schema(), &draft.schema, None, None)?;
                    println!("{}", serde_json::to_string_pretty(&report)?);
                }
                PolicyDraftAction::Submit { id, .. } => {
                    let uid =
                        uuid::Uuid::parse_str(id).with_context(|| format!("invalid id: {id}"))?;
                    let draft = engine.submit_policy_draft_for_review(uid)?;
                    println!("{}", serde_json::to_string_pretty(&draft)?);
                }
                PolicyDraftAction::Approve { id, .. } => {
                    let uid =
                        uuid::Uuid::parse_str(id).with_context(|| format!("invalid id: {id}"))?;
                    let draft = engine.approve_policy_draft(uid)?;
                    println!("{}", serde_json::to_string_pretty(&draft)?);
                }
                PolicyDraftAction::Reject { id, reason, .. } => {
                    let uid =
                        uuid::Uuid::parse_str(id).with_context(|| format!("invalid id: {id}"))?;
                    let draft = engine.reject_policy_draft(uid, reason)?;
                    println!("{}", serde_json::to_string_pretty(&draft)?);
                }
                PolicyDraftAction::Publish { id, .. } => {
                    let uid =
                        uuid::Uuid::parse_str(id).with_context(|| format!("invalid id: {id}"))?;
                    let result = engine.publish_policy_draft(uid)?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                PolicyDraftAction::Archive { id, .. } => {
                    let uid =
                        uuid::Uuid::parse_str(id).with_context(|| format!("invalid id: {id}"))?;
                    let draft = engine.archive_policy_draft(uid)?;
                    println!("{}", serde_json::to_string_pretty(&draft)?);
                }
                PolicyDraftAction::List { status, .. } => {
                    let filter = match status {
                        Some(s) => {
                            let status_val = match s.to_lowercase().as_str() {
                                "drafting" => DraftStatus::Drafting,
                                "under_review" | "underreview" => DraftStatus::UnderReview,
                                "approved" => DraftStatus::Approved,
                                "published" => DraftStatus::Published,
                                "rejected" => DraftStatus::Rejected,
                                "superseded" => DraftStatus::Superseded,
                                "archived" => DraftStatus::Archived,
                                _ => anyhow::bail!("invalid status: {s}"),
                            };
                            Some(status_val)
                        }
                        None => None,
                    };
                    let drafts = engine.list_policy_drafts(filter)?;
                    println!("{}", serde_json::to_string_pretty(&drafts)?);
                }
            }
        }
        Commands::Schedule(action) => {
            let db = match action {
                ScheduleAction::Create { db, .. } => db,
                ScheduleAction::List { db } => db,
                ScheduleAction::Delete { db, .. } => db,
                ScheduleAction::Run { db, .. } => db,
                ScheduleAction::Runs { db, .. } => db,
            };
            let engine = mk_engine(db, None)?;
            match action {
                ScheduleAction::Create { config, .. } => {
                    let json_str = std::fs::read_to_string(config)
                        .with_context(|| format!("failed to read config file {config}"))?;
                    let cfg: aegis_core::engine::scheduler::AnalysisScheduleConfig =
                        serde_json::from_str(&json_str)
                            .context("failed to parse schedule config")?;
                    let schedule = engine.create_analysis_schedule(
                        &cfg.name,
                        cfg.interval_seconds,
                        cfg.queries,
                        cfg.compare_schema,
                    )?;
                    println!("{}", serde_json::to_string_pretty(&schedule)?);
                }
                ScheduleAction::List { .. } => {
                    let schedules = engine.list_analysis_schedules()?;
                    println!("{}", serde_json::to_string_pretty(&schedules)?);
                }
                ScheduleAction::Delete { id, .. } => {
                    let uid =
                        uuid::Uuid::parse_str(id).with_context(|| format!("invalid id: {id}"))?;
                    let deleted = engine.delete_analysis_schedule(uid)?;
                    println!("{}", if deleted { "deleted" } else { "not found" });
                }
                ScheduleAction::Run { schedule_id, .. } => {
                    let uid = schedule_id
                        .as_ref()
                        .map(|id| uuid::Uuid::parse_str(id))
                        .transpose()
                        .with_context(|| "invalid schedule id")?;
                    let runs = engine.run_analysis_now(uid)?;
                    println!("{}", serde_json::to_string_pretty(&runs)?);
                }
                ScheduleAction::Runs { limit, .. } => {
                    let runs = engine.get_analysis_runs(*limit)?;
                    println!("{}", serde_json::to_string_pretty(&runs)?);
                }
            }
        }
        Commands::Enforcement(action) => {
            let db = match action {
                EnforcementAction::Set { db, .. } => db,
                EnforcementAction::Get { db } => db,
                EnforcementAction::Trends { db, .. } => db,
            };
            let engine = mk_engine(db, None)?;
            match action {
                EnforcementAction::Set { config, .. } => {
                    let json_str = std::fs::read_to_string(config)
                        .with_context(|| format!("failed to read config file {config}"))?;
                    let config: aegis_core::engine::enforcement_history::EnforcementHistoryConfig =
                        serde_json::from_str(&json_str)
                            .context("failed to parse enforcement config")?;
                    engine.set_enforcement_history_config(config)?;
                    println!("ok");
                }
                EnforcementAction::Get { .. } => {
                    let config = engine.get_enforcement_history_config()?;
                    println!("{}", serde_json::to_string_pretty(&config)?);
                }
                EnforcementAction::Trends { limit, .. } => {
                    let trends = engine.enforcement_trends(*limit)?;
                    println!("{}", serde_json::to_string_pretty(&trends)?);
                }
            }
        }
        Commands::Subscribe {
            event_types,
            db,
            schema,
        } => {
            let engine = mk_engine(db, schema.as_deref())?;
            let types: Vec<WatchEventType> = event_types
                .split(',')
                .map(|s| match s.trim().to_lowercase().as_str() {
                    "tupleadded" => Ok(WatchEventType::TupleAdded),
                    "tupleremoved" => Ok(WatchEventType::TupleRemoved),
                    "policyversioncreated" => Ok(WatchEventType::PolicyVersionCreated),
                    "policyrolledback" => Ok(WatchEventType::PolicyRolledBack),
                    "integrityfinding" => Ok(WatchEventType::IntegrityFinding),
                    "analysiscompleted" => Ok(WatchEventType::AnalysisCompleted),
                    "ratelimitwarning" => Ok(WatchEventType::RateLimitWarning),
                    _ => anyhow::bail!("unknown event type: {}", s),
                })
                .collect::<Result<Vec<_>>>()?;
            let sub = engine.subscribe(types);
            println!(
                "Subscribed (id: {}). Polling... Press Ctrl+C to stop.",
                sub.id()
            );
            loop {
                if let Some(event) = sub.try_recv().ok() {
                    let json = serde_json::json!({
                        "event_type": format!("{:?}", event.event_type),
                        "subject": event.subject,
                        "relation": event.relation,
                        "object": event.object,
                        "revision": event.revision.as_u64(),
                        "payload": event.payload,
                    });
                    println!("{}", serde_json::to_string(&json)?);
                }
                std::thread::sleep(Duration::from_millis(200));
            }
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
