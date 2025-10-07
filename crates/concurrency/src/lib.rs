#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Mutex, OnceLock, RwLock},
};

pub use std::sync::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};

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

    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.inner
            .lock()
            .expect("dashmap poisoned")
            .insert(key, value)
    }

    pub fn get(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        self.inner
            .lock()
            .expect("dashmap poisoned")
            .get(key)
            .cloned()
    }

    pub fn remove(&self, key: &K) -> Option<V> {
        self.inner.lock().expect("dashmap poisoned").remove(key)
    }

    pub fn clear(&self) {
        self.inner.lock().expect("dashmap poisoned").clear();
    }

    pub fn retain<F>(&self, mut f: F)
    where
        F: FnMut(&K, &mut V) -> bool,
    {
        self.inner
            .lock()
            .expect("dashmap poisoned")
            .retain(|k, v| f(k, v));
    }

    pub fn values(&self) -> Vec<V>
    where
        V: Clone,
    {
        self.inner
            .lock()
            .expect("dashmap poisoned")
            .values()
            .cloned()
            .collect()
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

pub type MutexT<T> = Mutex<T>;
pub type RwLockT<T> = RwLock<T>;

pub fn mutex<T>(value: T) -> Mutex<T> {
    Mutex::new(value)
}

pub fn rw_lock<T>(value: T) -> RwLock<T> {
    RwLock::new(value)
}
