#![allow(clippy::module_name_repetitions)]

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry<K, V> {
    key: K,
    value: V,
}

/// Deterministic insertion-order map used to replace `indexmap`.
///
/// The structure keeps a `Vec` of entries to preserve insertion order and a
/// parallel hash map that tracks the index for each key. The implementation is
/// intentionally conservative: every operation updates the index map eagerly so
/// callers can rely on stable iteration order while still benefiting from
/// `O(1)` lookups.
#[derive(Default)]
pub struct OrderedMap<K, V>
where
    K: Eq + Hash + Clone,
{
    indices: HashMap<K, usize>,
    entries: Vec<Entry<K, V>>,
}

impl<K, V> OrderedMap<K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn new() -> Self {
        Self {
            indices: HashMap::new(),
            entries: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            indices: HashMap::with_capacity(capacity),
            entries: Vec::with_capacity(capacity),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.indices.clear();
        self.entries.clear();
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.indices.contains_key(key)
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some(index) = self.indices.get(&key).copied() {
            let entry = &mut self.entries[index];
            Some(std::mem::replace(&mut entry.value, value))
        } else {
            let index = self.entries.len();
            self.indices.insert(key.clone(), index);
            self.entries.push(Entry { key, value });
            None
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.indices
            .get(key)
            .map(|&index| &self.entries[index].value)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.indices
            .get(key)
            .copied()
            .map(|index| &mut self.entries[index].value)
    }

    pub fn keys(&self) -> Keys<'_, K, V> {
        Keys {
            inner: self.entries.iter(),
        }
    }

    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            inner: self.entries.iter(),
        }
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
        IterMut {
            inner: self.entries.iter_mut(),
        }
    }

    pub fn values(&self) -> Values<'_, K, V> {
        Values {
            inner: self.entries.iter(),
        }
    }

    pub fn values_mut(&mut self) -> ValuesMut<'_, K, V> {
        ValuesMut {
            inner: self.entries.iter_mut(),
        }
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&K, &mut V) -> bool,
    {
        let mut i = 0;
        while i < self.entries.len() {
            let key_clone = self.entries[i].key.clone();
            let keep = {
                let value = &mut self.entries[i].value;
                f(&key_clone, value)
            };
            if keep {
                i += 1;
            } else {
                self.entries.remove(i);
                self.indices.remove(&key_clone);
                self.reindex_from(i);
            }
        }
    }

    pub fn swap_remove(&mut self, key: &K) -> Option<V> {
        let index = self.indices.get(key).copied()?;
        self.swap_remove_index(index).map(|(_, value)| value)
    }

    pub fn swap_remove_index(&mut self, index: usize) -> Option<(K, V)> {
        if index >= self.entries.len() {
            return None;
        }
        let entry = self.entries.swap_remove(index);
        self.indices.remove(&entry.key);
        if index < self.entries.len() {
            let moved_key = self.entries[index].key.clone();
            if let Some(idx) = self.indices.get_mut(&moved_key) {
                *idx = index;
            }
        }
        Some((entry.key, entry.value))
    }

    pub fn shift_remove_entry(&mut self, key: &K) -> Option<(K, V)> {
        let index = self.indices.get(key).copied()?;
        let entry = self.entries.remove(index);
        self.indices.remove(key);
        self.reindex_from(index);
        Some((entry.key, entry.value))
    }

    fn reindex_from(&mut self, start: usize) {
        for (idx, entry) in self.entries.iter().enumerate().skip(start) {
            if let Some(slot) = self.indices.get_mut(&entry.key) {
                *slot = idx;
            }
        }
    }

    pub fn entry(&mut self, key: K) -> EntryView<'_, K, V> {
        if let Some(index) = self.indices.get(&key).copied() {
            EntryView::Occupied(OccupiedEntry { map: self, index })
        } else {
            EntryView::Vacant(VacantEntry { map: self, key })
        }
    }
}

impl<K, V> Clone for OrderedMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        Self {
            indices: self.indices.clone(),
            entries: self.entries.clone(),
        }
    }
}

impl<K, V> std::fmt::Debug for OrderedMap<K, V>
where
    K: Eq + Hash + Clone + std::fmt::Debug,
    V: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<K, V> PartialEq for OrderedMap<K, V>
where
    K: Eq + Hash + Clone,
    V: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl<K, V> Eq for OrderedMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Eq,
{
}

pub struct Iter<'a, K, V> {
    inner: std::slice::Iter<'a, Entry<K, V>>,
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|entry| (&entry.key, &entry.value))
    }
}

pub struct IterMut<'a, K, V> {
    inner: std::slice::IterMut<'a, Entry<K, V>>,
}

impl<'a, K, V> Iterator for IterMut<'a, K, V> {
    type Item = (&'a K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|entry| (&entry.key, &mut entry.value))
    }
}

pub struct Keys<'a, K, V> {
    inner: std::slice::Iter<'a, Entry<K, V>>,
}

impl<'a, K, V> Iterator for Keys<'a, K, V> {
    type Item = &'a K;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|entry| &entry.key)
    }
}

pub struct Values<'a, K, V> {
    inner: std::slice::Iter<'a, Entry<K, V>>,
}

impl<'a, K, V> Iterator for Values<'a, K, V> {
    type Item = &'a V;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|entry| &entry.value)
    }
}

pub struct ValuesMut<'a, K, V> {
    inner: std::slice::IterMut<'a, Entry<K, V>>,
}

impl<'a, K, V> Iterator for ValuesMut<'a, K, V> {
    type Item = &'a mut V;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|entry| &mut entry.value)
    }
}

pub enum EntryView<'a, K, V>
where
    K: Eq + Hash + Clone,
{
    Occupied(OccupiedEntry<'a, K, V>),
    Vacant(VacantEntry<'a, K, V>),
}

impl<'a, K, V> EntryView<'a, K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn or_insert(self, value: V) -> &'a mut V {
        match self {
            EntryView::Occupied(entry) => entry.into_mut(),
            EntryView::Vacant(entry) => entry.insert(value),
        }
    }

    pub fn or_insert_with<F>(self, f: F) -> &'a mut V
    where
        F: FnOnce() -> V,
    {
        match self {
            EntryView::Occupied(entry) => entry.into_mut(),
            EntryView::Vacant(entry) => entry.insert(f()),
        }
    }
}

pub struct OccupiedEntry<'a, K, V>
where
    K: Eq + Hash + Clone,
{
    map: &'a mut OrderedMap<K, V>,
    index: usize,
}

impl<'a, K, V> OccupiedEntry<'a, K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn get(&self) -> &V {
        &self.map.entries[self.index].value
    }

    pub fn get_mut(&mut self) -> &mut V {
        &mut self.map.entries[self.index].value
    }

    pub fn into_mut(self) -> &'a mut V {
        &mut self.map.entries[self.index].value
    }
}

pub struct VacantEntry<'a, K, V>
where
    K: Eq + Hash + Clone,
{
    map: &'a mut OrderedMap<K, V>,
    key: K,
}

impl<'a, K, V> VacantEntry<'a, K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn insert(self, value: V) -> &'a mut V {
        let index = self.map.entries.len();
        self.map.indices.insert(self.key.clone(), index);
        self.map.entries.push(Entry {
            key: self.key,
            value,
        });
        &mut self.map.entries[index].value
    }
}

/// Ordered set backed by [`OrderedMap`].
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct OrderedSet<K>
where
    K: Eq + Hash + Clone,
{
    map: OrderedMap<K, ()>,
}

impl<K> OrderedSet<K>
where
    K: Eq + Hash + Clone,
{
    pub fn new() -> Self {
        Self {
            map: OrderedMap::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            map: OrderedMap::with_capacity(capacity),
        }
    }

    pub fn insert(&mut self, value: K) -> bool {
        self.map.insert(value, ()).is_none()
    }

    pub fn contains(&self, value: &K) -> bool {
        self.map.contains_key(value)
    }

    pub fn remove(&mut self, value: &K) -> bool {
        self.map.swap_remove(value).is_some()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn iter(&self) -> Keys<'_, K, ()> {
        self.map.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::OrderedMap;

    #[test]
    fn preserves_insertion_order() {
        let mut map = OrderedMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        let keys: Vec<_> = map.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn swap_remove_keeps_consistent_indices() {
        let mut map = OrderedMap::new();
        map.insert(1, "one");
        map.insert(2, "two");
        map.insert(3, "three");
        let removed = map.swap_remove(&2);
        assert_eq!(removed, Some("two"));
        assert_eq!(map.len(), 2);
        assert!(map.get(&2).is_none());
        let remaining: Vec<_> = map.iter().map(|(k, v)| (*k, *v)).collect();
        assert!(remaining.contains(&(1, "one")));
        assert!(remaining.contains(&(3, "three")));
    }

    #[test]
    fn shift_remove_preserves_order() {
        let mut map = OrderedMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        let removed = map.shift_remove_entry(&"b");
        assert_eq!(removed, Some(("b", 2)));
        let keys: Vec<_> = map.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["a", "c"]);
    }

    #[test]
    fn entry_api_inserts_and_updates() {
        let mut map = OrderedMap::new();
        map.entry("a").or_insert(1);
        assert_eq!(map.get(&"a"), Some(&1));
        map.entry("a").or_insert(2);
        assert_eq!(map.get(&"a"), Some(&1));
        *map.entry("a").or_insert(0) = 5;
        assert_eq!(map.get(&"a"), Some(&5));
    }
}
