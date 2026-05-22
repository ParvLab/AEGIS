use crate::storage::StorageBackend;
use crate::types::{Relation, ResourceId, Revision, SubjectId};
use crate::error::{AegisError, AegisResult};
use std::collections::{HashSet, VecDeque};

/// A single step in a traversal trace.
#[derive(Debug, Clone)]
pub struct TraceStep {
    pub subject: String,
    pub relation: String,
    pub object: String,
    pub depth: usize,
}

/// BFS traversal result.
#[derive(Debug, Clone)]
pub struct TraversalResult {
    pub found: bool,
    pub path_len: usize,
    pub trace: Vec<TraceStep>,
    pub revision: Revision,
}

/// Perform a BFS traversal from `subject` to see if they reach `object` through `relation`.
///
/// Uses:
/// - BFS queue for level-order traversal
/// - `HashSet<(SubjectId, Relation)>` for cycle detection (revisit same subject+relation)
/// - `StorageBackend::list_by_subject()` to find edges from each node
///
/// The traversal follows the pattern:
///   subject S has relation R on object O
///   → O might be a subject in another tuple
///   → look for tuples where O is the subject
///   → continue until we find the target object
pub fn bfs_traversal(
    storage: &dyn StorageBackend,
    subject: &SubjectId,
    relation: &Relation,
    target: &ResourceId,
    revision: Option<Revision>,
) -> AegisResult<TraversalResult> {
    // visited: (SubjectId, Relation) to detect cycles
    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut queue: VecDeque<(SubjectId, Vec<TraceStep>)> = VecDeque::new();

    // Direct check: does subject have relation on target?
    let found_direct = check_direct(storage, subject, relation, target)?;
    if found_direct {
        return Ok(TraversalResult {
            found: true,
            path_len: 0,
            trace: Vec::new(),
            revision: revision.unwrap_or(Revision::ZERO),
        });
    }

    queue.push_back((subject.clone(), Vec::new()));
    visited.insert((subject.to_string(), relation.to_string()));

    while let Some((current_subject, path)) = queue.pop_front() {
        // Get all tuples where current_subject is the subject with this relation
        // This gives us all objects that current_subject has this relation on
        let tuples = match storage.list_by_subject(&current_subject, Some(relation)) {
            Ok(t) => t,
            Err(e) => {
                // If storage not initialized, skip
                if matches!(e, AegisError::StorageNotInitialized) {
                    continue;
                }
                return Err(e);
            }
        };

        for tuple in &tuples {
            let object_str = tuple.object.as_str().to_string();

            // Check if this object is the target
            if tuple.object == *target {
                let mut full_path = path.clone();
                full_path.push(TraceStep {
                    subject: current_subject.to_string(),
                    relation: relation.to_string(),
                    object: object_str.clone(),
                    depth: path.len(),
                });
                return Ok(TraversalResult {
                    found: true,
                    path_len: full_path.len(),
                    trace: full_path,
                    revision: revision.unwrap_or(Revision::ZERO),
                });
            }

            // Otherwise, try to traverse through this object as a subject
            // But first check for cycles
            let object_as_subject = match SubjectId::new(&object_str) {
                Ok(s) => s,
                Err(_) => continue, // can't traverse through non-subject IDs
            };

            let visit_key = (object_as_subject.to_string(), relation.to_string());
            if visited.contains(&visit_key) {
                continue;
            }
            visited.insert(visit_key);

            let mut new_path = path.clone();
            new_path.push(TraceStep {
                subject: current_subject.to_string(),
                relation: relation.to_string(),
                object: object_str,
                depth: path.len(),
            });

            queue.push_back((object_as_subject, new_path));
        }
    }

    // Not found after BFS
    Ok(TraversalResult {
        found: false,
        path_len: 0,
        trace: Vec::new(),
        revision: revision.unwrap_or(Revision::ZERO),
    })
}

/// Direct check: does subject have the given relation on the target resource?
fn check_direct(
    storage: &dyn StorageBackend,
    subject: &SubjectId,
    relation: &Relation,
    target: &ResourceId,
) -> AegisResult<bool> {
    let tuples = storage.list_by_object(target, Some(relation))?;
    Ok(tuples.iter().any(|t| t.subject == *subject))
}

/// Collect all objects that a subject reaches through a given relation.
/// Returns all distinct objects found via BFS traversal.
pub fn collect_reachable(
    storage: &dyn StorageBackend,
    subject: &SubjectId,
    relation: &Relation,
) -> AegisResult<Vec<ResourceId>> {
    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut queue: VecDeque<SubjectId> = VecDeque::new();
    let mut results: Vec<ResourceId> = Vec::new();

    queue.push_back(subject.clone());

    while let Some(current) = queue.pop_front() {
        let tuples = storage.list_by_subject(&current, Some(relation))?;

        for tuple in &tuples {
            if !results.contains(&tuple.object) {
                results.push(tuple.object.clone());
            }

            if let Ok(next) = SubjectId::new(tuple.object.as_str()) {
                let key = (next.to_string(), relation.to_string());
                if visited.insert(key) {
                    queue.push_back(next);
                }
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use crate::testing::TestAegis;
    use crate::types::*;

    #[test]
    fn test_direct_traversal_found() {
        let mut aegis = TestAegis::new();
        aegis
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("repo:fluxbus").unwrap(),
            ))
            .unwrap();

        let tuples = aegis.list_by_object(&ResourceId::new("repo:fluxbus").unwrap(), None);
        assert_eq!(tuples.len(), 1);
    }
}
