#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::vec::IntoIter;

use storage_engine::{inhouse_engine::InhouseEngine, KeyValue, KeyValueIterator, StorageError};

/// Thin adapter that exposes just enough of the first-party storage engine
/// interface for the storage-market crate.
#[derive(Clone)]
pub struct Engine {
    inner: InhouseEngine,
    base: PathBuf,
}

impl Engine {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let base = path.to_path_buf();
        let label = base.to_string_lossy().into_owned();
        let engine = InhouseEngine::open(&label)?;
        Ok(Self {
            inner: engine,
            base,
        })
    }

    pub fn open_tree<S: AsRef<str>>(&self, name: S) -> Result<Tree, StorageError> {
        let cf = name.as_ref().to_string();
        self.inner.ensure_cf(&cf)?;
        Ok(Tree {
            engine: self.inner.clone(),
            name: cf,
        })
    }

    pub fn base_path(&self) -> &Path {
        &self.base
    }

    pub fn ensure_cf<S: AsRef<str>>(&self, name: S) -> Result<(), StorageError> {
        self.inner.ensure_cf(name.as_ref())
    }

    pub fn list_cfs(&self) -> Result<Vec<String>, StorageError> {
        self.inner.list_cfs()
    }
}

#[derive(Clone)]
pub struct Tree {
    engine: InhouseEngine,
    name: String,
}

impl Tree {
    pub fn insert<K, V>(&self, key: K, value: V) -> Result<Option<Vec<u8>>, StorageError>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.engine
            .put(&self.name, key.as_ref(), value.as_ref())
            .map_err(Into::into)
    }

    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, StorageError> {
        self.engine.get(&self.name, key.as_ref())
    }

    pub fn clear(&self) -> Result<(), StorageError> {
        let mut iter = self.engine.prefix_iterator(&self.name, &[])?;
        while let Some((key, _)) = iter.next()? {
            let _ = self.engine.delete(&self.name, &key)?;
        }
        Ok(())
    }

    pub fn compare_and_swap<K: AsRef<[u8]>>(
        &self,
        key: K,
        expected: Option<Vec<u8>>,
        replacement: Option<Vec<u8>>,
    ) -> Result<std::result::Result<Option<Vec<u8>>, Option<Vec<u8>>>, StorageError> {
        let current = self.engine.get(&self.name, key.as_ref())?;
        if current == expected {
            match replacement {
                Some(value) => {
                    self.engine.put_bytes(&self.name, key.as_ref(), &value)?;
                }
                None => {
                    let _ = self.engine.delete(&self.name, key.as_ref())?;
                }
            }
            Ok(Ok(current))
        } else {
            Ok(Err(current))
        }
    }

    pub fn iter(&self) -> Iter {
        self.scan_prefix(Vec::<u8>::new())
    }

    fn scan_prefix<K: AsRef<[u8]>>(&self, prefix: K) -> Iter {
        match self.engine.prefix_iterator(&self.name, prefix.as_ref()) {
            Ok(inner) => Iter::from_iterator(inner),
            Err(err) => Iter::from_error(err),
        }
    }
}

pub struct Iter {
    data: IntoIter<(Vec<u8>, Vec<u8>)>,
    pending_error: Option<StorageError>,
}

impl Iter {
    fn from_iterator(mut inner: <InhouseEngine as KeyValue>::Iter) -> Self {
        let mut entries = Vec::new();
        let mut pending_error = None;
        loop {
            match inner.next() {
                Ok(Some((key, value))) => entries.push((key, value)),
                Ok(None) => break,
                Err(err) => {
                    pending_error = Some(err);
                    break;
                }
            }
        }
        Self {
            data: entries.into_iter(),
            pending_error,
        }
    }

    fn from_error(err: StorageError) -> Self {
        Self {
            data: Vec::new().into_iter(),
            pending_error: Some(err),
        }
    }
}

impl Iterator for Iter {
    type Item = Result<(Vec<u8>, Vec<u8>), StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(entry) = self.data.next() {
            Some(Ok(entry))
        } else if let Some(err) = self.pending_error.take() {
            Some(Err(err))
        } else {
            None
        }
    }
}
