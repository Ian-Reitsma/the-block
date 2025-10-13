#![forbid(unsafe_code)]

use core::fmt;
use std::{
    borrow::Cow,
    error::Error as StdError,
    sync::{atomic::AtomicUsize, atomic::Ordering, Arc, Mutex, OnceLock},
};

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

impl From<std::io::Error> for TbError {
    fn from(value: std::io::Error) -> Self {
        TbError::from_error(value)
    }
}

impl From<codec::Error> for TbError {
    fn from(value: codec::Error) -> Self {
        TbError::from_error(value)
    }
}

pub type Result<T, E = TbError> = std::result::Result<T, E>;

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

type SubscriberFn = dyn Fn(&LogRecord) + Send + Sync + 'static;

struct SubscriberEntry {
    id: usize,
    callback: Arc<SubscriberFn>,
}

struct SubscriberRegistry {
    callbacks: Mutex<Vec<SubscriberEntry>>,
    next_id: AtomicUsize,
}

impl SubscriberRegistry {
    fn new() -> Self {
        SubscriberRegistry {
            callbacks: Mutex::new(Vec::new()),
            next_id: AtomicUsize::new(1),
        }
    }

    fn register<F>(&self, callback: F) -> usize
    where
        F: Fn(&LogRecord) + Send + Sync + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut guard) = self.callbacks.lock() {
            guard.push(SubscriberEntry {
                id,
                callback: Arc::new(callback),
            });
        }
        id
    }

    fn unregister(&self, id: usize) {
        if let Ok(mut guard) = self.callbacks.lock() {
            guard.retain(|entry| entry.id != id);
        }
    }

    fn notify(&self, record: &LogRecord) {
        let callbacks = match self.callbacks.lock() {
            Ok(guard) => guard
                .iter()
                .map(|entry| entry.callback.clone())
                .collect::<Vec<_>>(),
            Err(_) => return,
        };

        for callback in callbacks {
            callback(record);
        }
    }
}

static SUBSCRIBERS: OnceLock<SubscriberRegistry> = OnceLock::new();

fn subscriber_registry() -> &'static SubscriberRegistry {
    SUBSCRIBERS.get_or_init(SubscriberRegistry::new)
}

fn notify_subscribers(record: &LogRecord) {
    if let Some(registry) = SUBSCRIBERS.get() {
        registry.notify(record);
    }
}

fn remove_subscriber(id: usize) {
    if let Some(registry) = SUBSCRIBERS.get() {
        registry.unregister(id);
    }
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
        let record = builder.finalize();
        super::notify_subscribers(&record);
        super::active_sink().log(&record);
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct TlsEnvWarningEvent {
        pub prefix: String,
        pub code: String,
        pub detail: String,
        pub variables: Vec<String>,
    }

    fn parse_variable_list(raw: &str) -> Vec<String> {
        let trimmed = raw.trim();
        if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
            return if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_string()]
            };
        }

        let inner = &trimmed[1..trimmed.len() - 1];
        if inner.trim().is_empty() {
            return Vec::new();
        }

        inner
            .split(',')
            .filter_map(|segment| {
                let value = segment.trim().trim_matches('"');
                if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                }
            })
            .collect()
    }

    pub fn install_tls_env_warning_subscriber<F>(callback: F) -> SubscriberGuard
    where
        F: Fn(&TlsEnvWarningEvent) + Send + Sync + 'static,
    {
        install_subscriber(move |record| {
            if record.target.as_ref() != "http_env.tls_env" {
                return;
            }
            if record.level.as_str() != Level::WARN.as_str() {
                return;
            }
            if record.message.as_ref() != "tls_env_warning" {
                return;
            }

            let mut prefix = None;
            let mut code = None;
            let mut detail = None;
            let mut variables: Vec<String> = Vec::new();

            for field in &record.fields {
                match field.key.as_ref() {
                    "prefix" => prefix = Some(field.value.clone()),
                    "code" => code = Some(field.value.clone()),
                    "detail" => detail = Some(field.value.clone()),
                    "variables" => variables = parse_variable_list(&field.value),
                    _ => {}
                }
            }

            let (Some(prefix), Some(code)) = (prefix, code) else {
                return;
            };

            let event = TlsEnvWarningEvent {
                prefix,
                code,
                detail: detail.unwrap_or_default(),
                variables,
            };

            callback(&event);
        })
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

    #[derive(Debug)]
    pub struct SubscriberGuard {
        id: usize,
    }

    impl Drop for SubscriberGuard {
        fn drop(&mut self) {
            super::remove_subscriber(self.id);
        }
    }

    pub fn install_subscriber<F>(callback: F) -> SubscriberGuard
    where
        F: Fn(&LogRecord) + Send + Sync + 'static,
    {
        let id = super::subscriber_registry().register(callback);
        SubscriberGuard { id }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_variable_lists() {
            assert!(parse_variable_list("[]").is_empty());
            assert_eq!(
                parse_variable_list("[\"A\", \"B\"]"),
                vec!["A".to_string(), "B".to_string()]
            );
            assert_eq!(parse_variable_list("single"), vec!["single".to_string()]);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    #[test]
    fn subscribers_receive_emitted_records() {
        let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = received.clone();
        let guard = internal::install_subscriber(move |record| {
            if record.target.as_ref() == "diagnostics.test" {
                if let Ok(mut sink) = captured.lock() {
                    sink.push(record.message.to_string());
                }
            }
        });

        warn!(target: "diagnostics.test", "first");
        warn!(target: "diagnostics.other", "ignored");

        drop(guard);

        warn!(target: "diagnostics.test", "second");

        let messages = received.lock().unwrap().clone();
        assert_eq!(messages, vec![String::from("first")]);
    }
}
