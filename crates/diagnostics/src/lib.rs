#![forbid(unsafe_code)]

use core::fmt;
use std::{borrow::Cow, error::Error as StdError, sync::OnceLock};

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
pub struct FieldValue {
    pub key: Cow<'static, str>,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct LogRecord {
    pub level: Level,
    pub target: Cow<'static, str>,
    pub message: Cow<'static, str>,
    pub module_path: Cow<'static, str>,
    pub file: Cow<'static, str>,
    pub line: u32,
    pub fields: Vec<FieldValue>,
}

pub trait LogSink: Send + Sync + 'static {
    fn log(&self, record: &LogRecord);

    fn flush(&self) {}
}

#[derive(Debug)]
struct StdErrSink;

impl LogSink for StdErrSink {
    fn log(&self, record: &LogRecord) {
        use std::io::Write as _;

        let mut stderr = std::io::stderr();
        let _ = write!(
            stderr,
            "[{}] {} -- {}",
            record.level.as_str().to_uppercase(),
            record.target,
            record.message
        );

        if !record.fields.is_empty() {
            let _ = write!(stderr, " | ");
            for (idx, field) in record.fields.iter().enumerate() {
                if idx > 0 {
                    let _ = write!(stderr, ", ");
                }
                if field.key.is_empty() {
                    let _ = write!(stderr, "{}", field.value);
                } else {
                    let _ = write!(stderr, "{}={}", field.key, field.value);
                }
            }
        }

        let _ = writeln!(
            stderr,
            " ({}:{}::{})",
            record.file, record.line, record.module_path
        );
    }
}

static LOG_SINK: OnceLock<Box<dyn LogSink>> = OnceLock::new();
static DEFAULT_STDERR: StdErrSink = StdErrSink;

pub fn install_log_sink(sink: Box<dyn LogSink>) -> std::result::Result<(), Box<dyn LogSink>> {
    LOG_SINK.set(sink)
}

fn active_sink() -> &'static dyn LogSink {
    LOG_SINK
        .get()
        .map(|sink| &**sink as &dyn LogSink)
        .unwrap_or(&DEFAULT_STDERR)
}

pub fn flush_logs() {
    active_sink().flush();
}

#[derive(Debug, Clone)]
pub struct Span {
    name: Cow<'static, str>,
    level: Level,
    fields: Vec<FieldValue>,
}

impl Span {
    pub fn new(name: Cow<'static, str>, level: Level, fields: Vec<FieldValue>) -> Self {
        Span {
            name,
            level,
            fields,
        }
    }

    pub fn enter(&self) -> SpanGuard {
        SpanGuard
    }

    pub fn entered(&self) -> SpanGuard {
        self.enter()
    }

    pub fn in_scope<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = self.enter();
        f()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn level(&self) -> Level {
        self.level
    }

    pub fn fields(&self) -> &[FieldValue] {
        &self.fields
    }
}

#[derive(Debug)]
pub struct SpanGuard;

impl Drop for SpanGuard {
    fn drop(&mut self) {}
}

#[doc(hidden)]
pub mod internal {
    use super::*;

    #[derive(Debug)]
    pub struct LogEventBuilder {
        level: Level,
        target: Option<Cow<'static, str>>,
        message: Option<Cow<'static, str>>,
        module_path: Cow<'static, str>,
        file: Cow<'static, str>,
        line: u32,
        fields: Vec<FieldValue>,
    }

    impl LogEventBuilder {
        pub fn new(level: Level, module_path: &'static str, file: &'static str, line: u32) -> Self {
            LogEventBuilder {
                level,
                target: None,
                message: None,
                module_path: Cow::Borrowed(module_path),
                file: Cow::Borrowed(file),
                line,
                fields: Vec::new(),
            }
        }

        pub fn set_target(&mut self, target: impl Into<Cow<'static, str>>) {
            self.target = Some(target.into());
        }

        pub fn add_field(&mut self, key: impl Into<Cow<'static, str>>, value: String) {
            self.fields.push(FieldValue {
                key: key.into(),
                value,
            });
        }

        pub fn set_message(&mut self, message: impl Into<Cow<'static, str>>) {
            self.message = Some(message.into());
        }

        pub fn finalize(mut self) -> LogRecord {
            let default_target = match &self.module_path {
                Cow::Borrowed(s) => Cow::Borrowed(*s),
                Cow::Owned(s) => Cow::Owned(s.clone()),
            };

            let target = self.target.take().unwrap_or(default_target);

            let message = self.message.take().unwrap_or_else(|| Cow::Borrowed(""));

            LogRecord {
                level: self.level,
                target,
                message,
                module_path: self.module_path,
                file: self.file,
                line: self.line,
                fields: self.fields,
            }
        }
    }

    pub fn emit(builder: LogEventBuilder) {
        super::active_sink().log(&builder.finalize());
    }

    #[derive(Debug)]
    pub struct SpanBuilder {
        name: Cow<'static, str>,
        level: Level,
        fields: Vec<FieldValue>,
    }

    impl SpanBuilder {
        pub fn new(level: Level, name: impl Into<Cow<'static, str>>) -> Self {
            SpanBuilder {
                name: name.into(),
                level,
                fields: Vec::new(),
            }
        }

        pub fn add_field(&mut self, key: impl Into<Cow<'static, str>>, value: String) {
            self.fields.push(FieldValue {
                key: key.into(),
                value,
            });
        }

        pub fn build(self) -> Span {
            Span::new(self.name, self.level, self.fields)
        }
    }

    pub use LogEventBuilder as LogBuilder;
}

#[macro_export]
#[doc(hidden)]
macro_rules! __diagnostics_log_parse {
    ($builder:ident,) => {};
    ($builder:ident) => {};
    ($builder:ident, target: $target:expr $(, $($rest:tt)*)?) => {{
        $builder.set_target($target);
        $crate::__diagnostics_log_parse!($builder $(, $($rest)*)?);
    }};
    ($builder:ident, parent: $parent:expr $(, $($rest:tt)*)?) => {{
        let _ = &$parent;
        $crate::__diagnostics_log_parse!($builder $(, $($rest)*)?);
    }};
    ($builder:ident, $key:ident = %$value:expr, $($rest:tt)*) => {{
        $builder.add_field(stringify!($key), format!("{}", &$value));
        $crate::__diagnostics_log_parse!($builder, $($rest)*);
    }};
    ($builder:ident, $key:ident = ?$value:expr, $($rest:tt)*) => {{
        $builder.add_field(stringify!($key), format!("{:?}", &$value));
        $crate::__diagnostics_log_parse!($builder, $($rest)*);
    }};
    ($builder:ident, $key:ident = $value:expr, $($rest:tt)*) => {{
        $builder.add_field(stringify!($key), format!("{}", &$value));
        $crate::__diagnostics_log_parse!($builder, $($rest)*);
    }};
    ($builder:ident, %$value:expr, $($rest:tt)*) => {{
        $builder.add_field("value", format!("{}", &$value));
        $crate::__diagnostics_log_parse!($builder, $($rest)*);
    }};
    ($builder:ident, ?$value:expr, $($rest:tt)*) => {{
        $builder.add_field("value", format!("{:?}", &$value));
        $crate::__diagnostics_log_parse!($builder, $($rest)*);
    }};
    ($builder:ident, %$value:expr $(,)?) => {{
        $builder.set_message(format!("{}", &$value));
    }};
    ($builder:ident, ?$value:expr $(,)?) => {{
        $builder.set_message(format!("{:?}", &$value));
    }};
    ($builder:ident, $fmt:literal $(, $args:expr)* $(,)?) => {{
        $builder.set_message(format!($fmt $(, $args)*));
    }};
    ($builder:ident, $value:expr, $($rest:tt)*) => {{
        $builder.add_field("", format!("{}", &$value));
        $crate::__diagnostics_log_parse!($builder, $($rest)*);
    }};
    ($builder:ident, $value:expr $(,)?) => {{
        $builder.set_message(format!("{}", &$value));
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __diagnostics_span_parse {
    ($builder:ident,) => {};
    ($builder:ident) => {};
    ($builder:ident, $key:ident = %$value:expr, $($rest:tt)*) => {{
        $builder.add_field(stringify!($key), format!("{}", &$value));
        $crate::__diagnostics_span_parse!($builder, $($rest)*);
    }};
    ($builder:ident, $key:ident = %$value:expr $(,)?) => {{
        $builder.add_field(stringify!($key), format!("{}", &$value));
    }};
    ($builder:ident, $key:ident = ?$value:expr, $($rest:tt)*) => {{
        $builder.add_field(stringify!($key), format!("{:?}", &$value));
        $crate::__diagnostics_span_parse!($builder, $($rest)*);
    }};
    ($builder:ident, $key:ident = ?$value:expr $(,)?) => {{
        $builder.add_field(stringify!($key), format!("{:?}", &$value));
    }};
    ($builder:ident, $key:ident = $value:expr, $($rest:tt)*) => {{
        $builder.add_field(stringify!($key), format!("{}", &$value));
        $crate::__diagnostics_span_parse!($builder, $($rest)*);
    }};
    ($builder:ident, $key:ident = $value:expr $(,)?) => {{
        $builder.add_field(stringify!($key), format!("{}", &$value));
    }};
    ($builder:ident, %$value:expr, $($rest:tt)*) => {{
        $builder.add_field("value", format!("{}", &$value));
        $crate::__diagnostics_span_parse!($builder, $($rest)*);
    }};
    ($builder:ident, ?$value:expr, $($rest:tt)*) => {{
        $builder.add_field("value", format!("{:?}", &$value));
        $crate::__diagnostics_span_parse!($builder, $($rest)*);
    }};
    ($builder:ident, $value:expr, $($rest:tt)*) => {{
        $builder.add_field("", format!("{}", &$value));
        $crate::__diagnostics_span_parse!($builder, $($rest)*);
    }};
    ($builder:ident, $value:expr $(,)?) => {{
        $builder.add_field("", format!("{}", &$value));
    }};
}

#[macro_export]
macro_rules! info {
    ($($tt:tt)*) => {{
        let mut builder = $crate::internal::LogBuilder::new(
            $crate::Level::INFO,
            module_path!(),
            file!(),
            line!(),
        );
        $crate::__diagnostics_log_parse!(builder, $($tt)*);
        $crate::internal::emit(builder);
    }};
}

#[macro_export]
macro_rules! warn {
    ($($tt:tt)*) => {{
        let mut builder = $crate::internal::LogBuilder::new(
            $crate::Level::WARN,
            module_path!(),
            file!(),
            line!(),
        );
        $crate::__diagnostics_log_parse!(builder, $($tt)*);
        $crate::internal::emit(builder);
    }};
}

#[macro_export]
macro_rules! error {
    ($($tt:tt)*) => {{
        let mut builder = $crate::internal::LogBuilder::new(
            $crate::Level::ERROR,
            module_path!(),
            file!(),
            line!(),
        );
        $crate::__diagnostics_log_parse!(builder, $($tt)*);
        $crate::internal::emit(builder);
    }};
}

#[macro_export]
macro_rules! debug {
    ($($tt:tt)*) => {{
        let mut builder = $crate::internal::LogBuilder::new(
            $crate::Level::DEBUG,
            module_path!(),
            file!(),
            line!(),
        );
        $crate::__diagnostics_log_parse!(builder, $($tt)*);
        $crate::internal::emit(builder);
    }};
}

#[macro_export]
macro_rules! trace {
    ($($tt:tt)*) => {{
        let mut builder = $crate::internal::LogBuilder::new(
            $crate::Level::TRACE,
            module_path!(),
            file!(),
            line!(),
        );
        $crate::__diagnostics_log_parse!(builder, $($tt)*);
        $crate::internal::emit(builder);
    }};
}

#[macro_export]
macro_rules! log {
    ($level:expr, $($tt:tt)*) => {{
        let mut builder = $crate::internal::LogBuilder::new(
            $level,
            module_path!(),
            file!(),
            line!(),
        );
        $crate::__diagnostics_log_parse!(builder, $($tt)*);
        $crate::internal::emit(builder);
    }};
}

#[macro_export]
macro_rules! info_span {
    ($name:expr $(, $($rest:tt)*)?) => {{
        let mut builder = $crate::internal::SpanBuilder::new($crate::Level::INFO, $name);
        $( $crate::__diagnostics_span_parse!(builder, $($rest)*); )?
        builder.build()
    }};
}

#[macro_export]
macro_rules! span {
    ($level:expr, $name:expr $(, $($rest:tt)*)?) => {{
        let mut builder = $crate::internal::SpanBuilder::new($level, $name);
        $( $crate::__diagnostics_span_parse!(builder, $($rest)*); )?
        builder.build()
    }};
}

pub mod anyhow {
    pub use crate::{anyhow, bail, ensure, Context, Result, TbError as Error};
}

pub mod tracing {
    pub use crate::{
        debug, error, info, info_span, log, span, trace, warn, FieldValue, Level, Span, SpanGuard,
    };
}

pub mod log {
    pub use crate::{debug, error, info, log, trace, warn, FieldValue, Level, LogRecord, LogSink};

    pub fn logger() -> Logger {
        Logger
    }

    #[derive(Debug, Clone, Copy)]
    pub struct Logger;

    impl Logger {
        pub fn flush(&self) {
            crate::flush_logs();
        }
    }
}
