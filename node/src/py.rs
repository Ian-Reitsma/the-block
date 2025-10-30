#![allow(dead_code)]

#[cfg(feature = "python-bindings")]
pub type PyError = python_bridge::Error;
#[cfg(feature = "python-bindings")]
pub type PyErrorKind = python_bridge::ErrorKind;
#[cfg(feature = "python-bindings")]
pub type PyResult<T> = python_bridge::Result<T>;

#[cfg(feature = "python-bindings")]
pub use python_bridge::{getter, new, setter, staticmethod};

#[cfg(feature = "python-bindings")]
#[allow(unused_imports)]
pub use python_bridge::{
    ensure_enabled, prepare_freethreaded_python, report_disabled, with_interpreter, Interpreter,
};

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

    #[derive(Debug, Default, Clone, Copy)]
    pub struct Interpreter;

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

    impl Interpreter {
        pub fn run(&self, _code: &str) -> PyResult<()> {
            Err(PyError::feature_disabled())
        }
    }

    pub fn ensure_enabled() -> PyResult<()> {
        Err(PyError::feature_disabled())
    }

    pub fn prepare_freethreaded_python() -> PyResult<()> {
        ensure_enabled()
    }

    pub fn with_interpreter<F, T>(f: F) -> PyResult<T>
    where
        F: FnOnce(&Interpreter) -> PyResult<T>,
    {
        ensure_enabled().and_then(|()| f(&Interpreter))
    }

    pub fn report_disabled() -> PyError {
        PyError::feature_disabled()
    }

    pub use PyError as Error;
    pub use PyErrorKind as ErrorKind;
}

#[cfg(not(feature = "python-bindings"))]
#[allow(unused_imports)]
pub use stub::{
    ensure_enabled, prepare_freethreaded_python, report_disabled, with_interpreter, Interpreter,
};
#[cfg(not(feature = "python-bindings"))]
#[allow(unused_imports)]
pub use stub::{Error as PyError, ErrorKind as PyErrorKind, PyResult};
