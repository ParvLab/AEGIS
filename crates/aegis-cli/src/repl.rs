use std::collections::HashMap;

use anyhow::{Context, Result};
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Config, Context as RlContext, Editor, Helper};

use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::StorageBackend;
use aegis_core::types::*;

const COMMANDS: &[&str] = &[
    "check", "write", "delete", "list", "explain", "health", "dry-run",
    "audit", "export", "schema", "help", "exit",
];

struct CmdHelper;

impl Completer for CmdHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        _pos: usize,
        _ctx: &RlContext<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        let trimmed = line.trim();
        let mut candidates = Vec::new();
        for cmd in COMMANDS {
            if cmd.starts_with(trimmed) || trimmed.is_empty() {
                candidates.push(Pair {
                    display: cmd.to_string(),
                    replacement: format!("{cmd} "),
                });
            }
        }
        Ok((0, candidates))
    }
}

impl Hinter for CmdHelper {
    type Hint = String;
}

impl Highlighter for CmdHelper {}

impl Validator for CmdHelper {
    fn validate(&self, _ctx: &mut ValidationContext<'_>) -> Result<ValidationResult, ReadlineError> {
        Ok(ValidationResult::Valid(None))
    }
}

impl Helper for CmdHelper {}

fn print_help() {
    println!("Aegis REPL commands:");
    println!("  check <subject> <permission> <resource>   - Check authorization");
    println!("  write <subject> <relation> <resource>     - Write a relationship tuple");
    println!("  delete <subject> <relation> <resource>    - Delete a relationship tuple");
    println!("  list <object> [relation]                  - List tuples for an object");
    println!("  explain <subject> <permission> <resource>  - Explain authorization decision");
    println!("  health                                    - Show engine health report");
    println!("  dry-run check <subject> <perm> <resource> - Dry-run (no cache, no hook)");
    println!("  dry-run write <subject> <rel> <resource>  - Dry-run write (validate only)");
    println!("  audit <object> [--from N] [--to N]        - Query audit log");
    println!("  export <subject>                          - Export all tuples for subject");
    println!("  schema                                    - Show current schema");
    println!("  help                                      - Show this help");
    println!("  exit                                      - Exit the REPL");
}

pub fn run_repl(db_path: &str, schema_path: Option<&str>) -> Result<()> {
    let engine = load_engine(db_path, schema_path)?;

    let config = Config::builder()
        .history_ignore_dups(true)?
        .max_history_size(1000)?
        .build();
    let mut rl: Editor<CmdHelper, _> = Editor::with_config(config)?;
    rl.set_helper(Some(CmdHelper));

    let history_file = dirs_or_default("aegis_history.txt");

    if rl.load_history(&history_file).is_err() {
        // History file doesn't exist yet; ignore.
    }

    println!("Aegis REPL. Type 'help' for commands, 'exit' to quit.");

    loop {
        let readline = rl.readline("aegis> ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(trimmed);
                if let Err(e) = process_command(&engine, trimmed) {
                    eprintln!("Error: {e}");
                }
            }
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(e) => {
                eprintln!("Readline error: {e}");
                break;
            }
        }
    }

    let _ = rl.save_history(&history_file);
    let _ = engine.close();
    Ok(())
}

fn dirs_or_default(filename: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        home.join(filename).to_string_lossy().to_string()
    } else {
        filename.to_string()
    }
}

fn load_engine(db_path: &str, schema_path: Option<&str>) -> Result<GraphEngine> {
    let config = SqliteConfig {
        path: db_path.to_string(),
        ..Default::default()
    };
    let mut storage =
        SqliteStorage::new(config).with_context(|| format!("failed to create SQLite storage at {db_path}"))?;
    storage.initialize().context("failed to initialize storage")?;

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

fn process_command(engine: &GraphEngine, line: &str) -> Result<()> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }

    match parts[0] {
        "check" => cmd_check(engine, &parts[1..]),
        "write" => cmd_write(engine, &parts[1..]),
        "delete" => cmd_delete(engine, &parts[1..]),
        "list" => cmd_list(engine, &parts[1..]),
        "explain" => cmd_explain(engine, &parts[1..]),
        "health" => cmd_health(engine),
        "dry-run" => cmd_dry_run(engine, &parts[1..]),
        "audit" => cmd_audit(engine, &parts[1..]),
        "export" => cmd_export(engine, &parts[1..]),
        "schema" => cmd_schema(engine),
        "help" => {
            print_help();
            Ok(())
        }
        "exit" => std::process::exit(0),
        other => {
            eprintln!("Unknown command: {other}. Type 'help' for available commands.");
            Ok(())
        }
    }
}

fn cmd_check(engine: &GraphEngine, args: &[&str]) -> Result<()> {
    if args.len() < 3 {
        eprintln!("Usage: check <subject> <permission> <resource>");
        return Ok(());
    }
    let subject = SubjectId::new(args[0])?;
    let permission = args[1];
    let resource = ResourceId::new(args[2])?;
    let result = engine.check(&subject, permission, &resource, None)?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "allowed": result.allowed,
            "revision": result.revision.as_u64(),
        }))?
    );
    Ok(())
}

fn cmd_write(engine: &GraphEngine, args: &[&str]) -> Result<()> {
    if args.len() < 3 {
        eprintln!("Usage: write <subject> <relation> <resource>");
        return Ok(());
    }
    let subject = SubjectId::new(args[0])?;
    let relation = Relation::new(args[1])?;
    let resource = ResourceId::new(args[2])?;
    let tuple = RelationshipTuple::new(subject, relation, resource);
    let token = engine.write(&tuple)?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "revision": token.revision.as_u64(),
        }))?
    );
    Ok(())
}

fn cmd_delete(engine: &GraphEngine, args: &[&str]) -> Result<()> {
    if args.len() < 3 {
        eprintln!("Usage: delete <subject> <relation> <resource>");
        return Ok(());
    }
    let subject = SubjectId::new(args[0])?;
    let relation = Relation::new(args[1])?;
    let resource = ResourceId::new(args[2])?;
    let key = TupleKey {
        subject,
        relation,
        object: resource,
    };
    let token = engine.delete(&key)?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "revision": token.revision.as_u64(),
        }))?
    );
    Ok(())
}

fn cmd_list(engine: &GraphEngine, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: list <object> [relation]");
        return Ok(());
    }
    let object = ResourceId::new(args[0])?;
    let relation = args.get(1).map(|r| Relation::new(*r)).transpose()?;
    let tuples = engine.storage().list_by_object(&object, relation.as_ref(), &ConsistencyMode::MinimizeLatency)?;
    println!("{}", serde_json::to_string(&tuples)?);
    Ok(())
}

fn cmd_explain(engine: &GraphEngine, args: &[&str]) -> Result<()> {
    if args.len() < 3 {
        eprintln!("Usage: explain <subject> <permission> <resource>");
        return Ok(());
    }
    let subject = SubjectId::new(args[0])?;
    let permission = args[1];
    let resource = ResourceId::new(args[2])?;
    let result = engine.explain(&subject, permission, &resource, None)?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "allowed": result.allowed,
            "revision": result.revision.as_u64(),
            "resolved_via": result.resolved_via,
            "trace": result.trace,
        }))?
    );
    Ok(())
}

fn cmd_health(engine: &GraphEngine) -> Result<()> {
    let report = engine.health();
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn cmd_dry_run(engine: &GraphEngine, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: dry-run check <subject> <permission> <resource>");
        eprintln!("       dry-run write <subject> <relation> <resource>");
        return Ok(());
    }
    match args[0] {
        "check" => {
            if args.len() < 4 {
                eprintln!("Usage: dry-run check <subject> <permission> <resource>");
                return Ok(());
            }
            let subject = SubjectId::new(args[1])?;
            let permission = args[2];
            let resource = ResourceId::new(args[3])?;
            let result = engine.check_dry_run(&subject, permission, &resource, None)?;
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "allowed": result.allowed,
                    "revision": result.revision.as_u64(),
                    "dry_run": true,
                }))?
            );
        }
        "write" => {
            if args.len() < 4 {
                eprintln!("Usage: dry-run write <subject> <relation> <resource>");
                return Ok(());
            }
            let subject = SubjectId::new(args[1])?;
            let relation = Relation::new(args[2])?;
            let resource = ResourceId::new(args[3])?;
            let tuple = RelationshipTuple::new(subject, relation, resource);
            let token = engine.write_dry_run(&tuple)?;
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "revision": token.revision.as_u64(),
                    "dry_run": true,
                    "valid": true,
                }))?
            );
        }
        other => {
            eprintln!("Unknown dry-run subcommand: {other}. Use 'check' or 'write'.");
        }
    }
    Ok(())
}

fn cmd_audit(engine: &GraphEngine, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: audit <object> [--from N] [--to N]");
        return Ok(());
    }
    let object = ResourceId::new(args[0])?;
    let from_rev = None;
    let to_rev = None;
    let pagination = PaginationParams {
        limit: 50,
        cursor: None,
    };
    let entries = engine.query_audit(&object, from_rev, to_rev, &pagination)?;
    println!("{}", serde_json::to_string_pretty(&entries)?);
    Ok(())
}

fn cmd_export(engine: &GraphEngine, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: export <subject>");
        return Ok(());
    }
    let subject = SubjectId::new(args[0])?;
    let tuples = engine.export_subject(&subject)?;
    println!("{}", serde_json::to_string_pretty(&tuples)?);
    Ok(())
}

fn cmd_schema(engine: &GraphEngine) -> Result<()> {
    let schema = engine.schema();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": schema.schema_version,
            "namespace": schema.namespace,
            "types": schema.types,
        }))?
    );
    Ok(())
}
