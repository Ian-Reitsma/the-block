use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::Arc;

#[derive(Debug)]
pub struct SharedKey<K>(Arc<K>);

impl<K> Clone for SharedKey<K> {
    fn clone(&self) -> Self {
        SharedKey(Arc::clone(&self.0))
    }
}

impl<K> SharedKey<K> {
    fn new(key: K) -> Self {
        SharedKey(Arc::new(key))
    }

    fn as_ref(&self) -> &K {
        self.0.as_ref()
    }

    fn try_unwrap(self) -> Result<K, Arc<K>> {
        Arc::try_unwrap(self.0)
    }
}

impl<K: Eq> PartialEq for SharedKey<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<K: Eq> Eq for SharedKey<K> {}

impl<K: Hash> Hash for SharedKey<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}

impl<K> Borrow<K> for SharedKey<K> {
    fn borrow(&self) -> &K {
        self.as_ref()
    }
}

impl Borrow<str> for SharedKey<String> {
    fn borrow(&self) -> &str {
        self.as_ref()
    }
}

#[derive(Debug)]
struct Node<K, V> {
    key: SharedKey<K>,
    value: V,
    prev: Option<usize>,
    next: Option<usize>,
}

impl<K, V> Node<K, V> {
    fn new(key: SharedKey<K>, value: V) -> Self {
        Node {
            key,
            value,
            prev: None,
            next: None,
        }
    }
}

/// Minimal lock-free LRU cache used throughout the node for replay detection,
/// signature memoization, and explorer view materialization.
#[derive(Debug)]
pub struct LruCache<K, V> {
    map: HashMap<SharedKey<K>, usize>,
    nodes: Vec<Option<Node<K, V>>>,
    free_list: Vec<usize>,
    head: Option<usize>,
    tail: Option<usize>,
    len: usize,
    cap: NonZeroUsize,
}

impl<K, V> LruCache<K, V>
where
    K: Eq + Hash,
{
    /// Construct a cache with the provided non-zero capacity.
    #[must_use]
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            map: HashMap::new(),
            nodes: Vec::new(),
            free_list: Vec::new(),
            head: None,
            tail: None,
            len: 0,
            cap: capacity,
        }
    }

    /// Returns the configured capacity.
    #[must_use]
    pub fn cap(&self) -> NonZeroUsize {
        self.cap
    }

    /// Number of items currently stored in the cache.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` when the cache stores no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Removes every cached entry.
    pub fn clear(&mut self) {
        self.map.clear();
        self.nodes.clear();
        self.free_list.clear();
        self.head = None;
        self.tail = None;
        self.len = 0;
    }

    /// Returns whether the cache currently stores an entry for `key` without
    /// updating its position.
    #[must_use]
    pub fn contains<Q>(&self, key: &Q) -> bool
    where
        SharedKey<K>: Borrow<Q>,
        Q: ?Sized + Eq + Hash,
    {
        self.map.contains_key(key)
    }

    /// Returns the value for `key` while keeping the access order unchanged.
    pub fn peek<Q>(&mut self, key: &Q) -> Option<&V>
    where
        SharedKey<K>: Borrow<Q>,
        Q: ?Sized + Eq + Hash,
    {
        let idx = *self.map.get(key)?;
        self.nodes[idx].as_ref().map(|node| &node.value)
    }

    /// Returns the least recently used entry without touching the cache.
    pub fn peek_lru(&self) -> Option<(&K, &V)> {
        let idx = self.tail?;
        self.nodes[idx]
            .as_ref()
            .map(|node| (node.key.as_ref(), &node.value))
    }

    /// Fetch the value for `key`, marking it as the most recently used entry.
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        SharedKey<K>: Borrow<Q>,
        Q: ?Sized + Eq + Hash,
    {
        let idx = *self.map.get(key)?;
        self.move_to_front(idx);
        self.nodes[idx].as_ref().map(|node| &node.value)
    }

    /// Insert (or replace) the value associated with `key` and return the
    /// previous value if one existed.
    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        if let Some(&idx) = self.map.get(&key) {
            let node = self.nodes[idx].as_mut().expect("lru node missing");
            let old = std::mem::replace(&mut node.value, value);
            self.move_to_front(idx);
            return Some(old);
        }

        let key_shared = SharedKey::new(key);
        let idx = self.allocate_slot();
        self.map.insert(key_shared.clone(), idx);
        self.nodes[idx] = Some(Node::new(key_shared, value));
        self.attach_front(idx);
        self.len += 1;
        self.evict_excess();
        None
    }

    /// Remove the entry keyed by `key`, returning the stored value when
    /// present.
    pub fn pop<Q>(&mut self, key: &Q) -> Option<V>
    where
        SharedKey<K>: Borrow<Q>,
        Q: ?Sized + Eq + Hash,
    {
        let idx = self.map.remove(key)?;
        self.detach(idx);
        let node = self.take_node(idx);
        self.len -= 1;
        Some(node.value)
    }

    /// Removes and returns the least recently used entry.
    pub fn pop_lru(&mut self) -> Option<(K, V)> {
        let idx = self.tail?;
        self.detach(idx);
        let node = self.take_node(idx);
        self.map
            .remove(&node.key)
            .expect("lru map/tail out of sync");
        self.len -= 1;
        let key = match node.key.try_unwrap() {
            Ok(key) => key,
            Err(_) => panic!("dangling LRU key reference"),
        };
        Some((key, node.value))
    }

    fn allocate_slot(&mut self) -> usize {
        if let Some(idx) = self.free_list.pop() {
            return idx;
        }
        let idx = self.nodes.len();
        self.nodes.push(None);
        idx
    }

    fn take_node(&mut self, idx: usize) -> Node<K, V> {
        if let Some(node) = self.nodes[idx].take() {
            self.free_list.push(idx);
            node
        } else {
            panic!("attempted to take missing node at {idx}");
        }
    }

    fn move_to_front(&mut self, idx: usize) {
        if self.head == Some(idx) {
            return;
        }
        self.detach(idx);
        self.attach_front(idx);
    }

    fn attach_front(&mut self, idx: usize) {
        {
            let node = self.nodes[idx].as_mut().expect("missing node");
            node.prev = None;
            node.next = self.head;
        }
        if let Some(old_head) = self.head {
            if let Some(head_node) = self.nodes[old_head].as_mut() {
                head_node.prev = Some(idx);
            }
        } else {
            self.tail = Some(idx);
        }
        self.head = Some(idx);
    }

    fn detach(&mut self, idx: usize) {
        let (prev, next) = {
            let node = self.nodes[idx].as_ref().expect("missing node");
            (node.prev, node.next)
        };

        match prev {
            Some(prev_idx) => {
                if let Some(prev_node) = self.nodes[prev_idx].as_mut() {
                    prev_node.next = next;
                }
            }
            None => {
                self.head = next;
            }
        }

        match next {
            Some(next_idx) => {
                if let Some(next_node) = self.nodes[next_idx].as_mut() {
                    next_node.prev = prev;
                }
            }
            None => {
                self.tail = prev;
            }
        }

        if let Some(node) = self.nodes[idx].as_mut() {
            node.prev = None;
            node.next = None;
        }
    }

    fn evict_excess(&mut self) {
        while self.len > self.cap.get() {
            let _ = self.pop_lru();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LruCache;
    use std::num::NonZeroUsize;

    #[test]
    fn maintains_order_on_get() {
        let mut cache = LruCache::new(NonZeroUsize::new(2).unwrap());
        cache.put(1, "a");
        cache.put(2, "b");
        assert_eq!(cache.peek_lru().map(|(k, _)| *k), Some(1));
        assert_eq!(cache.get(&1), Some(&"a"));
        assert_eq!(cache.peek_lru().map(|(k, _)| *k), Some(2));
    }

    #[test]
    fn evicts_least_recently_used() {
        let mut cache = LruCache::new(NonZeroUsize::new(2).unwrap());
        cache.put(1, "a");
        cache.put(2, "b");
        cache.put(3, "c");
        assert_eq!(cache.len(), 2);
        assert!(cache.contains(&2));
        assert!(cache.contains(&3));
        assert!(!cache.contains(&1));
    }

    #[test]
    fn pop_and_pop_lru_return_values() {
        let mut cache = LruCache::new(NonZeroUsize::new(2).unwrap());
        cache.put(1, "a");
        cache.put(2, "b");
        assert_eq!(cache.pop(&1), Some("a"));
        assert_eq!(cache.len(), 1);
        let (key, value) = cache.pop_lru().expect("pop_lru");
        assert_eq!(key, 2);
        assert_eq!(value, "b");
        assert!(cache.is_empty());
    }
}
