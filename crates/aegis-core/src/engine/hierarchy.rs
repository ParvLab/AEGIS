use crate::error::AegisResult;
use crate::storage::StorageBackend;
use crate::types::{ConsistencyMode, PartitionId, Relation, ResourceId, SubjectId};
use std::collections::HashSet;

/// Walk up the resource hierarchy from `resource` following the given `relation`.
/// Returns all ancestors reachable via the relation (breadth-first).
/// For example, `get_ancestors(storage, "file:doc1", "parent")` returns
/// ancestors of `file:doc1` following `parent` edges.
pub fn get_ancestors(
    partition_id: &PartitionId,
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
        let tuples = storage.list_by_object(partition_id, &current, Some(relation), consistency)?;
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
    partition_id: &PartitionId,
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
            partition_id,
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
    partition_id: &PartitionId,
    storage: &dyn StorageBackend,
    ancestor: &ResourceId,
    descendant: &ResourceId,
    relation: &Relation,
    consistency: &ConsistencyMode,
) -> AegisResult<bool> {
    let ancestors = get_ancestors(partition_id, storage, descendant, relation, consistency)?;
    Ok(ancestors.iter().any(|a| a == ancestor))
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    #[cfg(feature = "sqlite")]
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::types::RelationshipTuple;

    fn setup() -> (Box<SqliteStorage>, ConsistencyMode, PartitionId) {
        let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
        storage.initialize().unwrap();
        let consistency = ConsistencyMode::MinimizeLatency;
        let partition_id = PartitionId::default();
        (Box::new(storage), consistency, partition_id)
    }

    #[test]
    fn test_get_ancestors() {
        let (storage, consistency, partition_id) = setup();
        let org = ResourceId::new("org:acme").unwrap();
        let team = ResourceId::new("team:eng").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage
            .write_tuple(
                &partition_id,
                &RelationshipTuple::new(
                    SubjectId::new("org:acme").unwrap(),
                    Relation::new("parent").unwrap(),
                    team.clone(),
                ),
            )
            .unwrap();
        storage
            .write_tuple(
                &partition_id,
                &RelationshipTuple::new(
                    SubjectId::new("team:eng").unwrap(),
                    Relation::new("parent").unwrap(),
                    repo.clone(),
                ),
            )
            .unwrap();

        let ancestors = get_ancestors(
            &partition_id,
            storage.as_ref(),
            &repo,
            &Relation::new("parent").unwrap(),
            &consistency,
        )
        .unwrap();
        assert!(ancestors.contains(&team));
        assert!(ancestors.contains(&org));
    }

    #[test]
    fn test_get_descendants() {
        let (storage, consistency, partition_id) = setup();
        let org = ResourceId::new("org:acme").unwrap();
        let team = ResourceId::new("team:eng").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage
            .write_tuple(
                &partition_id,
                &RelationshipTuple::new(
                    SubjectId::new("org:acme").unwrap(),
                    Relation::new("parent").unwrap(),
                    team.clone(),
                ),
            )
            .unwrap();
        storage
            .write_tuple(
                &partition_id,
                &RelationshipTuple::new(
                    SubjectId::new("team:eng").unwrap(),
                    Relation::new("parent").unwrap(),
                    repo.clone(),
                ),
            )
            .unwrap();

        let descendants = get_descendants(
            &partition_id,
            storage.as_ref(),
            &org,
            &Relation::new("parent").unwrap(),
            &consistency,
        )
        .unwrap();
        assert!(descendants.contains(&team));
        assert!(descendants.contains(&repo));
    }

    #[test]
    fn test_is_ancestor() {
        let (storage, consistency, partition_id) = setup();
        let org = ResourceId::new("org:acme").unwrap();
        let repo = ResourceId::new("repo:fluxbus").unwrap();

        storage
            .write_tuple(
                &partition_id,
                &RelationshipTuple::new(
                    SubjectId::new("org:acme").unwrap(),
                    Relation::new("parent").unwrap(),
                    repo.clone(),
                ),
            )
            .unwrap();

        assert!(
            is_ancestor(
                &partition_id,
                storage.as_ref(),
                &org,
                &repo,
                &Relation::new("parent").unwrap(),
                &consistency
            )
            .unwrap()
        );
        assert!(
            !is_ancestor(
                &partition_id,
                storage.as_ref(),
                &repo,
                &org,
                &Relation::new("parent").unwrap(),
                &consistency
            )
            .unwrap()
        );
    }
}
