#![forbid(unsafe_code)]

pub mod bytes;
pub mod cache;
pub mod collections;

pub use bytes::{Bytes, BytesMut};
pub use cache::LruCache;
pub use collections::{OrderedMap, OrderedSet};

use std::{
    collections::HashMap,
    hash::Hash,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, OnceLock, RwLock},
};

pub use std::sync::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};

pub trait MutexExt<T> {
    fn guard(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn guard(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|err| err.into_inner())
    }
}

impl<T> MutexExt<T> for Arc<Mutex<T>> {
    fn guard(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|err| err.into_inner())
    }
}

/// Lightweight drop-in for `once_cell::sync::Lazy` while the full
/// concurrency primitives are implemented.
pub struct Lazy<T> {
    init: fn() -> T,
    cell: OnceLock<T>,
}

impl<T> Lazy<T> {
    pub const fn new(init: fn() -> T) -> Self {
        Lazy {
            init,
            cell: OnceLock::new(),
        }
    }

    pub fn get(&self) -> &T {
        self.cell.get_or_init(self.init)
    }

    pub fn force(this: &Self) -> &T {
        this.get()
    }
}

impl<T> Default for Lazy<T>
where
    T: Default + Send + 'static,
{
    fn default() -> Self {
        Lazy::new(T::default)
    }
}

impl<T> std::ops::Deref for Lazy<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl<T> MutexExt<T> for Lazy<Mutex<T>> {
    fn guard(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|err| err.into_inner())
    }
}

/// Minimal replacement for `once_cell::sync::OnceCell`.
pub struct OnceCell<T> {
    cell: OnceLock<T>,
}

impl<T> OnceCell<T> {
    pub const fn new() -> Self {
        OnceCell {
            cell: OnceLock::new(),
        }
    }

    pub fn get(&self) -> Option<&T> {
        self.cell.get()
    }

    pub fn set(&self, value: T) -> Result<(), T> {
        self.cell.set(value)
    }

    pub fn get_or_init<F>(&self, init: F) -> &T
    where
        F: FnOnce() -> T,
    {
        self.cell.get_or_init(init)
    }
}

impl<T> Default for OnceCell<T> {
    fn default() -> Self {
        OnceCell::new()
    }
}

/// Simple single-threaded `DashMap` substitute backed by a mutex-protected
/// `HashMap`. The implementation is intentionally minimal while the
/// lock-free structures are developed.
pub struct DashMap<K, V> {
    inner: Mutex<HashMap<K, V>>,
}

impl<K, V> DashMap<K, V>
where
    K: Eq + Hash,
{
    pub fn new() -> Self {
        DashMap {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn guard(&self) -> MutexGuard<'_, HashMap<K, V>> {
        self.inner.lock().unwrap_or_else(|err| err.into_inner())
    }

    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.guard().insert(key, value)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.guard().contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.guard().len()
    }

    pub fn is_empty(&self) -> bool {
        self.guard().is_empty()
    }

    pub fn get(&self, key: &K) -> Option<Ref<'_, K, V>>
    where
        K: Clone,
    {
        let guard = self.guard();
        if !guard.contains_key(key) {
            return None;
        }
        Some(Ref {
            key: key.clone(),
            guard,
        })
    }

    pub fn get_mut(&self, key: &K) -> Option<RefMut<'_, K, V>>
    where
        K: Clone,
    {
        let guard = self.guard();
        if !guard.contains_key(key) {
            return None;
        }
        Some(RefMut {
            key: key.clone(),
            guard,
        })
    }

    pub fn entry(&self, key: K) -> Entry<'_, K, V>
    where
        K: Clone,
    {
        let guard = self.guard();
        if guard.contains_key(&key) {
            Entry::Occupied(OccupiedEntry { key, guard })
        } else {
            Entry::Vacant(VacantEntry { key, guard })
        }
    }

    pub fn remove(&self, key: &K) -> Option<(K, V)>
    where
        K: Clone,
    {
        let mut guard = self.guard();
        guard.remove(key).map(|value| (key.clone(), value))
    }

    pub fn clear(&self) {
        self.guard().clear();
    }

    pub fn retain<F>(&self, mut f: F)
    where
        F: FnMut(&K, &mut V) -> bool,
    {
        self.guard().retain(|k, v| f(k, v));
    }

    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(&K, &V),
    {
        let guard = self.guard();
        for (key, value) in guard.iter() {
            f(key, value);
        }
    }

    pub fn for_each_mut<F>(&self, mut f: F)
    where
        F: FnMut(&K, &mut V),
    {
        let mut guard = self.guard();
        for (key, value) in guard.iter_mut() {
            f(key, value);
        }
    }

    pub fn keys(&self) -> Vec<K>
    where
        K: Clone,
    {
        let guard = self.guard();
        guard.keys().cloned().collect()
    }

    pub fn values(&self) -> Vec<V>
    where
        V: Clone,
    {
        let guard = self.guard();
        guard.values().cloned().collect()
    }
}

impl<K, V> Default for DashMap<K, V>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        DashMap::new()
    }
}

pub struct Ref<'a, K, V> {
    key: K,
    guard: MutexGuard<'a, HashMap<K, V>>,
}

impl<'a, K, V> Ref<'a, K, V>
where
    K: Eq + Hash,
{
    pub fn key(&self) -> &K {
        &self.key
    }
}

impl<'a, K, V> Deref for Ref<'a, K, V>
where
    K: Eq + Hash,
{
    type Target = V;

    fn deref(&self) -> &Self::Target {
        self.guard
            .get(&self.key)
            .expect("dashmap reference missing value")
    }
}

pub struct RefMut<'a, K, V> {
    key: K,
    guard: MutexGuard<'a, HashMap<K, V>>,
}

impl<'a, K, V> RefMut<'a, K, V>
where
    K: Eq + Hash,
{
    pub fn key(&self) -> &K {
        &self.key
    }
}

impl<'a, K, V> Deref for RefMut<'a, K, V>
where
    K: Eq + Hash,
{
    type Target = V;

    fn deref(&self) -> &Self::Target {
        self.guard
            .get(&self.key)
            .expect("dashmap reference missing value")
    }
}

impl<'a, K, V> DerefMut for RefMut<'a, K, V>
where
    K: Eq + Hash,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .get_mut(&self.key)
            .expect("dashmap reference missing value")
    }
}

pub enum Entry<'a, K, V>
where
    K: Eq + Hash,
{
    Occupied(OccupiedEntry<'a, K, V>),
    Vacant(VacantEntry<'a, K, V>),
}

impl<'a, K, V> Entry<'a, K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn or_insert(self, value: V) -> RefMut<'a, K, V> {
        match self {
            Entry::Occupied(entry) => entry.into_ref(),
            Entry::Vacant(entry) => entry.insert(value),
        }
    }

    pub fn or_insert_with<F>(self, f: F) -> RefMut<'a, K, V>
    where
        F: FnOnce() -> V,
    {
        match self {
            Entry::Occupied(entry) => entry.into_ref(),
            Entry::Vacant(entry) => entry.insert(f()),
        }
    }

    pub fn or_default(self) -> RefMut<'a, K, V>
    where
        V: Default,
    {
        self.or_insert_with(V::default)
    }
}

pub struct OccupiedEntry<'a, K, V>
where
    K: Eq + Hash,
{
    key: K,
    guard: MutexGuard<'a, HashMap<K, V>>,
}

impl<'a, K, V> OccupiedEntry<'a, K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn into_ref(self) -> RefMut<'a, K, V> {
        RefMut {
            key: self.key,
            guard: self.guard,
        }
    }
}

pub struct VacantEntry<'a, K, V>
where
    K: Eq + Hash,
{
    key: K,
    guard: MutexGuard<'a, HashMap<K, V>>,
}

impl<'a, K, V> VacantEntry<'a, K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn insert(mut self, value: V) -> RefMut<'a, K, V> {
        self.guard.insert(self.key.clone(), value);
        RefMut {
            key: self.key,
            guard: self.guard,
        }
    }
}

pub mod dashmap {
    pub use crate::{Entry, OccupiedEntry, VacantEntry};
}

pub type MutexT<T> = Mutex<T>;
pub type RwLockT<T> = RwLock<T>;

pub fn mutex<T>(value: T) -> Mutex<T> {
    Mutex::new(value)
}

pub fn rw_lock<T>(value: T) -> RwLock<T> {
    RwLock::new(value)
}
