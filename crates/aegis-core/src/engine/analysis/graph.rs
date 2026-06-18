use crate::engine::GraphEngine;
use crate::error::AegisResult;
use crate::storage::{StorageBackend, TupleFilter};
use crate::types::analysis::*;
use crate::types::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

impl GraphEngine {
    /// Find all subjects reachable from a resource through the authorization graph.
    /// Bounded by max_depth, max_nodes, and timeout_ms.
    /// Results cached with configurable TTL.
    pub fn reachable_subjects(
        &self,
        resource: &ResourceId,
        max_depth: u32,
        max_nodes: u64,
        timeout_ms: u64,
        cache_ttl_ms: Option<u64>,
    ) -> AegisResult<ReachabilityReport> {
        // Cache check
        let cache_key = format!("reach:{}:{}:{}", resource.as_str(), max_depth, max_nodes);
        if let Some(ttl) = cache_ttl_ms {
            if let Some(cached) = self.get_cached_analysis(&cache_key, ttl) {
                return Ok(cached);
            }
        }

        let start = Instant::now();
        let pid = self.active_partition_id();
        let storage: &dyn StorageBackend = self.storage.as_ref();
        let _revision = self.resolve_revision(None)?;

        let mut visited_subjects: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, u32)> = VecDeque::new();
        let mut truncated = false;

        // Seed with direct tuples on this resource
        let all_tuples = storage
            .list_by_object(&pid, resource, None, &ConsistencyMode::MinimizeLatency)
            .unwrap_or_default();

        for t in &all_tuples {
            let subj = t.subject.as_str().to_string();
            if visited_subjects.insert(subj.clone()) {
                queue.push_back((subj, 0));
            }
            if visited_subjects.len() as u64 >= max_nodes {
                truncated = true;
                break;
            }
        }

        // BFS outward
        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth || start.elapsed().as_millis() as u64 > timeout_ms {
                truncated = true;
                continue;
            }

            let subject_id = match SubjectId::new(&current) {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Find edges: tuples where current is the subject
            let edges = storage
                .list_by_subject(&pid, &subject_id, None, &ConsistencyMode::MinimizeLatency)
                .unwrap_or_default();

            for t in &edges {
                let next = t.object.as_str().to_string();
                if visited_subjects.insert(next.clone()) {
                    queue.push_back((next, depth + 1));
                }
                if visited_subjects.len() as u64 >= max_nodes {
                    truncated = true;
                    break;
                }
            }

            if truncated {
                break;
            }
        }

        let duration_ms = start.elapsed().as_micros() as u64 / 1000;

        let report = ReachabilityReport {
            subject_count: visited_subjects.len() as u64,
            max_depth_reached: max_depth,
            truncated,
            duration_ms,
        };

        // Cache the result
        if let Some(ttl) = cache_ttl_ms {
            self.set_cached_analysis(&cache_key, &report, ttl);
        }

        Ok(report)
    }

    /// Find tuples that reference relations no longer in the active schema.
    /// Critical after schema migration.
    pub fn find_orphaned_tuples(&self) -> AegisResult<Vec<TupleKey>> {
        let pid = self.active_partition_id();
        let storage: &dyn StorageBackend = self.storage.as_ref();
        let schema = self.schema.read().unwrap();

        let all = storage
            .query_tuples(
                &pid,
                &TupleFilter::default(),
                &PaginationParams {
                    cursor: None,
                    limit: 1_000_000,
                },
                &ConsistencyMode::MinimizeLatency,
            )
            .map_err(|e| crate::error::AegisError::Internal(e.to_string()))?;

        let mut orphans = Vec::new();

        for t in &all.tuples {
            let resource_type = t.object.as_str().split(':').next().unwrap_or("");
            let type_def = schema.types.get(resource_type);
            let relation_valid = type_def.map_or(false, |td| {
                td.relations.contains_key(t.relation.as_str())
                    || td.permissions.contains_key(t.relation.as_str())
                    || td.deny.iter().any(|d| {
                        d.relations
                            .iter()
                            .any(|r| r.as_str() == t.relation.as_str())
                    })
            });

            if !relation_valid {
                orphans.push(TupleKey {
                    subject: t.subject.clone(),
                    relation: t.relation.clone(),
                    object: t.object.clone(),
                });
            }
        }

        Ok(orphans)
    }

    /// Find subjects that have access to more than `threshold` resources.
    /// Objective — does not infer business intent, just reports facts.
    pub fn find_high_access_subjects(&self, threshold: u64) -> AegisResult<Vec<HighAccessSubject>> {
        let pid = self.active_partition_id();
        let storage: &dyn StorageBackend = self.storage.as_ref();

        let all = storage
            .query_tuples(
                &pid,
                &TupleFilter::default(),
                &PaginationParams {
                    cursor: None,
                    limit: 1_000_000,
                },
                &ConsistencyMode::MinimizeLatency,
            )
            .map_err(|e| crate::error::AegisError::Internal(e.to_string()))?;

        let mut subject_resource_count: HashMap<String, HashSet<String>> = HashMap::new();

        for t in &all.tuples {
            let subj = t.subject.as_str().to_string();
            let obj = t.object.as_str().to_string();
            subject_resource_count.entry(subj).or_default().insert(obj);
        }

        let mut result: Vec<HighAccessSubject> = subject_resource_count
            .into_iter()
            .filter_map(|(subject, resources)| {
                let count = resources.len() as u64;
                if count > threshold {
                    Some(HighAccessSubject {
                        subject,
                        resource_count: count,
                    })
                } else {
                    None
                }
            })
            .collect();

        result.sort_by(|a, b| b.resource_count.cmp(&a.resource_count));
        Ok(result)
    }

    // --- Internal cache for analysis results ---

    fn get_cached_analysis<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
        _ttl_ms: u64,
    ) -> Option<T> {
        // Simple in-memory analysis cache (single-entry for now)
        let cache = self.analysis_cache.lock().ok()?;
        let entry = cache.get(key)?;
        if entry.0.elapsed().as_millis() as u64 <= entry.1 {
            Some(serde_json::from_str(&entry.2).ok()?)
        } else {
            None
        }
    }

    fn set_cached_analysis(&self, key: &str, value: &impl serde::Serialize, ttl_ms: u64) {
        if let Ok(mut cache) = self.analysis_cache.lock() {
            if let Ok(json) = serde_json::to_string(value) {
                cache.insert(key.to_string(), (Instant::now(), ttl_ms, json));
            }
        }
    }
}

/// Cross-partition leakage detection — integrated into IntegrityReport.
pub fn detect_tenant_leakage(
    storage: &dyn StorageBackend,
) -> crate::error::AegisResult<(bool, usize)> {
    let default_pid = PartitionId::default();
    let all = storage
        .query_tuples(
            &default_pid,
            &TupleFilter::default(),
            &PaginationParams {
                cursor: None,
                limit: 1_000_000,
            },
            &ConsistencyMode::MinimizeLatency,
        )
        .map_err(|e| crate::error::AegisError::Internal(e.to_string()))?;

    let mut partitions_found: HashSet<String> = HashSet::new();

    for t in &all.tuples {
        // Check if subject and object belong to different partitions
        let subj_parts: Vec<&str> = t.subject.as_str().split(':').collect();
        let obj_parts: Vec<&str> = t.object.as_str().split(':').collect();

        if subj_parts.len() >= 2 && obj_parts.len() >= 2 {
            let subj_ns = subj_parts[0];
            let obj_ns = obj_parts[0];

            if subj_ns != obj_ns {
                partitions_found.insert(format!("{}→{}", subj_ns, obj_ns));
            }
        }
    }

    let leaked = partitions_found.len();
    Ok((leaked == 0, leaked))
}
