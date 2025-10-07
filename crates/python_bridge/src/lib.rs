#![forbid(unsafe_code)]

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    FeatureDisabled,
    Unimplemented,
    Runtime,
    Value,
}

#[derive(Debug, Clone)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}

impl Error {
    pub fn feature_disabled() -> Self {
        Self {
            kind: ErrorKind::FeatureDisabled,
            message: "python bindings are disabled".to_string(),
        }
    }

    pub fn unimplemented(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Unimplemented,
            message: msg.into(),
        }
    }

    pub fn runtime(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Runtime,
            message: msg.into(),
        }
    }

    pub fn value(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Value,
            message: msg.into(),
        }
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn with_message(mut self, msg: impl Into<String>) -> Self {
        self.message = msg.into();
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub fn ensure_enabled() -> Result<()> {
    #[cfg(feature = "python-bindings")]
    {
        Ok(())
    }

    #[cfg(not(feature = "python-bindings"))]
    {
        Err(Error::feature_disabled())
    }
}

pub fn prepare_freethreaded_python() -> Result<()> {
    ensure_enabled()
}

pub fn with_interpreter<F, T>(func: F) -> Result<T>
where
    F: FnOnce() -> T,
{
    ensure_enabled().map(|()| func())
}

pub fn report_disabled() -> Error {
    Error::feature_disabled()
}

pub mod ffi {
    use super::{Error, Result};

    pub struct StubInterpreter;

    impl StubInterpreter {
        pub fn new() -> Result<Self> {
            Err(Error::feature_disabled())
        }
    }
}
