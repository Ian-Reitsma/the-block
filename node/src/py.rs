#[cfg(feature = "python-bindings")]
pub type PyError = python_bridge::Error;
#[cfg(feature = "python-bindings")]
pub type PyResult<T> = python_bridge::Result<T>;

#[cfg(feature = "python-bindings")]
pub use python_bridge::{getter, new, setter, staticmethod};

#[cfg(feature = "python-bindings")]
pub use python_bridge::prepare_freethreaded_python;

#[cfg(not(feature = "python-bindings"))]
mod stub {
    use std::fmt;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PyErrorKind {
        FeatureDisabled,
        Runtime,
        Value,
        Unimplemented,
    }

    #[derive(Debug, Clone)]
    pub struct PyError {
        kind: PyErrorKind,
        message: String,
    }

    impl PyError {
        pub fn feature_disabled() -> Self {
            Self {
                kind: PyErrorKind::FeatureDisabled,
                message: "python bindings are disabled".to_string(),
            }
        }

        pub fn runtime(msg: impl Into<String>) -> Self {
            Self {
                kind: PyErrorKind::Runtime,
                message: msg.into(),
            }
        }

        pub fn value(msg: impl Into<String>) -> Self {
            Self {
                kind: PyErrorKind::Value,
                message: msg.into(),
            }
        }

        pub fn unimplemented(msg: impl Into<String>) -> Self {
            Self {
                kind: PyErrorKind::Unimplemented,
                message: msg.into(),
            }
        }

        pub fn with_message(mut self, msg: impl Into<String>) -> Self {
            self.message = msg.into();
            self
        }

        pub fn kind(&self) -> PyErrorKind {
            self.kind
        }

        pub fn message(&self) -> &str {
            &self.message
        }
    }

    impl fmt::Display for PyError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl std::error::Error for PyError {}

    pub type PyResult<T> = Result<T, PyError>;

    pub fn prepare_freethreaded_python() -> PyResult<()> {
        Err(PyError::feature_disabled())
    }
}

#[cfg(not(feature = "python-bindings"))]
pub use stub::{prepare_freethreaded_python, PyError, PyErrorKind, PyResult};
