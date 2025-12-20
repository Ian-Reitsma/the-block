#![allow(
    clippy::needless_lifetimes,
    clippy::iter_cloned_collect,
    clippy::extra_unused_lifetimes,
    clippy::needless_borrow
)]
#![forbid(unsafe_code)]

use foundation_serialization::json::{
    self, Map as JsonMap, Number as JsonNumber, Value as JsonValue,
};
use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
    message: Cow<'static, str>,
}

impl ValueConversionError {
    pub fn new(message: impl Into<Cow<'static, str>>) -> Self {
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

    fn get(&self, index: usize) -> Result<&Value> {
        self.values
            .get(index)
            .ok_or_else(|| Error::Parse(format!("missing parameter {index}")))
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

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Parse(String),
    Serialization(foundation_serialization::Error),
    QueryReturnedNoRows,
    UnknownTable(String),
    UnknownColumn(String),
    ConstraintViolation(String),
    Value(ValueConversionError),
}

impl Error {
    fn parse(msg: impl Into<String>) -> Self {
        Self::Parse(msg.into())
    }

    fn unknown_table(name: &str) -> Self {
        Self::UnknownTable(name.to_string())
    }

    fn unknown_column(name: &str) -> Self {
        Self::UnknownColumn(name.to_string())
    }

    fn constraint(msg: impl Into<String>) -> Self {
        Self::ConstraintViolation(msg.into())
    }

    pub fn is_query_returned_no_rows(&self) -> bool {
        matches!(self, Self::QueryReturnedNoRows)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(err) => err.fmt(f),
            Error::Parse(msg) => write!(f, "parse error: {msg}"),
            Error::Serialization(err) => err.fmt(f),
            Error::QueryReturnedNoRows => write!(f, "query returned no rows"),
            Error::UnknownTable(name) => write!(f, "unknown table '{name}'"),
            Error::UnknownColumn(name) => write!(f, "unknown column '{name}'"),
            Error::ConstraintViolation(msg) => write!(f, "constraint violation: {msg}"),
            Error::Value(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            Error::Serialization(err) => Some(err),
            Error::Value(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<foundation_serialization::Error> for Error {
    fn from(err: foundation_serialization::Error) -> Self {
        Error::Serialization(err)
    }
}

impl From<ValueConversionError> for Error {
    fn from(err: ValueConversionError) -> Self {
        Error::Value(err)
    }
}

#[derive(Clone)]
struct ColumnDef {
    name: String,
    primary_key: bool,
    auto_increment: bool,
}

#[derive(Clone)]
struct Column {
    name: String,
}

#[derive(Clone)]
struct RowData {
    values: Vec<Value>,
}

#[derive(Clone)]
struct Table {
    columns: Vec<Column>,
    primary_key: Option<usize>,
    auto_increment: Option<usize>,
    next_auto_id: i64,
    rows: Vec<RowData>,
}

impl Table {
    fn new(
        columns: Vec<Column>,
        primary_key: Option<usize>,
        auto_increment: Option<usize>,
    ) -> Self {
        Self {
            columns,
            primary_key,
            auto_increment,
            next_auto_id: 1,
            rows: Vec::new(),
        }
    }
}

#[derive(Clone, Default)]
struct Database {
    tables: HashMap<String, Table>,
}

struct DatabaseFile {
    path: PathBuf,
    data: Database,
}

fn value_to_json(value: &Value) -> JsonValue {
    match value {
        Value::Null => JsonValue::Null,
        Value::Integer(v) => JsonValue::Number(JsonNumber::from(*v)),
        Value::Real(v) => JsonNumber::from_f64(*v)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::Text(text) => JsonValue::String(text.clone()),
        Value::Blob(bytes) => JsonValue::Array(
            bytes
                .iter()
                .map(|b| JsonValue::Number(JsonNumber::from(*b as i64)))
                .collect(),
        ),
    }
}

fn value_from_json(value: &JsonValue) -> Result<Value> {
    match value {
        JsonValue::Null => Ok(Value::Null),
        JsonValue::String(text) => Ok(Value::Text(text.clone())),
        JsonValue::Number(num) => {
            if let Some(int) = num.as_i64() {
                Ok(Value::Integer(int))
            } else {
                Ok(Value::Real(num.as_f64()))
            }
        }
        JsonValue::Array(items) => {
            let mut bytes = Vec::with_capacity(items.len());
            for item in items {
                let JsonValue::Number(num) = item else {
                    return Err(Error::parse("blob entries must be numbers"));
                };
                let Some(value) = num.as_u64() else {
                    return Err(Error::parse("blob entries must be unsigned integers"));
                };
                if value > 255 {
                    return Err(Error::parse("blob entry exceeds u8 range"));
                }
                bytes.push(value as u8);
            }
            Ok(Value::Blob(bytes))
        }
        JsonValue::Bool(flag) => Ok(Value::Integer(if *flag { 1 } else { 0 })),
        JsonValue::Object(_) => Err(Error::parse("objects are not valid value representations")),
    }
}

fn database_to_json(db: &Database) -> JsonValue {
    let mut tables = JsonMap::new();
    for (name, table) in &db.tables {
        let columns = JsonValue::Array(
            table
                .columns
                .iter()
                .map(|column| JsonValue::String(column.name.clone()))
                .collect(),
        );
        let rows = JsonValue::Array(
            table
                .rows
                .iter()
                .map(|row| JsonValue::Array(row.values.iter().map(value_to_json).collect()))
                .collect(),
        );
        let mut table_obj = JsonMap::new();
        table_obj.insert("columns".into(), columns);
        if let Some(pk) = table.primary_key {
            table_obj.insert(
                "primary_key".into(),
                JsonValue::Number(JsonNumber::from(pk as i64)),
            );
        }
        if let Some(auto) = table.auto_increment {
            table_obj.insert(
                "auto_increment".into(),
                JsonValue::Number(JsonNumber::from(auto as i64)),
            );
        }
        table_obj.insert(
            "next_auto_id".into(),
            JsonValue::Number(JsonNumber::from(table.next_auto_id)),
        );
        table_obj.insert("rows".into(), rows);
        tables.insert(name.clone(), JsonValue::Object(table_obj));
    }
    let mut root = JsonMap::new();
    root.insert("tables".into(), JsonValue::Object(tables));
    JsonValue::Object(root)
}

fn database_from_json(value: JsonValue) -> Result<Database> {
    let JsonValue::Object(mut root) = value else {
        return Err(Error::parse("database JSON must be an object"));
    };
    let tables_value = root
        .remove("tables")
        .unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
    let JsonValue::Object(tables_map) = tables_value else {
        return Err(Error::parse("tables must be an object"));
    };
    let mut tables = HashMap::new();
    for (name, table_value) in tables_map {
        let JsonValue::Object(mut table_obj) = table_value else {
            return Err(Error::parse("table entry must be an object"));
        };
        let columns_value = table_obj
            .remove("columns")
            .ok_or_else(|| Error::parse("table missing columns"))?;
        let JsonValue::Array(column_names) = columns_value else {
            return Err(Error::parse("columns must be an array"));
        };
        let mut columns = Vec::new();
        for value in column_names {
            let JsonValue::String(name) = value else {
                return Err(Error::parse("column names must be strings"));
            };
            columns.push(Column { name });
        }
        let primary_key = table_obj
            .get("primary_key")
            .and_then(|value| value.as_i64())
            .map(|v| v as usize);
        let auto_increment = table_obj
            .get("auto_increment")
            .and_then(|value| value.as_i64())
            .map(|v| v as usize);
        let next_auto_id = table_obj
            .get("next_auto_id")
            .and_then(|value| value.as_i64())
            .unwrap_or(1);
        let rows_value = table_obj
            .remove("rows")
            .ok_or_else(|| Error::parse("table missing rows"))?;
        let JsonValue::Array(rows_array) = rows_value else {
            return Err(Error::parse("rows must be an array"));
        };
        let mut rows = Vec::new();
        for row_value in rows_array {
            let JsonValue::Array(values) = row_value else {
                return Err(Error::parse("row must be an array"));
            };
            let mut row_values = Vec::with_capacity(values.len());
            for value in values {
                row_values.push(value_from_json(&value)?);
            }
            rows.push(RowData { values: row_values });
        }
        tables.insert(
            name,
            Table {
                columns,
                primary_key,
                auto_increment,
                next_auto_id,
                rows,
            },
        );
    }
    Ok(Database { tables })
}

impl DatabaseFile {
    fn load(path: PathBuf) -> Result<Self> {
        let data = if path.exists() {
            let bytes = fs::read(&path)?;
            if bytes.is_empty() {
                Database::default()
            } else {
                let json_value = json::value_from_slice(&bytes)?;
                database_from_json(json_value)?
            }
        } else {
            Database::default()
        };
        Ok(Self { path, data })
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let json_value = database_to_json(&self.data);
        let bytes = json::to_vec_value(&json_value);
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, &bytes)?;
        fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

pub struct Connection {
    inner: Arc<Mutex<DatabaseFile>>,
}

impl Clone for Connection {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Connection {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = DatabaseFile::load(path.as_ref().to_path_buf())?;
        Ok(Self {
            inner: Arc::new(Mutex::new(db)),
        })
    }

    pub fn execute<P: IntoParams>(&self, sql: &str, params: P) -> Result<usize> {
        let params = params.into_params();
        let kind = parse_statement(sql)?;
        self.execute_kind(&kind, &params)
    }

    fn execute_kind(&self, kind: &StatementKind, params: &Params) -> Result<usize> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| Error::parse("connection poisoned"))?;
        let mut db = guard.data.clone();
        let outcome = apply_statement(&mut db, kind, params)?;
        if outcome.mutated {
            guard.data = db;
            guard.save()?;
        }
        Ok(outcome.rows_affected)
    }

    pub fn query_row<P, F, T>(&self, sql: &str, params: P, f: F) -> Result<T>
    where
        P: IntoParams,
        F: FnOnce(&Row) -> Result<T>,
    {
        let params = params.into_params();
        let kind = parse_statement(sql)?;
        let rows = self.collect_rows(&kind, &params)?;
        let Some(first) = rows.into_iter().next() else {
            return Err(Error::QueryReturnedNoRows);
        };
        f(&first)
    }

    fn collect_rows(&self, kind: &StatementKind, params: &Params) -> Result<Vec<Row>> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| Error::parse("connection poisoned"))?;
        evaluate_select(&guard.data, kind, params)
    }

    pub fn prepare<'conn>(&'conn self, sql: &str) -> Result<Statement<'conn>> {
        let kind = parse_statement(sql)?;
        Ok(Statement {
            kind,
            context: StatementContext::Connection(self),
        })
    }

    pub fn transaction<'conn>(&'conn mut self) -> Result<Transaction<'conn>> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| Error::parse("connection poisoned"))?;
        Ok(Transaction {
            parent: self,
            data: RefCell::new(guard.data.clone()),
            dirty: RefCell::new(false),
        })
    }
}

pub struct Statement<'conn> {
    kind: StatementKind,
    context: StatementContext<'conn>,
}

enum StatementContext<'conn> {
    Connection(&'conn Connection),
    Transaction(&'conn Transaction<'conn>),
}

impl<'conn> Statement<'conn> {
    pub fn query_map<P, F, T>(&mut self, params: P, mut mapper: F) -> Result<Vec<Result<T>>>
    where
        P: IntoParams,
        F: FnMut(&Row) -> Result<T>,
    {
        let params = params.into_params();
        let rows = match &self.context {
            StatementContext::Connection(conn) => conn.collect_rows(&self.kind, &params)?,
            StatementContext::Transaction(tx) => tx.collect_rows(&self.kind, &params)?,
        };
        Ok(rows.into_iter().map(|row| mapper(&row)).collect())
    }

    pub fn execute<P: IntoParams>(&mut self, params: P) -> Result<usize> {
        let params = params.into_params();
        match &self.context {
            StatementContext::Connection(conn) => conn.execute_kind(&self.kind, &params),
            StatementContext::Transaction(tx) => tx.execute_kind(&self.kind, &params),
        }
    }
}

pub struct Transaction<'conn> {
    parent: &'conn Connection,
    data: RefCell<Database>,
    dirty: RefCell<bool>,
}

impl<'conn> Transaction<'conn> {
    pub fn execute<P: IntoParams>(&self, sql: &str, params: P) -> Result<usize> {
        let params = params.into_params();
        let kind = parse_statement(sql)?;
        self.execute_kind(&kind, &params)
    }

    fn execute_kind(&self, kind: &StatementKind, params: &Params) -> Result<usize> {
        let mut data = self.data.borrow().clone();
        let outcome = apply_statement(&mut data, kind, params)?;
        if outcome.mutated {
            *self.dirty.borrow_mut() = true;
            *self.data.borrow_mut() = data;
        }
        Ok(outcome.rows_affected)
    }

    pub fn prepare<'stmt>(&'stmt self, sql: &str) -> Result<Statement<'stmt>> {
        let kind = parse_statement(sql)?;
        Ok(Statement {
            kind,
            context: StatementContext::Transaction(self),
        })
    }

    fn collect_rows(&self, kind: &StatementKind, params: &Params) -> Result<Vec<Row>> {
        let data = self.data.borrow();
        evaluate_select(&data, kind, params)
    }

    pub fn commit(self) -> Result<()> {
        if !*self.dirty.borrow() {
            return Ok(());
        }
        let mut guard = self
            .parent
            .inner
            .lock()
            .map_err(|_| Error::parse("connection poisoned"))?;
        guard.data = self.data.into_inner();
        guard.save()?;
        Ok(())
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

#[derive(Clone)]
pub struct Row {
    columns: Arc<Vec<String>>,
    values: Vec<Value>,
}

impl Row {
    fn new(columns: Arc<Vec<String>>, values: Vec<Value>) -> Self {
        Self { columns, values }
    }

    pub fn get<K, T>(&self, key: K) -> Result<T>
    where
        K: Into<RowKey>,
        T: FromValue,
    {
        let value = match key.into() {
            RowKey::Index(index) => self
                .values
                .get(index)
                .ok_or_else(|| Error::parse(format!("column index {index} out of range")))?,
            RowKey::Name(name) => {
                let Some(position) = self.columns.iter().position(|candidate| candidate == &name)
                else {
                    return Err(Error::unknown_column(&name));
                };
                &self.values[position]
            }
        };
        T::from_value(value).map_err(Error::from)
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

struct ExecuteOutcome {
    rows_affected: usize,
    mutated: bool,
}

enum StatementKind {
    CreateTable(CreateTableStmt),
    Insert(InsertStmt),
    InsertOrReplace(InsertStmt),
    InsertWithConflict(InsertConflict),
    Delete(DeleteStmt),
    Select(SelectStmt),
    ProviderStatsJoin(ProviderStatsJoin),
}

struct CreateTableStmt {
    table: String,
    columns: Vec<ColumnDef>,
}

struct InsertStmt {
    table: String,
    columns: Vec<String>,
    values: Vec<ValueExpr>,
}

struct InsertConflict {
    insert: InsertStmt,
    conflict_column: String,
    updates: Vec<String>,
}

struct DeleteStmt {
    table: String,
}

struct SelectStmt {
    table: String,
    columns: Vec<String>,
    filter: Option<Filter>,
    order: Option<OrderClause>,
    limit: Option<ValueSource>,
}

struct ProviderStatsJoin {
    provider_table: String,
    contracts_table: String,
}

enum ValueExpr {
    Param(usize),
    Literal(Value),
}

enum ValueSource {
    Param(usize),
    Literal(i64),
}

enum Filter {
    EqualsParam { column: String, param: usize },
    EqualsLiteral { column: String, value: Value },
    LikeParam { column: String, param: usize },
}

struct OrderClause {
    column: String,
    descending: bool,
}

fn parse_statement(sql: &str) -> Result<StatementKind> {
    let trimmed = sql.trim();
    if let Some(rest) = trimmed.strip_prefix("CREATE TABLE") {
        return parse_create_table(rest).map(StatementKind::CreateTable);
    }
    if let Some(rest) = trimmed.strip_prefix("INSERT OR REPLACE INTO") {
        return parse_insert(rest).map(StatementKind::InsertOrReplace);
    }
    if let Some(rest) = trimmed.strip_prefix("INSERT INTO") {
        if let Some(conflict) = parse_conflict_clause(rest)? {
            return Ok(StatementKind::InsertWithConflict(conflict));
        }
        return parse_insert(rest).map(StatementKind::Insert);
    }
    if let Some(rest) = trimmed.strip_prefix("DELETE FROM") {
        return parse_delete(rest).map(StatementKind::Delete);
    }
    if trimmed.starts_with("SELECT") {
        if trimmed.contains("LEFT JOIN") {
            return parse_provider_join(trimmed).map(StatementKind::ProviderStatsJoin);
        }
        return parse_select(trimmed).map(StatementKind::Select);
    }
    Err(Error::parse(format!("unsupported statement: {trimmed}")))
}

fn parse_create_table(sql: &str) -> Result<CreateTableStmt> {
    let mut rest = sql.trim_start();
    if let Some(after_if) = rest.strip_prefix("IF NOT EXISTS") {
        rest = after_if.trim_start();
    }
    let open = rest
        .find('(')
        .ok_or_else(|| Error::parse("CREATE TABLE missing column list"))?;
    let table = rest[..open].trim();
    let close = rest
        .rfind(')')
        .ok_or_else(|| Error::parse("CREATE TABLE missing closing ')'"))?;
    let columns_str = &rest[open + 1..close];
    let mut columns = Vec::new();
    for raw_column in split_csv(columns_str) {
        let tokens: Vec<&str> = raw_column.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        let name = normalize_identifier(tokens[0]);
        let mut primary_key = false;
        let mut auto_increment = false;
        for token in tokens.iter().skip(1) {
            match token.to_ascii_uppercase().as_str() {
                "PRIMARY" | "KEY" => primary_key = true,
                "AUTOINCREMENT" => auto_increment = true,
                _ => {}
            }
        }
        columns.push(ColumnDef {
            name,
            primary_key,
            auto_increment,
        });
    }
    Ok(CreateTableStmt {
        table: table.to_string(),
        columns,
    })
}

fn parse_insert(sql: &str) -> Result<InsertStmt> {
    let rest = sql.trim_start();
    let open = rest
        .find('(')
        .ok_or_else(|| Error::parse("INSERT missing column list"))?;
    let table = rest[..open].trim();
    let close = rest
        .find(')')
        .ok_or_else(|| Error::parse("INSERT missing closing ')'"))?;
    let columns = rest[open + 1..close]
        .split(',')
        .map(|part| normalize_identifier(part.trim()))
        .collect::<Vec<_>>();
    let after_columns = rest[close + 1..].trim_start();
    let values_start = after_columns
        .strip_prefix("VALUES")
        .ok_or_else(|| Error::parse("INSERT missing VALUES"))?
        .trim_start();
    let open_values = values_start
        .find('(')
        .ok_or_else(|| Error::parse("INSERT missing values"))?;
    let close_values = find_matching_paren(values_start, open_values)
        .ok_or_else(|| Error::parse("INSERT missing value terminator"))?;
    let values_str = &values_start[open_values + 1..close_values];
    let values = split_csv(values_str)
        .into_iter()
        .map(|segment| parse_value_expr(segment.as_str()))
        .collect::<Result<Vec<_>>>()?;
    Ok(InsertStmt {
        table: table.to_string(),
        columns,
        values,
    })
}

fn parse_conflict_clause(sql: &str) -> Result<Option<InsertConflict>> {
    let clause_start = match sql.find("ON CONFLICT") {
        Some(idx) => idx,
        None => return Ok(None),
    };
    let clause = &sql[clause_start..];
    let Some(key_start) = clause.find('(') else {
        return Err(Error::parse("ON CONFLICT missing column"));
    };
    let Some(key_end) = clause.find(')') else {
        return Err(Error::parse("ON CONFLICT missing ')'"));
    };
    let conflict_column = normalize_identifier(clause[key_start + 1..key_end].trim());
    let update_start = clause
        .find("DO UPDATE SET")
        .ok_or_else(|| Error::parse("ON CONFLICT missing DO UPDATE"))?;
    let assignments = &clause[update_start + "DO UPDATE SET".len()..];
    let updates = split_csv(assignments)
        .into_iter()
        .map(|expr| {
            let parts: Vec<&str> = expr.split('=').collect();
            if parts.len() != 2 {
                return Err(Error::parse("unsupported conflict assignment"));
            }
            Ok(normalize_identifier(parts[0].trim()))
        })
        .collect::<Result<Vec<_>>>()?;
    let insert = parse_insert(sql)?;
    Ok(Some(InsertConflict {
        insert,
        conflict_column,
        updates,
    }))
}

fn parse_delete(sql: &str) -> Result<DeleteStmt> {
    Ok(DeleteStmt {
        table: normalize_identifier(sql.trim()),
    })
}

fn parse_select(sql: &str) -> Result<SelectStmt> {
    let after_select = sql
        .strip_prefix("SELECT")
        .ok_or_else(|| Error::parse("SELECT missing keyword"))?
        .trim_start();
    let from_idx = after_select
        .to_ascii_uppercase()
        .find(" FROM ")
        .ok_or_else(|| Error::parse("SELECT missing FROM"))?;
    let column_part = &after_select[..from_idx];
    let columns = split_csv(column_part)
        .into_iter()
        .map(|column| normalize_identifier(column.as_str()))
        .collect::<Vec<_>>();
    let mut remainder = after_select[from_idx + 6..].trim();
    let table;
    let mut filter = None;
    let mut order = None;
    let mut limit = None;
    if let Some(idx) = remainder.to_ascii_uppercase().find(" WHERE ") {
        table = normalize_identifier(remainder[..idx].trim());
        remainder = remainder[idx + 7..].trim();
        let (cond, tail) = split_once_keywords(remainder, &["ORDER BY", "LIMIT"]);
        filter = Some(parse_filter(cond.trim())?);
        remainder = tail;
    } else {
        let (tbl, tail) = split_once_keywords(remainder, &["ORDER BY", "LIMIT"]);
        table = normalize_identifier(tbl.trim());
        remainder = tail;
    }
    if !remainder.is_empty() {
        let mut working = remainder.trim_start();
        if let Some(rest) = working.strip_prefix("ORDER BY") {
            let (order_clause, tail) = split_once_keywords(rest.trim_start(), &["LIMIT"]);
            order = Some(parse_order(order_clause.trim())?);
            working = tail;
        }
        if let Some(rest) = working.strip_prefix("LIMIT") {
            limit = Some(parse_limit(rest.trim())?);
        }
    }
    Ok(SelectStmt {
        table,
        columns,
        filter,
        order,
        limit,
    })
}

fn parse_provider_join(sql: &str) -> Result<ProviderStatsJoin> {
    let upper = sql.to_ascii_uppercase();
    let from_idx = upper
        .find("FROM ")
        .ok_or_else(|| Error::parse("JOIN missing FROM"))?;
    let join_idx = upper
        .find(" LEFT JOIN ")
        .ok_or_else(|| Error::parse("JOIN missing LEFT JOIN"))?;
    let provider_table = strip_table_alias(sql[from_idx + 5..join_idx].trim());
    let on_idx = upper
        .find(" ON ")
        .ok_or_else(|| Error::parse("JOIN missing ON"))?;
    let contracts_table = strip_table_alias(sql[join_idx + 10..on_idx].trim());
    Ok(ProviderStatsJoin {
        provider_table,
        contracts_table,
    })
}

fn parse_filter(input: &str) -> Result<Filter> {
    let upper = input.to_ascii_uppercase();
    if let Some(idx) = upper.find(" LIKE ") {
        let column = normalize_identifier(&input[..idx].trim());
        let value = input[idx + 6..].trim();
        match parse_value_expr(value)? {
            ValueExpr::Param(param) => Ok(Filter::LikeParam { column, param }),
            ValueExpr::Literal(lit) => Ok(Filter::EqualsLiteral { column, value: lit }),
        }
    } else if let Some(idx) = upper.find('=') {
        let column = normalize_identifier(&input[..idx].trim());
        let value = input[idx + 1..].trim();
        match parse_value_expr(value)? {
            ValueExpr::Param(param) => Ok(Filter::EqualsParam { column, param }),
            ValueExpr::Literal(value) => Ok(Filter::EqualsLiteral { column, value }),
        }
    } else {
        Err(Error::parse("unsupported WHERE clause"))
    }
}

fn parse_order(input: &str) -> Result<OrderClause> {
    let mut parts = input.split_whitespace();
    let column = parts
        .next()
        .map(normalize_identifier)
        .ok_or_else(|| Error::parse("ORDER BY missing column"))?;
    let descending = parts
        .next()
        .map(|token| token.eq_ignore_ascii_case("DESC"))
        .unwrap_or(false);
    Ok(OrderClause { column, descending })
}

fn parse_limit(input: &str) -> Result<ValueSource> {
    match parse_value_expr(input)? {
        ValueExpr::Param(param) => Ok(ValueSource::Param(param)),
        ValueExpr::Literal(Value::Integer(v)) => Ok(ValueSource::Literal(v)),
        ValueExpr::Literal(_) => Err(Error::parse("LIMIT literal must be integer")),
    }
}

fn parse_value_expr(input: &str) -> Result<ValueExpr> {
    let trimmed = input.trim();
    if let Some(rest) = trimmed.strip_prefix('?') {
        let index = rest
            .parse::<usize>()
            .map_err(|_| Error::parse("invalid parameter reference"))?;
        return Ok(ValueExpr::Param(index - 1));
    }
    if let Some(rest) = trimmed.strip_prefix('\'') {
        let value = rest
            .strip_suffix('\'')
            .ok_or_else(|| Error::parse("unterminated string literal"))?;
        return Ok(ValueExpr::Literal(Value::Text(value.to_string())));
    }
    if let Some(rest) = trimmed.strip_prefix('"') {
        let value = rest
            .strip_suffix('"')
            .ok_or_else(|| Error::parse("unterminated identifier"))?;
        return Ok(ValueExpr::Literal(Value::Text(value.to_string())));
    }
    if trimmed.eq_ignore_ascii_case("NULL") {
        return Ok(ValueExpr::Literal(Value::Null));
    }
    if let Ok(int) = trimmed.parse::<i64>() {
        return Ok(ValueExpr::Literal(Value::Integer(int)));
    }
    if let Ok(float) = trimmed.parse::<f64>() {
        return Ok(ValueExpr::Literal(Value::Real(float)));
    }
    Ok(ValueExpr::Literal(Value::Text(trimmed.to_string())))
}

fn apply_statement(
    db: &mut Database,
    kind: &StatementKind,
    params: &Params,
) -> Result<ExecuteOutcome> {
    match kind {
        StatementKind::CreateTable(stmt) => apply_create_table(db, stmt),
        StatementKind::Insert(stmt) => apply_insert(db, stmt, params, false),
        StatementKind::InsertOrReplace(stmt) => apply_insert(db, stmt, params, true),
        StatementKind::InsertWithConflict(stmt) => apply_insert_conflict(db, stmt, params),
        StatementKind::Delete(stmt) => apply_delete(db, stmt),
        StatementKind::Select(_) | StatementKind::ProviderStatsJoin(_) => {
            Err(Error::parse("SELECT must use query APIs"))
        }
    }
}

fn apply_create_table(db: &mut Database, stmt: &CreateTableStmt) -> Result<ExecuteOutcome> {
    if db.tables.contains_key(&stmt.table) {
        return Ok(ExecuteOutcome {
            rows_affected: 0,
            mutated: false,
        });
    }
    let mut columns = Vec::new();
    let mut primary_key = None;
    let mut auto_increment = None;
    for (idx, column) in stmt.columns.iter().enumerate() {
        columns.push(Column {
            name: column.name.clone(),
        });
        if column.primary_key {
            primary_key = Some(idx);
        }
        if column.auto_increment {
            auto_increment = Some(idx);
        }
    }
    db.tables.insert(
        stmt.table.clone(),
        Table::new(columns, primary_key, auto_increment),
    );
    Ok(ExecuteOutcome {
        rows_affected: 0,
        mutated: true,
    })
}

fn apply_insert(
    db: &mut Database,
    stmt: &InsertStmt,
    params: &Params,
    replace: bool,
) -> Result<ExecuteOutcome> {
    let table = db
        .tables
        .get_mut(&stmt.table)
        .ok_or_else(|| Error::unknown_table(&stmt.table))?;
    if stmt.columns.len() != stmt.values.len() {
        return Err(Error::parse("column/value count mismatch"));
    }
    let mut row = vec![Value::Null; table.columns.len()];
    for (col, expr) in stmt.columns.iter().zip(stmt.values.iter()) {
        let idx = table
            .columns
            .iter()
            .position(|c| c.name == *col)
            .ok_or_else(|| Error::unknown_column(col))?;
        row[idx] = resolve_value(expr, params)?.clone();
    }
    if let Some(auto_idx) = table.auto_increment {
        if matches!(row[auto_idx], Value::Null) {
            row[auto_idx] = Value::Integer(table.next_auto_id);
            table.next_auto_id = table.next_auto_id.saturating_add(1);
        } else if let Value::Integer(v) = row[auto_idx] {
            if v >= table.next_auto_id {
                table.next_auto_id = v.saturating_add(1);
            }
        }
    }
    if replace {
        if let Some(pk) = table.primary_key {
            if matches!(row[pk], Value::Null) {
                return Err(Error::constraint("primary key cannot be NULL"));
            }
            if let Some(existing) = table
                .rows
                .iter()
                .position(|candidate| candidate.values.get(pk) == row.get(pk))
            {
                table.rows[existing] = RowData { values: row };
                return Ok(ExecuteOutcome {
                    rows_affected: 1,
                    mutated: true,
                });
            }
        }
    }
    table.rows.push(RowData { values: row });
    Ok(ExecuteOutcome {
        rows_affected: 1,
        mutated: true,
    })
}

fn apply_insert_conflict(
    db: &mut Database,
    stmt: &InsertConflict,
    params: &Params,
) -> Result<ExecuteOutcome> {
    let table = db
        .tables
        .get_mut(&stmt.insert.table)
        .ok_or_else(|| Error::unknown_table(&stmt.insert.table))?;
    let mut row = vec![Value::Null; table.columns.len()];
    for (col, expr) in stmt.insert.columns.iter().zip(stmt.insert.values.iter()) {
        let idx = table
            .columns
            .iter()
            .position(|c| c.name == *col)
            .ok_or_else(|| Error::unknown_column(col))?;
        row[idx] = resolve_value(expr, params)?.clone();
    }
    let conflict_idx = table
        .columns
        .iter()
        .position(|c| c.name == stmt.conflict_column)
        .ok_or_else(|| Error::unknown_column(&stmt.conflict_column))?;
    let conflict_value = row.get(conflict_idx).cloned().unwrap_or(Value::Null);
    if matches!(conflict_value, Value::Null) {
        return Err(Error::constraint("conflict column cannot be NULL"));
    }
    if let Some(existing) = table
        .rows
        .iter()
        .position(|candidate| candidate.values.get(conflict_idx) == Some(&conflict_value))
    {
        let mut updated = table.rows[existing].values.clone();
        for column in &stmt.updates {
            let idx = table
                .columns
                .iter()
                .position(|c| c.name == *column)
                .ok_or_else(|| Error::unknown_column(column))?;
            let base = match updated.get(idx) {
                Some(Value::Integer(v)) => *v,
                Some(_) => return Err(Error::constraint("conflict update expects integer")),
                None => 0,
            };
            let added = match row.get(idx) {
                Some(Value::Integer(v)) => *v,
                Some(_) => return Err(Error::constraint("conflict update expects integer")),
                None => 0,
            };
            updated[idx] = Value::Integer(base.saturating_add(added));
        }
        table.rows[existing] = RowData { values: updated };
        Ok(ExecuteOutcome {
            rows_affected: 1,
            mutated: true,
        })
    } else {
        table.rows.push(RowData { values: row });
        Ok(ExecuteOutcome {
            rows_affected: 1,
            mutated: true,
        })
    }
}

fn apply_delete(db: &mut Database, stmt: &DeleteStmt) -> Result<ExecuteOutcome> {
    let table = db
        .tables
        .get_mut(&stmt.table)
        .ok_or_else(|| Error::unknown_table(&stmt.table))?;
    let removed = table.rows.len();
    table.rows.clear();
    Ok(ExecuteOutcome {
        rows_affected: removed,
        mutated: removed > 0,
    })
}

fn evaluate_select(db: &Database, kind: &StatementKind, params: &Params) -> Result<Vec<Row>> {
    match kind {
        StatementKind::Select(stmt) => evaluate_basic_select(db, stmt, params),
        StatementKind::ProviderStatsJoin(stmt) => evaluate_provider_join(db, stmt),
        StatementKind::CreateTable(_)
        | StatementKind::Insert(_)
        | StatementKind::InsertOrReplace(_)
        | StatementKind::InsertWithConflict(_)
        | StatementKind::Delete(_) => Err(Error::parse("statement does not return rows")),
    }
}

fn evaluate_basic_select(db: &Database, stmt: &SelectStmt, params: &Params) -> Result<Vec<Row>> {
    let table = db
        .tables
        .get(&stmt.table)
        .ok_or_else(|| Error::unknown_table(&stmt.table))?;
    let mut matched: Vec<&RowData> = Vec::new();
    for row in &table.rows {
        if let Some(filter) = &stmt.filter {
            if !row_matches(table, filter, params, row)? {
                continue;
            }
        }
        matched.push(row);
    }
    if let Some(order) = &stmt.order {
        let idx = table
            .columns
            .iter()
            .position(|c| c.name == order.column)
            .ok_or_else(|| Error::unknown_column(&order.column))?;
        matched
            .sort_by(|a, b| compare_values(a.values.get(idx), b.values.get(idx), order.descending));
    }
    if let Some(limit) = &stmt.limit {
        let limit = match limit {
            ValueSource::Literal(v) => (*v).max(0) as usize,
            ValueSource::Param(idx) => match params.get(*idx)? {
                Value::Integer(v) => (*v).max(0) as usize,
                _ => return Err(Error::parse("LIMIT parameter must be integer")),
            },
        };
        if matched.len() > limit {
            matched.truncate(limit);
        }
    }
    let columns = Arc::new(stmt.columns.clone());
    matched
        .into_iter()
        .map(|row| {
            let values = project_row(table, &stmt.columns, row)?;
            Ok(Row::new(Arc::clone(&columns), values))
        })
        .collect()
}

fn evaluate_provider_join(db: &Database, stmt: &ProviderStatsJoin) -> Result<Vec<Row>> {
    let providers = db
        .tables
        .get(&stmt.provider_table)
        .ok_or_else(|| Error::unknown_table(&stmt.provider_table))?;
    let contracts = db
        .tables
        .get(&stmt.contracts_table)
        .ok_or_else(|| Error::unknown_table(&stmt.contracts_table))?;
    let provider_id_idx = providers
        .columns
        .iter()
        .position(|c| c.name == "provider_id")
        .ok_or_else(|| Error::unknown_column("provider_id"))?;
    let capacity_idx = providers
        .columns
        .iter()
        .position(|c| c.name == "capacity_bytes")
        .ok_or_else(|| Error::unknown_column("capacity_bytes"))?;
    let reputation_idx = providers
        .columns
        .iter()
        .position(|c| c.name == "reputation")
        .ok_or_else(|| Error::unknown_column("reputation"))?;
    let contract_provider_idx = contracts
        .columns
        .iter()
        .position(|c| c.name == "provider_id")
        .ok_or_else(|| Error::unknown_column("provider_id"))?;
    let mut rows = Vec::new();
    for provider in &providers.rows {
        let provider_id = provider.values[provider_id_idx].clone();
        let contracts_count = contracts
            .rows
            .iter()
            .filter(|row| row.values.get(contract_provider_idx) == Some(&provider_id))
            .count() as i64;
        rows.push(vec![
            provider_id,
            provider.values[capacity_idx].clone(),
            provider.values[reputation_idx].clone(),
            Value::Integer(contracts_count),
        ]);
    }
    let columns = Arc::new(vec![
        "provider_id".to_string(),
        "capacity_bytes".to_string(),
        "reputation".to_string(),
        "contracts".to_string(),
    ]);
    Ok(rows
        .into_iter()
        .map(|values| Row::new(Arc::clone(&columns), values))
        .collect())
}

fn resolve_value<'a>(expr: &'a ValueExpr, params: &'a Params) -> Result<&'a Value> {
    match expr {
        ValueExpr::Param(idx) => params.get(*idx),
        ValueExpr::Literal(value) => Ok(value),
    }
}

fn row_matches(table: &Table, filter: &Filter, params: &Params, row: &RowData) -> Result<bool> {
    match filter {
        Filter::EqualsParam { column, param } => {
            let idx = table
                .columns
                .iter()
                .position(|c| c.name == *column)
                .ok_or_else(|| Error::unknown_column(column))?;
            Ok(row.values.get(idx) == Some(params.get(*param)?))
        }
        Filter::EqualsLiteral { column, value } => {
            let idx = table
                .columns
                .iter()
                .position(|c| c.name == *column)
                .ok_or_else(|| Error::unknown_column(column))?;
            Ok(row.values.get(idx) == Some(value))
        }
        Filter::LikeParam { column, param } => {
            let idx = table
                .columns
                .iter()
                .position(|c| c.name == *column)
                .ok_or_else(|| Error::unknown_column(column))?;
            let pattern = params.get(*param)?;
            like_matches(row.values.get(idx), pattern)
        }
    }
}

fn like_matches(value: Option<&Value>, pattern: &Value) -> Result<bool> {
    let Value::Text(pattern) = pattern else {
        return Err(Error::parse("LIKE pattern must be text"));
    };
    let haystack = match value {
        Some(Value::Text(text)) => text.as_str(),
        Some(Value::Integer(v)) => return Ok(pattern == &v.to_string()),
        Some(Value::Null) | None => return Ok(false),
        Some(Value::Real(v)) => return Ok(pattern == &v.to_string()),
        Some(Value::Blob(_)) => return Ok(false),
    };
    Ok(matches_pattern(haystack, pattern))
}

fn matches_pattern(haystack: &str, pattern: &str) -> bool {
    if !pattern.contains('%') {
        return haystack == pattern;
    }
    let parts: Vec<&str> = pattern.split('%').collect();
    if parts.len() == 1 {
        return haystack == pattern;
    }
    let mut remaining = haystack;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            if !remaining.ends_with(part) {
                return false;
            }
        } else if let Some(pos) = remaining.find(part) {
            remaining = &remaining[pos + part.len()..];
        } else {
            return false;
        }
    }
    true
}

fn project_row(table: &Table, columns: &[String], row: &RowData) -> Result<Vec<Value>> {
    columns
        .iter()
        .map(|column| {
            let idx = table
                .columns
                .iter()
                .position(|c| c.name == *column)
                .ok_or_else(|| Error::unknown_column(column))?;
            Ok(row.values[idx].clone())
        })
        .collect()
}

fn compare_values(a: Option<&Value>, b: Option<&Value>, descending: bool) -> Ordering {
    let ord = value_cmp(a, b);
    if descending {
        ord.reverse()
    } else {
        ord
    }
}

fn value_cmp(a: Option<&Value>, b: Option<&Value>) -> Ordering {
    match (a, b) {
        (Some(Value::Integer(lhs)), Some(Value::Integer(rhs))) => lhs.cmp(rhs),
        (Some(Value::Real(lhs)), Some(Value::Real(rhs))) => {
            lhs.partial_cmp(rhs).unwrap_or(Ordering::Equal)
        }
        (Some(Value::Text(lhs)), Some(Value::Text(rhs))) => lhs.cmp(rhs),
        (Some(Value::Null), Some(Value::Null)) => Ordering::Equal,
        (Some(Value::Null), _) => Ordering::Less,
        (_, Some(Value::Null)) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn split_csv(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for ch in input.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(ch);
            }
            ',' if !in_single && !in_double => {
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn split_once_keywords<'a>(input: &'a str, keywords: &[&str]) -> (&'a str, &'a str) {
    let upper = input.to_ascii_uppercase();
    for keyword in keywords {
        if let Some(idx) = upper.find(keyword) {
            let head = input[..idx].trim_end();
            let tail = input[idx..].trim_start();
            return (head, tail);
        }
    }
    (input, "")
}

fn normalize_identifier(input: &str) -> String {
    input.trim().trim_matches('"').to_string()
}

fn strip_table_alias(input: &str) -> String {
    input
        .split_whitespace()
        .next()
        .map(normalize_identifier)
        .unwrap_or_default()
}

fn find_matching_paren(input: &str, open_idx: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    if bytes.get(open_idx)? != &b'(' {
        return None;
    }
    let mut depth = 0usize;
    for (idx, ch) in bytes.iter().enumerate().skip(open_idx) {
        match ch {
            b'(' => depth = depth.saturating_add(1),
            b')' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("foundation_sqlite_tests");
        let _ = fs::create_dir_all(&dir);
        dir.join(format!("{name}.db"))
    }

    #[test]
    fn create_insert_select_round_trip() {
        let path = temp_path("basic");
        let _ = fs::remove_file(&path);
        let conn = Connection::open(&path).expect("open");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS blocks (hash TEXT PRIMARY KEY, height INTEGER, data BLOB)",
            params![],
        )
        .expect("create");
        conn.execute(
            "INSERT OR REPLACE INTO blocks (hash, height, data) VALUES (?1, ?2, ?3)",
            params!["abc", 42_i64, vec![1u8, 2, 3]],
        )
        .expect("insert");
        let row = conn
            .query_row(
                "SELECT hash, height, data FROM blocks WHERE hash=?1",
                params!["abc"],
                |row| {
                    let hash: String = row.get(0)?;
                    let height: i64 = row.get(1)?;
                    let data: Vec<u8> = row.get(2)?;
                    Ok((hash, height, data))
                },
            )
            .expect("query");
        assert_eq!(row.0, "abc");
        assert_eq!(row.1, 42);
        assert_eq!(row.2, vec![1u8, 2, 3]);
    }

    #[test]
    fn insert_conflict_updates_existing() {
        let path = temp_path("conflict");
        let _ = fs::remove_file(&path);
        let conn = Connection::open(&path).expect("open");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS peer_handshakes (peer_id TEXT PRIMARY KEY, success INTEGER, failure INTEGER)",
            params![],
        )
        .expect("create");
        conn.execute(
            "INSERT INTO peer_handshakes (peer_id, success, failure) VALUES (?1, ?2, ?3) ON CONFLICT(peer_id) DO UPDATE SET success = success + excluded.success, failure = failure + excluded.failure",
            params!["node", 2_i64, 1_i64],
        )
        .expect("insert");
        conn.execute(
            "INSERT INTO peer_handshakes (peer_id, success, failure) VALUES (?1, ?2, ?3) ON CONFLICT(peer_id) DO UPDATE SET success = success + excluded.success, failure = failure + excluded.failure",
            params!["node", 3_i64, 4_i64],
        )
        .expect("conflict");
        let totals = conn
            .query_row(
                "SELECT success, failure FROM peer_handshakes WHERE peer_id=?1",
                params!["node"],
                |row| {
                    let success: i64 = row.get(0)?;
                    let failure: i64 = row.get(1)?;
                    Ok((success, failure))
                },
            )
            .expect("fetch");
        assert_eq!(totals.0, 5);
        assert_eq!(totals.1, 5);
    }

    #[test]
    fn like_filter_matches_patterns() {
        let path = temp_path("like");
        let _ = fs::remove_file(&path);
        let conn = Connection::open(&path).expect("open");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS txs (hash TEXT PRIMARY KEY, memo TEXT)",
            params![],
        )
        .expect("create");
        conn.execute(
            "INSERT OR REPLACE INTO txs (hash, memo) VALUES (?1, ?2)",
            params!["tx1", "hello world"],
        )
        .expect("insert");
        let results = conn
            .prepare("SELECT hash FROM txs WHERE memo LIKE ?1")
            .expect("prepare")
            .query_map(params!["%world%"], |row| row.get::<_, String>(0))
            .expect("query");
        let hashes: Vec<String> = results
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .expect("collect");
        assert_eq!(hashes, vec!["tx1".to_string()]);
    }

    #[test]
    fn order_and_limit() {
        let path = temp_path("order");
        let _ = fs::remove_file(&path);
        let conn = Connection::open(&path).expect("open");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS blocks (hash TEXT PRIMARY KEY, height INTEGER)",
            params![],
        )
        .expect("create");
        for (hash, height) in [("a", 1_i64), ("b", 3_i64), ("c", 2_i64)] {
            conn.execute(
                "INSERT OR REPLACE INTO blocks (hash, height) VALUES (?1, ?2)",
                params![hash, height],
            )
            .expect("insert");
        }
        let rows = conn
            .prepare("SELECT hash FROM blocks ORDER BY height DESC LIMIT 2")
            .expect("prepare")
            .query_map(params![], |row| row.get::<_, String>(0))
            .expect("query");
        let hashes: Vec<String> = rows
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .expect("collect");
        assert_eq!(hashes, vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn provider_join_counts_contracts() {
        let path = temp_path("join");
        let _ = fs::remove_file(&path);
        let conn = Connection::open(&path).expect("open");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS provider_stats (provider_id TEXT PRIMARY KEY, capacity_bytes INTEGER, reputation INTEGER)",
            params![],
        )
        .expect("create stats");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS storage_contracts (object_id TEXT PRIMARY KEY, provider_id TEXT, price_per_block INTEGER)",
            params![],
        )
        .expect("create contracts");
        conn.execute(
            "INSERT OR REPLACE INTO provider_stats (provider_id, capacity_bytes, reputation) VALUES (?1, ?2, ?3)",
            params!["provider-1", 10_i64, 5_i64],
        )
        .expect("insert provider");
        conn.execute(
            "INSERT OR REPLACE INTO storage_contracts (object_id, provider_id, price_per_block) VALUES (?1, ?2, ?3)",
            params!["obj", "provider-1", 7_i64],
        )
        .expect("insert contract");
        let rows = conn
            .prepare(
                "SELECT ps.provider_id, ps.capacity_bytes, ps.reputation, COUNT(sc.object_id) as contracts \n                 FROM provider_stats ps LEFT JOIN storage_contracts sc ON sc.provider_id = ps.provider_id \n                 GROUP BY ps.provider_id, ps.capacity_bytes, ps.reputation",
            )
            .expect("prepare")
            .query_map(params![], |row| {
                let id: String = row.get(0)?;
                let cap: i64 = row.get(1)?;
                let rep: i64 = row.get(2)?;
                let contracts: i64 = row.get(3)?;
                Ok((id, cap, rep, contracts))
            })
            .expect("query");
        let result: Vec<_> = rows
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .expect("collect");
        assert_eq!(result, vec![("provider-1".to_string(), 10, 5, 1)]);
    }
}
