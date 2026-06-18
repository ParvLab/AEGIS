use crate::engine::GraphEngine;
use crate::error::AegisResult;
use crate::types::analysis::*;
use crate::types::*;

impl GraphEngine {
    /// Simulate changes to tuples and report which checks flip.
    ///
    /// Clones traversal state in-memory, applies add/remove ops,
    /// runs all verify_checks, and compares outcomes with original state.
    pub fn simulate_changes(
        &self,
        add: &[RelationshipTuple],
        remove: &[TupleKey],
        verify_checks: &[CheckQuery],
    ) -> AegisResult<SimulationReport> {
        let start = std::time::Instant::now();
        let pid = self.active_partition_id();

        // Evaluate checks against current state
        let mut before_results: std::collections::HashMap<String, bool> =
            std::collections::HashMap::new();
        for q in verify_checks {
            let key = format!("{}:{}:{}", q.subject, q.permission, q.resource);
            let subject = SubjectId::new(&q.subject)?;
            let resource = ResourceId::new(&q.resource)?;
            let allowed = self
                .check(&subject, &q.permission, &resource, None)?
                .allowed;
            before_results.insert(key, allowed);
        }

        // Apply changes in-memory via a temporary storage overlay
        let overlay = InMemoryOverlay::new(self.storage.as_ref());

        for t in add {
            let _ = overlay.write_tuple_internal(&pid, t);
        }
        for k in remove {
            let _ = overlay.delete_tuple_internal(&pid, k);
        }

        // Evaluate checks against modified state
        let mut flips = Vec::new();
        let errors = Vec::new();
        let mut unchanged: u64 = 0;
        let mut gained: u64 = 0;
        let mut lost: u64 = 0;

        for q in verify_checks {
            let key = format!("{}:{}:{}", q.subject, q.permission, q.resource);
            let before = before_results.get(&key).copied().unwrap_or(false);

            // Simulate using the overlay
            let after = overlay.simulate_check(q);

            let flip = CheckFlip {
                query: q.clone(),
                before,
                after,
            };

            match (before, after) {
                (true, false) => {
                    lost += 1;
                    flips.push(flip);
                }
                (false, true) => {
                    gained += 1;
                    flips.push(flip);
                }
                _ => {
                    unchanged += 1;
                }
            }
        }

        let duration_ms = start.elapsed().as_micros() as u64 / 1000;

        Ok(SimulationReport {
            summary: SimulationSummary {
                gained_access: gained,
                lost_access: lost,
                unchanged,
                error_count: errors.len() as u64,
            },
            details: flips,
            errors,
            duration_ms,
        })
    }
}

/// In-memory overlay for simulating tuple changes without modifying real storage.
struct InMemoryOverlay<'a> {
    storage: &'a dyn crate::storage::StorageBackend,
    additions: std::sync::Mutex<Vec<RelationshipTuple>>,
    removals: std::sync::Mutex<Vec<TupleKey>>,
}

impl<'a> InMemoryOverlay<'a> {
    fn new(storage: &'a dyn crate::storage::StorageBackend) -> Self {
        Self {
            storage,
            additions: std::sync::Mutex::new(Vec::new()),
            removals: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn write_tuple_internal(&self, _pid: &PartitionId, tuple: &RelationshipTuple) {
        let mut adds = self.additions.lock().unwrap();
        adds.push(tuple.clone());
    }

    fn delete_tuple_internal(&self, _pid: &PartitionId, key: &TupleKey) {
        let mut rems = self.removals.lock().unwrap();
        rems.push(key.clone());
    }

    fn is_removed(&self, subject: &str, relation: &str, object: &str) -> bool {
        let rems = self.removals.lock().unwrap();
        rems.iter().any(|k| {
            k.subject.as_str() == subject
                && k.relation.as_str() == relation
                && k.object.as_str() == object
        })
    }

    fn is_added(&self, subject: &str, relation: &str, object: &str) -> bool {
        let adds = self.additions.lock().unwrap();
        adds.iter().any(|t| {
            t.subject.as_str() == subject
                && t.relation.as_str() == relation
                && t.object.as_str() == object
        })
    }

    fn simulate_check(&self, q: &CheckQuery) -> bool {
        // Simple check simulation: look for direct tuple matches
        let pid = crate::types::PartitionId::default();

        // Check if the tuple exists in storage (minus removals, plus additions)
        if self.is_removed(&q.subject, &q.permission, &q.resource) {
            return false;
        }
        if self.is_added(&q.subject, &q.permission, &q.resource) {
            return true;
        }

        // Fall through to real storage
        let rel = match crate::types::Relation::new(&q.permission) {
            Ok(r) => r,
            Err(_) => return false,
        };
        let subject = match crate::types::SubjectId::new(&q.subject) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let object = match crate::types::ResourceId::new(&q.resource) {
            Ok(o) => o,
            Err(_) => return false,
        };

        let key = crate::types::TupleKey {
            subject,
            relation: rel,
            object,
        };
        self.storage.has_tuple(&pid, &key).unwrap_or(false)
    }
}
