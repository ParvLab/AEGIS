use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A monotonically increasing revision number for the graph.
/// Every write to the graph bumps this counter by exactly 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Revision(u64);

impl Revision {
    pub const ZERO: Revision = Revision(0);

    pub fn new(n: u64) -> Self {
        Self(n)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl std::fmt::Display for Revision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for Revision {
    fn from(n: u64) -> Self {
        Self(n)
    }
}

/// An opaque consistency token returned by every write.
/// Guarantees read-your-writes semantics when passed back to a subsequent read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionToken {
    pub revision: Revision,
    pub node_id: Uuid,
    pub timestamp: DateTime<Utc>,
}

impl RevisionToken {
    pub fn new(revision: Revision, node_id: Uuid) -> Self {
        Self {
            revision,
            node_id,
            timestamp: Utc::now(),
        }
    }
}

/// Controls the consistency guarantee for a read operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsistencyMode {
    /// Fast path: read from the latest available local snapshot.
    /// May be slightly stale in multi-instance deployments.
    MinimizeLatency,

    /// Read from a snapshot at least as fresh as the given revision.
    /// Guarantees read-your-writes.
    AtRevision(Revision),

    /// Read the absolute latest committed state.
    /// Highest latency in distributed setups.
    FullyConsistent,
}

impl Default for ConsistencyMode {
    fn default() -> Self {
        Self::MinimizeLatency
    }
}

/// Represents the result of a graph mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResult {
    pub revision: Revision,
    pub token: RevisionToken,
}

/// Represents the result of a permission check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub allowed: bool,
    pub revision: Revision,
}

/// A single step in an explain trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainTrace {
    pub subject: String,
    pub relation: String,
    pub object: String,
}

/// Detailed explanation of a permission check decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainResult {
    pub allowed: bool,
    pub revision: Revision,
    pub trace: Vec<ExplainTrace>,
    pub resolved_via: String,
    pub duration_ms: u64,
}

/// Health status of the engine and its components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub healthy: bool,
    pub revision: Revision,
    pub schema_version: u32,
    pub backend: String,
    pub backend_healthy: bool,
    pub telemetry_healthy: bool,
    pub cache_hit_rate: f64,
    pub cache_entries: usize,
    pub storage_integrity: bool,
    pub error: Option<String>,
}

/// Configuration for fail-closed vs fail-open behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailClosedMode {
    DenyOnError,
    AllowOnError,
}

impl Default for FailClosedMode {
    fn default() -> Self {
        Self::DenyOnError
    }
}

/// Represents a single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub revision: Revision,
    pub action: TupleMutation,
    pub subject: String,
    pub relation: String,
    pub object: String,
    pub timestamp: DateTime<Utc>,
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TupleMutation {
    Add,
    Remove,
}

/// Pagination cursor for list/query results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationCursor {
    pub offset: u64,
    pub revision: Revision,
}

/// Pagination parameters for list/query requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationParams {
    pub limit: u64,
    pub cursor: Option<PaginationCursor>,
}

impl PaginationParams {
    /// Maximum number of results per page.
    pub const MAX_LIMIT: u64 = 10_000;

    /// Create a new PaginationParams with limit capped at MAX_LIMIT.
    pub fn new(limit: u64, cursor: Option<PaginationCursor>) -> Self {
        Self {
            limit: limit.min(Self::MAX_LIMIT),
            cursor,
        }
    }

    /// Cap the limit at MAX_LIMIT (used when deserializing from untrusted input).
    pub fn capped(self) -> Self {
        Self {
            limit: self.limit.min(Self::MAX_LIMIT),
            cursor: self.cursor,
        }
    }
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            limit: 100,
            cursor: None,
        }
    }
}

/// Paginated list of tuples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedTuples {
    pub tuples: Vec<super::tuple::RelationshipTuple>,
    pub next_cursor: Option<PaginationCursor>,
    pub revision: Revision,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_increments() {
        let r1 = Revision::new(1);
        assert_eq!(r1.next(), Revision::new(2));
    }

    #[test]
    fn revision_zero_constant() {
        assert_eq!(Revision::ZERO.as_u64(), 0);
    }

    #[test]
    fn revision_from_u64() {
        let r: Revision = 42.into();
        assert_eq!(r.as_u64(), 42);
    }

    #[test]
    fn revision_ordering() {
        let r1 = Revision::new(1);
        let r2 = Revision::new(2);
        assert!(r1 < r2);
    }

    #[test]
    fn consistency_mode_default() {
        assert_eq!(ConsistencyMode::default(), ConsistencyMode::MinimizeLatency);
    }

    #[test]
    fn revision_token_creation() {
        let node_id = Uuid::new_v4();
        let token = RevisionToken::new(Revision::new(5), node_id);
        assert_eq!(token.revision.as_u64(), 5);
        assert_eq!(token.node_id, node_id);
    }

    #[test]
    fn pagination_default_limit() {
        let p = PaginationParams::default();
        assert_eq!(p.limit, 100);
        assert!(p.cursor.is_none());
    }
}
