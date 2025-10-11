#![forbid(unsafe_code)]

/// Value type representing parameters passed into SQL statements.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueConversionError {
    message: std::borrow::Cow<'static, str>,
}

impl ValueConversionError {
    pub fn new(message: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ValueConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ValueConversionError {}

pub trait FromValue: Sized {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError>;
}

impl FromValue for Value {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        Ok(value.clone())
    }
}

impl FromValue for String {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        match value {
            Value::Text(text) => Ok(text.clone()),
            Value::Null => Ok(String::new()),
            Value::Integer(v) => Ok(v.to_string()),
            Value::Real(v) => Ok(v.to_string()),
            Value::Blob(_) => Err(ValueConversionError::new("cannot convert blob to string")),
        }
    }
}

impl FromValue for Vec<u8> {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        match value {
            Value::Blob(bytes) => Ok(bytes.clone()),
            Value::Text(text) => Ok(text.as_bytes().to_vec()),
            Value::Null => Ok(Vec::new()),
            Value::Integer(v) => Ok(v.to_be_bytes().to_vec()),
            Value::Real(v) => Ok(v.to_bits().to_be_bytes().to_vec()),
        }
    }
}

impl FromValue for i64 {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        match value {
            Value::Integer(v) => Ok(*v),
            Value::Real(v) => Ok(*v as i64),
            Value::Text(text) => text
                .parse::<i64>()
                .map_err(|_| ValueConversionError::new("failed to parse integer")),
            Value::Blob(bytes) if bytes.len() == 8 => {
                let mut array = [0u8; 8];
                array.copy_from_slice(bytes);
                Ok(i64::from_be_bytes(array))
            }
            Value::Blob(_) => Err(ValueConversionError::new("cannot convert blob to integer")),
            Value::Null => Err(ValueConversionError::new("unexpected NULL for integer")),
        }
    }
}

impl FromValue for u64 {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        <i64 as FromValue>::from_value(value).map(|v| v as u64)
    }
}

impl FromValue for i32 {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        <i64 as FromValue>::from_value(value).map(|v| v as i32)
    }
}

impl FromValue for u32 {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        <i64 as FromValue>::from_value(value).map(|v| v as u32)
    }
}

impl FromValue for i16 {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        <i64 as FromValue>::from_value(value).map(|v| v as i16)
    }
}

impl FromValue for u16 {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        <i64 as FromValue>::from_value(value).map(|v| v as u16)
    }
}

impl FromValue for i8 {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        <i64 as FromValue>::from_value(value).map(|v| v as i8)
    }
}

impl FromValue for u8 {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        <i64 as FromValue>::from_value(value).map(|v| v as u8)
    }
}

impl FromValue for bool {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        <i64 as FromValue>::from_value(value).map(|v| v != 0)
    }
}

impl FromValue for f64 {
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        match value {
            Value::Real(v) => Ok(*v),
            Value::Integer(v) => Ok(*v as f64),
            Value::Text(text) => text
                .parse::<f64>()
                .map_err(|_| ValueConversionError::new("failed to parse float")),
            Value::Blob(bytes) if bytes.len() == 8 => {
                let mut array = [0u8; 8];
                array.copy_from_slice(bytes);
                Ok(f64::from_bits(u64::from_be_bytes(array)))
            }
            Value::Blob(_) => Err(ValueConversionError::new("cannot convert blob to float")),
            Value::Null => Err(ValueConversionError::new("unexpected NULL for float")),
        }
    }
}

impl<T> FromValue for Option<T>
where
    T: FromValue,
{
    fn from_value(value: &Value) -> std::result::Result<Self, ValueConversionError> {
        if matches!(value, Value::Null) {
            Ok(None)
        } else {
            T::from_value(value).map(Some)
        }
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::Text(value)
    }
}

impl From<&String> for Value {
    fn from(value: &String) -> Self {
        Value::Text(value.clone())
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::Text(value.to_string())
    }
}

impl From<&[u8]> for Value {
    fn from(value: &[u8]) -> Self {
        Value::Blob(value.to_vec())
    }
}

impl From<&Vec<u8>> for Value {
    fn from(value: &Vec<u8>) -> Self {
        Value::Blob(value.clone())
    }
}

impl From<Vec<u8>> for Value {
    fn from(value: Vec<u8>) -> Self {
        Value::Blob(value)
    }
}

impl From<&i64> for Value {
    fn from(value: &i64) -> Self {
        Value::Integer(*value)
    }
}

impl From<&u64> for Value {
    fn from(value: &u64) -> Self {
        Value::Integer(*value as i64)
    }
}

impl From<&i32> for Value {
    fn from(value: &i32) -> Self {
        Value::Integer(*value as i64)
    }
}

impl From<&u32> for Value {
    fn from(value: &u32) -> Self {
        Value::Integer(*value as i64)
    }
}

impl From<&i16> for Value {
    fn from(value: &i16) -> Self {
        Value::Integer(*value as i64)
    }
}

impl From<&u16> for Value {
    fn from(value: &u16) -> Self {
        Value::Integer(*value as i64)
    }
}

impl From<&i8> for Value {
    fn from(value: &i8) -> Self {
        Value::Integer(*value as i64)
    }
}

impl From<&u8> for Value {
    fn from(value: &u8) -> Self {
        Value::Integer(*value as i64)
    }
}

impl From<&bool> for Value {
    fn from(value: &bool) -> Self {
        Value::Integer(if *value { 1 } else { 0 })
    }
}

impl From<&f64> for Value {
    fn from(value: &f64) -> Self {
        Value::Real(*value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Integer(value)
    }
}

impl From<u64> for Value {
    fn from(value: u64) -> Self {
        Value::Integer(value as i64)
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Value::Integer(value as i64)
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Value::Integer(value as i64)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Integer(if value { 1 } else { 0 })
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Real(value)
    }
}

impl From<&Value> for Value {
    fn from(value: &Value) -> Self {
        value.clone()
    }
}

/// A collection of SQL parameter values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params {
    values: Vec<Value>,
}

impl Params {
    pub fn new() -> Self {
        Self { values: Vec::new() }
    }

    pub fn push<V: Into<Value>>(&mut self, value: V) {
        self.values.push(value.into());
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn values(&self) -> &[Value] {
        &self.values
    }

    pub fn into_values(self) -> Vec<Value> {
        self.values
    }
}

pub trait IntoParams {
    fn into_params(self) -> Params;
}

impl IntoParams for Params {
    fn into_params(self) -> Params {
        self
    }
}

impl<'a> IntoParams for &'a Params {
    fn into_params(self) -> Params {
        self.clone()
    }
}

impl IntoParams for () {
    fn into_params(self) -> Params {
        Params::new()
    }
}

impl IntoParams for Vec<Value> {
    fn into_params(self) -> Params {
        Params { values: self }
    }
}

impl<'a> IntoParams for &'a [Value] {
    fn into_params(self) -> Params {
        Params {
            values: self.iter().cloned().collect(),
        }
    }
}

impl<'a, const N: usize> IntoParams for [Value; N] {
    fn into_params(self) -> Params {
        Params {
            values: self.into_iter().collect(),
        }
    }
}

impl<'a, const N: usize> IntoParams for &'a [Value; N] {
    fn into_params(self) -> Params {
        Params {
            values: self.iter().cloned().collect(),
        }
    }
}

pub fn params_from_iter<I, T>(iter: I) -> Params
where
    I: IntoIterator<Item = T>,
    T: Into<Value>,
{
    let mut params = Params::new();
    for value in iter {
        params.push(value);
    }
    params
}

#[macro_export]
macro_rules! params {
    () => {
        {
            let params = $crate::Params::new();
            params
        }
    };
    ($($value:expr),+ $(,)?) => {
        {
            let mut params = $crate::Params::new();
            $(params.push($value);)+
            params
        }
    };
}

#[macro_export]
macro_rules! params_from_iter {
    ($iter:expr) => {
        $crate::params_from_iter($iter)
    };
}

#[cfg(feature = "rusqlite-backend")]
mod backend {
    use super::{IntoParams, Params, Value};
    use std::fmt;
    use std::path::Path;

    pub type Result<T> = std::result::Result<T, Error>;

    #[derive(Debug)]
    enum ErrorKind {
        Rusqlite(rusqlite::Error),
        Conversion(super::ValueConversionError),
    }

    #[derive(Debug)]
    pub struct Error {
        kind: ErrorKind,
    }

    impl Error {
        fn from_rusqlite(err: rusqlite::Error) -> Self {
            Self {
                kind: ErrorKind::Rusqlite(err),
            }
        }

        fn is_query_returned_no_rows(&self) -> bool {
            matches!(
                self.kind,
                ErrorKind::Rusqlite(rusqlite::Error::QueryReturnedNoRows)
            )
        }

        fn conversion(err: super::ValueConversionError) -> Self {
            Self {
                kind: ErrorKind::Conversion(err),
            }
        }
    }

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match &self.kind {
                ErrorKind::Rusqlite(err) => write!(f, "{err}"),
                ErrorKind::Conversion(err) => write!(f, "{err}"),
            }
        }
    }

    impl std::error::Error for Error {}

    fn value_from_ref(value: rusqlite::types::ValueRef<'_>) -> Result<super::Value> {
        Ok(match value {
            rusqlite::types::ValueRef::Null => super::Value::Null,
            rusqlite::types::ValueRef::Integer(v) => super::Value::Integer(v),
            rusqlite::types::ValueRef::Real(v) => super::Value::Real(v),
            rusqlite::types::ValueRef::Text(text) => {
                super::Value::Text(String::from_utf8_lossy(text).into_owned())
            }
            rusqlite::types::ValueRef::Blob(bytes) => super::Value::Blob(bytes.to_vec()),
        })
    }

    fn convert_value(value: Value) -> rusqlite::types::Value {
        match value {
            Value::Null => rusqlite::types::Value::Null,
            Value::Integer(v) => rusqlite::types::Value::Integer(v),
            Value::Real(v) => rusqlite::types::Value::Real(v),
            Value::Text(v) => rusqlite::types::Value::Text(v),
            Value::Blob(v) => rusqlite::types::Value::Blob(v),
        }
    }

    fn into_rusqlite_params(params: Params) -> Vec<rusqlite::types::Value> {
        params
            .into_values()
            .into_iter()
            .map(convert_value)
            .collect()
    }

    pub struct Connection {
        inner: rusqlite::Connection,
    }

    impl Connection {
        pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
            let inner = rusqlite::Connection::open(path).map_err(Error::from_rusqlite)?;
            Ok(Self { inner })
        }

        pub fn execute<P: IntoParams>(&self, sql: &str, params: P) -> Result<usize> {
            let params = into_rusqlite_params(params.into_params());
            let result = self
                .inner
                .execute(sql, rusqlite::params_from_iter(params.iter()))
                .map_err(Error::from_rusqlite)?;
            Ok(result)
        }

        pub fn query_row<P, F, T>(&self, sql: &str, params: P, f: F) -> Result<T>
        where
            P: IntoParams,
            F: FnOnce(&Row<'_>) -> Result<T>,
        {
            let params_vec = into_rusqlite_params(params.into_params());
            let mut stmt = self.inner.prepare(sql).map_err(Error::from_rusqlite)?;
            let mut rows = stmt
                .query(rusqlite::params_from_iter(params_vec.iter()))
                .map_err(Error::from_rusqlite)?;
            match rows.next().map_err(Error::from_rusqlite)? {
                Some(row) => {
                    let wrapper = Row { inner: row };
                    f(&wrapper)
                }
                None => Err(Error::from_rusqlite(rusqlite::Error::QueryReturnedNoRows)),
            }
        }

        pub fn prepare<'conn>(&'conn self, sql: &str) -> Result<Statement<'conn>> {
            let stmt = self.inner.prepare(sql).map_err(Error::from_rusqlite)?;
            Ok(Statement { inner: stmt })
        }

        pub fn transaction<'conn>(&'conn mut self) -> Result<Transaction<'conn>> {
            let tx = self.inner.transaction().map_err(Error::from_rusqlite)?;
            Ok(Transaction { inner: tx })
        }
    }

    pub struct Statement<'conn> {
        inner: rusqlite::Statement<'conn>,
    }

    impl<'conn> Statement<'conn> {
        pub fn query_map<P, F, T>(&mut self, params: P, mut mapper: F) -> Result<Vec<Result<T>>>
        where
            P: IntoParams,
            F: FnMut(&Row<'_>) -> Result<T>,
        {
            let params_vec = into_rusqlite_params(params.into_params());
            let mut rows = self
                .inner
                .query(rusqlite::params_from_iter(params_vec.iter()))
                .map_err(Error::from_rusqlite)?;
            let mut results = Vec::new();
            while let Some(row) = rows.next().map_err(Error::from_rusqlite)? {
                let wrapper = Row { inner: row };
                results.push(mapper(&wrapper));
            }
            Ok(results)
        }

        pub fn execute<P: IntoParams>(&mut self, params: P) -> Result<usize> {
            let params_vec = into_rusqlite_params(params.into_params());
            let result = self
                .inner
                .execute(rusqlite::params_from_iter(params_vec.iter()))
                .map_err(Error::from_rusqlite)?;
            Ok(result)
        }
    }

    pub struct Row<'stmt> {
        inner: &'stmt rusqlite::Row<'stmt>,
    }

    impl<'stmt> Row<'stmt> {
        pub fn get<K, T>(&self, key: K) -> Result<T>
        where
            K: Into<RowKey>,
            T: super::FromValue,
        {
            let value_ref = match key.into() {
                RowKey::Index(i) => self.inner.get_ref(i).map_err(Error::from_rusqlite)?,
                RowKey::Name(name) => self
                    .inner
                    .get_ref(name.as_str())
                    .map_err(Error::from_rusqlite)?,
            };
            let value = value_from_ref(value_ref)?;
            T::from_value(&value).map_err(Error::conversion)
        }
    }

    impl From<super::ValueConversionError> for Error {
        fn from(err: super::ValueConversionError) -> Self {
            Error::conversion(err)
        }
    }

    pub struct Transaction<'conn> {
        inner: rusqlite::Transaction<'conn>,
    }

    impl<'conn> Transaction<'conn> {
        pub fn execute<P: IntoParams>(&self, sql: &str, params: P) -> Result<usize> {
            let params_vec = into_rusqlite_params(params.into_params());
            let result = self
                .inner
                .execute(sql, rusqlite::params_from_iter(params_vec.iter()))
                .map_err(Error::from_rusqlite)?;
            Ok(result)
        }

        pub fn prepare<'stmt>(&'stmt self, sql: &str) -> Result<Statement<'stmt>> {
            let stmt = self.inner.prepare(sql).map_err(Error::from_rusqlite)?;
            Ok(Statement { inner: stmt })
        }

        pub fn commit(self) -> Result<()> {
            self.inner.commit().map_err(Error::from_rusqlite)
        }
    }

    #[derive(Clone, Debug)]
    pub enum RowKey {
        Index(usize),
        Name(String),
    }

    impl From<usize> for RowKey {
        fn from(value: usize) -> Self {
            RowKey::Index(value)
        }
    }

    impl From<u64> for RowKey {
        fn from(value: u64) -> Self {
            RowKey::Index(value as usize)
        }
    }

    impl From<u32> for RowKey {
        fn from(value: u32) -> Self {
            RowKey::Index(value as usize)
        }
    }

    impl From<u16> for RowKey {
        fn from(value: u16) -> Self {
            RowKey::Index(value as usize)
        }
    }

    impl From<u8> for RowKey {
        fn from(value: u8) -> Self {
            RowKey::Index(value as usize)
        }
    }

    impl From<i64> for RowKey {
        fn from(value: i64) -> Self {
            assert!(value >= 0, "column index must be non-negative");
            RowKey::Index(value as usize)
        }
    }

    impl From<i32> for RowKey {
        fn from(value: i32) -> Self {
            assert!(value >= 0, "column index must be non-negative");
            RowKey::Index(value as usize)
        }
    }

    impl From<i16> for RowKey {
        fn from(value: i16) -> Self {
            assert!(value >= 0, "column index must be non-negative");
            RowKey::Index(value as usize)
        }
    }

    impl From<i8> for RowKey {
        fn from(value: i8) -> Self {
            assert!(value >= 0, "column index must be non-negative");
            RowKey::Index(value as usize)
        }
    }

    impl From<&str> for RowKey {
        fn from(value: &str) -> Self {
            RowKey::Name(value.to_string())
        }
    }

    impl From<String> for RowKey {
        fn from(value: String) -> Self {
            RowKey::Name(value)
        }
    }

    impl From<&String> for RowKey {
        fn from(value: &String) -> Self {
            RowKey::Name(value.clone())
        }
    }

    pub trait OptionalExtension<T> {
        fn optional(self) -> Result<Option<T>>;
    }

    impl<T> OptionalExtension<T> for Result<T> {
        fn optional(self) -> Result<Option<T>> {
            match self {
                Ok(value) => Ok(Some(value)),
                Err(err) if err.is_query_returned_no_rows() => Ok(None),
                Err(err) => Err(err),
            }
        }
    }

    pub use OptionalExtension as _OptionalExtension;
    pub use Result as _Result;
    pub use Transaction as _Transaction;
    pub use {
        Connection as _Connection, Error as _Error, Row as _Row, RowKey as _RowKey,
        Statement as _Statement,
    };
}

#[cfg(not(feature = "rusqlite-backend"))]
mod stub {
    use super::{IntoParams, Params, Value};
    use std::fmt;
    use std::path::Path;

    #[derive(Debug, Clone)]
    pub struct Error {
        message: Cow<'static, str>,
    }

    impl Error {
        fn backend_unavailable() -> Self {
            Self {
                message: Cow::Borrowed(
                    "foundation_sqlite compiled without the rusqlite-backend feature",
                ),
            }
        }

        fn query_returned_no_rows() -> Self {
            Self {
                message: Cow::Borrowed("query returned no rows"),
            }
        }

        fn is_query_returned_no_rows(&self) -> bool {
            matches!(self.message.as_ref(), "query returned no rows")
        }
    }

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl std::error::Error for Error {}

    pub type Result<T> = std::result::Result<T, Error>;

    #[derive(Clone, Debug)]
    pub struct Connection;

    impl Connection {
        pub fn open<P: AsRef<Path>>(_path: P) -> Result<Self> {
            Err(Error::backend_unavailable())
        }

        pub fn execute<P: IntoParams>(&self, _sql: &str, _params: P) -> Result<usize> {
            Err(Error::backend_unavailable())
        }

        pub fn query_row<P, F, T>(&self, _sql: &str, _params: P, _f: F) -> Result<T>
        where
            P: IntoParams,
            F: FnOnce(&Row<'_>) -> Result<T>,
        {
            Err(Error::backend_unavailable())
        }

        pub fn prepare<'conn>(&'conn self, _sql: &str) -> Result<Statement<'conn>> {
            Err(Error::backend_unavailable())
        }

        pub fn transaction<'conn>(&'conn mut self) -> Result<Transaction<'conn>> {
            Err(Error::backend_unavailable())
        }
    }

    #[derive(Clone, Debug)]
    pub struct Statement<'conn> {
        _phantom: std::marker::PhantomData<&'conn ()>,
    }

    impl<'conn> Statement<'conn> {
        pub fn query_map<P, F, T>(&mut self, _params: P, _mapper: F) -> Result<Vec<Result<T>>>
        where
            P: IntoParams,
            F: FnMut(&Row<'_>) -> Result<T>,
        {
            Err(Error::backend_unavailable())
        }

        pub fn execute<P: IntoParams>(&mut self, _params: P) -> Result<usize> {
            Err(Error::backend_unavailable())
        }
    }

    #[derive(Clone, Debug)]
    pub struct Row<'stmt> {
        _phantom: std::marker::PhantomData<&'stmt ()>,
    }

    impl<'stmt> Row<'stmt> {
        pub fn get<K, T>(&self, _key: K) -> Result<T>
        where
            K: Into<RowKey>,
            T: super::FromValue,
        {
            Err(Error::backend_unavailable())
        }
    }

    #[derive(Clone, Debug)]
    pub struct Transaction<'conn> {
        _phantom: std::marker::PhantomData<&'conn ()>,
    }

    impl<'conn> Transaction<'conn> {
        pub fn execute<P: IntoParams>(&self, _sql: &str, _params: P) -> Result<usize> {
            Err(Error::backend_unavailable())
        }

        pub fn prepare<'stmt>(&'stmt self, _sql: &str) -> Result<Statement<'stmt>> {
            Err(Error::backend_unavailable())
        }

        pub fn commit(self) -> Result<()> {
            Err(Error::backend_unavailable())
        }
    }

    #[derive(Clone, Debug)]
    pub enum RowKey {
        Index(usize),
        Name(String),
    }

    impl From<usize> for RowKey {
        fn from(value: usize) -> Self {
            RowKey::Index(value)
        }
    }

    impl From<&str> for RowKey {
        fn from(value: &str) -> Self {
            RowKey::Name(value.to_string())
        }
    }

    impl From<String> for RowKey {
        fn from(value: String) -> Self {
            RowKey::Name(value)
        }
    }

    impl From<&String> for RowKey {
        fn from(value: &String) -> Self {
            RowKey::Name(value.clone())
        }
    }

    pub trait OptionalExtension<T> {
        fn optional(self) -> Result<Option<T>>;
    }

    impl<T> OptionalExtension<T> for Result<T> {
        fn optional(self) -> Result<Option<T>> {
            match self {
                Ok(value) => Ok(Some(value)),
                Err(err) if err.is_query_returned_no_rows() => Ok(None),
                Err(err) => Err(err),
            }
        }
    }

    pub use Connection as _Connection;
    pub use Error as _Error;
    pub use OptionalExtension as _OptionalExtension;
    pub use Result as _Result;
    pub use Row as _Row;
    pub use RowKey as _RowKey;
    pub use Statement as _Statement;
    pub use Transaction as _Transaction;
}

#[cfg(feature = "rusqlite-backend")]
pub use backend::{
    _Connection as Connection, _Error as Error, _OptionalExtension as OptionalExtension,
    _Result as Result, _Row as Row, _RowKey as RowKey, _Statement as Statement,
    _Transaction as Transaction,
};

#[cfg(not(feature = "rusqlite-backend"))]
pub use stub::{
    _Connection as Connection, _Error as Error, _OptionalExtension as OptionalExtension,
    _Result as Result, _Row as Row, _RowKey as RowKey, _Statement as Statement,
    _Transaction as Transaction,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_to_string_round_trip() {
        let text = Value::Text("hello".into());
        let converted: String = FromValue::from_value(&text).expect("convert to string");
        assert_eq!(converted, "hello");
    }

    #[test]
    fn value_to_integer_variants() {
        let int_val = Value::Integer(42);
        let converted: i64 = FromValue::from_value(&int_val).expect("convert to i64");
        assert_eq!(converted, 42);
        let converted_u: u64 = FromValue::from_value(&int_val).expect("convert to u64");
        assert_eq!(converted_u, 42);
        let converted_bool: bool = FromValue::from_value(&int_val).expect("convert to bool");
        assert!(converted_bool);
    }

    #[test]
    fn option_conversion_handles_null() {
        let null_val = Value::Null;
        let converted: Option<i64> = FromValue::from_value(&null_val).expect("convert to option");
        assert!(converted.is_none());

        let int_val = Value::Integer(7);
        let converted: Option<i64> = FromValue::from_value(&int_val).expect("convert to option");
        assert_eq!(converted, Some(7));
    }

    #[test]
    fn blob_conversion_from_integer() {
        let value = Value::Integer(0x0102_0304_0506_0708);
        let bytes: Vec<u8> = FromValue::from_value(&value).expect("convert to bytes");
        assert_eq!(bytes.len(), 8);
    }
}
