use std::fmt::Write;
use std::string::{String, ToString};
use std::vec::Vec;

use super::{Error, Result};

/// Trait implemented by types that can be encoded into deterministic JSON.
pub trait JsonEncode {
    fn encode_json(&self, writer: &mut JsonWriter);
}

/// Read-only helper for JSON decoding.
pub trait JsonEncoder: Sized {
    fn decode_json(input: &str) -> Result<Self>;
}

/// Deterministic JSON builder with a minimal feature set.
#[derive(Default, Debug)]
pub struct JsonWriter {
    buffer: String,
    stack: Vec<State>,
    needs_comma: Vec<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Object,
    Array,
}

impl JsonWriter {
    fn push_state(&mut self, state: State) {
        self.stack.push(state);
        self.needs_comma.push(false);
    }

    fn write_separator(&mut self) {
        if matches!(self.needs_comma.last(), Some(true)) {
            self.buffer.push(',');
        }
        if let Some(last) = self.needs_comma.last_mut() {
            *last = true;
        }
    }

    pub fn begin_object(&mut self) {
        self.write_separator();
        self.buffer.push('{');
        self.push_state(State::Object);
    }

    pub fn end_object(&mut self) {
        self.stack.pop();
        self.needs_comma.pop();
        self.buffer.push('}');
    }

    pub fn begin_array(&mut self) {
        self.write_separator();
        self.buffer.push('[');
        self.push_state(State::Array);
    }

    pub fn end_array(&mut self) {
        self.stack.pop();
        self.needs_comma.pop();
        self.buffer.push(']');
    }

    pub fn object_key(&mut self, key: &str) {
        if !matches!(self.stack.last(), Some(State::Object)) {
            panic!("object_key called outside of object context");
        }
        self.write_separator();
        self.write_string_raw(key);
        self.buffer.push(':');
        if let Some(last) = self.needs_comma.last_mut() {
            *last = false;
        }
    }

    pub fn string(&mut self, value: &str) {
        self.write_separator();
        self.write_string_raw(value);
    }

    fn write_string_raw(&mut self, value: &str) {
        self.buffer.push('"');
        for ch in value.chars() {
            match ch {
                '"' => self.buffer.push_str("\\\""),
                '\\' => self.buffer.push_str("\\\\"),
                '\n' => self.buffer.push_str("\\n"),
                '\r' => self.buffer.push_str("\\r"),
                '\t' => self.buffer.push_str("\\t"),
                c if c.is_control() => {
                    let mut hex = String::new();
                    write!(&mut hex, "\\u{:04X}", c as u32).expect("write hex");
                    self.buffer.push_str(&hex);
                }
                other => self.buffer.push(other),
            }
        }
        self.buffer.push('"');
    }

    pub fn number(&mut self, value: u64) {
        self.write_separator();
        self.buffer.push_str(&value.to_string());
    }

    pub fn boolean(&mut self, value: bool) {
        self.write_separator();
        self.buffer.push_str(if value { "true" } else { "false" });
    }

    pub fn null(&mut self) {
        self.write_separator();
        self.buffer.push_str("null");
    }

    pub fn finish(self) -> String {
        self.buffer
    }
}

impl JsonEncode for bool {
    fn encode_json(&self, writer: &mut JsonWriter) {
        writer.boolean(*self);
    }
}

impl JsonEncode for u64 {
    fn encode_json(&self, writer: &mut JsonWriter) {
        writer.number(*self);
    }
}

impl JsonEncode for &str {
    fn encode_json(&self, writer: &mut JsonWriter) {
        writer.string(self);
    }
}

impl JsonEncode for String {
    fn encode_json(&self, writer: &mut JsonWriter) {
        writer.string(self);
    }
}

impl<T> JsonEncode for [T]
where
    T: JsonEncode,
{
    fn encode_json(&self, writer: &mut JsonWriter) {
        writer.begin_array();
        for value in self {
            value.encode_json(writer);
        }
        writer.end_array();
    }
}

impl<T> JsonEncode for Vec<T>
where
    T: JsonEncode,
{
    fn encode_json(&self, writer: &mut JsonWriter) {
        self.as_slice().encode_json(writer);
    }
}

impl<T> JsonEncode for Option<T>
where
    T: JsonEncode,
{
    fn encode_json(&self, writer: &mut JsonWriter) {
        match self {
            Some(value) => value.encode_json(writer),
            None => writer.null(),
        }
    }
}

impl<T> JsonEncoder for Option<T>
where
    T: JsonEncoder,
{
    fn decode_json(input: &str) -> Result<Self> {
        match input.trim() {
            "null" => Ok(None),
            other => Ok(Some(T::decode_json(other)?)),
        }
    }
}

impl JsonEncoder for String {
    fn decode_json(input: &str) -> Result<Self> {
        if let Some(stripped) = input
            .trim()
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
        {
            Ok(stripped.replace("\\\"", "\""))
        } else {
            Err(Error::Custom("invalid string literal"))
        }
    }
}

impl JsonEncoder for bool {
    fn decode_json(input: &str) -> Result<Self> {
        match input.trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(Error::Custom("invalid bool literal")),
        }
    }
}

impl JsonEncoder for u64 {
    fn decode_json(input: &str) -> Result<Self> {
        input
            .trim()
            .parse()
            .map_err(|_| Error::Custom("invalid number"))
    }
}

impl<T> JsonEncoder for Vec<T>
where
    T: JsonEncoder,
{
    fn decode_json(input: &str) -> Result<Self> {
        let mut result = Vec::new();
        let trimmed = input.trim();
        if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
            return Err(Error::Custom("invalid array"));
        }
        let inner = &trimmed[1..trimmed.len() - 1];
        if inner.trim().is_empty() {
            return Ok(result);
        }
        for chunk in inner.split(',') {
            result.push(T::decode_json(chunk.trim())?);
        }
        Ok(result)
    }
}
