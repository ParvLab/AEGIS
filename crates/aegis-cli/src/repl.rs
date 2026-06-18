use std::collections::HashMap;
use std::sync::mpsc::TryRecvError;

use anyhow::{Context, Result};
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Config, Context as RlContext, Editor, Helper};

use aegis_core::engine::enforcement_history::EnforcementHistoryConfig;
use aegis_core::engine::policy_lifecycle::DraftStatus;
use aegis_core::engine::scheduler::{AnalysisScheduleConfig, AnalysisRunStatus};
use aegis_core::engine::watch::{WatchEventType, WatchFilter, WatchSubscription};
use aegis_core::engine::GraphEngine;
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::storage::{StorageBackend, TupleFilter};
use aegis_core::types::*;
use sha2::{Digest, Sha256};

#[cfg(feature = "rocksdb")]
use aegis_core::storage::RocksDbStorage;

const COMMANDS: &[&str] = &[
    "check", "write", "delete", "list", "explain", "health", "dry-run",
    "audit", "export", "export-subject", "schema", "query", "watch", "unwatch",
    "backup", "restore", "import", "recover", "delete-subject",
    "policy-draft", "schedule", "enforcement", "subscribe",
    "help", "exit",
];

struct ReplState {
    engine: GraphEngine,
    watch_sub: Option<WatchSubscription>,
    json_mode: bool,
}

struct CmdHelper {
    entity_names: Vec<String>,
}

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
        for name in &self.entity_names {
            if name.starts_with(trimmed) || trimmed.is_empty() {
                candidates.push(Pair {
                    display: name.clone(),
                    replacement: format!("{name} "),
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

fn green(s: &str) -> String {
    format!("\x1b[32m{}\x1b[0m", s)
}

fn red(s: &str) -> String {
    format!("\x1b[31m{}\x1b[0m", s)
}

fn yellow(s: &str) -> String {
    format!("\x1b[33m{}\x1b[0m", s)
}

fn bold(s: &str) -> String {
    format!("\x1b[1m{}\x1b[0m", s)
}

fn print_help() {
    println!("Aegis REPL commands:");
    println!("  check <subject> <permission> <resource>          - Check authorization");
    println!("  write <subject> <relation> <resource>            - Write a relationship tuple");
    println!("  delete <subject> <relation> <resource>           - Delete a relationship tuple");
    println!("  list <object> [relation]                         - List tuples for an object");
    println!("  explain <subject> <permission> <resource>        - Explain authorization decision");
    println!("  health                                           - Show engine health report");
    println!("  dry-run check <subject> <perm> <resource>        - Dry-run (no cache, no hook)");
    println!("  dry-run write <subject> <rel> <resource>         - Dry-run write (validate only)");
    println!("  audit <object> [--from N] [--to N]               - Query audit log");
    println!("  export <subject>                                 - Export all tuples for subject");
    println!("  schema                                           - Show current schema");
    println!("  query [--subject-type X] [--relation Y] [--object-type Z] [--limit N]");
    println!("                                                   - Query tuples with filters");
    println!("  watch <object>                                   - Watch events for an object");
    println!("  watch --all                                      - Watch all events");
    println!("  unwatch                                          - Stop watching");
    println!("  backup <path>                                    - Backup all tuples/events to file");
    println!("  restore <path>                                   - Restore tuples/events from backup");
    println!("  import <path>                                    - Import tuples from JSON file");
    println!("  recover [--to-revision N] [--dry-run]             - Recover from event log");
    println!("  delete-subject <subject> --policy <cascade|fail|transfer> [--transfer-to X]");
    println!("                                                   - Delete subject with policy");
    println!("  export-subject <subject>                          - Export all tuples for a subject");
    println!("  policy-draft create <name> <desc>                 - Create a policy draft");
    println!("  policy-draft validate <id>                        - Validate a policy draft");
    println!("  policy-draft diff <id>                            - Diff draft against current schema");
    println!("  policy-draft submit <id>                          - Submit draft for review");
    println!("  policy-draft approve <id>                         - Approve a draft");
    println!("  policy-draft reject <id> <reason>                 - Reject a draft");
    println!("  policy-draft publish <id>                         - Publish an approved draft");
    println!("  policy-draft archive <id>                         - Archive a draft");
    println!("  policy-draft list [status]                        - List drafts");
    println!("  schedule create <config>                          - Create analysis schedule from JSON");
    println!("  schedule list                                     - List schedules");
    println!("  schedule delete <id>                              - Delete a schedule");
    println!("  schedule run [id]                                 - Run analysis now");
    println!("  schedule runs [limit]                             - Show analysis run history");
    println!("  enforcement set <config>                          - Set enforcement config from JSON");
    println!("  enforcement get                                   - Show enforcement config");
    println!("  enforcement trends [limit]                        - Show enforcement trends");
    println!("  subscribe <event_types>                           - Subscribe to engine events");
    println!("  help                                             - Show this help");
    println!("  exit                                             - Exit the REPL");
}

pub fn run_repl(db_path: &str, schema_path: Option<&str>, storage_type: &str, json_mode: bool) -> Result<()> {
    let engine = load_engine(db_path, schema_path, storage_type)?;

    let entity_names = extract_entity_names(&engine);

    let config = Config::builder()
        .history_ignore_dups(true)?
        .max_history_size(1000)?
        .build();
    let mut rl: Editor<CmdHelper, _> = Editor::with_config(config)?;
    rl.set_helper(Some(CmdHelper { entity_names }));

    let history_file = dirs_or_default("aegis_history.txt");

    if rl.load_history(&history_file).is_err() {
        // History file doesn't exist yet; ignore.
    }

    println!("Aegis REPL. Type 'help' for commands, 'exit' to quit.");

    let mut state = ReplState {
        engine,
        watch_sub: None,
        json_mode,
    };

    loop {
        let readline = rl.readline("aegis> ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(trimmed);
                if let Err(e) = process_command(&mut state, trimmed) {
                    eprintln!("Error: {e}");
                }
                poll_watch(&state);
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
    let _ = state.engine.close();
    Ok(())
}

fn dirs_or_default(filename: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        home.join(filename).to_string_lossy().to_string()
    } else {
        filename.to_string()
    }
}

fn load_storage(
    db_path: &str,
    storage_type: &str,
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
            anyhow::bail!("rocksdb backend is not enabled. Rebuild with --features rocksdb");
        }
        _ => anyhow::bail!(
            "unknown storage backend: {storage_type}. Supported: sqlite, rocksdb"
        ),
    }
}

fn load_engine(db_path: &str, schema_path: Option<&str>, storage_type: &str) -> Result<GraphEngine> {
    let storage = load_storage(db_path, storage_type)?;

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

fn extract_entity_names(engine: &GraphEngine) -> Vec<String> {
    let schema = engine.schema();
    let mut names: Vec<String> = Vec::new();
    for type_name in schema.types.keys() {
        names.push(type_name.clone());
        if let Some(td) = schema.types.get(type_name) {
            for rel_name in td.relations.keys() {
                if !names.contains(rel_name) {
                    names.push(rel_name.clone());
                }
            }
        }
    }
    names
}

fn poll_watch(state: &ReplState) {
    if let Some(ref sub) = state.watch_sub {
        loop {
            match sub.try_recv() {
                Ok(event) => {
                    let icon = match event.event_type {
                        WatchEventType::TupleAdded => "+",
                        WatchEventType::TupleRemoved => "-",
                        WatchEventType::PolicyVersionCreated => "P",
                        WatchEventType::PolicyRolledBack => "R",
                        WatchEventType::IntegrityFinding => "!",
                        WatchEventType::AnalysisCompleted => "A",
                        WatchEventType::RateLimitWarning => "W",
                    };
                    if state.json_mode {
                        println!(r#"{{"event_type":"{:?}","subject":"{}","relation":"{}","object":"{}","revision":{},"payload":{}}}"#,
                            event.event_type, event.subject, event.relation, event.object, event.revision.as_u64(),
                            event.payload.as_ref().map(|v| v.to_string()).unwrap_or_default());
                    } else {
                        println!(
                            "  {} {} {} {} (rev={})",
                            yellow(icon),
                            event.subject,
                            event.relation,
                            event.object,
                            event.revision.as_u64()
                        );
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // subscription was dropped
                    break;
                }
            }
        }
    }
}

fn process_command(state: &mut ReplState, line: &str) -> Result<()> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }

    match parts[0] {
        "check" => cmd_check(state, &parts[1..]),
        "write" => cmd_write(state, &parts[1..]),
        "delete" => cmd_delete(state, &parts[1..]),
        "list" => cmd_list(state, &parts[1..]),
        "explain" => cmd_explain(state, &parts[1..]),
        "health" => cmd_health(state),
        "dry-run" => cmd_dry_run(state, &parts[1..]),
        "audit" => cmd_audit(state, &parts[1..]),
        "export" => cmd_export(state, &parts[1..]),
        "export-subject" => cmd_export_subject_repl(state, &parts[1..]),
        "schema" => cmd_schema(state),
        "query" => cmd_query(state, &parts[1..]),
        "watch" => cmd_watch(state, &parts[1..]),
        "unwatch" => cmd_unwatch(state),
        "backup" => cmd_backup(state, &parts[1..]),
        "restore" => cmd_restore(state, &parts[1..]),
        "import" => cmd_import(state, &parts[1..]),
        "recover" => cmd_recover_repl(state, &parts[1..]),
        "delete-subject" => cmd_delete_subject_repl(state, &parts[1..]),
        "policy-draft" => cmd_policy_draft(state, &parts[1..]),
        "schedule" => cmd_schedule(state, &parts[1..]),
        "enforcement" => cmd_enforcement(state, &parts[1..]),
        "subscribe" => cmd_subscribe(state, &parts[1..]),
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

fn cmd_check(state: &ReplState, args: &[&str]) -> Result<()> {
    if args.len() < 3 {
        eprintln!("Usage: check <subject> <permission> <resource>");
        return Ok(());
    }
    let subject = SubjectId::new(args[0])?;
    let permission = args[1];
    let resource = ResourceId::new(args[2])?;
    let result = state.engine.check(&subject, permission, &resource, None)?;
    if state.json_mode {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "allowed": result.allowed,
                "revision": result.revision.as_u64(),
            }))?
        );
    } else {
        if result.allowed {
            println!("  {} ALLOWED (revision={})", green("✓"), result.revision.as_u64());
        } else {
            println!("  {} DENIED (revision={})", red("✗"), result.revision.as_u64());
        }
    }
    Ok(())
}

fn cmd_write(state: &ReplState, args: &[&str]) -> Result<()> {
    if args.len() < 3 {
        eprintln!("Usage: write <subject> <relation> <resource>");
        return Ok(());
    }
    let subject = SubjectId::new(args[0])?;
    let relation = Relation::new(args[1])?;
    let resource = ResourceId::new(args[2])?;
    let tuple = RelationshipTuple::new(subject, relation, resource);
    let token = state.engine.write(&tuple)?;
    if state.json_mode {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "revision": token.revision.as_u64(),
            }))?
        );
    } else {
        println!("  {} Written (revision={})", green("✓"), token.revision.as_u64());
    }
    Ok(())
}

fn cmd_delete(state: &ReplState, args: &[&str]) -> Result<()> {
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
    let token = state.engine.delete(&key)?;
    if state.json_mode {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "revision": token.revision.as_u64(),
            }))?
        );
    } else {
        println!("  {} Deleted (revision={})", green("✓"), token.revision.as_u64());
    }
    Ok(())
}

fn cmd_list(state: &ReplState, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: list <object> [relation]");
        return Ok(());
    }
    let object = ResourceId::new(args[0])?;
    let relation = args.get(1).map(|r| Relation::new(*r)).transpose()?;
    let tuples = state.engine.storage().list_by_object(&PartitionId::default(), &object, relation.as_ref(), &ConsistencyMode::MinimizeLatency)?;
    if state.json_mode {
        println!("{}", serde_json::to_string(&tuples)?);
    } else {
        if tuples.is_empty() {
            println!("  {} No tuples found", yellow("!"));
        } else {
            for t in &tuples {
                println!("  {} {} {} {}", green("•"), t.subject.as_str(), t.relation.as_str(), t.object.as_str());
            }
            println!("  {} {} tuple(s)", bold(&tuples.len().to_string()), "results");
        }
    }
    Ok(())
}

fn cmd_explain(state: &ReplState, args: &[&str]) -> Result<()> {
    if args.len() < 3 {
        eprintln!("Usage: explain <subject> <permission> <resource>");
        return Ok(());
    }
    let subject = SubjectId::new(args[0])?;
    let permission = args[1];
    let resource = ResourceId::new(args[2])?;
    let result = state.engine.explain(&subject, permission, &resource, None)?;
    if state.json_mode {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "allowed": result.allowed,
                "revision": result.revision.as_u64(),
                "resolved_via": result.resolved_via,
                "trace": result.trace,
            }))?
        );
    } else {
        if result.allowed {
            println!("  {} ALLOWED (revision={})", green("✓"), result.revision.as_u64());
        } else {
            println!("  {} DENIED (revision={})", red("✗"), result.revision.as_u64());
        }
        println!("  Resolved via: {}", result.resolved_via);
        for t in &result.trace {
            println!("    {} {} {}", t.subject, t.relation, t.object);
        }
    }
    Ok(())
}

fn cmd_health(state: &ReplState) -> Result<()> {
    let report = state.engine.health();
    if state.json_mode {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("  {}: {}", bold("Engine"), if report.healthy { green("Healthy") } else { red("Unhealthy") });
        println!("  {}:        {}", bold("Backend"), report.backend);
        println!("  {}: {}", bold("Revision"), report.revision.as_u64());
        println!("  {}:  {}", bold("Schema ver"), report.schema_version);
        println!("  {}:    {}", bold("Cache hit"), format!("{:.1}%", report.cache_hit_rate * 100.0));
        println!("  {}:   {}", bold("Cache size"), report.cache_entries);
    }
    Ok(())
}

fn cmd_dry_run(state: &ReplState, args: &[&str]) -> Result<()> {
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
            let result = state.engine.check_dry_run(&subject, permission, &resource, None)?;
            if state.json_mode {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "allowed": result.allowed,
                        "revision": result.revision.as_u64(),
                        "dry_run": true,
                    }))?
                );
            } else {
                let status = if result.allowed { green("ALLOWED") } else { red("DENIED") };
                println!("  {} {} (dry-run, revision={})", status, if result.allowed { "✓" } else { "✗" }, result.revision.as_u64());
            }
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
            let token = state.engine.write_dry_run(&tuple)?;
            if state.json_mode {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "revision": token.revision.as_u64(),
                        "dry_run": true,
                        "valid": true,
                    }))?
                );
            } else {
                println!("  {} Valid (dry-run, revision={})", green("✓"), token.revision.as_u64());
            }
        }
        other => {
            eprintln!("Unknown dry-run subcommand: {other}. Use 'check' or 'write'.");
        }
    }
    Ok(())
}

fn cmd_audit(state: &ReplState, args: &[&str]) -> Result<()> {
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
    let entries = state.engine.query_audit(&object, from_rev, to_rev, &pagination)?;
    if state.json_mode {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        if entries.is_empty() {
            println!("  {} No audit entries found", yellow("!"));
        } else {
            for e in &entries {
                let action = match e.action {
                    TupleMutation::Add => green("ADD"),
                    TupleMutation::Remove => red("DEL"),
                };
                println!("  [{}] {} {} {} (rev={})", action, e.subject, e.relation, e.object, e.revision.as_u64());
            }
        }
    }
    Ok(())
}

fn cmd_export(state: &ReplState, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: export <subject>");
        return Ok(());
    }
    let subject = SubjectId::new(args[0])?;
    let tuples = state.engine.export_subject(&subject)?;
    if state.json_mode {
        println!("{}", serde_json::to_string_pretty(&tuples)?);
    } else {
        if tuples.is_empty() {
            println!("  {} No tuples found for subject", yellow("!"));
        } else {
            for t in &tuples {
                println!("  {} {} {} {}", green("•"), t.subject.as_str(), t.relation.as_str(), t.object.as_str());
            }
            println!("  {} tuple(s)", tuples.len());
        }
    }
    Ok(())
}

fn cmd_schema(state: &ReplState) -> Result<()> {
    let schema = state.engine.schema();
    if state.json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": schema.schema_version,
                "namespace": schema.namespace,
                "types": schema.types,
            }))?
        );
    } else {
        println!("  {} v{}", bold("Schema"), schema.schema_version);
        println!("  {}: {}", bold("Namespace"), schema.namespace);
        println!("  {}:", bold("Types"));
        for (type_name, td) in &schema.types {
            println!("    {} {}", green(type_name), bold("relations:"));
            for rel_name in td.relations.keys() {
                println!("      - {}", rel_name);
            }
            if !td.permissions.is_empty() {
                println!("    {} {:?}", bold("permissions:"), td.permissions.keys());
            }
        }
    }
    Ok(())
}

fn cmd_watch(state: &mut ReplState, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: watch <object>  or  watch --all");
        return Ok(());
    }
    let filter = if args[0] == "--all" {
        WatchFilter::default()
    } else {
        WatchFilter {
            objects: Some(vec![args[0].to_string()]),
            ..Default::default()
        }
    };
    let sub = state.engine.watch(filter);
    state.watch_sub = Some(sub);
    if state.json_mode {
        println!(r#"{{"status":"watching"}}"#);
    } else {
        println!("  {} Watching for events...", green("✓"));
    }
    Ok(())
}

fn cmd_unwatch(state: &mut ReplState) -> Result<()> {
    if state.watch_sub.is_some() {
        state.watch_sub = None;
        if state.json_mode {
            println!(r#"{{"status":"unwatched"}}"#);
        } else {
            println!("  {} Stopped watching", green("✓"));
        }
    } else {
        if state.json_mode {
            println!(r#"{{"status":"not_watching"}}"#);
        } else {
            println!("  {} Not currently watching", yellow("!"));
        }
    }
    Ok(())
}

fn parse_key_value_args(args: &[&str]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        if args[i].starts_with("--") && i + 1 < args.len() {
            let key = args[i].trim_start_matches("--").to_string();
            i += 1;
            map.insert(key, args[i].to_string());
        }
        i += 1;
    }
    map
}

fn cmd_query(state: &ReplState, args: &[&str]) -> Result<()> {
    let kv = parse_key_value_args(args);
    let subject_type = kv.get("subject-type").cloned();
    let relation_str = kv.get("relation").cloned();
    let object_type = kv.get("object-type").cloned();
    let limit: u64 = kv.get("limit").and_then(|v| v.parse().ok()).unwrap_or(100);

    let relation = relation_str
        .as_ref()
        .map(|r| Relation::new(r.as_str()))
        .transpose()?;

    let filter = TupleFilter {
        subject_type,
        relation,
        object_type,
        metadata_key: None,
        metadata_value: None,
        ..Default::default()
    };

    let pagination = PaginationParams {
        limit,
        cursor: None,
    };

    let result = state.engine.storage().query_tuples(
        &PartitionId::default(),
        &filter,
        &pagination,
        &ConsistencyMode::MinimizeLatency,
    )?;

    if state.json_mode {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        if result.tuples.is_empty() {
            println!("  {} No matching tuples", yellow("!"));
        } else {
            let has_more = result.next_cursor.is_some();
            println!("  {} {} tuple(s) found", bold(&result.tuples.len().to_string()), if has_more { "(more available)" } else { "" });
            for t in &result.tuples {
                println!("  {:20} {:15} {}", t.subject.as_str(), t.relation.as_str(), t.object.as_str());
            }
            if let Some(cursor) = &result.next_cursor {
                println!("  {} Cursor at offset {}", yellow("!"), cursor.offset);
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

fn cmd_backup(state: &ReplState, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: backup <path>");
        return Ok(());
    }
    let path = args[0];

    let all_tuples = state
        .engine
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

    let events = state.engine.query_audit_all(
        None,
        None,
        &PaginationParams {
            limit: u64::MAX,
            cursor: None,
        },
    )?;

    let revision = state.engine.storage().current_revision(&PartitionId::default())?;
    let backend_type = state.engine.storage().backend_type().to_string();
    let exported_at = chrono::Utc::now().to_rfc3339();

    let mut backup = serde_json::json!({
        "version": 3,
        "schema_yaml": "",
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
    let checksum = hash.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    backup.as_object_mut().unwrap().insert(
        "checksum".to_string(),
        serde_json::Value::String(format!("sha256:{}", checksum)),
    );

    let output = serde_json::to_string_pretty(&backup)?;
    std::fs::write(path, output)
        .with_context(|| format!("failed to write backup to {path}"))?;

    if state.json_mode {
        println!(r#"{{"status":"ok","tuples":{},"events":{},"revision":{}}}"#,
            all_tuples.len(), events.len(), revision.as_u64());
    } else {
        println!("  {} Backup written to {} ({} tuples, {} events, rev={})",
            green("✓"), path, all_tuples.len(), events.len(), revision.as_u64());
    }
    Ok(())
}

fn cmd_restore(state: &ReplState, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: restore <path>");
        return Ok(());
    }
    let path = args[0];
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read backup from {path}"))?;
    let mut backup: serde_json::Value = serde_json::from_str(&content)?;
    let stored_checksum = backup.get("checksum")
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
        let computed = hash.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        if stored_checksum != computed {
            anyhow::bail!("checksum mismatch: backup may be corrupted");
        }
    }
    let version = backup.get("version").and_then(|v| v.as_i64()).unwrap_or(1);

    if version >= 2 {
        if let Some(sy) = backup.get("schema_yaml").and_then(|s| s.as_str()) {
            if !sy.is_empty() {
                let schema = parse_schema(sy)
                    .context("failed to parse schema from backup")?;
                state.engine.reload_schema(schema)?;
            }
        }
    }

    let tuples: Vec<RelationshipTuple> = serde_json::from_value(
        backup.get("tuples").cloned().unwrap_or(serde_json::Value::Null),
    )
    .context("invalid backup format: missing or invalid 'tuples' field")?;
    let events: Vec<AuditEntry> = serde_json::from_value(
        backup.get("events").cloned().unwrap_or(serde_json::Value::Array(vec![])),
    )
    .context("invalid backup format: missing or invalid 'events' field")?;
    let revision = backup
        .get("metadata")
        .and_then(|m| m.get("revision"))
        .and_then(|r| r.as_u64())
        .map(Revision::new)
        .unwrap_or(Revision::ZERO);
    let count = tuples.len();
    state.engine.storage().restore_backup(&PartitionId::default(), &tuples, &events, revision)?;

    if state.json_mode {
        println!(r#"{{"status":"ok","restored":{count}}}"#);
    } else {
        println!("  {} Restored {} tuples from {}", green("✓"), count, path);
    }
    Ok(())
}

fn cmd_import(state: &ReplState, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: import <path>");
        return Ok(());
    }
    let path = args[0];
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read import file {path}"))?;
    let tuples: Vec<TupleImport> = serde_json::from_str(&content)
        .context("invalid import format: expected array of tuples")?;

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
        state.engine.write_batch(&batch)?;
        count += batch.len();
    }

    if state.json_mode {
        println!(r#"{{"status":"ok","imported":{count}}}"#);
    } else {
        println!("  {} Imported {} tuples from {}", green("✓"), count, path);
    }
    Ok(())
}

fn cmd_recover_repl(state: &ReplState, args: &[&str]) -> Result<()> {
    let mut to_revision: Option<u64> = None;
    let mut dry_run = false;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--to-revision" | "--to" => {
                i += 1;
                to_revision = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("missing revision number"))?
                        .parse::<u64>()
                        .with_context(|| "invalid revision number")?,
                );
            }
            "--dry-run" => dry_run = true,
            other => {
                eprintln!("Unknown flag: {other}");
                return Ok(());
            }
        }
        i += 1;
    }

    let to_rev = to_revision.map(|r| Revision::new(r));
    if dry_run {
        let current_rev = state.engine.storage().current_revision(&PartitionId::default())?;
        let target_rev = to_rev.unwrap_or(current_rev);
        if state.json_mode {
            println!(
                "{}",
                serde_json::json!({
                    "status": "dry_run",
                    "current_revision": current_rev.as_u64(),
                    "target_revision": target_rev.as_u64(),
                })
            );
        } else {
            println!("  {} Dry-run: would recover events up to revision {} (current: {})",
                yellow("!"), target_rev.as_u64(), current_rev.as_u64());
        }
    } else {
        let revision = state.engine.recover_from_events(to_rev)?;
        if state.json_mode {
            println!(
                "{}",
                serde_json::json!({
                    "status": "ok",
                    "revision": revision.as_u64(),
                })
            );
        } else {
            println!("  {} Recovered to revision {}", green("✓"), revision.as_u64());
        }
    }
    Ok(())
}

fn cmd_delete_subject_repl(state: &ReplState, args: &[&str]) -> Result<()> {
    if args.len() < 2 {
        eprintln!("Usage: delete-subject <subject> --policy <cascade|fail|transfer> [--transfer-to X]");
        return Ok(());
    }
    let subject = SubjectId::new(args[0])?;
    let mut policy = "cascade";
    let mut transfer_to: Option<SubjectId> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "--policy" => {
                i += 1;
                policy = args.get(i)
                    .ok_or_else(|| anyhow::anyhow!("missing policy value"))?;
            }
            "--transfer-to" => {
                i += 1;
                let subj = args.get(i)
                    .ok_or_else(|| anyhow::anyhow!("missing transfer target subject"))?;
                transfer_to = Some(SubjectId::new(*subj)?);
            }
            other => {
                eprintln!("Unknown flag: {other}");
                return Ok(());
            }
        }
        i += 1;
    }
    let token = state.engine.delete_subject_with_policy(&subject, policy, transfer_to.as_ref())?;
    if state.json_mode {
        println!(
            "{}",
            serde_json::json!({
                "status": "ok",
                "revision": token.revision.as_u64(),
            })
        );
    } else {
        println!("  {} Subject deleted (revision={})", green("✓"), token.revision.as_u64());
    }
    Ok(())
}

fn cmd_export_subject_repl(state: &ReplState, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: export-subject <subject>");
        return Ok(());
    }
    let subject = SubjectId::new(args[0])?;
    let tuples = state.engine.export_subject(&subject)?;
    if state.json_mode {
        println!("{}", serde_json::to_string_pretty(&tuples)?);
    } else {
        if tuples.is_empty() {
            println!("  {} No tuples found for subject", yellow("!"));
        } else {
            for t in &tuples {
                println!("  {} {} {} {}", green("•"), t.subject.as_str(), t.relation.as_str(), t.object.as_str());
            }
            println!("  {} tuple(s)", tuples.len());
        }
    }
    Ok(())
}

fn cmd_policy_draft(state: &mut ReplState, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: policy-draft <subcommand> [args]");
        return Ok(());
    }
    match args[0] {
        "create" => {
            if args.len() < 3 {
                eprintln!("Usage: policy-draft create <name> <description>");
                return Ok(());
            }
            let draft = state.engine.create_policy_draft(args[1], args[2])?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&draft)?);
            } else {
                println!("  {} Created draft {} ({})", green("✓"), draft.name, draft.id);
            }
        }
        "validate" => {
            if args.len() < 2 {
                eprintln!("Usage: policy-draft validate <id>");
                return Ok(());
            }
            let uid = uuid::Uuid::parse_str(args[1])
                .with_context(|| format!("invalid id: {}", args[1]))?;
            let report = state.engine.validate_policy_draft(uid)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("  {} Validation: {}", green("✓"),
                    if report.schema_valid { "valid" } else { "invalid" });
            }
        }
        "diff" => {
            if args.len() < 2 {
                eprintln!("Usage: policy-draft diff <id>");
                return Ok(());
            }
            let uid = uuid::Uuid::parse_str(args[1])
                .with_context(|| format!("invalid id: {}", args[1]))?;
            let drafts = state.engine.list_policy_drafts(None)?;
            let draft = drafts.into_iter()
                .find(|d| d.id == uid)
                .ok_or_else(|| anyhow::anyhow!("draft {} not found", args[1]))?;
            let report = state.engine.access_diff(&*state.engine.schema(), &draft.schema, None, None)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("  {} Diff computed", green("✓"));
            }
        }
        "submit" => {
            if args.len() < 2 {
                eprintln!("Usage: policy-draft submit <id>");
                return Ok(());
            }
            let uid = uuid::Uuid::parse_str(args[1])
                .with_context(|| format!("invalid id: {}", args[1]))?;
            let draft = state.engine.submit_policy_draft_for_review(uid)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&draft)?);
            } else {
                println!("  {} Draft {} submitted for review", green("✓"), draft.id);
            }
        }
        "approve" => {
            if args.len() < 2 {
                eprintln!("Usage: policy-draft approve <id>");
                return Ok(());
            }
            let uid = uuid::Uuid::parse_str(args[1])
                .with_context(|| format!("invalid id: {}", args[1]))?;
            let draft = state.engine.approve_policy_draft(uid)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&draft)?);
            } else {
                println!("  {} Draft {} approved", green("✓"), draft.id);
            }
        }
        "reject" => {
            if args.len() < 3 {
                eprintln!("Usage: policy-draft reject <id> <reason>");
                return Ok(());
            }
            let uid = uuid::Uuid::parse_str(args[1])
                .with_context(|| format!("invalid id: {}", args[1]))?;
            let reason = args[2..].join(" ");
            let draft = state.engine.reject_policy_draft(uid, &reason)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&draft)?);
            } else {
                println!("  {} Draft {} rejected", green("✓"), draft.id);
            }
        }
        "publish" => {
            if args.len() < 2 {
                eprintln!("Usage: policy-draft publish <id>");
                return Ok(());
            }
            let uid = uuid::Uuid::parse_str(args[1])
                .with_context(|| format!("invalid id: {}", args[1]))?;
            let result = state.engine.publish_policy_draft(uid)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("  {} Draft published as policy version {}", green("✓"), result.policy_version);
            }
        }
        "archive" => {
            if args.len() < 2 {
                eprintln!("Usage: policy-draft archive <id>");
                return Ok(());
            }
            let uid = uuid::Uuid::parse_str(args[1])
                .with_context(|| format!("invalid id: {}", args[1]))?;
            let draft = state.engine.archive_policy_draft(uid)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&draft)?);
            } else {
                println!("  {} Draft {} archived", green("✓"), draft.id);
            }
        }
        "list" => {
            let filter = args.get(1).and_then(|s| {
                match s.to_lowercase().as_str() {
                    "drafting" => Some(DraftStatus::Drafting),
                    "under_review" | "underreview" => Some(DraftStatus::UnderReview),
                    "approved" => Some(DraftStatus::Approved),
                    "published" => Some(DraftStatus::Published),
                    "rejected" => Some(DraftStatus::Rejected),
                    "superseded" => Some(DraftStatus::Superseded),
                    "archived" => Some(DraftStatus::Archived),
                    _ => {
                        eprintln!("  {} Invalid status: {s}", yellow("!"));
                        None
                    }
                }
            });
            let drafts = state.engine.list_policy_drafts(filter)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&drafts)?);
            } else {
                if drafts.is_empty() {
                    println!("  {} No drafts found", yellow("!"));
                } else {
                    for d in &drafts {
                        println!("  {} {}  [{}]  {}", green("•"), d.id, d.status, d.name);
                    }
                }
            }
        }
        other => {
            eprintln!("Unknown policy-draft subcommand: {other}");
        }
    }
    Ok(())
}

fn cmd_schedule(state: &mut ReplState, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: schedule <subcommand> [args]");
        return Ok(());
    }
    match args[0] {
        "create" => {
            if args.len() < 2 {
                eprintln!("Usage: schedule create <config_path>");
                return Ok(());
            }
            let json_str = std::fs::read_to_string(args[1])
                .with_context(|| format!("failed to read config: {}", args[1]))?;
            let cfg: AnalysisScheduleConfig = serde_json::from_str(&json_str)?;
            let schedule = state.engine.create_analysis_schedule(&cfg.name, cfg.interval_seconds, cfg.queries, cfg.compare_schema)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&schedule)?);
            } else {
                println!("  {} Created schedule {} ({})", green("✓"), schedule.name, schedule.id);
            }
        }
        "list" => {
            let schedules = state.engine.list_analysis_schedules()?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&schedules)?);
            } else {
                if schedules.is_empty() {
                    println!("  {} No schedules found", yellow("!"));
                } else {
                    for s in &schedules {
                        println!("  {} {}  ({}s interval)", green("•"), s.name, s.interval_seconds);
                    }
                }
            }
        }
        "delete" => {
            if args.len() < 2 {
                eprintln!("Usage: schedule delete <id>");
                return Ok(());
            }
            let uid = uuid::Uuid::parse_str(args[1])?;
            let deleted = state.engine.delete_analysis_schedule(uid)?;
            if state.json_mode {
                println!(r#"{{"deleted":{deleted}}}"#);
            } else {
                println!("  {} {}",
                    if deleted { green("✓") } else { yellow("!") },
                    if deleted { "Schedule deleted" } else { "Schedule not found" });
            }
        }
        "run" => {
            let schedule_id = args.get(1).map(|s| uuid::Uuid::parse_str(s)).transpose()?;
            let runs = state.engine.run_analysis_now(schedule_id)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&runs)?);
            } else {
                println!("  {} {} analysis run(s) completed", green("✓"), runs.len());
            }
        }
        "runs" => {
            let limit: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(100);
            let runs = state.engine.get_analysis_runs(limit)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&runs)?);
            } else {
                if runs.is_empty() {
                    println!("  {} No runs found", yellow("!"));
                } else {
                    for r in &runs {
                        println!("  {} {}  [{}]", green("•"), r.id,
                            if r.status == AnalysisRunStatus::Completed { "completed" } else { "failed" });
                    }
                }
            }
        }
        other => {
            eprintln!("Unknown schedule subcommand: {other}");
        }
    }
    Ok(())
}

fn cmd_enforcement(state: &mut ReplState, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: enforcement <subcommand> [args]");
        return Ok(());
    }
    match args[0] {
        "set" => {
            if args.len() < 2 {
                eprintln!("Usage: enforcement set <config_path>");
                return Ok(());
            }
            let json_str = std::fs::read_to_string(args[1])
                .with_context(|| format!("failed to read config: {}", args[1]))?;
            let config: EnforcementHistoryConfig = serde_json::from_str(&json_str)?;
            state.engine.set_enforcement_history_config(config)?;
            if state.json_mode {
                println!(r#"{{"status":"ok"}}"#);
            } else {
                println!("  {} Enforcement config updated", green("✓"));
            }
        }
        "get" => {
            let config = state.engine.get_enforcement_history_config()?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&config)?);
            } else {
                println!("  {} Enabled: {}", bold("Enforcement"), config.enabled);
                println!("  {}: {:?}", bold("Sampling"), config.sampling);
                println!("  {}: {}", bold("Max events/min"), config.max_events_per_minute);
                println!("  {}: {}", bold("Max rows"), config.max_rows);
                println!("  {}: {} days", bold("Max age"), config.max_days);
            }
        }
        "trends" => {
            let limit: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(100);
            let trends = state.engine.enforcement_trends(limit)?;
            if state.json_mode {
                println!("{}", serde_json::to_string_pretty(&trends)?);
            } else {
                println!("  {} Total events: {}", bold("Trends"), trends.total_events);
                println!("  {} Allowed: {}", green("✓"), trends.allowed_count);
                println!("  {} Denied: {}", red("✗"), trends.denied_count);
                println!("  {} By resource:", bold("Top"));
                for (i, (res, count)) in trends.by_resource.iter().take(5).enumerate() {
                    println!("    {}. {} ({})", i + 1, res, count);
                }
            }
        }
        other => {
            eprintln!("Unknown enforcement subcommand: {other}");
        }
    }
    Ok(())
}

fn cmd_subscribe(state: &mut ReplState, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        eprintln!("Usage: subscribe <event_types>");
        eprintln!("  event_types: comma-separated, e.g. TupleAdded,TupleRemoved");
        return Ok(());
    }
    let types: Vec<WatchEventType> = args[0]
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
    let sub = state.engine.subscribe(types);
    state.watch_sub = Some(sub);
    if state.json_mode {
        println!(r#"{{"status":"subscribed"}}"#);
    } else {
        println!("  {} Subscribed to events. Type 'unwatch' to stop.", green("✓"));
    }
    Ok(())
}




