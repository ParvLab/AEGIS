use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;

use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::StorageBackend;
use aegis_core::storage::TupleFilter;
use aegis_core::types::*;

mod repl;

#[derive(Parser)]
#[command(name = "aegis", version, about = "Aegis authorization engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
}

fn load_db(path: &str, schema_path: Option<&str>) -> Result<GraphEngine> {
    let config = SqliteConfig {
        path: path.to_string(),
        ..Default::default()
    };
    let mut storage = SqliteStorage::new(config)
        .with_context(|| format!("failed to create SQLite storage at {path}"))?;
    storage
        .initialize()
        .context("failed to initialize storage")?;

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

    Ok(GraphEngine::new(Box::new(storage), schema))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Check {
            subject,
            permission,
            resource,
            db,
            schema,
        } => {
            let engine = load_db(db, schema.as_deref())?;
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
            let engine = load_db(db, schema.as_deref())?;
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
            let engine = load_db(db, None)?;
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
            let engine = load_db(db, None)?;
            let resource_id = ResourceId::new(object.as_str())
                .with_context(|| format!("invalid object: {object}"))?;
            let relation_filter = relation
                .as_ref()
                .map(|r| Relation::new(r.as_str()))
                .transpose()
                .with_context(|| "invalid relation filter")?;
            let tuples = engine
                .storage()
                .list_by_object(&resource_id, relation_filter.as_ref())?;
            println!("{}", serde_json::to_string(&tuples)?);
        }
        Commands::Explain {
            subject,
            permission,
            resource,
            db,
            schema,
        } => {
            let engine = load_db(db, schema.as_deref())?;
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
            let engine = load_db(db, schema.as_deref())?;
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
            let engine = load_db(db, schema.as_deref())?;
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
            let engine = load_db(db, schema.as_deref())?;
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
            let engine = load_db(db, None)?;
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
            let engine = load_db(db, None)?;
            let subject_id = SubjectId::new(subject.as_str())
                .with_context(|| format!("invalid subject: {subject}"))?;
            let tuples = engine.export_subject(&subject_id)?;
            println!("{}", serde_json::to_string(&tuples)?);
        }
        Commands::Repl { db, schema } => {
            repl::run_repl(db, schema.as_deref())?;
        }
        Commands::Query {
            subject_type,
            relation,
            object_type,
            limit,
            db,
        } => {
            let engine = load_db(db, None)?;
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
    }

    Ok(())
}
