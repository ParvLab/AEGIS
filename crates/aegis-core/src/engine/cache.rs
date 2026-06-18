use crate::types::Revision;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Default time-to-live for cached decisions.
const DEFAULT_TTL: Duration = Duration::from_secs(300); // 5 minutes

/// A cached decision entry.
#[derive(Debug, Clone)]
struct CacheEntry {
    allowed: bool,
    revision: Revision,
    created_at: Instant,
}

/// Simple decision cache with revision-based and TTL-based invalidation.
///
/// Cache key: `(subject, permission, resource, partition_id)`
/// On each lookup, compares the entry's revision against current revision.
/// If stale (entry.revision < current_revision), evicts and returns None.
/// If TTL expired, evicts and returns None.
pub struct DecisionCache {
    entries: HashMap<(String, String, String, String), CacheEntry>,
    access_order: VecDeque<(String, String, String, String)>,
    capacity: usize,
    ttl: Duration,
    hits: u64,
    misses: u64,
}

impl DecisionCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            access_order: VecDeque::with_capacity(capacity),
            capacity,
            ttl: DEFAULT_TTL,
            hits: 0,
            misses: 0,
        }
    }

    /// Set a custom TTL for cache entries.
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Look up a cached decision.
    /// Returns `None` if not cached, stale, or TTL expired.
    pub fn get(
        &mut self,
        subject: &str,
        permission: &str,
        resource: &str,
        partition_id: &str,
        current_revision: Revision,
    ) -> Option<bool> {
        let key = (
            subject.to_string(),
            permission.to_string(),
            resource.to_string(),
            partition_id.to_string(),
        );

        let is_valid = self.entries.get(&key).map_or(false, |entry| {
            entry.revision >= current_revision && entry.created_at.elapsed() < self.ttl
        });

        if is_valid {
            self.hits += 1;
            // Move to MRU position
            if let Some(pos) = self.access_order.iter().position(|k| k == &key) {
                self.access_order.remove(pos);
                self.access_order.push_back(key.clone());
            }
            self.entries.get(&key).map(|e| e.allowed)
        } else if self.entries.contains_key(&key) {
            // Stale or expired entry - remove it
            self.entries.remove(&key);
            self.access_order.retain(|k| k != &key);
            self.misses += 1;
            None
        } else {
            self.misses += 1;
            None
        }
    }

    /// Insert a decision into the cache.
    pub fn insert(
        &mut self,
        subject: &str,
        permission: &str,
        resource: &str,
        partition_id: &str,
        allowed: bool,
        revision: Revision,
    ) {
        let key = (
            subject.to_string(),
            permission.to_string(),
            resource.to_string(),
            partition_id.to_string(),
        );

        // Remove existing entry from access order so it gets re-inserted at MRU
        self.access_order.retain(|k| k != &key);

        // Evict LRU entry if at capacity
        if self.entries.len() >= self.capacity {
            if let Some(lru_key) = self.access_order.pop_front() {
                self.entries.remove(&lru_key);
            }
        }

        self.access_order.push_back(key.clone());
        self.entries.insert(
            key,
            CacheEntry {
                allowed,
                revision,
                created_at: Instant::now(),
            },
        );
    }

    /// Remove all entries for a given partition.
    pub fn remove(&mut self, partition_id: &str) {
        self.entries.retain(|k, _| k.3 != partition_id);
        self.access_order.retain(|k| k.3 != partition_id);
    }

    /// Invalidate all entries with revisions older than a threshold.
    pub fn invalidate_before(&mut self, revision: Revision) {
        self.entries.retain(|_, entry| entry.revision >= revision);
        // Rebuild access_order to match remaining entries
        self.access_order.retain(|k| self.entries.contains_key(k));
    }

    /// Clear the entire cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.access_order.clear();
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

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Intermediate traversal cache: caches `(subject, relation) -> Vec<ResourceId>` lookups.
pub struct TraversalCache {
    entries: HashMap<(String, String), (Vec<String>, Revision)>,
    access_order: VecDeque<(String, String)>,
    capacity: usize,
}

impl TraversalCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            access_order: VecDeque::with_capacity(capacity),
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
        let is_valid = self
            .entries
            .get(&key)
            .map_or(false, |(_, rev)| *rev >= current_revision);
        if is_valid {
            // Move to MRU position
            if let Some(pos) = self.access_order.iter().position(|k| k == &key) {
                self.access_order.remove(pos);
                self.access_order.push_back(key.clone());
            }
            self.entries
                .get(&key)
                .map(|(resources, _)| resources.clone())
        } else {
            if self.entries.contains_key(&key) {
                self.entries.remove(&key);
                self.access_order.retain(|k| k != &key);
            }
            None
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
        let key = (subject.to_string(), relation.to_string());

        // Remove existing entry from access order
        self.access_order.retain(|k| k != &key);

        // Evict LRU entry if at capacity
        if self.entries.len() >= self.capacity {
            if let Some(lru_key) = self.access_order.pop_front() {
                self.entries.remove(&lru_key);
            }
        }

        self.access_order.push_back(key.clone());
        self.entries.insert(key, (resources, revision));
    }

    pub fn invalidate_before(&mut self, revision: Revision) {
        self.entries.retain(|_, (_, rev)| *rev >= revision);
        self.access_order.retain(|k| self.entries.contains_key(k));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.access_order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_hit() {
        let mut cache = DecisionCache::new(100);
        cache.insert(
            "user:1",
            "read",
            "repo:a",
            "default",
            true,
            Revision::new(5),
        );

        assert_eq!(
            cache.get("user:1", "read", "repo:a", "default", Revision::new(5)),
            Some(true)
        );
        assert_eq!(cache.hit_rate(), 1.0);
    }

    #[test]
    fn test_cache_miss() {
        let mut cache = DecisionCache::new(100);
        assert_eq!(
            cache.get("user:1", "read", "repo:a", "default", Revision::new(5)),
            None
        );
    }

    #[test]
    fn test_cache_stale_eviction() {
        let mut cache = DecisionCache::new(100);
        cache.insert(
            "user:1",
            "read",
            "repo:a",
            "default",
            true,
            Revision::new(5),
        );

        // Revision 10 > 5 → entry is stale
        assert_eq!(
            cache.get("user:1", "read", "repo:a", "default", Revision::new(10)),
            None
        );
    }

    #[test]
    fn test_cache_capacity() {
        let mut cache = DecisionCache::new(2);
        cache.insert(
            "user:1",
            "read",
            "repo:a",
            "default",
            true,
            Revision::new(1),
        );
        cache.insert(
            "user:2",
            "read",
            "repo:b",
            "default",
            true,
            Revision::new(2),
        );
        cache.insert(
            "user:3",
            "read",
            "repo:c",
            "default",
            true,
            Revision::new(3),
        );

        // At most 2 entries should remain (one evicted due to capacity)
        assert!(cache.len() <= 2);
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = DecisionCache::new(100);
        cache.insert(
            "user:1",
            "read",
            "repo:a",
            "default",
            true,
            Revision::new(1),
        );
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
        cache.insert(
            "user:1",
            "owner",
            vec!["repo:a".to_string()],
            Revision::new(5),
        );

        assert_eq!(cache.get("user:1", "owner", Revision::new(10)), None);
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = DecisionCache::new(3);

        // Fill cache to capacity
        cache.insert(
            "user:1",
            "read",
            "repo:a",
            "default",
            true,
            Revision::new(1),
        );
        cache.insert(
            "user:2",
            "read",
            "repo:b",
            "default",
            true,
            Revision::new(2),
        );
        cache.insert(
            "user:3",
            "read",
            "repo:c",
            "default",
            true,
            Revision::new(3),
        );

        // Access user:1 and user:2 to make them MRU
        assert_eq!(
            cache.get("user:1", "read", "repo:a", "default", Revision::new(1)),
            Some(true)
        );
        assert_eq!(
            cache.get("user:2", "read", "repo:b", "default", Revision::new(2)),
            Some(true)
        );

        // Insert 4th entry — should evict LRU entry (user:3)
        cache.insert(
            "user:4",
            "read",
            "repo:d",
            "default",
            true,
            Revision::new(4),
        );

        // user:3 should be evicted
        assert_eq!(
            cache.get("user:3", "read", "repo:c", "default", Revision::new(3)),
            None
        );
        // user:1 and user:2 should still be present
        assert_eq!(
            cache.get("user:1", "read", "repo:a", "default", Revision::new(1)),
            Some(true)
        );
        assert_eq!(
            cache.get("user:2", "read", "repo:b", "default", Revision::new(2)),
            Some(true)
        );
        // user:4 should be present
        assert_eq!(
            cache.get("user:4", "read", "repo:d", "default", Revision::new(4)),
            Some(true)
        );
    }
}
