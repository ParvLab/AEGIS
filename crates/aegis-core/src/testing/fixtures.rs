use crate::error::{AegisError, AegisResult};
use crate::types::{Relation, ResourceId, SubjectId};
use serde::Deserialize;

/// A loaded test fixture containing a list of relationship tuples.
#[derive(Debug, Clone)]
pub struct TestFixture {
    pub tuples: Vec<(SubjectId, Relation, ResourceId)>,
}

/// Raw fixture format from YAML.
#[derive(Debug, Deserialize)]
struct RawFixture {
    tuples: Vec<RawFixtureTuple>,
}

#[derive(Debug, Deserialize)]
struct RawFixtureTuple {
    subject: String,
    relation: String,
    object: String,
}

/// Load a test fixture from a YAML string.
pub fn load_fixture_yaml(yaml: &str) -> AegisResult<TestFixture> {
    let raw: RawFixture = serde_yaml::from_str(yaml)
        .map_err(|e| AegisError::SchemaValidation(format!("invalid fixture YAML: {e}")))?;

    let mut tuples = Vec::with_capacity(raw.tuples.len());
    for rt in raw.tuples {
        let subject = SubjectId::new(&rt.subject).map_err(|e| AegisError::Validation(e))?;
        let relation = Relation::new(&rt.relation).map_err(|e| AegisError::Validation(e))?;
        let object = ResourceId::new(&rt.object).map_err(|e| AegisError::Validation(e))?;
        tuples.push((subject, relation, object));
    }

    Ok(TestFixture { tuples })
}

/// Load a test fixture from a YAML file path.
pub fn load_fixture_file(path: &str) -> AegisResult<TestFixture> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        AegisError::StorageConnection(format!("cannot read fixture file '{path}': {e}"))
    })?;
    load_fixture_yaml(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_fixture_yaml_valid() {
        let fixture = load_fixture_yaml(
            r#"
tuples:
  - subject: "user:123"
    relation: "editor"
    object: "repo:fluxbus"
  - subject: "team:eng"
    relation: "owner"
    object: "workspace:core"
"#,
        )
        .unwrap();
        assert_eq!(fixture.tuples.len(), 2);

        let (subject, relation, object) = &fixture.tuples[0];
        assert_eq!(subject.as_str(), "user:123");
        assert_eq!(relation.as_str(), "editor");
        assert_eq!(object.as_str(), "repo:fluxbus");
    }

    #[test]
    fn load_fixture_yaml_invalid_subject() {
        let err = load_fixture_yaml(
            r#"
tuples:
  - subject: ""
    relation: "editor"
    object: "repo:fluxbus"
"#,
        )
        .unwrap_err();
        assert!(matches!(err, AegisError::Validation(_)));
    }

    #[test]
    fn load_fixture_yaml_invalid_relation() {
        let err = load_fixture_yaml(
            r#"
tuples:
  - subject: "user:1"
    relation: ""
    object: "repo:fluxbus"
"#,
        )
        .unwrap_err();
        assert!(matches!(err, AegisError::Validation(_)));
    }

    #[test]
    fn load_fixture_yaml_empty() {
        let fixture = load_fixture_yaml(r#"tuples: []"#).unwrap();
        assert!(fixture.tuples.is_empty());
    }

    #[test]
    fn load_fixture_yaml_missing_tuples_field() {
        let err = load_fixture_yaml(r#"other: data"#).unwrap_err();
        assert!(matches!(err, AegisError::SchemaValidation(_)));
    }
}
