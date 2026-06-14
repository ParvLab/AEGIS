use crate::error::AegisResult;
use crate::storage::StorageBackend;
use crate::types::{ConsistencyMode, Relation, ResourceId, SubjectId};
use std::collections::HashSet;

/// Walk up the resource hierarchy from `resource` following the given `relation`.
/// Returns all ancestors reachable via the relation (breadth-first).
/// For example, `get_ancestors(storage, "file:doc1", "parent")` returns
/// ancestors of `file:doc1` following `parent` edges.
pub fn get_ancestors(
    storage: &dyn StorageBackend,
    resource: &ResourceId,
    relation: &Relation,
    consistency: &ConsistencyMode,
) -> AegisResult<Vec<ResourceId>> {
    let mut ancestors = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = vec![resource.clone()];
    visited.insert(resource.as_str().to_string());

    while let Some(current) = queue.pop() {
        let tuples = storage.list_by_object(&current, Some(relation), consistency)?;
        for t in &tuples {
            if let Ok(rid) = ResourceId::new(t.subject.as_str()) {
                let key = rid.as_str().to_string();
                if visited.insert(key) {
                    ancestors.push(rid.clone());
                    queue.push(rid);
                }
            }
        }
    }
    Ok(ancestors)
}

/// Walk down the resource hierarchy from `resource` following the given `relation`.
/// Returns all descendants reachable via the relation.
pub fn get_descendants(
    storage: &dyn StorageBackend,
    resource: &ResourceId,
    relation: &Relation,
    consistency: &ConsistencyMode,
) -> AegisResult<Vec<ResourceId>> {
    let mut descendants = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = vec![resource.clone()];
    visited.insert(resource.as_str().to_string());

    while let Some(current) = queue.pop() {
        let tuples = storage.list_by_subject(
            &SubjectId::new(current.as_str()).map_err(|_| {
                crate::error::AegisError::Validation(
                    crate::types::ValidationError::InvalidCharacters(current.as_str().to_string()),
                )
            })?,
            Some(relation),
            consistency,
        )?;
        for t in &tuples {
            let key = t.object.as_str().to_string();
            if visited.insert(key) {
                descendants.push(t.object.clone());
                queue.push(t.object.clone());
            }
        }
    }
    Ok(descendants)
}

/// Check if `ancestor` is an ancestor of `descendant` via the given relation.
pub fn is_ancestor(
    storage: &dyn StorageBackend,
    ancestor: &ResourceId,
    descendant: &ResourceId,
    relation: &Relation,
    consistency: &ConsistencyMode,
) -> AegisResult<bool> {
    let ancestors = get_ancestors(storage, descendant, relation, consistency)?;
    Ok(ancestors.iter().any(|a| a == ancestor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::types::RelationshipTuple;

    fn setup() -> (Box<SqliteStorage>, ConsistencyMode) {
        let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
        storage.initialize().unwrap();
        let consistency = ConsistencyMode::MinimizeLatency;
        (Box::new(storage), consistency)
    }

    #[test]
    fn test_get_ancestors() {
        let (storage, consistency) = setup();
        let org = ResourceId::new("org:acme").unwrap();
        let team = ResourceId::new("team:eng").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage.write_tuple(&RelationshipTuple::new(
            SubjectId::new("org:acme").unwrap(),
            Relation::new("parent").unwrap(),
            team.clone(),
        )).unwrap();
        storage.write_tuple(&RelationshipTuple::new(
            SubjectId::new("team:eng").unwrap(),
            Relation::new("parent").unwrap(),
            repo.clone(),
        )).unwrap();

        let ancestors = get_ancestors(storage.as_ref(), &repo, &Relation::new("parent").unwrap(), &consistency).unwrap();
        assert!(ancestors.contains(&team));
        assert!(ancestors.contains(&org));
    }

    #[test]
    fn test_get_descendants() {
        let (storage, consistency) = setup();
        let org = ResourceId::new("org:acme").unwrap();
        let team = ResourceId::new("team:eng").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage.write_tuple(&RelationshipTuple::new(
            SubjectId::new("org:acme").unwrap(),
            Relation::new("parent").unwrap(),
            team.clone(),
        )).unwrap();
        storage.write_tuple(&RelationshipTuple::new(
            SubjectId::new("team:eng").unwrap(),
            Relation::new("parent").unwrap(),
            repo.clone(),
        )).unwrap();

        let descendants = get_descendants(storage.as_ref(), &org, &Relation::new("parent").unwrap(), &consistency).unwrap();
        assert!(descendants.contains(&team));
        assert!(descendants.contains(&repo));
    }

    #[test]
    fn test_is_ancestor() {
        let (storage, consistency) = setup();
        let org = ResourceId::new("org:acme").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage.write_tuple(&RelationshipTuple::new(
            SubjectId::new("org:acme").unwrap(),
            Relation::new("parent").unwrap(),
            repo.clone(),
        )).unwrap();

        assert!(is_ancestor(storage.as_ref(), &org, &repo, &Relation::new("parent").unwrap(), &consistency).unwrap());
        assert!(!is_ancestor(storage.as_ref(), &repo, &org, &Relation::new("parent").unwrap(), &consistency).unwrap());
    }
}
