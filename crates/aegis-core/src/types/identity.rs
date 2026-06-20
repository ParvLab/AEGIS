use serde::{Deserialize, Serialize};
use std::fmt;

const MAX_IDENTITY_LENGTH: usize = 256;

/// A validated principal identity string (e.g. "user:123", "team:eng", "team:eng#member").
///
/// When the string contains `#`, it represents a *subject-set* reference:
/// the subject is not a single principal but rather the set of all principals
/// holding a given relation on a given object (e.g. `team:eng#member` means
/// "all members of team:eng").
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

    /// If this subject is a subject-set reference (e.g. `team:eng#member`),
    /// parse it into its object + relation components.
    /// Returns `None` for simple subjects (no `#`).
    pub fn as_subject_set(&self) -> Option<SubjectSet> {
        if let Some(pos) = self.0.rfind('#') {
            let object_str = &self.0[..pos];
            let relation_str = &self.0[pos + 1..];
            if object_str.is_empty() || relation_str.is_empty() {
                return None;
            }
            let object = ResourceId::new(object_str).ok()?;
            let relation = Relation::new(relation_str).ok()?;
            Some(SubjectSet { object, relation })
        } else {
            None
        }
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
        // Allow `#` for subject-set references (e.g. `team:eng#member`)
        if !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_' || c == '-' || c == '#')
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

/// A subject-set reference: all principals holding `relation` on `object`.
///
/// In Zanzibar-style ReBAC this is written as `object#relation` (e.g. `team:eng#member`).
/// A tuple `(team:eng#member, editor, repo:fluxbus)` means "anyone with `member` on `team:eng`
/// is an `editor` of `repo:fluxbus`".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubjectSet {
    pub object: ResourceId,
    pub relation: Relation,
}

impl SubjectSet {
    pub fn new(object: ResourceId, relation: Relation) -> Self {
        Self { object, relation }
    }

    /// Format as `object#relation` string (e.g. `team:eng#member`).
    pub fn subject_id_string(&self) -> String {
        format!("{}#{}", self.object.as_str(), self.relation.as_str())
    }

    /// Look up all principals that satisfy this subject-set.
    /// Returns the subject field from tuples where `object == self.object` and `relation == self.relation`.
    pub fn resolve(
        &self,
        storage: &dyn crate::storage::StorageBackend,
        partition_id: &PartitionId,
        consistency: &crate::types::ConsistencyMode,
    ) -> Result<Vec<SubjectId>, crate::error::AegisError> {
        let tuples = storage.list_by_object(
            partition_id,
            &self.object,
            Some(&self.relation),
            consistency,
        )?;
        Ok(tuples.into_iter().map(|t| t.subject).collect())
    }
}

/// A validated partition identifier string.
///
/// Partitions logically isolate authorization graphs within a single engine.
/// Each partition has its own rate limiter, traversal budget, metrics, and cache namespace.
/// Partitions are NOT tenants — an application decides what a partition represents
/// (e.g. tenant_id, org_id, workspace_id, guild_id, or "default").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PartitionId(String);

const MAX_PARTITION_LENGTH: usize = 64;

impl PartitionId {
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
        if s.len() > MAX_PARTITION_LENGTH {
            return Err(ValidationError::TooLong {
                max: MAX_PARTITION_LENGTH,
                actual: s.len(),
            });
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ValidationError::InvalidCharacters(s.to_string()));
        }
        Ok(())
    }
}

impl Default for PartitionId {
    fn default() -> Self {
        Self("default".to_string())
    }
}

impl fmt::Display for PartitionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for PartitionId {
    type Error = ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for PartitionId {
    type Error = ValidationError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Validation errors for identity/resource/relation/partition strings
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
    fn test_empty_relation() {
        let err = Relation::new("").unwrap_err();
        assert!(matches!(err, ValidationError::Empty));
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

    #[test]
    fn valid_partition_id() {
        let pid = PartitionId::new("acme").unwrap();
        assert_eq!(pid.as_str(), "acme");
    }

    #[test]
    fn valid_partition_id_with_special_chars() {
        let pid = PartitionId::new("tenant-42_prod").unwrap();
        assert_eq!(pid.as_str(), "tenant-42_prod");
    }

    #[test]
    fn empty_partition_id_rejected() {
        let err = PartitionId::new("").unwrap_err();
        assert!(matches!(err, ValidationError::Empty));
    }

    #[test]
    fn partition_id_too_long() {
        let long = "a".repeat(65);
        let err = PartitionId::new(long).unwrap_err();
        assert!(matches!(err, ValidationError::TooLong { .. }));
    }

    #[test]
    fn partition_id_colon_rejected() {
        let err = PartitionId::new("tenant:acme").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidCharacters(_)));
    }

    #[test]
    fn partition_id_default() {
        let pid = PartitionId::default();
        assert_eq!(pid.as_str(), "default");
    }
}
