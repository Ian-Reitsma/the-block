use std::fmt;
use std::io;

use thiserror::Error;

/// Unified error type for storage engines so callers do not depend on backend-specific
/// error enums.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("storage backend error: {0}")]
    Backend(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

impl StorageError {
    pub fn backend<E: fmt::Display>(err: E) -> Self {
        StorageError::Backend(err.to_string())
    }
}
