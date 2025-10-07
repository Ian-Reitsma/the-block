#![forbid(unsafe_code)]

use core::fmt;
use std::error::Error as StdError;

/// Canonical error type used across the workspace while the detailed
/// diagnostics stack is rebuilt in-house.
#[derive(Debug)]
pub struct TbError {
    message: String,
    source: Option<Box<dyn StdError + Send + Sync + 'static>>,
}

impl TbError {
    pub fn new(message: impl Into<String>) -> Self {
        TbError {
            message: message.into(),
            source: None,
        }
    }

    pub fn with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        TbError {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl StdError for TbError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_ref()
            .map(|err| &**err as &(dyn StdError + 'static))
    }
}

impl From<&str> for TbError {
    fn from(value: &str) -> Self {
        TbError::new(value)
    }
}

impl From<String> for TbError {
    fn from(value: String) -> Self {
        TbError::new(value)
    }
}

impl TbError {
    pub fn from_error<E>(value: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        TbError::with_source(value.to_string(), value)
    }
}

pub type Result<T> = std::result::Result<T, TbError>;

/// Lightweight substitute for the `anyhow::Context` trait.
pub trait Context<T> {
    fn context(self, msg: impl Into<String>) -> Result<T>;
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String;
}

impl<T, E> Context<T> for std::result::Result<T, E>
where
    E: StdError + Send + Sync + 'static,
{
    fn context(self, msg: impl Into<String>) -> Result<T> {
        self.map_err(|err| {
            let base = TbError::from_error(err);
            TbError::with_source(msg.into(), base)
        })
    }

    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.context(f())
    }
}

impl<T> Context<T> for Option<T> {
    fn context(self, msg: impl Into<String>) -> Result<T> {
        self.ok_or_else(|| TbError::new(msg.into()))
    }

    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.context(f())
    }
}

#[macro_export]
macro_rules! anyhow {
    ($fmt:literal $(, $args:expr)*) => {
        $crate::TbError::new(format!($fmt $(, $args)*))
    };
    ($err:expr) => {
        $crate::TbError::from_error($err)
    };
}

#[macro_export]
macro_rules! bail {
    ($err:expr) => {
        return Err($crate::anyhow!($err));
    };
    ($fmt:literal $(, $args:expr)*) => {
        return Err($crate::anyhow!($fmt $(, $args)*));
    };
}

#[macro_export]
macro_rules! ensure {
    ($cond:expr, $fmt:literal $(, $args:expr)*) => {
        if !($cond) {
            $crate::bail!($fmt $(, $args)*);
        }
    };
    ($cond:expr, $err:expr) => {
        if !($cond) {
            $crate::bail!($err);
        }
    };
}

#[derive(Clone, Copy, Debug)]
pub struct Level(&'static str);

impl Level {
    pub const TRACE: Level = Level("trace");
    pub const DEBUG: Level = Level("debug");
    pub const INFO: Level = Level("info");
    pub const WARN: Level = Level("warn");
    pub const ERROR: Level = Level("error");

    pub fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct Span;

impl Span {
    pub fn new() -> Self {
        Span
    }

    pub fn enter(&self) -> SpanGuard {
        SpanGuard
    }
}

#[derive(Debug)]
pub struct SpanGuard;

impl Drop for SpanGuard {
    fn drop(&mut self) {}
}

#[macro_export]
macro_rules! info {
    ($($tt:tt)*) => {{
        let _ = stringify!($($tt)*);
    }};
}

#[macro_export]
macro_rules! warn {
    ($($tt:tt)*) => {{
        let _ = stringify!($($tt)*);
    }};
}

#[macro_export]
macro_rules! error {
    ($($tt:tt)*) => {{
        let _ = stringify!($($tt)*);
    }};
}

#[macro_export]
macro_rules! debug {
    ($($tt:tt)*) => {{
        let _ = stringify!($($tt)*);
    }};
}

#[macro_export]
macro_rules! trace {
    ($($tt:tt)*) => {{
        let _ = stringify!($($tt)*);
    }};
}

#[macro_export]
macro_rules! info_span {
    ($($tt:tt)*) => {{
        let _ = stringify!($($tt)*);
        $crate::Span::new()
    }};
}

#[macro_export]
macro_rules! span {
    ($($tt:tt)*) => {{
        let _ = stringify!($($tt)*);
        $crate::Span::new()
    }};
}

#[macro_export]
macro_rules! log {
    ($($tt:tt)*) => {{
        let _ = stringify!($($tt)*);
    }};
}

pub mod anyhow {
    pub use crate::{anyhow, bail, ensure, Context, Result, TbError as Error};
}

pub mod tracing {
    pub use crate::{debug, error, info, info_span, span, trace, warn, Level, Span, SpanGuard};
}

pub mod log {
    pub use crate::{debug, error, info, log, trace, warn, Level};

    pub fn logger() -> Logger {
        Logger
    }

    #[derive(Debug, Clone, Copy)]
    pub struct Logger;

    impl Logger {
        pub fn flush(&self) {}
    }
}
