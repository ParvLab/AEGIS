//! Test utilities for the Aegis authorization runtime.
//!
//! Provides helper functions for creating test fixtures and
//! setting up isolated Aegis instances for integration and E2E tests.

use aegis_core::AegisResult;
use aegis_core::testing::TestAegis;

/// Create a fresh TestAegis instance and load a fixture by name.
///
/// Built-in fixtures:
/// - "basic-team": user:123 ∈ team:eng, team:eng owner workspace:core
/// - "multi-tenant": two isolated tenants alpha and beta
/// - "deep-hierarchy": 5-level nested org structure
pub fn create_test_aegis(fixture_name: Option<&str>) -> AegisResult<TestAegis> {
    let mut aegis = TestAegis::new();
    if let Some(name) = fixture_name {
        let yaml = builtin_fixture(name)?;
        aegis.load_fixture_yaml(yaml)?;
    }
    Ok(aegis)
}

/// Get a built-in fixture definition as a YAML string.
pub fn builtin_fixture(name: &str) -> AegisResult<&'static str> {
    match name {
        "basic-team" => Ok(BASIC_TEAM_FIXTURE),
        "multi-tenant" => Ok(MULTI_TENANT_FIXTURE),
        "deep-hierarchy" => Ok(DEEP_HIERARCHY_FIXTURE),
        "circular" => Ok(CIRCULAR_FIXTURE),
        _ => Err(aegis_core::AegisError::SchemaValidation(format!(
            "unknown built-in fixture: '{name}'"
        ))),
    }
}

const BASIC_TEAM_FIXTURE: &str = r#"
tuples:
  - subject: "user:123"
    relation: "member"
    object: "team:eng"
  - subject: "team:eng"
    relation: "owner"
    object: "workspace:core"
  - subject: "workspace:core"
    relation: "contains"
    object: "repo:fluxbus"
  - subject: "user:456"
    relation: "collaborator"
    object: "repo:fluxbus"
"#;

const MULTI_TENANT_FIXTURE: &str = r#"
tuples:
  - subject: "user:alpha1"
    relation: "member"
    object: "tenant:alpha"
  - subject: "tenant:alpha"
    relation: "member"
    object: "workspace:core"
  - subject: "user:beta1"
    relation: "member"
    object: "tenant:beta"
  - subject: "tenant:beta"
    relation: "member"
    object: "workspace:core"
"#;

const DEEP_HIERARCHY_FIXTURE: &str = r#"
tuples:
  - subject: "user:1"
    relation: "member"
    object: "org:root"
  - subject: "org:root"
    relation: "member"
    object: "org:a"
  - subject: "org:a"
    relation: "member"
    object: "org:b"
  - subject: "org:b"
    relation: "member"
    object: "workspace:1"
  - subject: "workspace:1"
    relation: "member"
    object: "workspace:2"
  - subject: "workspace:2"
    relation: "member"
    object: "repo:deep"
"#;

const CIRCULAR_FIXTURE: &str = r#"
tuples:
  - subject: "node:a"
    relation: "linked"
    object: "node:b"
  - subject: "node:b"
    relation: "linked"
    object: "node:c"
  - subject: "node:c"
    relation: "linked"
    object: "node:a"
"#;

/// Generate N tuples for stress testing.
/// Pattern: user:{i} editor repo:{i % M}
pub fn generate_large_fixture(n: usize, m: usize) -> String {
    let mut yaml = String::from("tuples:\n");
    for i in 0..n {
        let repo_idx = i % m;
        yaml.push_str(&format!(
            "  - subject: \"user:{i}\"\n    relation: \"editor\"\n    object: \"repo:{repo_idx}\"\n"
        ));
    }
    yaml
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_empty_test_aegis() {
        let aegis = create_test_aegis(None).unwrap();
        assert_eq!(aegis.tuple_count(), 0);
    }

    #[test]
    fn create_basic_team_fixture() {
        let aegis = create_test_aegis(Some("basic-team")).unwrap();
        assert_eq!(aegis.tuple_count(), 4);
    }

    #[test]
    fn create_multi_tenant_fixture() {
        let aegis = create_test_aegis(Some("multi-tenant")).unwrap();
        assert_eq!(aegis.tuple_count(), 4);
    }

    #[test]
    fn create_deep_hierarchy_fixture() {
        let aegis = create_test_aegis(Some("deep-hierarchy")).unwrap();
        assert_eq!(aegis.tuple_count(), 6);
    }

    #[test]
    fn create_circular_fixture() {
        let aegis = create_test_aegis(Some("circular")).unwrap();
        assert_eq!(aegis.tuple_count(), 3);
    }

    #[test]
    fn unknown_fixture_name() {
        let err = create_test_aegis(Some("nonexistent")).unwrap_err();
        assert!(err.to_string().contains("nonexistent"));
    }

    #[test]
    fn generate_large_fixture_correct_count() {
        let yaml = generate_large_fixture(100, 10);
        // Count lines starting with "    subject:"
        let count = yaml.matches("subject:").count();
        assert_eq!(count, 100);
    }

    #[test]
    fn builtin_fixtures_are_valid() {
        for name in &["basic-team", "multi-tenant", "deep-hierarchy", "circular"] {
            let mut aegis = TestAegis::new();
            let yaml = builtin_fixture(name).unwrap();
            aegis.load_fixture_yaml(yaml).unwrap();
        }
    }
}
