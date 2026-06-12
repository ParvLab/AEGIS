use crate::types::ValidationError;
use thiserror::Error;

/// Unified error type for all Aegis operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AegisError {
    // ── Storage Errors ──
    #[error("storage connection failed: {0}")]
    StorageConnection(String),

    #[error("storage query failed: {0}")]
    StorageQuery(String),

    #[error("storage is not initialized; call initialize() first")]
    StorageNotInitialized,

    #[error("disk full or storage unavailable")]
    StorageExhausted,

    #[error("database integrity check failed: {0}")]
    StorageCorruption(String),

    // ── Schema Errors ──
    #[error("schema validation failed: {0}")]
    SchemaValidation(String),

    #[error("schema version mismatch: expected {expected}, found {actual}")]
    SchemaVersionMismatch { expected: u32, actual: u32 },

    #[error("schema migration failed: {0}")]
    SchemaMigration(String),

    #[error("schema not found at path: {0}")]
    SchemaNotFound(String),

    // ── Validation Errors ──
    #[error("input validation failed: {0}")]
    Validation(#[from] ValidationError),

    #[error("metadata validation failed: {0}")]
    MetadataValidation(String),

    #[error("subject type '{0}' is not defined in the schema")]
    UnknownSubjectType(String),

    #[error("relation '{relation}' not defined on type '{type_name}'")]
    UnknownRelation { type_name: String, relation: String },

    #[error("permission '{permission}' not defined on type '{type_name}'")]
    UnknownPermission {
        type_name: String,
        permission: String,
    },

    // ── Consistency Errors ──
    #[error("consistency error: {0}")]
    Consistency(String),

    #[error("revision token from a different node is incompatible with single-node mode")]
    CrossNodeToken,

    #[error("revision {0} is ahead of the current local revision")]
    RevisionFromFuture(usize),

    // ── Authorization Errors ──
    #[error("permission denied")]
    PermissionDenied,

    #[error("operation not permitted: {0}")]
    OperationNotPermitted(String),

    // ── Rate Limiting ──
    #[error("rate limit exceeded for {0}: try again later")]
    RateLimitExceeded(String),

    // ── Internal Errors ──
    #[error("internal error: {0}")]
    Internal(String),

    #[error("engine is closed and no longer accepting requests")]
    EngineClosed,
}

/// Convenience alias for Result types throughout the Aegis codebase.
pub type AegisResult<T> = Result<T, AegisError>;

impl From<std::io::Error> for AegisError {
    fn from(e: std::io::Error) -> Self {
        AegisError::StorageConnection(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_storage_connection() {
        let err = AegisError::StorageConnection("connection refused".into());
        assert_eq!(
            err.to_string(),
            "storage connection failed: connection refused"
        );
    }

    #[test]
    fn error_display_schema_validation() {
        let err = AegisError::SchemaValidation("missing required field".into());
        assert_eq!(
            err.to_string(),
            "schema validation failed: missing required field"
        );
    }

    #[test]
    fn error_display_validation_from() {
        let val_err = ValidationError::Empty;
        let err: AegisError = val_err.into();
        assert_eq!(
            err.to_string(),
            "input validation failed: value cannot be empty"
        );
    }

    #[test]
    fn error_display_permission_denied() {
        let err = AegisError::PermissionDenied;
        assert_eq!(err.to_string(), "permission denied");
    }

    #[test]
    fn error_display_rate_limit() {
        let err = AegisError::RateLimitExceeded("tenant:alpha".into());
        assert_eq!(
            err.to_string(),
            "rate limit exceeded for tenant:alpha: try again later"
        );
    }

    #[test]
    fn error_display_schema_version_mismatch() {
        let err = AegisError::SchemaVersionMismatch {
            expected: 3,
            actual: 1,
        };
        assert_eq!(
            err.to_string(),
            "schema version mismatch: expected 3, found 1"
        );
    }

    #[test]
    fn error_display_internal() {
        let err = AegisError::Internal("unexpected state".into());
        assert_eq!(err.to_string(), "internal error: unexpected state");
    }

    #[test]
    fn aegis_result_type_alias() {
        fn ok_fn() -> AegisResult<i32> {
            Ok(42)
        }
        fn err_fn() -> AegisResult<i32> {
            Err(AegisError::Internal("fail".into()))
        }
        assert_eq!(ok_fn().unwrap(), 42);
        assert!(err_fn().is_err());
    }

    #[test]
    fn io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "test");
        let aegis_err: AegisError = io_err.into();
        assert!(matches!(aegis_err, AegisError::StorageConnection(_)));
    }

    #[test]
    fn error_equality() {
        let a = AegisError::PermissionDenied;
        let b = AegisError::PermissionDenied;
        assert_eq!(a, b);
    }

    #[test]
    fn cross_node_token_error() {
        let err = AegisError::CrossNodeToken;
        assert_eq!(
            err.to_string(),
            "revision token from a different node is incompatible with single-node mode"
        );
    }

}
