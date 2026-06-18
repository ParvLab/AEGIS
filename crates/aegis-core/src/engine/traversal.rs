use crate::engine::cache::TraversalCache;
use crate::engine::condition::{self, ConditionEvalContext};
use crate::error::{AegisError, AegisResult};
use crate::storage::StorageBackend;
use crate::types::{
    ConsistencyMode, PartitionId, Relation, ResourceId, Revision, SubjectId, SubjectSet,
};
use chrono::Utc;
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

/// Default maximum traversal depth.
pub const DEFAULT_MAX_DEPTH: usize = 10;
/// Default maximum number of tuples to visit.
pub const DEFAULT_MAX_VISITS: usize = 10_000;

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
    partition_id: &PartitionId,
    storage: &dyn StorageBackend,
    subject: &SubjectId,
    relation: &Relation,
    target: &ResourceId,
    revision: Option<Revision>,
    consistency: Option<ConsistencyMode>,
) -> AegisResult<TraversalResult> {
    bfs_traversal_with_limits_and_context(
        partition_id,
        storage,
        subject,
        relation,
        target,
        revision,
        consistency,
        DEFAULT_MAX_DEPTH,
        DEFAULT_MAX_VISITS,
        None,
        None,
        None,
    )
}

/// BFS traversal with context for tuple condition evaluation.
#[allow(clippy::too_many_arguments)]
pub fn bfs_traversal_with_context(
    partition_id: &PartitionId,
    storage: &dyn StorageBackend,
    subject: &SubjectId,
    relation: &Relation,
    target: &ResourceId,
    revision: Option<Revision>,
    consistency: Option<ConsistencyMode>,
    context: Option<&ConditionEvalContext>,
) -> AegisResult<TraversalResult> {
    bfs_traversal_with_limits_and_context(
        partition_id,
        storage,
        subject,
        relation,
        target,
        revision,
        consistency,
        DEFAULT_MAX_DEPTH,
        DEFAULT_MAX_VISITS,
        None,
        context,
        None,
    )
}

/// BFS traversal with configurable depth and visit limits.
#[allow(clippy::too_many_arguments)]
pub fn bfs_traversal_with_limits(
    partition_id: &PartitionId,
    storage: &dyn StorageBackend,
    subject: &SubjectId,
    relation: &Relation,
    target: &ResourceId,
    revision: Option<Revision>,
    consistency: Option<ConsistencyMode>,
    max_depth: usize,
    max_visits: usize,
    cache: Option<&mut TraversalCache>,
) -> AegisResult<TraversalResult> {
    bfs_traversal_with_limits_and_context(
        partition_id,
        storage,
        subject,
        relation,
        target,
        revision,
        consistency,
        max_depth,
        max_visits,
        cache,
        None,
        None,
    )
}

/// BFS traversal with limits, condition context, and per-branch visit limiting.
///
/// `per_branch_max_visits` limits how many tuples each branch may explore before
/// being pruned. This prevents a single deep branch from dominating the traversal budget.
#[allow(clippy::too_many_arguments)]
pub fn bfs_traversal_with_limits_and_context(
    partition_id: &PartitionId,
    storage: &dyn StorageBackend,
    subject: &SubjectId,
    relation: &Relation,
    target: &ResourceId,
    revision: Option<Revision>,
    consistency: Option<ConsistencyMode>,
    max_depth: usize,
    max_visits: usize,
    mut cache: Option<&mut TraversalCache>,
    context: Option<&ConditionEvalContext>,
    per_branch_max_visits: Option<usize>,
) -> AegisResult<TraversalResult> {
    let consistency_ref = consistency
        .as_ref()
        .unwrap_or(&ConsistencyMode::MinimizeLatency);

    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut queue: VecDeque<(SubjectId, Vec<TraceStep>)> = VecDeque::new();
    let mut visit_count = 0usize;
    let mut per_branch_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let per_branch_limit = per_branch_max_visits.unwrap_or(usize::MAX);

    let found_direct = check_direct(
        partition_id,
        storage,
        subject,
        relation,
        target,
        consistency_ref,
        context,
    )?;
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
        if path.len() >= max_depth {
            continue;
        }

        let tuples = {
            let current_rev = revision.unwrap_or(Revision::ZERO);
            if let Some(ref mut c) = cache {
                if let Some(cached_objects) =
                    c.get(current_subject.as_str(), relation.as_str(), current_rev)
                {
                    let mut result = Vec::new();
                    for obj_str in &cached_objects {
                        if let Ok(obj) = ResourceId::new(obj_str) {
                            result.push(crate::types::RelationshipTuple::new(
                                current_subject.clone(),
                                relation.clone(),
                                obj,
                            ));
                        }
                    }
                    result
                } else {
                    let tuples = load_tuples(
                        partition_id,
                        storage,
                        &current_subject,
                        relation,
                        consistency_ref,
                    )?;
                    let objects: Vec<String> = tuples
                        .iter()
                        .map(|t| t.object.as_str().to_string())
                        .collect();
                    if !objects.is_empty() {
                        c.insert(
                            current_subject.as_str(),
                            relation.as_str(),
                            objects,
                            current_rev,
                        );
                    }
                    tuples
                }
            } else {
                load_tuples(
                    partition_id,
                    storage,
                    &current_subject,
                    relation,
                    consistency_ref,
                )?
            }
        };

        for tuple in &tuples {
            // Skip expired tuples (valid_until in the past)
            if tuple.valid_until.is_some_and(|v| v <= Utc::now()) {
                continue;
            }

            // Skip tuples whose condition is not met
            if !evaluate_tuple_condition(&tuple.condition, context) {
                continue;
            }

            // Per-branch visit limiting: limit edges followed from each node
            let origin_key = current_subject.to_string();
            let branch_count = per_branch_counts.entry(origin_key).or_insert(0);
            *branch_count += 1;
            if *branch_count > per_branch_limit {
                continue;
            }

            visit_count += 1;
            if visit_count > max_visits {
                return Ok(TraversalResult {
                    found: false,
                    path_len: 0,
                    trace: Vec::new(),
                    revision: revision.unwrap_or(Revision::ZERO),
                });
            }

            // Subject-set resolution: if the tuple's subject is a subject-set
            // (e.g. "team:eng#member"), we need to verify that our original
            // traversal subject satisfies the subject-set condition.
            #[allow(clippy::collapsible_if)]
            if let Some(ref subject_set) = tuple.subject.as_subject_set() {
                if !is_subject_set_member(
                    partition_id,
                    storage,
                    subject,
                    subject_set,
                    consistency_ref,
                    context,
                )? {
                    continue;
                }
            }
            // For subject-set tuples, the edge still goes from current_subject
            // (which equals subject_set.object) to tuple.object via tuple.relation.
            // We continue processing normally below.

            let object_str = tuple.object.as_str().to_string();

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

            let object_as_subject = match SubjectId::new(&object_str) {
                Ok(s) => s,
                Err(_) => continue,
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

    Ok(TraversalResult {
        found: false,
        path_len: 0,
        trace: Vec::new(),
        revision: revision.unwrap_or(Revision::ZERO),
    })
}

/// Load tuples from the current subject, including both direct subject matches
/// AND subject-set tuples where the subject-set's object matches the current subject.
/// Subject-set tuples are those whose subject field is of the form `{object}#{relation}`
/// (e.g. `team:eng#member`).
fn load_tuples(
    partition_id: &PartitionId,
    storage: &dyn StorageBackend,
    current_subject: &SubjectId,
    relation: &Relation,
    consistency: &ConsistencyMode,
) -> AegisResult<Vec<crate::types::RelationshipTuple>> {
    // 1. Direct subject match (existing behavior)
    let mut tuples =
        match storage.list_by_subject(partition_id, current_subject, Some(relation), consistency) {
            Ok(t) => t,
            Err(e) => {
                if matches!(e, AegisError::StorageNotInitialized) {
                    Vec::new()
                } else {
                    return Err(e);
                }
            }
        };

    // 2. Subject-set match: find tuples where subject is `{current_subject}#{relation}`
    //    e.g. if current_subject = team:eng, find tuples with subject = team:eng#member
    //    If current_subject is itself a subject-set, extract its object.
    let set_object = if let Some(ss) = current_subject.as_subject_set() {
        ss.object
    } else if let Ok(rid) = ResourceId::new(current_subject.as_str()) {
        rid
    } else {
        return Ok(tuples);
    };
    let set_tuples =
        storage.list_by_subject_set_of(partition_id, &set_object, Some(relation), consistency);
    if let Ok(mut st) = set_tuples {
        tuples.append(&mut st);
    }

    Ok(tuples)
}

/// Direct check: does subject have the given relation on the target resource?
/// Handles both direct subject match AND subject-set resolution.
fn check_direct(
    partition_id: &PartitionId,
    storage: &dyn StorageBackend,
    subject: &SubjectId,
    relation: &Relation,
    target: &ResourceId,
    consistency: &ConsistencyMode,
    context: Option<&ConditionEvalContext>,
) -> AegisResult<bool> {
    let tuples = storage.list_by_object(partition_id, target, Some(relation), consistency)?;
    let now = Utc::now();
    for t in &tuples {
        // Skip expired tuples
        if t.valid_until.is_some_and(|v| v <= now) {
            continue;
        }
        // Skip tuples whose condition is not met
        if !evaluate_tuple_condition(&t.condition, context) {
            continue;
        }
        // Direct subject match
        if t.subject == *subject {
            return Ok(true);
        }
        // Subject-set match: subject is like `team:eng#member`
        #[allow(clippy::collapsible_if)]
        if let Some(ref subject_set) = t.subject.as_subject_set() {
            if is_subject_set_member(
                partition_id,
                storage,
                subject,
                subject_set,
                consistency,
                context,
            )? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Check if `subject` is a member of the given subject-set.
/// Subject-set `team:eng#member` means: does subject have `member` relation on `team:eng`?
fn is_subject_set_member(
    partition_id: &PartitionId,
    storage: &dyn StorageBackend,
    subject: &SubjectId,
    subject_set: &SubjectSet,
    consistency: &ConsistencyMode,
    context: Option<&ConditionEvalContext>,
) -> AegisResult<bool> {
    let tuples = storage.list_by_object(
        partition_id,
        &subject_set.object,
        Some(&subject_set.relation),
        consistency,
    )?;
    let now = Utc::now();
    Ok(tuples.iter().any(|t| {
        t.subject == *subject
            && t.valid_until.is_none_or(|v| v > now)
            && evaluate_tuple_condition(&t.condition, context)
    }))
}

/// Evaluate a tuple-level condition against the available context.
/// Returns `true` if the tuple has no condition or if the condition evaluates to `true`.
/// Returns `false` if no context is available (condition cannot be evaluated).
fn evaluate_tuple_condition(
    condition: &Option<String>,
    context: Option<&ConditionEvalContext>,
) -> bool {
    match condition {
        Some(cond) => match context {
            Some(ctx) => match condition::parse_condition(cond) {
                Ok(expr) => condition::evaluate_condition(&expr, ctx),
                Err(_) => false,
            },
            None => false,
        },
        None => true,
    }
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

        let tuples = aegis.list_by_object(&ResourceId::new("repo:fluxbus").unwrap(), None, None);
        assert_eq!(tuples.len(), 1);
    }
}
