use crate::engine::GraphEngine;
use crate::error::AegisResult;
use crate::types::Schema;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Tracks schema file state for hot-reload detection.
pub struct SchemaWatcher {
    schema_path: String,
    last_modified: Mutex<SystemTime>,
    last_checksum: Mutex<String>,
    changed: Arc<AtomicBool>,
    _file_watcher: Option<RecommendedWatcher>,
}

impl SchemaWatcher {
    /// Create a new schema watcher for a given file path.
    pub fn new(schema_path: &str) -> Self {
        let now = SystemTime::UNIX_EPOCH;
        let changed = Arc::new(AtomicBool::new(false));
        let file_watcher = Self::create_file_watcher(Path::new(schema_path), Arc::clone(&changed));
        Self {
            schema_path: schema_path.to_string(),
            last_modified: Mutex::new(now),
            last_checksum: Mutex::new(String::new()),
            changed,
            _file_watcher: file_watcher,
        }
    }

    fn create_file_watcher(schema_path: &Path, changed: Arc<AtomicBool>) -> Option<RecommendedWatcher> {
        let schema_dir = schema_path.parent()?;
        let schema_file_name = schema_path.file_name()?.to_string_lossy().to_string();
        let changed_inner = changed.clone();
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                        if event.paths.iter().any(|p| {
                            p.file_name().and_then(|n| n.to_str()) == Some(&schema_file_name)
                        }) {
                            changed_inner.store(true, Ordering::Relaxed);
                        }
                    }
                }
            },
            Config::default(),
        )
        .ok()?;
        watcher.watch(schema_dir, RecursiveMode::NonRecursive).ok()?;
        Some(watcher)
    }

    /// Check if the schema file has changed since the last reload.
    /// If changed, parse and validate the new schema, and if compatible, apply it.
    ///
    /// Returns `true` if a reload occurred.
    pub fn check_and_reload(&self, engine: &GraphEngine) -> AegisResult<bool> {
        let path = Path::new(&self.schema_path);
        if !path.exists() {
            return Ok(false);
        }

        let changed_by_watcher = self.changed.swap(false, Ordering::Relaxed);

        let modified = path
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let checksum = compute_file_checksum(path);

        if !changed_by_watcher {
            let last_modified = self.last_modified.lock().unwrap();
            let last_checksum = self.last_checksum.lock().unwrap();
            if modified <= *last_modified && checksum == *last_checksum {
                return Ok(false);
            }
        }

        let yaml_content = std::fs::read_to_string(path)
            .map_err(|e| crate::error::AegisError::SchemaNotFound(e.to_string()))?;

        let new_schema: Schema = crate::schema::parse_schema(&yaml_content)?;

        {
            let existing = engine.schema();
            let report = crate::engine::migration::check_compatibility(&existing, &new_schema);
            if !report.compatible {
                return Err(crate::error::AegisError::SchemaValidation(
                    format!("incompatible schema change: {}", report.breaking.join(", ")),
                ));
            }
        }

        engine.reload_schema(new_schema)?;

        {
            let mut last_modified = self.last_modified.lock().unwrap();
            let mut last_checksum = self.last_checksum.lock().unwrap();
            *last_modified = modified;
            *last_checksum = checksum;
        }

        self.changed.store(false, Ordering::Relaxed);

        Ok(true)
    }
}

fn compute_file_checksum(path: &Path) -> String {
    use sha2::Digest;
    if let Ok(data) = std::fs::read(path) {
        let mut hasher = sha2::Sha256::new();
        hasher.update(&data);
        format!("{:x}", hasher.finalize())
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::types::*;
    use std::io::Write;

    #[test]
    fn test_schema_watcher_no_file() {
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types: std::collections::HashMap::new(),
        };
        let storage = Box::new(SqliteStorage::new(SqliteConfig::in_memory()).unwrap());
        let engine = GraphEngine::new(storage, schema);
        let watcher = SchemaWatcher::new("/nonexistent/schema.yaml");
        let result = watcher.check_and_reload(&engine).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_schema_watcher_new_file() {
        let mut tmpfile = std::env::temp_dir();
        tmpfile.push(format!("aegis_schema_test_{}", std::process::id()));

        let schema_yaml = r#"
schemaVersion: 1
namespace: test
types:
  repo:
    relations:
      owner: {}
      viewer: {}
    permissions:
      read:
        union_of: [viewer, owner]
"#;

        let mut f = std::fs::File::create(&tmpfile).unwrap();
        f.write_all(schema_yaml.as_bytes()).unwrap();
        f.flush().unwrap();
        drop(f);

        let schema = crate::schema::parse_schema(schema_yaml).unwrap();
        let storage = Box::new(SqliteStorage::new(SqliteConfig::in_memory()).unwrap());
        let engine = GraphEngine::new(storage, schema);
        let watcher = SchemaWatcher::new(tmpfile.to_str().unwrap());

        let result = watcher.check_and_reload(&engine).unwrap();
        assert!(result);

        std::fs::remove_file(&tmpfile).ok();
    }
}
