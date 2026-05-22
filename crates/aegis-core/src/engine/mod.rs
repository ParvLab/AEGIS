pub mod cache;
pub mod migration;
pub mod policy;
pub mod traversal;

use crate::engine::cache::DecisionCache;
use crate::error::AegisResult;
use crate::storage::StorageBackend;
use crate::types::{
    CheckResult, ConsistencyMode, ExplainResult, ExplainTrace, Relation, ResourceId, Revision,
    RevisionToken, Schema, SubjectId,
};
use std::sync::Mutex;

/// The core authorization engine.
///
/// Combines a `StorageBackend` for tuple data with a `Schema` for policy definitions.
/// Provides the primary `check()` and `explain()` APIs.
pub struct GraphEngine {
    storage: Box<dyn StorageBackend>,
    schema: Schema,
    cache: Mutex<DecisionCache>,
    node_id: uuid::Uuid,
}

impl GraphEngine {
    /// Create a new graph engine with the given storage and schema.
    pub fn new(storage: Box<dyn StorageBackend>, schema: Schema) -> Self {
        Self {
            storage,
            schema,
            cache: Mutex::new(DecisionCache::new(10_000)),
            node_id: uuid::Uuid::new_v4(),
        }
    }

    /// Set a custom cache capacity.
    pub fn with_cache_capacity(mut self, capacity: usize) -> Self {
        self.cache = Mutex::new(DecisionCache::new(capacity));
        self
    }

    /// Access the underlying storage backend.
    pub fn storage(&self) -> &dyn StorageBackend {
        self.storage.as_ref()
    }

    /// Access the schema.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Check whether a subject has a permission on a resource.
    ///
    /// Returns `CheckResult { allowed: bool, revision: Revision }`.
    pub fn check(
        &self,
        subject: &SubjectId,
        permission: &str,
        resource: &ResourceId,
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<CheckResult> {
        let revision = self.resolve_revision(consistency)?;

        // Check cache first
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(allowed) = cache.get(subject.as_str(), permission, resource.as_str(), revision) {
                return Ok(CheckResult { allowed, revision });
            }
        }

        // Resolve permission to relations
        let resource_type = resource_type_name(resource.as_str());
        let resolved = match policy::resolve_permission(&self.schema, &resource_type, permission) {
            Some(r) => r,
            None => {
                return Ok(CheckResult {
                    allowed: false,
                    revision,
                });
            }
        };

        // Try each relation - any match means allowed (union semantics)
        let mut allowed = false;
        for rel_name in &resolved.relations {
            let relation = match Relation::new(rel_name) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let result = traversal::bfs_traversal(
                self.storage.as_ref(),
                subject,
                &relation,
                resource,
                Some(revision),
            )?;

            if result.found {
                allowed = true;
                break;
            }
        }

        // Cache the decision
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(subject.as_str(), permission, resource.as_str(), allowed, revision);
        }

        Ok(CheckResult { allowed, revision })
    }

    /// Explain why a check returned its result, including the trace path.
    pub fn explain(
        &self,
        subject: &SubjectId,
        permission: &str,
        resource: &ResourceId,
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<ExplainResult> {
        let revision = self.resolve_revision(consistency)?;

        let resource_type = resource_type_name(resource.as_str());
        let resolved = match policy::resolve_permission(&self.schema, &resource_type, permission) {
            Some(r) => r,
            None => {
                return Ok(ExplainResult {
                    allowed: false,
                    revision,
                    trace: Vec::new(),
                    resolved_via: String::new(),
                    duration_ms: 0,
                });
            }
        };

        let start = std::time::Instant::now();

        let mut all_traces = Vec::new();
        let mut allowed = false;

        for rel_name in &resolved.relations {
            let relation = match Relation::new(rel_name) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let result = traversal::bfs_traversal(
                self.storage.as_ref(),
                subject,
                &relation,
                resource,
                Some(revision),
            )?;

            if result.found {
                allowed = true;
                let trace: Vec<ExplainTrace> = result
                    .trace
                    .iter()
                    .map(|s| ExplainTrace {
                        subject: s.subject.clone(),
                        relation: s.relation.clone(),
                        object: s.object.clone(),
                    })
                    .collect();
                all_traces = trace;
                break;
            }
        }

        let duration_ms = start.elapsed().as_micros() as u64 / 1000;

        let resolved_via = if allowed && !all_traces.is_empty() {
            let steps: Vec<String> = all_traces
                .iter()
                .map(|t| format!("{}#{}", t.subject, t.relation))
                .collect();
            format!("→ {}", steps.join(" → "))
        } else if allowed {
            format!("direct relation '{}'", permission)
        } else {
            "no path found".to_string()
        };

        Ok(ExplainResult {
            allowed,
            revision,
            trace: all_traces,
            resolved_via,
            duration_ms,
        })
    }

    /// Write a relationship tuple and return a revision token.
    pub fn write(&self, tuple: &crate::types::RelationshipTuple) -> AegisResult<RevisionToken> {
        let revision = self.storage.write_tuple(tuple)?;
        Ok(RevisionToken::new(revision, self.node_id))
    }

    /// Delete a tuple by key.
    pub fn delete(&self, key: &crate::types::TupleKey) -> AegisResult<RevisionToken> {
        let revision = self.storage.delete_tuple(key)?;
        Ok(RevisionToken::new(revision, self.node_id))
    }

    /// Invalidate the decision cache.
    pub fn invalidate_cache(&self) {
        self.cache.lock().unwrap().clear();
    }

    /// Invalidate cache entries older than a revision.
    pub fn invalidate_cache_before(&self, revision: Revision) {
        self.cache.lock().unwrap().invalidate_before(revision);
    }

    /// Resolve the revision to use for a check operation.
    fn resolve_revision(&self, consistency: Option<ConsistencyMode>) -> AegisResult<Revision> {
        match consistency {
            Some(ConsistencyMode::AtRevision(rev)) => {
                let current = self.storage.current_revision()?;
                if rev > current {
                    return Err(crate::error::AegisError::RevisionFromFuture(
                        rev.as_u64() as usize,
                    ));
                }
                Ok(rev)
            }
            _ => self.storage.current_revision(),
        }
    }
}

/// Extract the type name from a resource ID (e.g., "repo:fluxbus" -> "repo").
fn resource_type_name(id: &str) -> String {
    id.split(':').next().unwrap_or(id).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::types::*;

    fn make_engine() -> GraphEngine {
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types: {
                let mut types = std::collections::HashMap::new();
                let mut relations = std::collections::HashMap::new();
                relations.insert(
                    "owner".to_string(),
                    crate::types::schema::RelationDef {
                        inherit_from: vec![],
                        description: None,
                    },
                );
                relations.insert(
                    "viewer".to_string(),
                    crate::types::schema::RelationDef {
                        inherit_from: vec![],
                        description: None,
                    },
                );
                let mut permissions = std::collections::HashMap::new();
                permissions.insert(
                    "read".to_string(),
                    crate::types::schema::PermissionDef {
                        union_of: vec!["viewer".to_string(), "owner".to_string()],
                        condition: None,
                        description: None,
                    },
                );
                permissions.insert(
                    "admin".to_string(),
                    crate::types::schema::PermissionDef {
                        union_of: vec!["owner".to_string()],
                        condition: None,
                        description: None,
                    },
                );
                types.insert(
                    "repo".to_string(),
                    crate::types::schema::TypeDef {
                        relations,
                        permissions,
                    },
                );
                types
            },
        };

        let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
        storage.initialize().unwrap();
        GraphEngine::new(Box::new(storage), schema)
    }

    #[test]
    fn test_check_direct_allowed() {
        let engine = make_engine();
        let subject = SubjectId::new("user:alice").unwrap();
        let resource = ResourceId::new("repo:fluxbus").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let result = engine
            .check(&subject, "read", &resource, None)
            .unwrap();
        assert!(result.allowed);
        assert!(result.revision.as_u64() > 0);
    }

    #[test]
    fn test_check_denied() {
        let engine = make_engine();
        let result = engine
            .check(
                &SubjectId::new("user:alice").unwrap(),
                "read",
                &ResourceId::new("repo:fluxbus").unwrap(),
                None,
            )
            .unwrap();
        assert!(!result.allowed);
    }

    #[test]
    fn test_check_admin_permission() {
        let engine = make_engine();
        let subject = SubjectId::new("user:admin").unwrap();
        let resource = ResourceId::new("repo:critical").unwrap();

        // Admin has owner, so admin permission should be allowed
        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let result = engine
            .check(&subject, "admin", &resource, None)
            .unwrap();
        assert!(result.allowed);

        // viewer should NOT have admin
        let viewer = SubjectId::new("user:viewer").unwrap();
        engine
            .write(&RelationshipTuple::new(
                viewer.clone(),
                Relation::new("viewer").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let result = engine
            .check(&viewer, "admin", &resource, None)
            .unwrap();
        assert!(!result.allowed);
    }

    #[test]
    fn test_explain_returns_trace() {
        let engine = make_engine();
        let subject = SubjectId::new("user:alice").unwrap();
        let resource = ResourceId::new("repo:fluxbus").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let explain = engine
            .explain(&subject, "read", &resource, None)
            .unwrap();
        assert!(explain.allowed);
        assert!(explain.revision.as_u64() > 0);
    }

    #[test]
    fn test_check_unknown_permission_denies() {
        let engine = make_engine();
        let result = engine
            .check(
                &SubjectId::new("user:alice").unwrap(),
                "nonexistent",
                &ResourceId::new("repo:fluxbus").unwrap(),
                None,
            )
            .unwrap();
        assert!(!result.allowed);
    }

    #[test]
    fn test_cache_invalidation() {
        let engine = make_engine();
        let subject = SubjectId::new("user:cached").unwrap();
        let resource = ResourceId::new("repo:cached").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        // First check populates cache
        let result = engine
            .check(&subject, "read", &resource, None)
            .unwrap();
        assert!(result.allowed);

        // Invalidate and verify still works (cache miss is fine)
        engine.invalidate_cache();
        let result = engine
            .check(&subject, "read", &resource, None)
            .unwrap();
        assert!(result.allowed);
    }

    #[test]
    fn test_resource_type_extraction() {
        assert_eq!(resource_type_name("repo:fluxbus"), "repo");
        assert_eq!(resource_type_name("workspace:acme"), "workspace");
        assert_eq!(resource_type_name("nocolon"), "nocolon");
    }
}
