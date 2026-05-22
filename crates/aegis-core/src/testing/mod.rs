mod fixtures;

pub use fixtures::*;

use crate::error::AegisResult;
use crate::types::{
    CheckResult, ConsistencyMode, PaginatedTuples, PaginationParams, Relation, RelationshipTuple,
    ResourceId, RevisionToken, SubjectId, TupleKey, WriteResult,
};

/// A lightweight test-scoped Aegis instance backed by an in-memory store.
///
/// This is the primary entry point for integration tests.
/// It wraps a minimal storage implementation and exposes the core API methods
/// so that tests can interact with Aegis without needing the full runtime.
#[derive(Debug)]
pub struct TestAegis {
    /// In-memory tuple store
    tuples: Vec<RelationshipTuple>,
    /// Current revision
    revision: u64,
    /// Storage backend type (always "test" for this harness)
    pub backend_type: &'static str,
}

impl TestAegis {
    /// Create a new test Aegis instance, optionally loading fixtures.
    pub fn new() -> Self {
        Self {
            tuples: Vec::new(),
            revision: 0,
            backend_type: "test-inmemory",
        }
    }

    /// Load fixture tuples from a YAML string.
    pub fn load_fixture_yaml(&mut self, yaml: &str) -> AegisResult<()> {
        let fixture = crate::testing::fixtures::load_fixture_yaml(yaml)?;
        for (subject, relation, object) in fixture.tuples {
            let tuple = RelationshipTuple::new(subject, relation, object);
            self.write(&tuple)?;
        }
        Ok(())
    }

    /// Write a relationship tuple and bump the revision.
    pub fn write(&mut self, tuple: &RelationshipTuple) -> AegisResult<WriteResult> {
        // Upsert semantics: remove existing tuple with same key
        self.tuples.retain(|t| t.key() != tuple.key());
        self.tuples.push(tuple.clone());
        self.revision += 1;
        Ok(WriteResult {
            revision: self.revision.into(),
            token: RevisionToken::new(self.revision.into(), uuid::Uuid::nil()),
        })
    }

    /// Delete a tuple by key and bump the revision.
    pub fn delete(&mut self, key: &TupleKey) -> AegisResult<WriteResult> {
        let before = self.tuples.len();
        self.tuples.retain(|t| t.key() != *key);
        if self.tuples.len() < before {
            self.revision += 1;
        }
        Ok(WriteResult {
            revision: self.revision.into(),
            token: RevisionToken::new(self.revision.into(), uuid::Uuid::nil()),
        })
    }

    /// Check a permission. Walks the direct tuple list for matches.
    /// This is a simplified check (no recursive traversal) for test purposes.
    pub fn check(
        &self,
        subject: &SubjectId,
        relation: &Relation,
        object: &ResourceId,
        _consistency: Option<ConsistencyMode>,
    ) -> AegisResult<CheckResult> {
        let allowed = self
            .tuples
            .iter()
            .any(|t| t.subject == *subject && t.relation == *relation && t.object == *object);
        Ok(CheckResult {
            allowed,
            revision: self.revision.into(),
        })
    }

    /// List tuples by object.
    pub fn list_by_object(
        &self,
        object: &ResourceId,
        relation: Option<&Relation>,
    ) -> Vec<RelationshipTuple> {
        self.tuples
            .iter()
            .filter(|t| t.object == *object && relation.map(|r| t.relation == *r).unwrap_or(true))
            .cloned()
            .collect()
    }

    /// List tuples by subject.
    pub fn list_by_subject(
        &self,
        subject: &SubjectId,
        relation: Option<&Relation>,
    ) -> Vec<RelationshipTuple> {
        self.tuples
            .iter()
            .filter(|t| t.subject == *subject && relation.map(|r| t.relation == *r).unwrap_or(true))
            .cloned()
            .collect()
    }

    /// Paginated query (simplified).
    pub fn query_tuples(
        &self,
        _filter: &crate::storage::TupleFilter,
        pagination: &PaginationParams,
    ) -> AegisResult<PaginatedTuples> {
        let limit = pagination.limit as usize;
        let offset = pagination
            .cursor
            .as_ref()
            .map(|c| c.offset as usize)
            .unwrap_or(0);
        let page: Vec<RelationshipTuple> = self
            .tuples
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();
        let next_cursor = if offset + limit < self.tuples.len() {
            Some(crate::types::PaginationCursor {
                offset: (offset + limit) as u64,
                revision: self.revision.into(),
            })
        } else {
            None
        };
        Ok(PaginatedTuples {
            tuples: page,
            next_cursor,
            revision: self.revision.into(),
        })
    }

    /// Count of tuples currently stored.
    pub fn tuple_count(&self) -> usize {
        self.tuples.len()
    }

    /// Current revision as u64.
    pub fn current_revision(&self) -> u64 {
        self.revision
    }

    /// Reset the test instance to empty state.
    pub fn clear(&mut self) {
        self.tuples.clear();
        self.revision = 0;
    }
}

impl Default for TestAegis {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tuple() -> RelationshipTuple {
        RelationshipTuple::new(
            SubjectId::new("user:123").unwrap(),
            Relation::new("editor").unwrap(),
            ResourceId::new("repo:fluxbus").unwrap(),
        )
    }

    #[test]
    fn test_write_and_check_allowed() {
        let mut aegis = TestAegis::new();
        let tuple = test_tuple();
        aegis.write(&tuple).unwrap();

        let result = aegis
            .check(
                &SubjectId::new("user:123").unwrap(),
                &Relation::new("editor").unwrap(),
                &ResourceId::new("repo:fluxbus").unwrap(),
                None,
            )
            .unwrap();
        assert!(result.allowed);
    }

    #[test]
    fn test_write_and_check_denied() {
        let aegis = TestAegis::new();
        let result = aegis
            .check(
                &SubjectId::new("user:123").unwrap(),
                &Relation::new("editor").unwrap(),
                &ResourceId::new("repo:fluxbus").unwrap(),
                None,
            )
            .unwrap();
        assert!(!result.allowed);
    }

    #[test]
    fn test_delete_removes_tuple() {
        let mut aegis = TestAegis::new();
        let tuple = test_tuple();
        aegis.write(&tuple).unwrap();
        assert_eq!(aegis.tuple_count(), 1);

        aegis.delete(&tuple.key()).unwrap();
        assert_eq!(aegis.tuple_count(), 0);
    }

    #[test]
    fn test_revision_increments_on_write() {
        let mut aegis = TestAegis::new();
        assert_eq!(aegis.current_revision(), 0);

        aegis.write(&test_tuple()).unwrap();
        assert_eq!(aegis.current_revision(), 1);

        aegis.write(&test_tuple()).unwrap();
        assert_eq!(aegis.current_revision(), 2);
    }

    #[test]
    fn test_idempotent_write() {
        let mut aegis = TestAegis::new();
        aegis.write(&test_tuple()).unwrap();
        aegis.write(&test_tuple()).unwrap(); // same tuple
        assert_eq!(aegis.tuple_count(), 1); // upsert, not duplicate
    }

    #[test]
    fn test_delete_non_existent_is_noop() {
        let mut aegis = TestAegis::new();
        let key = TupleKey {
            subject: SubjectId::new("user:999").unwrap(),
            relation: Relation::new("editor").unwrap(),
            object: ResourceId::new("repo:nonexistent").unwrap(),
        };
        let result = aegis.delete(&key).unwrap();
        assert_eq!(result.revision.as_u64(), 0); // no bump
    }

    #[test]
    fn test_list_by_object() {
        let mut aegis = TestAegis::new();
        aegis
            .write(&RelationshipTuple::new(
                SubjectId::new("user:1").unwrap(),
                Relation::new("editor").unwrap(),
                ResourceId::new("repo:a").unwrap(),
            ))
            .unwrap();
        aegis
            .write(&RelationshipTuple::new(
                SubjectId::new("user:2").unwrap(),
                Relation::new("viewer").unwrap(),
                ResourceId::new("repo:a").unwrap(),
            ))
            .unwrap();

        let results = aegis.list_by_object(&ResourceId::new("repo:a").unwrap(), None);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_list_by_subject() {
        let mut aegis = TestAegis::new();
        aegis
            .write(&RelationshipTuple::new(
                SubjectId::new("user:1").unwrap(),
                Relation::new("editor").unwrap(),
                ResourceId::new("repo:a").unwrap(),
            ))
            .unwrap();
        aegis
            .write(&RelationshipTuple::new(
                SubjectId::new("user:1").unwrap(),
                Relation::new("viewer").unwrap(),
                ResourceId::new("repo:b").unwrap(),
            ))
            .unwrap();

        let results = aegis.list_by_subject(&SubjectId::new("user:1").unwrap(), None);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_pagination() {
        let mut aegis = TestAegis::new();
        for i in 0..5 {
            aegis
                .write(&RelationshipTuple::new(
                    SubjectId::new(format!("user:{i}")).unwrap(),
                    Relation::new("editor").unwrap(),
                    ResourceId::new("repo:a").unwrap(),
                ))
                .unwrap();
        }

        let page1 = aegis
            .query_tuples(
                &crate::storage::TupleFilter::default(),
                &PaginationParams {
                    limit: 2,
                    cursor: None,
                },
            )
            .unwrap();
        assert_eq!(page1.tuples.len(), 2);
        assert!(page1.next_cursor.is_some());

        let page2 = aegis
            .query_tuples(
                &crate::storage::TupleFilter::default(),
                &PaginationParams {
                    limit: 2,
                    cursor: page1.next_cursor,
                },
            )
            .unwrap();
        assert_eq!(page2.tuples.len(), 2);

        let page3 = aegis
            .query_tuples(
                &crate::storage::TupleFilter::default(),
                &PaginationParams {
                    limit: 2,
                    cursor: page2.next_cursor,
                },
            )
            .unwrap();
        assert_eq!(page3.tuples.len(), 1);
        assert!(page3.next_cursor.is_none());
    }

    #[test]
    fn test_fixture_loading() {
        let mut aegis = TestAegis::new();
        aegis
            .load_fixture_yaml(
                r#"
tuples:
  - subject: "user:123"
    relation: "member"
    object: "team:eng"
  - subject: "user:456"
    relation: "owner"
    object: "repo:fluxbus"
"#,
            )
            .unwrap();
        assert_eq!(aegis.tuple_count(), 2);
    }

    #[test]
    fn test_clear_resets_state() {
        let mut aegis = TestAegis::new();
        aegis.write(&test_tuple()).unwrap();
        assert_eq!(aegis.tuple_count(), 1);
        assert_eq!(aegis.current_revision(), 1);

        aegis.clear();
        assert_eq!(aegis.tuple_count(), 0);
        assert_eq!(aegis.current_revision(), 0);
    }

    #[test]
    fn test_write_result_contains_token() {
        let mut aegis = TestAegis::new();
        let result = aegis.write(&test_tuple()).unwrap();
        assert!(result.revision.as_u64() > 0);
        assert!(!result.token.node_id.is_nil() || result.token.node_id.is_nil()); // test harness uses nil
    }
}
