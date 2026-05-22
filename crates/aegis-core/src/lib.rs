//! # Aegis Core
//!
//! Embedded, relationship-based authorization runtime (ReBAC).
//!
//! ## Architecture
//!
//! - **types**: Core data model types (SubjectId, ResourceId, Relation, Tuple, Revision, Schema)
//! - **error**: Unified error hierarchy (AegisError)
//! - **schema**: YAML schema parser, linter, compatibility checker
//! - **storage**: Pluggable StorageBackend trait (SQLite, PostgreSQL, RocksDB, IndexedDB)
//! - **testing**: Test harness (TestAegis) + fixture loader for integration tests

pub mod error;
pub mod schema;
pub mod storage;
pub mod testing;
pub mod types;

/// Re-export the most commonly used types at the crate root.
pub use crate::error::{AegisError, AegisResult};
pub use crate::types::{
    CheckResult, ConsistencyMode, Relation, RelationshipTuple, ResourceId, Revision, RevisionToken,
    SubjectId, TupleKey, WriteResult,
};

/// Library version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod integration_tests {
    use crate::testing::TestAegis;
    use crate::types::*;

    /// Create a fresh test instance for integration tests.
    fn test_aegis() -> TestAegis {
        TestAegis::new()
    }

    #[test]
    fn full_integration_cycle() {
        let mut aegis = test_aegis();

        // Write
        let write_result = aegis
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("workspace:acme").unwrap(),
            ))
            .unwrap();
        assert!(write_result.revision.as_u64() > 0);

        // Check (allowed)
        let check = aegis
            .check(
                &SubjectId::new("user:alice").unwrap(),
                &Relation::new("owner").unwrap(),
                &ResourceId::new("workspace:acme").unwrap(),
                None,
            )
            .unwrap();
        assert!(check.allowed);

        // Check (denied)
        let denied = aegis
            .check(
                &SubjectId::new("user:bob").unwrap(),
                &Relation::new("owner").unwrap(),
                &ResourceId::new("workspace:acme").unwrap(),
                None,
            )
            .unwrap();
        assert!(!denied.allowed);

        // List
        let list = aegis.list_by_object(&ResourceId::new("workspace:acme").unwrap(), None);
        assert_eq!(list.len(), 1);

        // Delete
        let key = TupleKey {
            subject: SubjectId::new("user:alice").unwrap(),
            relation: Relation::new("owner").unwrap(),
            object: ResourceId::new("workspace:acme").unwrap(),
        };
        let del = aegis.delete(&key).unwrap();
        assert!(del.revision.as_u64() > write_result.revision.as_u64());

        // Verify deletion
        let after_delete = aegis
            .check(
                &SubjectId::new("user:alice").unwrap(),
                &Relation::new("owner").unwrap(),
                &ResourceId::new("workspace:acme").unwrap(),
                None,
            )
            .unwrap();
        assert!(!after_delete.allowed);
    }

    #[test]
    fn multi_tenant_isolation() {
        let mut aegis = test_aegis();

        // Tenant alpha
        aegis
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alpha1").unwrap(),
                Relation::new("member").unwrap(),
                ResourceId::new("tenant:alpha").unwrap(),
            ))
            .unwrap();

        // Tenant beta
        aegis
            .write(&RelationshipTuple::new(
                SubjectId::new("user:beta1").unwrap(),
                Relation::new("member").unwrap(),
                ResourceId::new("tenant:beta").unwrap(),
            ))
            .unwrap();

        // Alpha user cannot access beta
        let cross = aegis
            .check(
                &SubjectId::new("user:alpha1").unwrap(),
                &Relation::new("member").unwrap(),
                &ResourceId::new("tenant:beta").unwrap(),
                None,
            )
            .unwrap();
        assert!(!cross.allowed);

        // Each tenant has its own tuples
        let alpha_tuples = aegis.list_by_subject(&SubjectId::new("user:alpha1").unwrap(), None);
        assert_eq!(alpha_tuples.len(), 1);
        assert_eq!(alpha_tuples[0].object.as_str(), "tenant:alpha");
    }

    #[test]
    fn batch_writes_and_revision_order() {
        let mut aegis = test_aegis();

        let r1 = aegis
            .write(&RelationshipTuple::new(
                SubjectId::new("user:1").unwrap(),
                Relation::new("editor").unwrap(),
                ResourceId::new("repo:a").unwrap(),
            ))
            .unwrap();

        let r2 = aegis
            .write(&RelationshipTuple::new(
                SubjectId::new("user:2").unwrap(),
                Relation::new("viewer").unwrap(),
                ResourceId::new("repo:a").unwrap(),
            ))
            .unwrap();

        let r3 = aegis
            .write(&RelationshipTuple::new(
                SubjectId::new("team:eng").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("workspace:core").unwrap(),
            ))
            .unwrap();

        // Revisions are strictly increasing
        assert!(r1.revision < r2.revision);
        assert!(r2.revision < r3.revision);
        assert_eq!(r1.revision.as_u64() + 1, r2.revision.as_u64());
        assert_eq!(r2.revision.as_u64() + 1, r3.revision.as_u64());
    }

    #[test]
    fn fixture_based_test() {
        let mut aegis = test_aegis();
        aegis
            .load_fixture_yaml(
                r#"
tuples:
  - subject: "user:admin"
    relation: "owner"
    object: "repo:critical"
  - subject: "user:dev"
    relation: "editor"
    object: "repo:critical"
"#,
            )
            .unwrap();

        assert_eq!(aegis.tuple_count(), 2);

        let admin = aegis
            .check(
                &SubjectId::new("user:admin").unwrap(),
                &Relation::new("owner").unwrap(),
                &ResourceId::new("repo:critical").unwrap(),
                None,
            )
            .unwrap();
        assert!(admin.allowed);

        let dev_owner = aegis
            .check(
                &SubjectId::new("user:dev").unwrap(),
                &Relation::new("owner").unwrap(),
                &ResourceId::new("repo:critical").unwrap(),
                None,
            )
            .unwrap();
        assert!(!dev_owner.allowed);
    }
}
