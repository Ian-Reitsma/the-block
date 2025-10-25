use core::fmt;
use std::collections::BTreeMap;
use std::io;
use std::ops::{Index, IndexMut};

use serde::de::value::StringDeserializer;
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde::ser::{
    self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use serde::{Deserialize, Deserializer, Serialize};

pub type Result<T> = std::result::Result<T, Error>;

/// Error returned when JSON encoding or decoding fails.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    Message(String),
    Io(io::Error),
    UnexpectedToken {
        expected: String,
        found: Option<char>,
    },
    TrailingCharacters,
    InvalidNumber,
    InvalidUnicodeEscape,
    InvalidMapKey,
    NonFiniteFloat,
}

impl Error {
    fn message<T: fmt::Display>(message: T) -> Self {
        Self {
            kind: ErrorKind::Message(message.to_string()),
        }
    }

    pub fn io(err: io::Error) -> Self {
        Self {
            kind: ErrorKind::Io(err),
        }
    }

    fn unexpected_token(expected: impl Into<String>, found: Option<char>) -> Self {
        Self {
            kind: ErrorKind::UnexpectedToken {
                expected: expected.into(),
                found,
            },
        }
    }

    fn invalid_number() -> Self {
        Self {
            kind: ErrorKind::InvalidNumber,
        }
    }

    fn invalid_unicode_escape() -> Self {
        Self {
            kind: ErrorKind::InvalidUnicodeEscape,
        }
    }

    fn invalid_map_key() -> Self {
        Self {
            kind: ErrorKind::InvalidMapKey,
        }
    }

    fn non_finite_float() -> Self {
        Self {
            kind: ErrorKind::NonFiniteFloat,
        }
    }
}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::message(msg)
    }
}

impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::message(msg)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Message(msg) => write!(f, "{msg}"),
            ErrorKind::Io(err) => write!(f, "io error: {err}"),
            ErrorKind::UnexpectedToken { expected, found } => match found {
                Some(ch) => write!(f, "expected {expected}, found '{ch}'"),
                None => write!(f, "expected {expected}, found end of input"),
            },
            ErrorKind::TrailingCharacters => write!(f, "trailing characters after JSON value"),
            ErrorKind::InvalidNumber => write!(f, "invalid number"),
            ErrorKind::InvalidUnicodeEscape => write!(f, "invalid unicode escape"),
            ErrorKind::InvalidMapKey => write!(
                f,
                "object keys must serialize to a string, number, bool, or null"
            ),
            ErrorKind::NonFiniteFloat => {
                write!(f, "non-finite floating point numbers are unsupported")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::Io(err) => Some(err),
            _ => None,
        }
    }
}

/// JSON object map implementation.
pub type Map = BTreeMap<String, Value>;

/// Canonical representation of a JSON number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Number(NumberRepr);

#[derive(Clone, Copy, Debug, PartialEq)]
enum NumberRepr {
    PosInt(u64),
    NegInt(i64),
    Float(f64),
}

impl Number {
    pub fn from_f64(value: f64) -> Option<Self> {
        if value.is_finite() {
            Some(Number(NumberRepr::Float(value)))
        } else {
            None
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self.0 {
            NumberRepr::PosInt(v) => Some(v),
            NumberRepr::Float(v) if v.is_finite() && v >= 0.0 && v.fract() == 0.0 => Some(v as u64),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self.0 {
            NumberRepr::PosInt(v) => v.try_into().ok(),
            NumberRepr::NegInt(v) => Some(v),
            NumberRepr::Float(v) if v.is_finite() && v.fract() == 0.0 => Some(v as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> f64 {
        match self.0 {
            NumberRepr::PosInt(v) => v as f64,
            NumberRepr::NegInt(v) => v as f64,
            NumberRepr::Float(v) => v,
        }
    }
}

impl core::fmt::Display for Number {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            NumberRepr::PosInt(v) => write!(f, "{}", v),
            NumberRepr::NegInt(v) => write!(f, "{}", v),
            NumberRepr::Float(v) => {
                if v.fract() == 0.0 {
                    let mut s = format!("{:.1}", v);
                    if s.contains('.') {
                        while s.ends_with('0') {
                            s.pop();
                        }
                        if s.ends_with('.') {
                            s.push('0');
                        }
                    }
                    f.write_str(&s)
                } else {
                    write!(f, "{}", v)
                }
            }
        }
    }
}

impl From<u8> for Number {
    fn from(value: u8) -> Self {
        Number(NumberRepr::PosInt(value as u64))
    }
}

impl From<u16> for Number {
    fn from(value: u16) -> Self {
        Number(NumberRepr::PosInt(value as u64))
    }
}

impl From<u32> for Number {
    fn from(value: u32) -> Self {
        Number(NumberRepr::PosInt(value as u64))
    }
}

impl From<u64> for Number {
    fn from(value: u64) -> Self {
        Number(NumberRepr::PosInt(value))
    }
}

impl From<i8> for Number {
    fn from(value: i8) -> Self {
        Number::from(value as i64)
    }
}

impl From<i16> for Number {
    fn from(value: i16) -> Self {
        Number::from(value as i64)
    }
}

impl From<i32> for Number {
    fn from(value: i32) -> Self {
        Number::from(value as i64)
    }
}

impl From<i64> for Number {
    fn from(value: i64) -> Self {
        if value >= 0 {
            Number(NumberRepr::PosInt(value as u64))
        } else {
            Number(NumberRepr::NegInt(value))
        }
    }
}

impl From<f32> for Number {
    fn from(value: f32) -> Self {
        Number(NumberRepr::Float(value as f64))
    }
}

impl From<f64> for Number {
    fn from(value: f64) -> Self {
        Number(NumberRepr::Float(value))
    }
}

/// Representation of a JSON value.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Object(Map),
}

static NULL: Value = Value::Null;

impl Serialize for Number {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        match self.0 {
            NumberRepr::PosInt(value) => serializer.serialize_u64(value),
            NumberRepr::NegInt(value) => serializer.serialize_i64(value),
            NumberRepr::Float(value) => serializer.serialize_f64(value),
        }
    }
}

impl<'de> Deserialize<'de> for Number {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct NumberVisitor;

        impl Visitor<'_> for NumberVisitor {
            type Value = Number;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON number")
            }

            fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Number::from(v))
            }

            fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Number::from(v))
            }

            fn visit_f64<E>(self, v: f64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Number::from_f64(v).ok_or_else(|| E::custom("non-finite float"))
            }

            fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                if v.contains(['.', 'e', 'E']) {
                    let value: f64 = v.parse().map_err(E::custom)?;
                    Number::from_f64(value).ok_or_else(|| E::custom("non-finite float"))
                } else if let Ok(int) = v.parse::<i64>() {
                    Ok(Number::from(int))
                } else {
                    v.parse::<u64>().map(Number::from).map_err(E::custom)
                }
            }
        }

        deserializer.deserialize_any(NumberVisitor)
    }
}

impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(value) => serializer.serialize_bool(*value),
            Value::Number(number) => number.serialize(serializer),
            Value::String(value) => serializer.serialize_str(value),
            Value::Array(elements) => {
                let mut seq = serializer.serialize_seq(Some(elements.len()))?;
                for element in elements {
                    seq.serialize_element(element)?;
                }
                seq.end()
            }
            Value::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_key(key)?;
                    map.serialize_value(value)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct ValueVisitor;

        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = Value;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("any JSON value")
            }

            fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::Bool(value))
            }

            fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::Number(Number::from(value)))
            }

            fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::Number(Number::from(value)))
            }

            fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Number::from_f64(value)
                    .map(Value::Number)
                    .ok_or_else(|| E::custom("non-finite float"))
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::String(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::String(value))
            }

            fn visit_unit<E>(self) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::Null)
            }

            fn visit_none<E>(self) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::Null)
            }

            fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
            where
                D: de::Deserializer<'de>,
            {
                Value::deserialize(deserializer)
            }

            fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut elements = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(value) = seq.next_element()? {
                    elements.push(value);
                }
                Ok(Value::Array(elements))
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut entries = Map::new();
                while let Some((key, value)) = map.next_entry()? {
                    entries.insert(key, value);
                }
                Ok(Value::Object(entries))
            }
        }

        deserializer.deserialize_any(ValueVisitor)
    }
}

impl Value {
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    pub fn as_object(&self) -> Option<&Map> {
        if let Value::Object(map) = self {
            Some(map)
        } else {
            None
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Map> {
        if let Value::Object(map) = self {
            Some(map)
        } else {
            None
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        if let Value::Array(values) = self {
            Some(values)
        } else {
            None
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        if let Value::Array(values) = self {
            Some(values)
        } else {
            None
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(flag) = self {
            Some(*flag)
        } else {
            None
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Value::String(s) = self {
            Some(s)
        } else {
            None
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let Value::Number(num) = self {
            Some(num.as_f64())
        } else {
            None
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        if let Value::Number(num) = self {
            num.as_i64()
        } else {
            None
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        if let Value::Number(num) = self {
            num.as_u64()
        } else {
            None
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            Value::Object(map) => map.get_mut(key),
            _ => None,
        }
    }

    pub fn get_index(&self, index: usize) -> Option<&Value> {
        match self {
            Value::Array(values) => values.get(index),
            _ => None,
        }
    }

    pub fn get_index_mut(&mut self, index: usize) -> Option<&mut Value> {
        match self {
            Value::Array(values) => values.get_mut(index),
            _ => None,
        }
    }

    fn render_compact(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            Value::Number(num) => out.push_str(&num.to_string()),
            Value::String(s) => write_escaped_string(s, out),
            Value::Array(values) => {
                out.push('[');
                let mut first = true;
                for value in values {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    value.render_compact(out);
                }
                out.push(']');
            }
            Value::Object(map) => {
                out.push('{');
                let mut first = true;
                for (key, value) in map {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    write_escaped_string(key, out);
                    out.push(':');
                    value.render_compact(out);
                }
                out.push('}');
            }
        }
    }

    fn render_pretty(&self, out: &mut String, depth: usize) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            Value::Number(num) => out.push_str(&num.to_string()),
            Value::String(s) => write_escaped_string(s, out),
            Value::Array(values) => {
                if values.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                out.push('\n');
                let indent = "  ".repeat(depth + 1);
                for (idx, value) in values.iter().enumerate() {
                    if idx > 0 {
                        out.push(',');
                        out.push('\n');
                    }
                    out.push_str(&indent);
                    value.render_pretty(out, depth + 1);
                }
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
                out.push(']');
            }
            Value::Object(map) => {
                if map.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                out.push('\n');
                let indent = "  ".repeat(depth + 1);
                for (idx, (key, value)) in map.iter().enumerate() {
                    if idx > 0 {
                        out.push(',');
                        out.push('\n');
                    }
                    out.push_str(&indent);
                    write_escaped_string(key, out);
                    out.push_str(": ");
                    value.render_pretty(out, depth + 1);
                }
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
                out.push('}');
            }
        }
    }
}

impl Index<&str> for Value {
    type Output = Value;

    fn index(&self, index: &str) -> &Self::Output {
        match self {
            Value::Object(map) => map.get(index).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
}

impl IndexMut<&str> for Value {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        if !matches!(self, Value::Object(_)) {
            *self = Value::Object(Map::new());
        }

        if let Value::Object(map) = self {
            map.entry(index.to_owned()).or_insert(Value::Null)
        } else {
            unreachable!("value was converted to an object");
        }
    }
}

impl Index<usize> for Value {
    type Output = Value;

    fn index(&self, index: usize) -> &Self::Output {
        match self {
            Value::Array(values) => values.get(index).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
}

impl IndexMut<usize> for Value {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if !matches!(self, Value::Array(_)) {
            *self = Value::Array(Vec::new());
        }

        if let Value::Array(values) = self {
            if index >= values.len() {
                values.resize(index + 1, Value::Null);
            }
            &mut values[index]
        } else {
            unreachable!("value was converted to an array");
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut rendered = String::new();
        self.render_compact(&mut rendered);
        f.write_str(&rendered)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::String(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::String(value)
    }
}

impl From<Number> for Value {
    fn from(value: Number) -> Self {
        Value::Number(value)
    }
}

impl From<u8> for Value {
    fn from(value: u8) -> Self {
        Value::Number(Number::from(value))
    }
}

impl From<u16> for Value {
    fn from(value: u16) -> Self {
        Value::Number(Number::from(value))
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Value::Number(Number::from(value))
    }
}

impl From<u64> for Value {
    fn from(value: u64) -> Self {
        Value::Number(Number::from(value))
    }
}

impl From<i8> for Value {
    fn from(value: i8) -> Self {
        Value::Number(Number::from(value))
    }
}

impl From<i16> for Value {
    fn from(value: i16) -> Self {
        Value::Number(Number::from(value))
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Value::Number(Number::from(value))
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Number(Number::from(value))
    }
}

impl From<f32> for Value {
    fn from(value: f32) -> Self {
        Value::Number(Number::from(value))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Number(Number::from(value))
    }
}

impl From<Vec<Value>> for Value {
    fn from(values: Vec<Value>) -> Self {
        Value::Array(values)
    }
}

fn write_escaped_string(value: &str, out: &mut String) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use fmt::Write as _;
                let mut buf = String::new();
                write!(&mut buf, "\\u{:04X}", c as u32).expect("format control char");
                out.push_str(&buf);
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Serialize a value into a compact JSON string.
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    let value = to_value(value)?;
    let mut rendered = String::new();
    value.render_compact(&mut rendered);
    Ok(rendered)
}

/// Serialize a value into a pretty-printed JSON string.
pub fn to_string_pretty<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    let value = to_value(value)?;
    let mut rendered = String::new();
    value.render_pretty(&mut rendered, 0);
    Ok(rendered)
}

/// Serialize a value into a compact JSON byte vector.
pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    Ok(to_string(value)?.into_bytes())
}

/// Serialize a value into a pretty JSON byte vector.
pub fn to_vec_pretty<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    Ok(to_string_pretty(value)?.into_bytes())
}

/// Serialize a JSON [`Value`] into a compact byte vector.
pub fn to_vec_value(value: &Value) -> Vec<u8> {
    to_string_value(value).into_bytes()
}

/// Serialize a JSON [`Value`] into a compact string.
pub fn to_string_value(value: &Value) -> String {
    let mut rendered = String::new();
    value.render_compact(&mut rendered);
    rendered
}

/// Serialize a JSON [`Value`] into a pretty-printed string.
pub fn to_string_value_pretty(value: &Value) -> String {
    let mut rendered = String::new();
    value.render_pretty(&mut rendered, 0);
    rendered
}

/// Deserialize a value from a JSON string slice.
pub fn from_str<T: DeserializeOwned>(input: &str) -> Result<T> {
    let mut parser = Parser::new(input);
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.peek_char().is_some() {
        return Err(Error {
            kind: ErrorKind::TrailingCharacters,
        });
    }
    from_value(value)
}

/// Deserialize a value from a byte slice containing JSON.
pub fn from_slice<T: DeserializeOwned>(input: &[u8]) -> Result<T> {
    let text = std::str::from_utf8(input).map_err(|_| Error::unexpected_token("utf-8", None))?;
    from_str(text)
}

/// Deserialize a JSON [`Value`] from a string slice.
pub fn value_from_str(input: &str) -> Result<Value> {
    let mut parser = Parser::new(input);
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.peek_char().is_some() {
        return Err(Error {
            kind: ErrorKind::TrailingCharacters,
        });
    }
    Ok(value)
}

/// Deserialize a JSON [`Value`] from a byte slice containing JSON.
pub fn value_from_slice(input: &[u8]) -> Result<Value> {
    let text = std::str::from_utf8(input).map_err(|_| Error::unexpected_token("utf-8", None))?;
    value_from_str(text)
}

/// Convert a serializable value into a JSON [`Value`].
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value> {
    value.serialize(ValueSerializer)
}

/// Convert a JSON [`Value`] into a strongly typed structure.
pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T> {
    T::deserialize(ValueDeserializer { value })
}

struct ValueSerializer;

impl ser::Serializer for ValueSerializer {
    type Ok = Value;
    type Error = Error;
    type SerializeSeq = SeqCollector;
    type SerializeTuple = SeqCollector;
    type SerializeTupleStruct = SeqCollector;
    type SerializeTupleVariant = TupleVariantCollector;
    type SerializeMap = MapCollector;
    type SerializeStruct = StructCollector;
    type SerializeStructVariant = StructVariantCollector;

    fn serialize_bool(self, v: bool) -> Result<Value> {
        Ok(Value::Bool(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Value> {
        Ok(Value::from(v))
    }

    fn serialize_i16(self, v: i16) -> Result<Value> {
        Ok(Value::from(v))
    }

    fn serialize_i32(self, v: i32) -> Result<Value> {
        Ok(Value::from(v))
    }

    fn serialize_i64(self, v: i64) -> Result<Value> {
        Ok(Value::from(v))
    }

    fn serialize_u8(self, v: u8) -> Result<Value> {
        Ok(Value::from(v))
    }

    fn serialize_u16(self, v: u16) -> Result<Value> {
        Ok(Value::from(v))
    }

    fn serialize_u32(self, v: u32) -> Result<Value> {
        Ok(Value::from(v))
    }

    fn serialize_u64(self, v: u64) -> Result<Value> {
        Ok(Value::from(v))
    }

    fn serialize_f32(self, v: f32) -> Result<Value> {
        if v.is_finite() {
            Ok(Value::from(v))
        } else {
            Err(Error::non_finite_float())
        }
    }

    fn serialize_f64(self, v: f64) -> Result<Value> {
        if v.is_finite() {
            Ok(Value::from(v))
        } else {
            Err(Error::non_finite_float())
        }
    }

    fn serialize_char(self, v: char) -> Result<Value> {
        Ok(Value::String(v.to_string()))
    }

    fn serialize_str(self, v: &str) -> Result<Value> {
        Ok(Value::String(v.to_owned()))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Value> {
        let values = v.iter().map(|b| Value::from(*b)).collect();
        Ok(Value::Array(values))
    }

    fn serialize_none(self) -> Result<Value> {
        Ok(Value::Null)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Value> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Value> {
        Ok(Value::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value> {
        Ok(Value::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Value> {
        Ok(Value::String(variant.to_owned()))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value> {
        let mut map = Map::new();
        map.insert(variant.to_owned(), value.serialize(ValueSerializer)?);
        Ok(Value::Object(map))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        Ok(SeqCollector::new(len))
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        Ok(SeqCollector::new(Some(len)))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Ok(SeqCollector::new(Some(len)))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Ok(TupleVariantCollector::new(variant, len))
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap> {
        Ok(MapCollector::new(len))
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct> {
        Ok(StructCollector::new(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Ok(StructVariantCollector::new(variant, len))
    }
}

struct SeqCollector {
    values: Vec<Value>,
}

impl SeqCollector {
    fn new(len: Option<usize>) -> Self {
        Self {
            values: Vec::with_capacity(len.unwrap_or(0)),
        }
    }
}

impl SerializeSeq for SeqCollector {
    type Ok = Value;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        self.values.push(value.serialize(ValueSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value> {
        Ok(Value::Array(self.values))
    }
}

impl SerializeTuple for SeqCollector {
    type Ok = Value;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value> {
        SerializeSeq::end(self)
    }
}

impl SerializeTupleStruct for SeqCollector {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value> {
        SerializeSeq::end(self)
    }
}

struct TupleVariantCollector {
    name: &'static str,
    values: Vec<Value>,
}

impl TupleVariantCollector {
    fn new(name: &'static str, len: usize) -> Self {
        Self {
            name,
            values: Vec::with_capacity(len),
        }
    }
}

impl SerializeTupleVariant for TupleVariantCollector {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        self.values.push(value.serialize(ValueSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value> {
        let mut map = Map::new();
        map.insert(self.name.to_owned(), Value::Array(self.values));
        Ok(Value::Object(map))
    }
}

struct MapCollector {
    entries: Map,
    next_key: Option<String>,
}

impl MapCollector {
    fn new(_len: Option<usize>) -> Self {
        Self {
            entries: Map::new(),
            next_key: None,
        }
    }

    fn key_from_value(value: Value) -> Result<String> {
        match value {
            Value::String(s) => Ok(s),
            Value::Number(n) => Ok(n.to_string()),
            Value::Bool(b) => Ok(if b { "true" } else { "false" }.to_owned()),
            Value::Null => Ok("null".to_owned()),
            _ => Err(Error::invalid_map_key()),
        }
    }
}

impl SerializeMap for MapCollector {
    type Ok = Value;
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<()> {
        let key = key.serialize(ValueSerializer)?;
        self.next_key = Some(Self::key_from_value(key)?);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        let key = self
            .next_key
            .take()
            .ok_or_else(|| Error::message("serialize_value called before serialize_key"))?;
        let value = value.serialize(ValueSerializer)?;
        self.entries.insert(key, value);
        Ok(())
    }

    fn end(self) -> Result<Value> {
        Ok(Value::Object(self.entries))
    }
}

struct StructCollector {
    entries: Map,
}

impl StructCollector {
    fn new(_len: usize) -> Self {
        Self {
            entries: Map::new(),
        }
    }
}

impl SerializeStruct for StructCollector {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        self.entries
            .insert(key.to_owned(), value.serialize(ValueSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value> {
        Ok(Value::Object(self.entries))
    }
}

struct StructVariantCollector {
    name: &'static str,
    entries: Map,
}

impl StructVariantCollector {
    fn new(name: &'static str, _len: usize) -> Self {
        Self {
            name,
            entries: Map::new(),
        }
    }
}

impl SerializeStructVariant for StructVariantCollector {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        self.entries
            .insert(key.to_owned(), value.serialize(ValueSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value> {
        let mut outer = Map::new();
        outer.insert(self.name.to_owned(), Value::Object(self.entries));
        Ok(Value::Object(outer))
    }
}

struct ValueDeserializer {
    value: Value,
}

impl<'de> de::Deserializer<'de> for ValueDeserializer {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Null => visitor.visit_unit(),
            Value::Bool(v) => visitor.visit_bool(v),
            Value::Number(n) => {
                if let Some(v) = n.as_i64() {
                    visitor.visit_i64(v)
                } else if let Some(v) = n.as_u64() {
                    visitor.visit_u64(v)
                } else {
                    visitor.visit_f64(n.as_f64())
                }
            }
            Value::String(s) => visitor.visit_string(s),
            Value::Array(values) => visitor.visit_seq(ValueSeqAccess::new(values)),
            Value::Object(map) => visitor.visit_map(ValueMapAccess::new(map)),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Bool(v) => visitor.visit_bool(v),
            other => Err(unexpected_type("bool", other)),
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Number(n) => n
                .as_i64()
                .ok_or_else(Error::invalid_number)
                .and_then(|v| visitor.visit_i64(v)),
            other => Err(unexpected_type("integer", other)),
        }
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Number(n) => n
                .as_u64()
                .ok_or_else(Error::invalid_number)
                .and_then(|v| visitor.visit_u64(v)),
            other => Err(unexpected_type("unsigned integer", other)),
        }
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_f64(visitor)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Number(n) => visitor.visit_f64(n.as_f64()),
            other => Err(unexpected_type("float", other)),
        }
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(s) => {
                let ch = s
                    .chars()
                    .next()
                    .ok_or_else(|| Error::unexpected_token("character", None))?;
                if s.chars().count() == 1 {
                    visitor.visit_char(ch)
                } else {
                    Err(Error::message("expected single-character string"))
                }
            }
            other => Err(unexpected_type("char", other)),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(s) => visitor.visit_string(s),
            other => Err(unexpected_type("string", other)),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Array(values) => {
                let mut bytes = Vec::with_capacity(values.len());
                for value in values {
                    match value {
                        Value::Number(n) => {
                            let byte = n.as_u64().ok_or_else(Error::invalid_number)?;
                            bytes.push(byte as u8);
                        }
                        other => return Err(unexpected_type("byte", other)),
                    }
                }
                visitor.visit_byte_buf(bytes)
            }
            other => Err(unexpected_type("byte array", other)),
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Null => visitor.visit_none(),
            other => visitor.visit_some(ValueDeserializer { value: other }),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Null => visitor.visit_unit(),
            other => Err(unexpected_type("unit", other)),
        }
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(ValueDeserializer { value: self.value })
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Array(values) => visitor.visit_seq(ValueSeqAccess::new(values)),
            other => Err(unexpected_type("array", other)),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Object(map) => visitor.visit_map(ValueMapAccess::new(map)),
            other => Err(unexpected_type("object", other)),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(variant) => visitor.visit_enum(VariantAccessSingle::new(variant)),
            Value::Object(map) => {
                if map.len() != 1 {
                    return Err(Error::message("expected single-entry enum object"));
                }
                let (variant, value) = map.into_iter().next().unwrap();
                visitor.visit_enum(VariantAccessComplex::new(variant, value))
            }
            other => Err(unexpected_type("enum", other)),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

fn unexpected_type(expected: &'static str, found: Value) -> Error {
    match found {
        Value::Null => Error::unexpected_token(expected, None),
        Value::Bool(_) => Error::unexpected_token(expected, Some('b')),
        Value::Number(_) => Error::unexpected_token(expected, Some('0')),
        Value::String(_) => Error::unexpected_token(expected, Some('"')),
        Value::Array(_) => Error::unexpected_token(expected, Some('[')),
        Value::Object(_) => Error::unexpected_token(expected, Some('{')),
    }
}

struct ValueSeqAccess {
    iter: std::vec::IntoIter<Value>,
}

impl ValueSeqAccess {
    fn new(values: Vec<Value>) -> Self {
        Self {
            iter: values.into_iter(),
        }
    }
}

impl<'de> de::SeqAccess<'de> for ValueSeqAccess {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(value) => seed.deserialize(ValueDeserializer { value }).map(Some),
            None => Ok(None),
        }
    }
}

struct ValueMapAccess {
    entries: std::vec::IntoIter<(String, Value)>,
    current: Option<Value>,
}

impl ValueMapAccess {
    fn new(map: Map) -> Self {
        Self {
            entries: map.into_iter().collect::<Vec<_>>().into_iter(),
            current: None,
        }
    }
}

impl<'de> de::MapAccess<'de> for ValueMapAccess {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>>
    where
        K: DeserializeSeed<'de>,
    {
        match self.entries.next() {
            Some((key, value)) => {
                self.current = Some(value);
                let de = StringDeserializer::new(key);
                seed.deserialize(de).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value>
    where
        V: DeserializeSeed<'de>,
    {
        let value = self
            .current
            .take()
            .ok_or_else(|| Error::message("value without matching key"))?;
        seed.deserialize(ValueDeserializer { value })
    }
}

struct VariantAccessSingle {
    variant: String,
}

impl VariantAccessSingle {
    fn new(variant: String) -> Self {
        Self { variant }
    }
}

impl<'de> EnumAccess<'de> for VariantAccessSingle {
    type Error = Error;
    type Variant = UnitVariant;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant)>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(StringDeserializer::new(self.variant))?;
        Ok((variant, UnitVariant))
    }
}

struct UnitVariant;

impl<'de> VariantAccess<'de> for UnitVariant {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, _seed: T) -> Result<T::Value>
    where
        T: DeserializeSeed<'de>,
    {
        Err(Error::message("expected unit variant"))
    }

    fn tuple_variant<V>(self, _len: usize, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::message("expected unit variant"))
    }

    fn struct_variant<V>(self, _fields: &'static [&'static str], _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(Error::message("expected unit variant"))
    }
}

struct VariantAccessComplex {
    variant: String,
    value: Value,
}

impl VariantAccessComplex {
    fn new(variant: String, value: Value) -> Self {
        Self { variant, value }
    }
}

impl<'de> EnumAccess<'de> for VariantAccessComplex {
    type Error = Error;
    type Variant = ComplexVariant;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant)>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(StringDeserializer::new(self.variant))?;
        Ok((variant, ComplexVariant { value: self.value }))
    }
}

struct ComplexVariant {
    value: Value,
}

impl<'de> VariantAccess<'de> for ComplexVariant {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        match self.value {
            Value::Null => Ok(()),
            other => Err(unexpected_type("unit", other)),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(ValueDeserializer { value: self.value })
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        ValueDeserializer { value: self.value }.deserialize_seq(visitor)
    }

    fn struct_variant<V>(self, _fields: &'static [&'static str], visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        ValueDeserializer { value: self.value }.deserialize_map(visitor)
    }
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    fn peek_char(&self) -> Option<char> {
        let remaining = std::str::from_utf8(&self.input[self.pos..]).ok()?;
        remaining.chars().next()
    }

    fn next_char(&mut self) -> Option<char> {
        let remaining = std::str::from_utf8(&self.input[self.pos..]).ok()?;
        let ch = remaining.chars().next()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn advance_by(&mut self, len: usize) {
        self.pos += len;
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.next_char();
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self) -> Result<Value> {
        self.skip_whitespace();
        match self.peek_char() {
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('"') => self.parse_string().map(Value::String),
            Some('t') | Some('f') => self.parse_bool(),
            Some('n') => self.parse_null(),
            Some('-') | Some('0'..='9') => self.parse_number().map(Value::Number),
            Some(ch) => Err(Error::unexpected_token("value", Some(ch))),
            None => Err(Error::unexpected_token("value", None)),
        }
    }

    fn parse_object(&mut self) -> Result<Value> {
        self.expect_char('{')?;
        self.skip_whitespace();
        let mut map = Map::new();
        if self.peek_char() == Some('}') {
            self.next_char();
            return Ok(Value::Object(map));
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect_char(':')?;
            let value = self.parse_value()?;
            map.insert(key, value);
            self.skip_whitespace();
            match self.peek_char() {
                Some(',') => {
                    self.next_char();
                }
                Some('}') => {
                    self.next_char();
                    break;
                }
                other => return Err(Error::unexpected_token("',' or '}'", other)),
            }
        }
        Ok(Value::Object(map))
    }

    fn parse_array(&mut self) -> Result<Value> {
        self.expect_char('[')?;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.peek_char() == Some(']') {
            self.next_char();
            return Ok(Value::Array(values));
        }
        loop {
            let value = self.parse_value()?;
            values.push(value);
            self.skip_whitespace();
            match self.peek_char() {
                Some(',') => {
                    self.next_char();
                }
                Some(']') => {
                    self.next_char();
                    break;
                }
                other => return Err(Error::unexpected_token("',' or ']'", other)),
            }
        }
        Ok(Value::Array(values))
    }

    fn parse_string(&mut self) -> Result<String> {
        self.expect_char('"')?;
        let mut result = String::new();
        loop {
            match self.next_char() {
                Some('"') => break,
                Some('\\') => {
                    let escaped = self
                        .next_char()
                        .ok_or_else(|| Error::unexpected_token("escape", None))?;
                    match escaped {
                        '"' => result.push('"'),
                        '\\' => result.push('\\'),
                        '/' => result.push('/'),
                        'b' => result.push('\u{0008}'),
                        'f' => result.push('\u{000C}'),
                        'n' => result.push('\n'),
                        'r' => result.push('\r'),
                        't' => result.push('\t'),
                        'u' => {
                            let code = self.parse_unicode_escape()?;
                            result.push(code);
                        }
                        other => return Err(Error::unexpected_token("valid escape", Some(other))),
                    }
                }
                Some(ch) => {
                    if ch.is_control() {
                        return Err(Error::unexpected_token("non-control character", Some(ch)));
                    }
                    result.push(ch);
                }
                None => return Err(Error::unexpected_token("string", None)),
            }
        }
        Ok(result)
    }

    fn parse_unicode_escape(&mut self) -> Result<char> {
        let mut hex = String::new();
        for _ in 0..4 {
            let ch = self
                .next_char()
                .ok_or_else(|| Error::unexpected_token("unicode escape", None))?;
            hex.push(ch);
        }
        let value = u16::from_str_radix(&hex, 16).map_err(|_| Error::invalid_unicode_escape())?;
        if (0xD800..=0xDBFF).contains(&value) {
            // High surrogate must be followed by low surrogate.
            self.expect_char('\\')?;
            self.expect_char('u')?;
            let mut hex_low = String::new();
            for _ in 0..4 {
                let ch = self
                    .next_char()
                    .ok_or_else(|| Error::unexpected_token("unicode escape", None))?;
                hex_low.push(ch);
            }
            let low =
                u16::from_str_radix(&hex_low, 16).map_err(|_| Error::invalid_unicode_escape())?;
            let combined =
                decode_surrogate_pair(value, low).ok_or_else(Error::invalid_unicode_escape)?;
            Ok(combined)
        } else {
            char::from_u32(value as u32).ok_or_else(Error::invalid_unicode_escape)
        }
    }

    fn parse_bool(&mut self) -> Result<Value> {
        if self.consume_literal("true") {
            Ok(Value::Bool(true))
        } else if self.consume_literal("false") {
            Ok(Value::Bool(false))
        } else {
            Err(Error::unexpected_token("boolean", self.peek_char()))
        }
    }

    fn parse_null(&mut self) -> Result<Value> {
        if self.consume_literal("null") {
            Ok(Value::Null)
        } else {
            Err(Error::unexpected_token("null", self.peek_char()))
        }
    }

    fn consume_literal(&mut self, literal: &str) -> bool {
        if self.input[self.pos..].starts_with(literal.as_bytes()) {
            self.advance_by(literal.len());
            true
        } else {
            false
        }
    }

    fn parse_number(&mut self) -> Result<Number> {
        let start = self.pos;
        if self.peek_byte() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek_byte() {
            Some(b'0') => {
                self.pos += 1;
            }
            Some(b'1'..=b'9') => {
                while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            other => return Err(Error::unexpected_token("digit", other.map(char::from))),
        }

        if self.peek_byte() == Some(b'.') {
            self.pos += 1;
            if !matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                return Err(Error::invalid_number());
            }
            while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }

        if matches!(self.peek_byte(), Some(b'e') | Some(b'E')) {
            self.pos += 1;
            if matches!(self.peek_byte(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                return Err(Error::invalid_number());
            }
            while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }

        let slice = &self.input[start..self.pos];
        let text = std::str::from_utf8(slice).map_err(|_| Error::invalid_number())?;
        if text.contains(['.', 'e', 'E']) {
            let value: f64 = text.parse().map_err(|_| Error::invalid_number())?;
            Number::from_f64(value).ok_or_else(Error::non_finite_float)
        } else if text.starts_with('-') {
            let value: i64 = text.parse().map_err(|_| Error::invalid_number())?;
            Ok(Number::from(value))
        } else {
            let value: u64 = text.parse().map_err(|_| Error::invalid_number())?;
            Ok(Number::from(value))
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn expect_char(&mut self, expected: char) -> Result<()> {
        match self.next_char() {
            Some(ch) if ch == expected => Ok(()),
            other => {
                let expected_str = expected.to_string();
                Err(Error::unexpected_token(expected_str, other))
            }
        }
    }
}

fn decode_surrogate_pair(high: u16, low: u16) -> Option<char> {
    if (0xDC00..=0xDFFF).contains(&low) {
        let high_ten = (high as u32) - 0xD800;
        let low_ten = (low as u32) - 0xDC00;
        let code_point = 0x10000 + ((high_ten << 10) | low_ten);
        char::from_u32(code_point)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ser::SerializeStruct;
    use crate::{de, ser, Deserialize, Serialize};

    use core::{fmt, result::Result as StdResult};

    #[derive(Debug, PartialEq)]
    struct Sample {
        id: u32,
        name: String,
        flags: Vec<bool>,
    }

    impl Serialize for Sample {
        fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            let mut state = serializer.serialize_struct("Sample", 3)?;
            state.serialize_field("id", &self.id)?;
            state.serialize_field("name", &self.name)?;
            state.serialize_field("flags", &self.flags)?;
            state.end()
        }
    }

    impl<'de> Deserialize<'de> for Sample {
        fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            enum Field {
                Id,
                Name,
                Flags,
            }

            struct FieldVisitor;

            impl<'de> de::Visitor<'de> for FieldVisitor {
                type Value = Field;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("`id`, `name`, or `flags`")
                }

                fn visit_str<E>(self, value: &str) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        "id" => Ok(Field::Id),
                        "name" => Ok(Field::Name),
                        "flags" => Ok(Field::Flags),
                        other => Err(de::Error::unknown_field(other, FIELDS)),
                    }
                }

                fn visit_bytes<E>(self, value: &[u8]) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        b"id" => Ok(Field::Id),
                        b"name" => Ok(Field::Name),
                        b"flags" => Ok(Field::Flags),
                        other => {
                            let text = core::str::from_utf8(other).unwrap_or("");
                            Err(de::Error::unknown_field(text, FIELDS))
                        }
                    }
                }

                fn visit_string<E>(self, value: String) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    self.visit_str(&value)
                }
            }

            impl<'de> Deserialize<'de> for Field {
                fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
                where
                    D: de::Deserializer<'de>,
                {
                    deserializer.deserialize_identifier(FieldVisitor)
                }
            }

            struct SampleVisitor;

            impl<'de> de::Visitor<'de> for SampleVisitor {
                type Value = Sample;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("Sample struct")
                }

                fn visit_map<M>(self, mut map: M) -> StdResult<Sample, M::Error>
                where
                    M: de::MapAccess<'de>,
                {
                    let mut id = None;
                    let mut name = None;
                    let mut flags = None;
                    while let Some(field) = map.next_key::<Field>()? {
                        match field {
                            Field::Id => {
                                if id.is_some() {
                                    return Err(de::Error::duplicate_field("id"));
                                }
                                id = Some(map.next_value()?);
                            }
                            Field::Name => {
                                if name.is_some() {
                                    return Err(de::Error::duplicate_field("name"));
                                }
                                name = Some(map.next_value()?);
                            }
                            Field::Flags => {
                                if flags.is_some() {
                                    return Err(de::Error::duplicate_field("flags"));
                                }
                                flags = Some(map.next_value()?);
                            }
                        }
                    }
                    Ok(Sample {
                        id: id.ok_or_else(|| de::Error::missing_field("id"))?,
                        name: name.ok_or_else(|| de::Error::missing_field("name"))?,
                        flags: flags.ok_or_else(|| de::Error::missing_field("flags"))?,
                    })
                }
            }

            const FIELDS: &[&str] = &["id", "name", "flags"];
            deserializer.deserialize_struct("Sample", FIELDS, SampleVisitor)
        }
    }

    #[test]
    fn roundtrip_json() {
        let sample = Sample {
            id: 7,
            name: "example".to_string(),
            flags: vec![true, false, true],
        };
        let encoded = to_string(&sample).expect("encode");
        let decoded: Sample = from_str(&encoded).expect("decode");
        assert_eq!(decoded, sample);
    }

    #[test]
    fn json_macro_objects() {
        let value = crate::json!({
            "id": 1,
            "name": "test",
            "flags": [true, false, null],
        });
        let string = to_string(&value).expect("serialize value");
        let parsed: Value = from_str(&string).expect("parse");
        assert_eq!(parsed, value);
    }

    #[test]
    fn json_macro_nested_objects_round_trip() {
        let value = crate::json!({
            "outer": {
                "inner": {
                    "leaf": 7,
                    "list": [1, 2, {"deep": null}],
                },
                "flag": true,
            },
        });

        let encoded = to_string(&value).expect("encode nested object");
        let reparsed: Value = from_str(&encoded).expect("reparse nested object");
        assert_eq!(reparsed, value);
    }

    #[test]
    fn json_macro_identifier_keys_are_stringified() {
        let value = crate::json!({ status: true, count: 3 });
        let map = value.as_object().expect("object");
        assert_eq!(map.get("status"), Some(&Value::Bool(true)));
        assert_eq!(map.get("count"), Some(&Value::from(3)));
    }

    #[test]
    fn numeric_accessors_respect_integer_boundaries() {
        let positive = Value::from(42_u64);
        assert_eq!(positive.as_u64(), Some(42));
        assert_eq!(positive.as_i64(), Some(42));
        assert_eq!(positive.as_f64(), Some(42.0));

        let negative = Value::from(-7_i64);
        assert_eq!(negative.as_i64(), Some(-7));
        assert_eq!(negative.as_u64(), None);

        let integral_float = Value::from(5.0_f64);
        assert_eq!(integral_float.as_i64(), Some(5));
        assert_eq!(integral_float.as_u64(), Some(5));

        let fractional_float = Value::from(5.25_f64);
        assert_eq!(fractional_float.as_i64(), None);
        assert_eq!(fractional_float.as_u64(), None);

        let text = Value::from("not-a-number");
        assert_eq!(text.as_i64(), None);
        assert_eq!(text.as_u64(), None);
        assert_eq!(text.as_f64(), None);
    }

    #[test]
    fn key_accessors_follow_object_and_array_semantics() {
        let mut object = Map::new();
        object.insert("flag".to_string(), Value::from(true));
        object.insert("count".to_string(), Value::from(3u64));
        let mut value = Value::Object(object);

        assert!(value.is_object());
        assert!(!value.is_array());
        assert_eq!(value.get("missing"), None);
        assert_eq!(value.get("flag").and_then(Value::as_bool), Some(true));

        if let Some(entry) = value.get_mut("count") {
            *entry = Value::from(4u64);
        } else {
            panic!("count missing");
        }

        assert_eq!(value.get("count").and_then(Value::as_u64), Some(4));

        let mut array = Value::Array(vec![Value::from("zero"), Value::from("one")]);
        assert!(array.is_array());
        assert!(!array.is_object());
        assert_eq!(array.get_index(1).and_then(Value::as_str), Some("one"));
        assert_eq!(array.get_index(2), None);

        if let Some(slot) = array.get_index_mut(0) {
            *slot = Value::from("updated");
        } else {
            panic!("index 0 missing");
        }

        match array {
            Value::Array(ref elements) => {
                assert_eq!(elements.len(), 2);
                assert_eq!(elements[0].as_str(), Some("updated"));
            }
            _ => panic!("array mutated to non-array"),
        }
    }

    #[test]
    fn display_matches_compact_serializer() {
        let value = crate::json!({
            "array": [1, 2, 3],
            "nested": {"flag": true, "label": "ok"},
            "nullish": null,
        });

        let rendered = value.to_string();
        let expected = to_string_value(&value);
        assert_eq!(rendered, expected);
    }

    #[test]
    fn pretty_value_indents_arrays() {
        let value = Value::Array(vec![Value::from(1u64), Value::from("two")]);
        let pretty = to_string_value_pretty(&value);
        assert!(pretty.starts_with("[\n"));
        assert!(pretty.contains("  1"));
        assert!(pretty.contains("  \"two\""));
        let compact = to_string_value(&value);
        let trimmed_pretty: String = pretty.chars().filter(|c| !c.is_whitespace()).collect();
        let trimmed_compact: String = compact.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(trimmed_pretty, trimmed_compact);
    }

    #[test]
    fn pretty_value_handles_objects() {
        let mut map = Map::new();
        map.insert("alpha".to_string(), Value::from(true));
        map.insert("beta".to_string(), Value::from(3u64));
        let value = Value::Object(map);
        let pretty = to_string_value_pretty(&value);
        assert!(pretty.starts_with("{\n"));
        assert!(pretty.contains("  \"alpha\": true"));
        assert!(pretty.contains("  \"beta\": 3"));
        assert!(pretty.trim_end().ends_with('}'));
    }
}
