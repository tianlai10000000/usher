use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct TimedValue<V> {
    value: Arc<V>,
    updated_at: Instant,
}

pub(super) struct PrefixCache<K, V> {
    capacity: usize,
    ttl: Duration,
    entries: HashMap<K, TimedValue<V>>,
    order: VecDeque<K>,
}

impl<K, V> PrefixCache<K, V>
where
    K: Clone + Eq + Hash,
{
    pub(super) fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity: capacity.max(1),
            ttl,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub(super) fn get(&mut self, key: &K) -> Option<Arc<V>> {
        self.get_at(key, Instant::now())
    }

    pub(super) fn get_at(&mut self, key: &K, now: Instant) -> Option<Arc<V>> {
        let entry = self.entries.get(key)?;
        if now.saturating_duration_since(entry.updated_at) > self.ttl {
            self.remove(key);
            return None;
        }
        let value = entry.value.clone();
        self.order.retain(|candidate| candidate != key);
        self.order.push_back(key.clone());
        Some(value)
    }

    pub(super) fn insert(&mut self, key: K, value: V) {
        self.insert_at(key, value, Instant::now());
    }

    pub(super) fn insert_at(&mut self, key: K, value: V, now: Instant) {
        self.remove(&key);
        while self.entries.len() >= self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                self.entries.clear();
                break;
            };
            self.entries.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.entries.insert(
            key,
            TimedValue {
                value: Arc::new(value),
                updated_at: now,
            },
        );
    }

    pub(super) fn remove(&mut self, key: &K) {
        self.entries.remove(key);
        self.order.retain(|candidate| candidate != key);
    }

    pub(super) fn invalidate_where(&mut self, predicate: impl Fn(&K) -> bool) {
        self.entries.retain(|key, _| !predicate(key));
        self.order.retain(|key| !predicate(key));
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(super) fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    #[cfg(test)]
    pub(super) fn newest_key(&self) -> Option<&K> {
        self.order.back()
    }
}

#[cfg(test)]
mod tests {
    use super::PrefixCache;
    use std::time::{Duration, Instant};

    #[test]
    fn cache_is_lru_bounded_and_ttl_expiring() {
        let now = Instant::now();
        let mut cache = PrefixCache::new(2, Duration::from_secs(30));
        cache.insert_at("one", 1, now);
        cache.insert_at("two", 2, now);
        assert_eq!(*cache.get_at(&"one", now).unwrap(), 1);
        cache.insert_at("three", 3, now);
        assert!(cache.get_at(&"two", now).is_none());
        assert!(
            cache
                .get_at(&"one", now + Duration::from_secs(31))
                .is_none()
        );
    }

    #[test]
    fn invalidation_removes_only_matching_keys() {
        let mut cache = PrefixCache::new(4, Duration::from_secs(30));
        cache.insert(("repo-a", "closed"), 1);
        cache.insert(("repo-a", "merged"), 2);
        cache.insert(("repo-b", "closed"), 3);

        cache.invalidate_where(|(repo, _)| *repo == "repo-a");

        assert!(cache.get(&("repo-a", "closed")).is_none());
        assert_eq!(*cache.get(&("repo-b", "closed")).unwrap(), 3);
    }
}
