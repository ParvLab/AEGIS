use serde::{Deserialize, Serialize};
use crate::types::*;

/// Reason a permission check was denied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DenialReason {
    MissingRelation {
        subject: String,
        relation: String,
        object: String,
    },
    ExplicitDeny {
        subject: String,
        rule: String,
    },
    TraversalBudgetExceeded {
        depth: u32,
        max: u32,
    },
    RateLimited {
        retry_after_ms: u64,
    },
    ResourceNotFound {
        resource: String,
    },
    SchemaMismatch {
        expected: String,
        actual: String,
    },
    SchemaVersionMismatch {
        current: String,
        requested: String,
    },
}

/// Extended explain result with denial reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainResultV2 {
    pub allowed: bool,
    pub denial_reason: Option<DenialReason>,
    pub revision: Revision,
    pub trace: Vec<ExplainTrace>,
    pub resolved_via: String,
    pub duration_ms: u64,
}

/// A subject with its access paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubjectWithPaths {
    pub subject: String,
    pub paths: Vec<ExplainTrace>,
}

/// Paginated list of subjects found by who-can-access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedSubjects {
    pub subjects: Vec<SubjectWithPaths>,
    pub total: u64,
    pub next_cursor: Option<PaginationCursor>,
}

/// A single check outcome change between two schemas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckChange {
    pub subject: String,
    pub permission: String,
    pub resource: String,
    pub before: bool,
    pub after: bool,
}

/// Access diff report between two schema versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessDiffReport {
    pub gained_access: Vec<ChangeSummary>,
    pub lost_access: Vec<ChangeSummary>,
    pub unchanged: u64,
    pub sampled: bool,
    pub sampled_total: Option<u64>,
    pub duration_ms: u64,
}

/// Summary of a batch of check changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSummary {
    pub subject: String,
    pub permission: String,
    pub resource: String,
}

/// A check query used in simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckQuery {
    pub subject: String,
    pub permission: String,
    pub resource: String,
}

/// One check flip during simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckFlip {
    pub query: CheckQuery,
    pub before: bool,
    pub after: bool,
}

/// Simulation error detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationError {
    pub query: CheckQuery,
    pub error: String,
}

/// Simulation report — Terraform Plan for auth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationReport {
    pub summary: SimulationSummary,
    pub details: Vec<CheckFlip>,
    pub errors: Vec<SimulationError>,
    pub duration_ms: u64,
}

/// Simulation summary (shown first).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationSummary {
    pub gained_access: u64,
    pub lost_access: u64,
    pub unchanged: u64,
    pub error_count: u64,
}

/// Reachability result for a resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReachabilityReport {
    pub subject_count: u64,
    pub max_depth_reached: u32,
    pub truncated: bool,
    pub duration_ms: u64,
}

/// Subject with high access count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighAccessSubject {
    pub subject: String,
    pub resource_count: u64,
}

/// An analysis report that can be exported as JSON or CSV.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub report_type: String,
    pub generated_at: String,
    pub duration_ms: u64,
    pub data: serde_json::Value,
}
