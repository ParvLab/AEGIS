pub mod graph;
pub mod simulate;

use crate::engine::GraphEngine;
use crate::engine::policy;
use crate::error::{AegisError, AegisResult};
use crate::storage::{StorageBackend, TupleFilter};
use crate::types::analysis::*;
use crate::types::*;

impl GraphEngine {
    /// Returns an extended explain result with a denial reason.
    pub fn explain_v2(
        &self,
        subject: &SubjectId,
        permission: &str,
        resource: &ResourceId,
        consistency: Option<ConsistencyMode>,
    ) -> AegisResult<ExplainResultV2> {
        let start = std::time::Instant::now();
        let revision = self.resolve_revision(consistency)?;
        let resource_type = resource_type_name(resource.as_str());
        let schema = self.schema.read().unwrap();

        let _resolved = match policy::resolve_permission(&schema, &resource_type, permission) {
            Some(r) => r,
            None => {
                return Ok(ExplainResultV2 {
                    allowed: false,
                    denial_reason: Some(DenialReason::ResourceNotFound {
                        resource: resource.as_str().to_string(),
                    }),
                    revision,
                    trace: Vec::new(),
                    resolved_via: String::new(),
                    duration_ms: start.elapsed().as_micros() as u64 / 1000,
                });
            }
        };
        drop(schema);

        let mut denial_reason = None::<DenialReason>;

        let traversal_result = self.explain(subject, permission, resource, consistency)?;
        let allowed = traversal_result.allowed;
        let all_traces = traversal_result.trace;

        if !allowed {
            let schema = self.schema.read().unwrap();
            let type_def = schema.types.get(&resource_type);
            if let Some(type_def) = type_def {
                if !type_def.deny.is_empty() {
                    'deny_check: for deny_def in &type_def.deny {
                        for deny_rel in &deny_def.relations {
                            let relation = match Relation::new(deny_rel) {
                                Ok(r) => r,
                                Err(_) => continue,
                            };
                            if let Ok(tr) = crate::engine::traversal::bfs_traversal(
                                &self.active_partition_id(),
                                self.storage.as_ref(),
                                subject,
                                &relation,
                                resource,
                                Some(revision),
                                consistency,
                            ) {
                                if tr.found {
                                    denial_reason = Some(DenialReason::ExplicitDeny {
                                        subject: subject.as_str().to_string(),
                                        rule: deny_rel.clone(),
                                    });
                                    break 'deny_check;
                                }
                            }
                        }
                    }
                }
            }
            drop(schema);

            if denial_reason.is_none() {
                denial_reason = Some(DenialReason::MissingRelation {
                    subject: subject.as_str().to_string(),
                    relation: permission.to_string(),
                    object: resource.as_str().to_string(),
                });
            }
        }

        let duration_ms = start.elapsed().as_micros() as u64 / 1000;
        let resolved_via = if allowed && !all_traces.is_empty() {
            let steps: Vec<String> = all_traces
                .iter()
                .map(|t| format!("{}#{}", t.subject, t.relation))
                .collect();
            format!("→ {}", steps.join(" → "))
        } else if allowed {
            format!("direct relation '{}'", permission)
        } else {
            "no path found".to_string()
        };

        Ok(ExplainResultV2 {
            allowed,
            denial_reason,
            revision,
            trace: all_traces,
            resolved_via,
            duration_ms,
        })
    }

    /// Find all subjects that can access a given resource with a given permission.
    /// Paginated, with optional path inclusion.
    pub fn who_can_access(
        &self,
        permission: &str,
        resource: &ResourceId,
        pagination: &PaginationParams,
        include_paths: bool,
        max_depth: u32,
        timeout_ms: u64,
    ) -> AegisResult<PaginatedSubjects> {
        let start = std::time::Instant::now();
        let revision = self.resolve_revision(None)?;
        let resource_type = resource_type_name(resource.as_str());
        let schema = self.schema.read().unwrap();

        let resolved = match policy::resolve_permission(&schema, &resource_type, permission) {
            Some(r) => r,
            None => {
                return Ok(PaginatedSubjects {
                    subjects: Vec::new(),
                    total: 0,
                    next_cursor: None,
                });
            }
        };
        let relations = resolved.relations.clone();
        drop(schema);

        let pid = self.active_partition_id();
        let storage: &dyn StorageBackend = self.storage.as_ref();

        // Get all tuples for the resource to find candidate subjects
        let mut candidates: std::collections::HashMap<String, Vec<ExplainTrace>> =
            std::collections::HashMap::new();

        for rel_name in &relations {
            let relation = match Relation::new(rel_name) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let tuples = match storage.list_by_object(
                &pid,
                resource,
                Some(&relation),
                &ConsistencyMode::MinimizeLatency,
            ) {
                Ok(t) => t,
                Err(_) => continue,
            };

            for t in &tuples {
                let subj = t.subject.as_str().to_string();
                let path = ExplainTrace {
                    subject: subj.clone(),
                    relation: t.relation.as_str().to_string(),
                    object: t.object.as_str().to_string(),
                };
                candidates.entry(subj).or_default().push(path);
            }

            if start.elapsed().as_millis() as u64 > timeout_ms {
                break;
            }
        }

        // Follow edges: if a candidate's object is a subject for another tuple, follow it
        let expand = (start.elapsed().as_millis() as u64) < timeout_ms;
        if expand && max_depth > 0 {
            for _ in 0..max_depth {
                if start.elapsed().as_millis() as u64 > timeout_ms {
                    break;
                }
                let mut new_candidates: std::collections::HashMap<String, Vec<ExplainTrace>> =
                    std::collections::HashMap::new();
                for (subj, paths) in &candidates {
                    let subject_id = match SubjectId::new(subj) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    for rel_name in &relations {
                        let relation = match Relation::new(rel_name) {
                            Ok(r) => r,
                            Err(_) => continue,
                        };
                        let ok = match storage.list_by_subject(
                            &pid,
                            &subject_id,
                            Some(&relation),
                            &ConsistencyMode::MinimizeLatency,
                        ) {
                            Ok(tuples) => tuples,
                            Err(_) => continue,
                        };
                        for t in &ok {
                            let new_subj =
                                format!("{}#{}", t.subject.as_str(), t.relation.as_str());
                            let mut new_paths = paths.clone();
                            new_paths.push(ExplainTrace {
                                subject: new_subj.clone(),
                                relation: t.relation.as_str().to_string(),
                                object: t.object.as_str().to_string(),
                            });
                            new_candidates
                                .entry(new_subj)
                                .or_default()
                                .extend(new_paths);
                        }
                    }
                }
                if new_candidates.is_empty() {
                    break;
                }
                // Merge
                for (k, v) in new_candidates {
                    candidates.entry(k).or_default().extend(v);
                }
            }
        }

        let total = candidates.len() as u64;

        // Apply pagination
        let offset = pagination
            .cursor
            .as_ref()
            .map(|c| c.offset as usize)
            .unwrap_or(0);
        let limit = pagination.limit as usize;
        let mut all_subjects: Vec<(String, Vec<ExplainTrace>)> = candidates.into_iter().collect();
        all_subjects.sort_by(|a, b| a.0.cmp(&b.0));
        let has_more = offset + limit < total as usize;
        let page: Vec<(String, Vec<ExplainTrace>)> =
            all_subjects.into_iter().skip(offset).take(limit).collect();

        let subjects: Vec<SubjectWithPaths> = page
            .into_iter()
            .map(|(s, paths)| SubjectWithPaths {
                subject: s,
                paths: if include_paths { paths } else { Vec::new() },
            })
            .collect();

        let _duration_ms = start.elapsed().as_micros() as u64 / 1000;

        Ok(PaginatedSubjects {
            subjects,
            total,
            next_cursor: has_more.then_some(PaginationCursor {
                offset: (offset + limit) as u64,
                revision,
            }),
        })
    }

    /// Compute the access diff between two schema versions.
    pub fn access_diff(
        &self,
        schema_before: &Schema,
        schema_after: &Schema,
        subject_sample: Option<&[SubjectId]>,
        max_checks: Option<u64>,
    ) -> AegisResult<AccessDiffReport> {
        let start = std::time::Instant::now();
        let pid = self.active_partition_id();
        let _revision = self.resolve_revision(None)?;

        // Collect all tuples
        let all_tuples = self
            .storage
            .query_tuples(
                &pid,
                &TupleFilter::default(),
                &PaginationParams {
                    cursor: None,
                    limit: 1_000_000,
                },
                &ConsistencyMode::MinimizeLatency,
            )
            .map_err(|e| AegisError::Internal(e.to_string()))?;

        let mut gained = Vec::new();
        let mut lost = Vec::new();
        let mut unchanged: u64 = 0;
        let max = max_checks.unwrap_or(u64::MAX);

        for t in &all_tuples.tuples {
            if (gained.len() as u64 + lost.len() as u64 + unchanged) >= max {
                break;
            }

            let subject_filter = subject_sample
                .map(|s| s.contains(&t.subject))
                .unwrap_or(true);
            if !subject_filter {
                continue;
            }

            // Evaluate all permissions against both schemas for this tuple
            let resource_type = resource_type_name(t.object.as_str());

            let perms_before = schema_before
                .types
                .get(&resource_type)
                .map(|td| td.permissions.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();

            let _perms_after = schema_after
                .types
                .get(&resource_type)
                .map(|td| td.permissions.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();

            for perm in &perms_before {
                if (gained.len() as u64 + lost.len() as u64 + unchanged) >= max {
                    break;
                }

                let resolved_before =
                    crate::engine::policy::resolve_permission(schema_before, &resource_type, perm);
                let resolved_after =
                    crate::engine::policy::resolve_permission(schema_after, &resource_type, perm);

                let before_allowed = resolved_before.is_some_and(|r| {
                    r.relations
                        .iter()
                        .any(|rel| rel.as_str() == t.relation.as_str())
                });
                let after_allowed = resolved_after.is_some_and(|r| {
                    r.relations
                        .iter()
                        .any(|rel| rel.as_str() == t.relation.as_str())
                });

                let subj_str = t.subject.as_str().to_string();
                let obj_str = t.object.as_str().to_string();
                let summary = ChangeSummary {
                    subject: subj_str,
                    permission: perm.clone(),
                    resource: obj_str,
                };

                if before_allowed && !after_allowed {
                    lost.push(summary);
                } else if !before_allowed && after_allowed {
                    gained.push(summary);
                } else {
                    unchanged += 1;
                }
            }
        }

        let duration_ms = start.elapsed().as_micros() as u64 / 1000;

        Ok(AccessDiffReport {
            gained_access: gained,
            lost_access: lost,
            unchanged,
            sampled: subject_sample.is_some(),
            sampled_total: subject_sample.map(|_| all_tuples.tuples.len() as u64),
            duration_ms,
        })
    }

    /// Build an analysis report for export.
    pub fn analysis_report(
        &self,
        report_type: &str,
        data: serde_json::Value,
    ) -> AegisResult<AnalysisReport> {
        Ok(AnalysisReport {
            report_type: report_type.to_string(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            duration_ms: 0,
            data,
        })
    }
}

fn resource_type_name(resource: &str) -> String {
    resource.split(':').next().unwrap_or("").to_string()
}
