use serde::{Deserialize, Serialize};
use std::fmt;

const MAX_IDENTITY_LENGTH: usize = 256;

/// A validated principal identity string (e.g. "user:123", "team:eng", "agent:planner")
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubjectId(String);

impl SubjectId {
    pub fn new(raw: impl Into<String>) -> Result<Self, ValidationError> {
        let s = raw.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    fn validate(s: &str) -> Result<(), ValidationError> {
        if s.is_empty() {
            return Err(ValidationError::Empty);
        }
        if s.len() > MAX_IDENTITY_LENGTH {
            return Err(ValidationError::TooLong {
                max: MAX_IDENTITY_LENGTH,
                actual: s.len(),
            });
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_' || c == '-')
        {
            return Err(ValidationError::InvalidCharacters(s.to_string()));
        }
        Ok(())
    }
}

impl fmt::Display for SubjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for SubjectId {
    type Error = ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for SubjectId {
    type Error = ValidationError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// A validated resource object string (e.g. "repo:fluxbus", "workspace:core")
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceId(String);

impl ResourceId {
    pub fn new(raw: impl Into<String>) -> Result<Self, ValidationError> {
        let s = raw.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    fn validate(s: &str) -> Result<(), ValidationError> {
        if s.is_empty() {
            return Err(ValidationError::Empty);
        }
        if s.len() > MAX_IDENTITY_LENGTH {
            return Err(ValidationError::TooLong {
                max: MAX_IDENTITY_LENGTH,
                actual: s.len(),
            });
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_' || c == '-')
        {
            return Err(ValidationError::InvalidCharacters(s.to_string()));
        }
        Ok(())
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for ResourceId {
    type Error = ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for ResourceId {
    type Error = ValidationError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// A validated relation name (e.g. "editor", "owner", "member")
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Relation(String);

const MAX_RELATION_LENGTH: usize = 64;

impl Relation {
    pub fn new(raw: impl Into<String>) -> Result<Self, ValidationError> {
        let s = raw.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    fn validate(s: &str) -> Result<(), ValidationError> {
        if s.is_empty() {
            return Err(ValidationError::Empty);
        }
        if s.len() > MAX_RELATION_LENGTH {
            return Err(ValidationError::TooLong {
                max: MAX_RELATION_LENGTH,
                actual: s.len(),
            });
        }
        if !s.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return Err(ValidationError::InvalidCharacters(s.to_string()));
        }
        Ok(())
    }
}

impl fmt::Display for Relation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for Relation {
    type Error = ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for Relation {
    type Error = ValidationError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Validation errors for identity/resource/relation strings
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("value cannot be empty")]
    Empty,

    #[error("value too long: max {max} characters, got {actual}")]
    TooLong { max: usize, actual: usize },

    #[error("invalid characters in value: '{0}'")]
    InvalidCharacters(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_subject_id() {
        let id = SubjectId::new("user:123").unwrap();
        assert_eq!(id.as_str(), "user:123");
    }

    #[test]
    fn valid_subject_id_with_special_chars() {
        let id = SubjectId::new("agent:planner-v2").unwrap();
        assert_eq!(id.as_str(), "agent:planner-v2");
    }

    #[test]
    fn empty_subject_id() {
        let err = SubjectId::new("").unwrap_err();
        assert!(matches!(err, ValidationError::Empty));
    }

    #[test]
    fn subject_id_too_long() {
        let long = "a".repeat(257);
        let err = SubjectId::new(long).unwrap_err();
        assert!(matches!(err, ValidationError::TooLong { .. }));
    }

    #[test]
    fn invalid_characters() {
        let err = SubjectId::new("user 123").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidCharacters(_)));
    }

    #[test]
    fn sql_injection_attempt_rejected() {
        let err = SubjectId::new("'; DROP TABLE; --").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidCharacters(_)));
    }

    #[test]
    fn valid_relation() {
        let rel = Relation::new("editor").unwrap();
        assert_eq!(rel.as_str(), "editor");
    }

    #[test]
    fn relation_uppercase_rejected() {
        let err = Relation::new("EDITOR").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidCharacters(_)));
    }

    #[test]
    fn relation_with_colon_rejected() {
        let err = Relation::new("role:admin").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidCharacters(_)));
    }

    #[test]
    fn valid_resource_id() {
        let id = ResourceId::new("repo:fluxbus").unwrap();
        assert_eq!(id.as_str(), "repo:fluxbus");
    }

    #[test]
    fn subject_id_display() {
        let id = SubjectId::new("user:123").unwrap();
        assert_eq!(format!("{id}"), "user:123");
    }

    #[test]
    fn try_from_str_subject() {
        let id: SubjectId = "user:42".try_into().unwrap();
        assert_eq!(id.as_str(), "user:42");
    }

    #[test]
    fn try_from_string_resource() {
        let id: ResourceId = "workspace:core".to_string().try_into().unwrap();
        assert_eq!(id.as_str(), "workspace:core");
    }
}
