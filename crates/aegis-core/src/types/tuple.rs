use crate::types::{Relation, ResourceId, SubjectId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Estimated overhead for JSON serialization framing (brackets, commas, quotes).
const SERIALIZATION_OVERHEAD: usize = 128;

/// The atomic unit of the authorization graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationshipTuple {
    pub subject: SubjectId,
    pub relation: Relation,
    pub object: ResourceId,
    pub created_at: DateTime<Utc>,
    pub metadata: Option<HashMap<String, String>>,
}

impl RelationshipTuple {
    pub fn new(subject: SubjectId, relation: Relation, object: ResourceId) -> Self {
        let tuple = Self {
            subject,
            relation,
            object,
            created_at: Utc::now(),
            metadata: None,
        };
        // Size check should never fail for a tuple without metadata,
        // but call it to enforce the invariant.
        let _ = tuple.ensure_size();
        tuple
    }

    pub fn with_metadata(
        subject: SubjectId,
        relation: Relation,
        object: ResourceId,
        metadata: HashMap<String, String>,
    ) -> Result<Self, MetadataValidationError> {
        validate_metadata(&metadata)?;
        let tuple = Self {
            subject,
            relation,
            object,
            created_at: Utc::now(),
            metadata: Some(metadata),
        };
        tuple.ensure_size()?;
        Ok(tuple)
    }

    /// Check that the serialized tuple size does not exceed the maximum.
    fn ensure_size(&self) -> Result<(), MetadataValidationError> {
        let estimated = self.estimated_serialized_size();
        if estimated > MAX_TUPLE_SERIALIZED_SIZE {
            return Err(MetadataValidationError::TupleTooLarge(estimated));
        }
        Ok(())
    }

    /// Rough estimate of serialized size (must be <= true serialized size).
    fn estimated_serialized_size(&self) -> usize {
        let mut size = SERIALIZATION_OVERHEAD;
        size += self.subject.as_str().len();
        size += self.relation.as_str().len();
        size += self.object.as_str().len();
        if let Some(ref meta) = self.metadata {
            for (k, v) in meta {
                size += k.len() + v.len() + 8;
            }
        }
        size
    }

    /// The canonical key for this tuple (subject + relation + object).
    /// Used for idempotency checks and indexing.
    pub fn key(&self) -> TupleKey {
        TupleKey {
            subject: self.subject.clone(),
            relation: self.relation.clone(),
            object: self.object.clone(),
        }
    }
}

/// The unique identifier for a relationship tuple.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TupleKey {
    pub subject: SubjectId,
    pub relation: Relation,
    pub object: ResourceId,
}

/// A mutation action on the authorization graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TupleAction {
    Add(RelationshipTuple),
    Remove(TupleKey),
}

const MAX_METADATA_PAIRS: usize = 16;
const MAX_METADATA_KEY_LENGTH: usize = 64;
const MAX_METADATA_VALUE_LENGTH: usize = 512;
/// Maximum serialized size of a relationship tuple in bytes.
const MAX_TUPLE_SERIALIZED_SIZE: usize = 65_536; // 64 KiB

pub fn validate_metadata(
    metadata: &HashMap<String, String>,
) -> Result<(), MetadataValidationError> {
    if metadata.len() > MAX_METADATA_PAIRS {
        return Err(MetadataValidationError::TooManyPairs {
            max: MAX_METADATA_PAIRS,
            actual: metadata.len(),
        });
    }
    for (key, value) in metadata {
        if key.is_empty() {
            return Err(MetadataValidationError::EmptyKey);
        }
        if key.len() > MAX_METADATA_KEY_LENGTH {
            return Err(MetadataValidationError::KeyTooLong {
                max: MAX_METADATA_KEY_LENGTH,
                actual: key.len(),
            });
        }
        if !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(MetadataValidationError::InvalidKey(key.clone()));
        }
        if value.len() > MAX_METADATA_VALUE_LENGTH {
            return Err(MetadataValidationError::ValueTooLong {
                max: MAX_METADATA_VALUE_LENGTH,
                actual: value.len(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MetadataValidationError {
    #[error("too many metadata pairs: max {max}, got {actual}")]
    TooManyPairs { max: usize, actual: usize },

    #[error("metadata key cannot be empty")]
    EmptyKey,

    #[error("metadata key too long: max {max} characters, got {actual}")]
    KeyTooLong { max: usize, actual: usize },

    #[error("invalid characters in metadata key: '{0}'")]
    InvalidKey(String),

    #[error("metadata value too long: max {max} characters, got {actual}")]
    ValueTooLong { max: usize, actual: usize },

    #[error("tuple too large: estimated {0} bytes, max 65536")]
    TupleTooLarge(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn tuple_key_uniqueness() {
        let t1 = RelationshipTuple::new(
            SubjectId::new("user:1").unwrap(),
            Relation::new("editor").unwrap(),
            ResourceId::new("repo:a").unwrap(),
        );
        let t2 = RelationshipTuple::new(
            SubjectId::new("user:1").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:a").unwrap(),
        );
        assert_ne!(t1.key(), t2.key());
    }

    #[test]
    fn tuple_with_valid_metadata() {
        let mut meta = HashMap::new();
        meta.insert("granted_by".to_string(), "admin:1".to_string());
        let tuple = RelationshipTuple::with_metadata(
            SubjectId::new("user:1").unwrap(),
            Relation::new("editor").unwrap(),
            ResourceId::new("repo:a").unwrap(),
            meta,
        );
        assert!(tuple.is_ok());
    }

    #[test]
    fn tuple_with_too_many_metadata_pairs() {
        let mut meta = HashMap::new();
        for i in 0..17 {
            meta.insert(format!("key{i}"), "value".to_string());
        }
        let err = RelationshipTuple::with_metadata(
            SubjectId::new("user:1").unwrap(),
            Relation::new("editor").unwrap(),
            ResourceId::new("repo:a").unwrap(),
            meta,
        )
        .unwrap_err();
        assert!(matches!(err, MetadataValidationError::TooManyPairs { .. }));
    }

    #[test]
    fn tuple_with_invalid_metadata_key() {
        let mut meta = HashMap::new();
        meta.insert("illegal key!".to_string(), "value".to_string());
        let err = RelationshipTuple::with_metadata(
            SubjectId::new("user:1").unwrap(),
            Relation::new("editor").unwrap(),
            ResourceId::new("repo:a").unwrap(),
            meta,
        )
        .unwrap_err();
        assert!(matches!(err, MetadataValidationError::InvalidKey(_)));
    }

    #[test]
    fn tuple_action_discrimination() {
        let tuple = RelationshipTuple::new(
            SubjectId::new("user:1").unwrap(),
            Relation::new("editor").unwrap(),
            ResourceId::new("repo:a").unwrap(),
        );
        let add = TupleAction::Add(tuple);
        let key = TupleKey {
            subject: SubjectId::new("user:1").unwrap(),
            relation: Relation::new("editor").unwrap(),
            object: ResourceId::new("repo:a").unwrap(),
        };
        let remove = TupleAction::Remove(key);

        match add {
            TupleAction::Add(t) => assert_eq!(t.subject.as_str(), "user:1"),
            _ => panic!("expected Add"),
        }
        match remove {
            TupleAction::Remove(k) => assert_eq!(k.subject.as_str(), "user:1"),
            _ => panic!("expected Remove"),
        }
    }
}
