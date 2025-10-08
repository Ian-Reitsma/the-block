use std::error::Error as StdError;
use std::fmt;
use std::io;

/// Unified error type for storage engines so callers do not depend on backend-specific
/// error enums.
#[derive(Debug)]
pub enum StorageError {
    Io(io::Error),
    Backend(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

impl StorageError {
    pub fn backend<E: fmt::Display>(err: E) -> Self {
        StorageError::Backend(err.to_string())
    }
}

impl From<io::Error> for StorageError {
    fn from(err: io::Error) -> Self {
        StorageError::Io(err)
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Io(err) => write!(f, "io error: {err}"),
            StorageError::Backend(msg) => write!(f, "storage backend error: {msg}"),
        }
    }
}

impl StdError for StorageError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            StorageError::Io(err) => Some(err),
            StorageError::Backend(_) => None,
        }
    }
}
