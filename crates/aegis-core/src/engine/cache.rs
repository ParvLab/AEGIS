use crate::types::Revision;
use std::collections::HashMap;

/// A cached decision entry.
#[derive(Debug, Clone)]
struct CacheEntry {
    allowed: bool,
    revision: Revision,
}

/// Simple decision cache with revision-based invalidation.
///
/// Cache key: `(subject, permission, resource)`
/// On each lookup, compares the entry's revision against current revision.
/// If stale (entry.revision < current_revision), evicts and returns None.
pub struct DecisionCache {
    entries: HashMap<(String, String, String), CacheEntry>,
    capacity: usize,
    hits: u64,
    misses: u64,
}

impl DecisionCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            capacity,
            hits: 0,
            misses: 0,
        }
    }

    /// Look up a cached decision.
    /// Returns `None` if not cached or stale.
    pub fn get(
        &mut self,
        subject: &str,
        permission: &str,
        resource: &str,
        current_revision: Revision,
    ) -> Option<bool> {
        let key = (
            subject.to_string(),
            permission.to_string(),
            resource.to_string(),
        );

        match self.entries.get(&key) {
            Some(entry) if entry.revision >= current_revision => {
                self.hits += 1;
                Some(entry.allowed)
            }
            Some(_) => {
                // Stale entry - remove it
                self.entries.remove(&key);
                self.misses += 1;
                None
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// Insert a decision into the cache.
    pub fn insert(
        &mut self,
        subject: &str,
        permission: &str,
        resource: &str,
        allowed: bool,
        revision: Revision,
    ) {
        // Evict oldest entry if at capacity
        if self.entries.len() >= self.capacity {
            if let Some(key) = self.entries.keys().next().cloned() {
                self.entries.remove(&key);
            }
        }

        let key = (
            subject.to_string(),
            permission.to_string(),
            resource.to_string(),
        );

        self.entries.insert(
            key,
            CacheEntry {
                allowed,
                revision,
            },
        );
    }

    /// Invalidate all entries with revisions older than a threshold.
    pub fn invalidate_before(&mut self, revision: Revision) {
        self.entries.retain(|_, entry| entry.revision >= revision);
    }

    /// Clear the entire cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.hits = 0;
        self.misses = 0;
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Intermediate traversal cache: caches `(subject, relation) -> Vec<ResourceId>` lookups.
pub struct TraversalCache {
    entries: HashMap<(String, String), (Vec<String>, Revision)>,
    capacity: usize,
}

impl TraversalCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            capacity,
        }
    }

    /// Get cached reachable resources for a subject+relation pair.
    pub fn get(
        &mut self,
        subject: &str,
        relation: &str,
        current_revision: Revision,
    ) -> Option<Vec<String>> {
        let key = (subject.to_string(), relation.to_string());
        match self.entries.get(&key) {
            Some((resources, rev)) if *rev >= current_revision => Some(resources.clone()),
            _ => None,
        }
    }

    /// Set cached reachable resources.
    pub fn insert(
        &mut self,
        subject: &str,
        relation: &str,
        resources: Vec<String>,
        revision: Revision,
    ) {
        if self.entries.len() >= self.capacity {
            if let Some(key) = self.entries.keys().next().cloned() {
                self.entries.remove(&key);
            }
        }
        self.entries.insert(
            (subject.to_string(), relation.to_string()),
            (resources, revision),
        );
    }

    pub fn invalidate_before(&mut self, revision: Revision) {
        self.entries.retain(|_, (_, rev)| *rev >= revision);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_hit() {
        let mut cache = DecisionCache::new(100);
        cache.insert("user:1", "read", "repo:a", true, Revision::new(5));

        assert_eq!(
            cache.get("user:1", "read", "repo:a", Revision::new(5)),
            Some(true)
        );
        assert_eq!(cache.hit_rate(), 1.0);
    }

    #[test]
    fn test_cache_miss() {
        let mut cache = DecisionCache::new(100);
        assert_eq!(
            cache.get("user:1", "read", "repo:a", Revision::new(5)),
            None
        );
    }

    #[test]
    fn test_cache_stale_eviction() {
        let mut cache = DecisionCache::new(100);
        cache.insert("user:1", "read", "repo:a", true, Revision::new(5));

        // Revision 10 > 5 → entry is stale
        assert_eq!(
            cache.get("user:1", "read", "repo:a", Revision::new(10)),
            None
        );
    }

    #[test]
    fn test_cache_capacity() {
        let mut cache = DecisionCache::new(2);
        cache.insert("user:1", "read", "repo:a", true, Revision::new(1));
        cache.insert("user:2", "read", "repo:b", true, Revision::new(2));
        cache.insert("user:3", "read", "repo:c", true, Revision::new(3));

        // At most 2 entries should remain (one evicted due to capacity)
        assert!(cache.len() <= 2);
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = DecisionCache::new(100);
        cache.insert("user:1", "read", "repo:a", true, Revision::new(1));
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.hit_rate(), 0.0);
    }

    #[test]
    fn test_traversal_cache() {
        let mut cache = TraversalCache::new(100);
        let resources = vec!["repo:a".to_string(), "repo:b".to_string()];
        cache.insert("user:1", "owner", resources.clone(), Revision::new(5));

        let result = cache.get("user:1", "owner", Revision::new(5));
        assert_eq!(result, Some(resources));
    }

    #[test]
    fn test_traversal_cache_stale() {
        let mut cache = TraversalCache::new(100);
        cache.insert("user:1", "owner", vec!["repo:a".to_string()], Revision::new(5));

        assert_eq!(cache.get("user:1", "owner", Revision::new(10)), None);
    }
}
