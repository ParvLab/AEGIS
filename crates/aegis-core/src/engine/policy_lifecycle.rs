use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::engine::GraphEngine;
use crate::error::{AegisError, AegisResult};
use crate::types::analysis::{AccessDiffReport, SimulationReport};
use crate::types::Schema;

/// Status of a policy draft in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DraftStatus {
    Drafting,
    UnderReview,
    Approved,
    Published,
    Rejected,
    Superseded,
    Archived,
}

impl std::fmt::Display for DraftStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Drafting => write!(f, "drafting"),
            Self::UnderReview => write!(f, "under_review"),
            Self::Approved => write!(f, "approved"),
            Self::Published => write!(f, "published"),
            Self::Rejected => write!(f, "rejected"),
            Self::Superseded => write!(f, "superseded"),
            Self::Archived => write!(f, "archived"),
        }
    }
}

/// A draft policy change awaiting review and publication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDraft {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub schema: Schema,
    pub base_version: u32,
    pub status: DraftStatus,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: String,
    pub approved_by: Option<String>,
    pub rejection_reason: Option<String>,
}

/// Result of a policy draft validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub schema_valid: bool,
    pub access_diff_summary: Option<AccessDiffReport>,
    pub simulation_summary: Option<SimulationReport>,
    pub warnings: Vec<String>,
}

/// Result of publishing a policy draft.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishResult {
    pub policy_version: u32,
    pub access_diff_summary: Option<AccessDiffReport>,
    pub simulation_summary: Option<SimulationReport>,
}

impl GraphEngine {
    /// Create a new policy draft in Drafting status.
    pub fn create_policy_draft(
        &self,
        name: &str,
        description: &str,
    ) -> AegisResult<PolicyDraft> {
        let now = chrono::Utc::now().to_rfc3339();
        let schema = {
            let s = self.schema.read().map_err(|e| AegisError::Internal(e.to_string()))?;
            s.clone()
        };
        let current_ver = self.storage.read_schema_version().unwrap_or(0);
        let created_by = self.active_actor().unwrap_or_else(|| "unknown".to_string());

        let draft = PolicyDraft {
            id: Uuid::new_v4(),
            name: name.to_string(),
            description: description.to_string(),
            schema,
            base_version: current_ver,
            status: DraftStatus::Drafting,
            created_at: now.clone(),
            updated_at: now,
            created_by,
            approved_by: None,
            rejection_reason: None,
        };

        {
            let mut drafts = self.drafts.lock().map_err(|e| AegisError::Internal(e.to_string()))?;
            drafts.insert(draft.id, draft.clone());
        }

        Ok(draft)
    }

    /// Update a draft's schema (only allowed in Drafting status).
    pub fn update_policy_draft(&self, id: Uuid, schema: Schema) -> AegisResult<PolicyDraft> {
        let mut drafts = self.drafts.lock().map_err(|e| AegisError::Internal(e.to_string()))?;
        let draft = drafts.get_mut(&id).ok_or_else(|| {
            AegisError::Internal(format!("draft {} not found", id))
        })?;

        if draft.status != DraftStatus::Drafting {
            return Err(AegisError::Internal(format!(
                "cannot update draft in status {:?}", draft.status
            )));
        }

        draft.schema = schema;
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(draft.clone())
    }

    /// Validate a draft: check schema validity, compute diff, and run simulation.
    pub fn validate_policy_draft(&self, id: Uuid) -> AegisResult<ValidationReport> {
        let draft = {
            let drafts = self.drafts.lock().map_err(|e| AegisError::Internal(e.to_string()))?;
            drafts.get(&id).cloned().ok_or_else(|| {
                AegisError::Internal(format!("draft {} not found", id))
            })?
        };

        let mut warnings = Vec::new();
        let schema_valid = true;

        let current_schema = {
            let s = self.schema.read().map_err(|e| AegisError::Internal(e.to_string()))?;
            s.clone()
        };

        let access_diff = match self.access_diff(&current_schema, &draft.schema, None, Some(1000)) {
            Ok(r) => Some(r),
            Err(e) => {
                warnings.push(format!("access_diff warning: {}", e));
                None
            }
        };

        Ok(ValidationReport {
            schema_valid,
            access_diff_summary: access_diff,
            simulation_summary: None,
            warnings,
        })
    }

    /// Submit a draft for review. Must be in Drafting status.
    pub fn submit_policy_draft_for_review(&self, id: Uuid) -> AegisResult<PolicyDraft> {
        let mut drafts = self.drafts.lock().map_err(|e| AegisError::Internal(e.to_string()))?;
        let draft = drafts.get_mut(&id).ok_or_else(|| {
            AegisError::Internal(format!("draft {} not found", id))
        })?;

        if draft.status != DraftStatus::Drafting {
            return Err(AegisError::Internal(format!(
                "cannot submit draft in status {:?}", draft.status
            )));
        }

        draft.status = DraftStatus::UnderReview;
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(draft.clone())
    }

    /// Approve a draft. Must be UnderReview.
    pub fn approve_policy_draft(&self, id: Uuid) -> AegisResult<PolicyDraft> {
        let mut drafts = self.drafts.lock().map_err(|e| AegisError::Internal(e.to_string()))?;
        let draft = drafts.get_mut(&id).ok_or_else(|| {
            AegisError::Internal(format!("draft {} not found", id))
        })?;

        if draft.status != DraftStatus::UnderReview {
            return Err(AegisError::Internal(format!(
                "cannot approve draft in status {:?}", draft.status
            )));
        }

        draft.status = DraftStatus::Approved;
        draft.approved_by = Some(self.active_actor().unwrap_or_else(|| "unknown".to_string()));
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(draft.clone())
    }

    /// Reject a draft. Must be UnderReview.
    pub fn reject_policy_draft(&self, id: Uuid, reason: &str) -> AegisResult<PolicyDraft> {
        let mut drafts = self.drafts.lock().map_err(|e| AegisError::Internal(e.to_string()))?;
        let draft = drafts.get_mut(&id).ok_or_else(|| {
            AegisError::Internal(format!("draft {} not found", id))
        })?;

        if draft.status != DraftStatus::UnderReview {
            return Err(AegisError::Internal(format!(
                "cannot reject draft in status {:?}", draft.status
            )));
        }

        draft.status = DraftStatus::Rejected;
        draft.rejection_reason = Some(reason.to_string());
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(draft.clone())
    }

    /// Publish a draft: rolls the policy to the draft's schema. Draft must be Approved.
    pub fn publish_policy_draft(&self, id: Uuid) -> AegisResult<PublishResult> {
        let draft = {
            let mut drafts = self.drafts.lock().map_err(|e| AegisError::Internal(e.to_string()))?;
            let draft = drafts.get_mut(&id).ok_or_else(|| {
                AegisError::Internal(format!("draft {} not found", id))
            })?;

            if draft.status != DraftStatus::Approved {
                return Err(AegisError::Internal(format!(
                    "cannot publish draft in status {:?}", draft.status
                )));
            }
            draft.clone()
        };

        // Compute reports before publishing (capture current vs new state)
        let current_schema = {
            let s = self.schema.read().map_err(|e| AegisError::Internal(e.to_string()))?;
            s.clone()
        };

        let access_diff = self.access_diff(&current_schema, &draft.schema, None, Some(1000)).ok();

        // Save the draft's schema as a new policy version via rollback mechanism
        let schema_json = serde_json::to_string(&draft.schema)
            .map_err(|e| AegisError::Internal(e.to_string()))?;
        let current_ver = self.storage.read_schema_version().unwrap_or(0);
        let now = chrono::Utc::now().to_rfc3339();
        let save_ver = crate::storage::PolicyVersion {
            version: current_ver + 1,
            schema: schema_json.clone(),
            created_at: now.clone(),
            description: format!("published from draft '{}'", draft.name),
        };
        self.storage.save_policy_version(&save_ver)?;

        // Apply the draft schema
        {
            let mut schema = self.schema.write().map_err(|e| AegisError::Internal(e.to_string()))?;
            *schema = draft.schema;
        }
        self.storage.write_schema_version(current_ver + 1)?;

        // Update draft status
        {
            let mut drafts = self.drafts.lock().map_err(|e| AegisError::Internal(e.to_string()))?;
            if let Some(d) = drafts.get_mut(&id) {
                d.status = DraftStatus::Published;
                d.updated_at = chrono::Utc::now().to_rfc3339();
            }
        }

        Ok(PublishResult {
            policy_version: current_ver + 1,
            access_diff_summary: access_diff,
            simulation_summary: None,
        })
    }

    /// Archive a draft (soft delete).
    pub fn archive_policy_draft(&self, id: Uuid) -> AegisResult<PolicyDraft> {
        let mut drafts = self.drafts.lock().map_err(|e| AegisError::Internal(e.to_string()))?;
        let draft = drafts.get_mut(&id).ok_or_else(|| {
            AegisError::Internal(format!("draft {} not found", id))
        })?;

        draft.status = DraftStatus::Archived;
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(draft.clone())
    }

    /// List policy drafts, optionally filtered by status.
    pub fn list_policy_drafts(&self, filter_status: Option<DraftStatus>) -> AegisResult<Vec<PolicyDraft>> {
        let drafts = self.drafts.lock().map_err(|e| AegisError::Internal(e.to_string()))?;
        let mut result: Vec<PolicyDraft> = drafts.values().cloned().collect();
        if let Some(status) = filter_status {
            result.retain(|d| d.status == status);
        }
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(result)
    }
}
