#![allow(dead_code)]

use std::char;
use std::collections::BTreeMap;
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct Error {
    message: String,
}

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Object(Map),
}

pub type Map = BTreeMap<String, Value>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Number(NumberRepr);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NumberRepr {
    Unsigned(u128),
    Signed(i128),
}

impl Number {
    fn is_negative(&self) -> bool {
        matches!(self.0, NumberRepr::Signed(_))
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self.0 {
            NumberRepr::Unsigned(value) => value.try_into().ok(),
            NumberRepr::Signed(value) if value >= 0 => (value as u128).try_into().ok(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self.0 {
            NumberRepr::Unsigned(value) => value.try_into().ok(),
            NumberRepr::Signed(value) => value.try_into().ok(),
        }
    }
}

impl From<u8> for Number {
    fn from(value: u8) -> Self {
        Number(NumberRepr::Unsigned(value as u128))
    }
}

impl From<u16> for Number {
    fn from(value: u16) -> Self {
        Number(NumberRepr::Unsigned(value as u128))
    }
}

impl From<u32> for Number {
    fn from(value: u32) -> Self {
        Number(NumberRepr::Unsigned(value as u128))
    }
}

impl From<u64> for Number {
    fn from(value: u64) -> Self {
        Number(NumberRepr::Unsigned(value as u128))
    }
}

impl From<i64> for Number {
    fn from(value: i64) -> Self {
        if value < 0 {
            Number(NumberRepr::Signed(value as i128))
        } else {
            Number(NumberRepr::Unsigned(value as u128))
        }
    }
}

pub fn value_from_slice(input: &[u8]) -> Result<Value> {
    let mut parser = Parser::new(input);
    let value = parser.parse_value()?;
    parser.consume_whitespace();
    if parser.peek().is_some() {
        return Err(Error::new("trailing characters after JSON value"));
    }
    Ok(value)
}

pub fn value_from_str(input: &str) -> Result<Value> {
    value_from_slice(input.as_bytes())
}

pub fn to_vec_value(value: &Value) -> Vec<u8> {
    let mut output = Vec::new();
    write_value(value, &mut output);
    output
}

pub fn to_string_value(value: &Value) -> String {
    String::from_utf8(to_vec_value(value)).expect("json output is valid utf-8")
}

pub fn from_value<T: FromJsonValue>(value: Value) -> Result<T> {
    T::from_json_value(value)
}

pub trait FromJsonValue: Sized {
    fn from_json_value(value: Value) -> Result<Self>;
}

impl FromJsonValue for u64 {
    fn from_json_value(value: Value) -> Result<Self> {
        match value {
            Value::Number(num) => num
                .as_u64()
                .ok_or_else(|| Error::new("value is not a valid unsigned integer")),
            _ => Err(Error::new("expected number")),
        }
    }
}

impl FromJsonValue for String {
    fn from_json_value(value: Value) -> Result<Self> {
        match value {
            Value::String(s) => Ok(s),
            Value::Number(num) => Ok(num.to_string()),
            Value::Bool(bool) => Ok(bool.to_string()),
            Value::Null => Ok(String::new()),
            _ => Err(Error::new("expected string")),
        }
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            NumberRepr::Unsigned(value) => write!(f, "{}", value),
            NumberRepr::Signed(value) => write!(f, "{}", value),
        }
    }
}

impl FromJsonValue for Vec<u8> {
    fn from_json_value(value: Value) -> Result<Self> {
        match value {
            Value::Array(items) => {
                let mut bytes = Vec::with_capacity(items.len());
                for item in items {
                    let number = match item {
                        Value::Number(num) => num,
                        _ => return Err(Error::new("byte arrays must contain numbers")),
                    };
                    let byte = number
                        .as_u64()
                        .ok_or_else(|| Error::new("byte value out of range"))?;
                    if byte > u8::MAX as u64 {
                        return Err(Error::new("byte value out of range"));
                    }
                    bytes.push(byte as u8);
                }
                Ok(bytes)
            }
            Value::String(s) => Ok(s.into_bytes()),
            Value::Null => Ok(Vec::new()),
            _ => Err(Error::new("expected byte array")),
        }
    }
}

struct Parser<'a> {
    input: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, index: 0 }
    }

    fn parse_value(&mut self) -> Result<Value> {
        self.consume_whitespace();
        let ch = self
            .peek()
            .ok_or_else(|| Error::new("unexpected end of input"))?;
        match ch {
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b'"' => self.parse_string().map(Value::String),
            b't' => {
                self.expect_literal(b"true")?;
                Ok(Value::Bool(true))
            }
            b'f' => {
                self.expect_literal(b"false")?;
                Ok(Value::Bool(false))
            }
            b'n' => {
                self.expect_literal(b"null")?;
                Ok(Value::Null)
            }
            b'-' | b'0'..=b'9' => self.parse_number().map(Value::Number),
            _ => Err(Error::new("invalid JSON value")),
        }
    }

    fn parse_object(&mut self) -> Result<Value> {
        self.expect_char(b'{')?;
        self.consume_whitespace();
        let mut map = Map::new();
        if self.try_consume_char(b'}') {
            return Ok(Value::Object(map));
        }
        loop {
            self.consume_whitespace();
            let key = self.parse_string()?;
            self.consume_whitespace();
            self.expect_char(b':')?;
            let value = self.parse_value()?;
            map.insert(key, value);
            self.consume_whitespace();
            if self.try_consume_char(b',') {
                continue;
            }
            self.expect_char(b'}')?;
            break;
        }
        Ok(Value::Object(map))
    }

    fn parse_array(&mut self) -> Result<Value> {
        self.expect_char(b'[')?;
        self.consume_whitespace();
        let mut items = Vec::new();
        if self.try_consume_char(b']') {
            return Ok(Value::Array(items));
        }
        loop {
            let value = self.parse_value()?;
            items.push(value);
            self.consume_whitespace();
            if self.try_consume_char(b',') {
                continue;
            }
            self.expect_char(b']')?;
            break;
        }
        Ok(Value::Array(items))
    }

    fn parse_string(&mut self) -> Result<String> {
        self.expect_char(b'"')?;
        let mut result = String::new();
        while let Some(ch) = self.next_char() {
            match ch {
                b'"' => return Ok(result),
                b'\\' => {
                    let escape = self
                        .next_char()
                        .ok_or_else(|| Error::new("unterminated escape sequence"))?;
                    match escape {
                        b'"' => result.push('"'),
                        b'\\' => result.push('\\'),
                        b'/' => result.push('/'),
                        b'b' => result.push('\u{0008}'),
                        b'f' => result.push('\u{000C}'),
                        b'n' => result.push('\n'),
                        b'r' => result.push('\r'),
                        b't' => result.push('\t'),
                        b'u' => {
                            let code_point = self.parse_hex_escape()?;
                            let ch = char::from_u32(code_point)
                                .ok_or_else(|| Error::new("invalid unicode escape"))?;
                            result.push(ch);
                        }
                        _ => {
                            return Err(Error::new("invalid escape sequence"));
                        }
                    }
                }
                _ if ch < 0x20 => {
                    return Err(Error::new("control characters must be escaped"));
                }
                _ => result.push(ch as char),
            }
        }
        Err(Error::new("unterminated string"))
    }

    fn parse_hex_escape(&mut self) -> Result<u32> {
        let mut value = 0u32;
        for _ in 0..4 {
            let digit = self
                .next_char()
                .ok_or_else(|| Error::new("unterminated unicode escape"))?;
            value = (value << 4)
                | match digit {
                    b'0'..=b'9' => (digit - b'0') as u32,
                    b'a'..=b'f' => (digit - b'a' + 10) as u32,
                    b'A'..=b'F' => (digit - b'A' + 10) as u32,
                    _ => return Err(Error::new("invalid unicode escape")),
                };
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Number> {
        let start = self.index;
        let negative = self.try_consume_char(b'-');
        let first_digit = self
            .next_char()
            .ok_or_else(|| Error::new("unexpected end of number"))?;
        if !first_digit.is_ascii_digit() {
            return Err(Error::new("invalid number"));
        }
        if first_digit == b'0' {
            if let Some(ch) = self.peek() {
                if ch.is_ascii_digit() {
                    return Err(Error::new("leading zeros are unsupported"));
                }
            }
        } else {
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() {
                    self.index += 1;
                } else {
                    break;
                }
            }
        }
        if let Some(ch) = self.peek() {
            if ch == b'.' || ch == b'e' || ch == b'E' {
                return Err(Error::new("floating point numbers are unsupported"));
            }
        }
        let end = self.index;
        let slice = &self.input[start..end];
        let text = std::str::from_utf8(slice).map_err(|_| Error::new("invalid UTF-8 number"))?;
        if negative {
            let value = text
                .parse::<i128>()
                .map_err(|_| Error::new("invalid integer value"))?;
            Ok(Number(NumberRepr::Signed(value)))
        } else {
            let value = text
                .parse::<u128>()
                .map_err(|_| Error::new("invalid integer value"))?;
            Ok(Number(NumberRepr::Unsigned(value)))
        }
    }

    fn consume_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if matches!(ch, b' ' | b'\n' | b'\r' | b'\t') {
                self.index += 1;
            } else {
                break;
            }
        }
    }

    fn expect_literal(&mut self, literal: &[u8]) -> Result<()> {
        for expected in literal {
            let ch = self
                .next_char()
                .ok_or_else(|| Error::new("unexpected end of input"))?;
            if ch != *expected {
                return Err(Error::new("invalid literal"));
            }
        }
        Ok(())
    }

    fn expect_char(&mut self, expected: u8) -> Result<()> {
        let ch = self
            .next_char()
            .ok_or_else(|| Error::new("unexpected end of input"))?;
        if ch == expected {
            Ok(())
        } else {
            Err(Error::new(format!("expected '{}'", expected as char)))
        }
    }

    fn try_consume_char(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn next_char(&mut self) -> Option<u8> {
        let ch = self.peek()?;
        self.index += 1;
        Some(ch)
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.index).copied()
    }
}

fn write_value(value: &Value, output: &mut Vec<u8>) {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Number(number) => output.extend_from_slice(number.to_string().as_bytes()),
        Value::String(text) => write_string(text, output),
        Value::Array(items) => {
            output.push(b'[');
            let mut first = true;
            for item in items {
                if !first {
                    output.push(b',');
                }
                first = false;
                write_value(item, output);
            }
            output.push(b']');
        }
        Value::Object(map) => {
            output.push(b'{');
            let mut first = true;
            for (key, val) in map.iter() {
                if !first {
                    output.push(b',');
                }
                first = false;
                write_string(key, output);
                output.push(b':');
                write_value(val, output);
            }
            output.push(b'}');
        }
    }
}

fn write_string(value: &str, output: &mut Vec<u8>) {
    output.push(b'"');
    for ch in value.chars() {
        match ch {
            '"' => output.extend_from_slice(b"\\\""),
            '\\' => output.extend_from_slice(b"\\\\"),
            '\n' => output.extend_from_slice(b"\\n"),
            '\r' => output.extend_from_slice(b"\\r"),
            '\t' => output.extend_from_slice(b"\\t"),
            ch if ch < ' ' => {
                let code = ch as u32;
                let mut buf = [0u8; 6];
                buf[0] = b'\\';
                buf[1] = b'u';
                buf[2] = hex_digit((code >> 12) & 0xF);
                buf[3] = hex_digit((code >> 8) & 0xF);
                buf[4] = hex_digit((code >> 4) & 0xF);
                buf[5] = hex_digit(code & 0xF);
                output.extend_from_slice(&buf);
            }
            ch => {
                let mut buffer = [0u8; 4];
                let encoded = ch.encode_utf8(&mut buffer);
                output.extend_from_slice(encoded.as_bytes());
            }
        }
    }
    output.push(b'"');
}

fn hex_digit(value: u32) -> u8 {
    match value {
        0..=9 => b'0' + value as u8,
        10..=15 => b'a' + (value as u8 - 10),
        _ => b'0',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_manifest_like_value() {
        let mut map = Map::new();
        let mut inner = Map::new();
        inner.insert("next_file_id".into(), Value::Number(Number::from(7u64)));
        inner.insert("sequence".into(), Value::Number(Number::from(3u64)));
        inner.insert(
            "sstables".into(),
            Value::Array(vec![Value::String("a.sst".into())]),
        );
        map.insert("default".into(), Value::Object(inner));
        let value = Value::Object(map);
        let encoded = to_vec_value(&value);
        let decoded = value_from_slice(&encoded).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn parse_byte_array() {
        let value = value_from_slice(b"[1,2,3]").expect("parse");
        let bytes = from_value::<Vec<u8>>(value).expect("bytes");
        assert_eq!(bytes, vec![1, 2, 3]);
    }

    #[test]
    fn rejects_trailing_characters() {
        let err = value_from_slice(b"true false").expect_err("should reject trailing bytes");
        assert_eq!(err.to_string(), "trailing characters after JSON value");
    }

    #[test]
    fn parses_unicode_escapes() {
        let value = value_from_slice(b"\"\\u0041\\u03A9\"").expect("parse");
        assert_eq!(value, Value::String("AΩ".to_string()));
    }

    #[test]
    fn rejects_invalid_escape_sequences() {
        let err = value_from_slice(b"\"\\x\"").expect_err("invalid escape");
        assert_eq!(err.to_string(), "invalid escape sequence");
    }

    #[test]
    fn parses_large_integers() {
        let value = value_from_slice(b"123456789012345678901234567890").expect("parse");
        match value {
            Value::Number(num) => {
                assert_eq!(num.to_string(), "123456789012345678901234567890");
                assert_eq!(num.as_u64(), None);
            }
            other => panic!("unexpected value: {:?}", other),
        }
    }

    #[test]
    fn rejects_leading_zero_numbers() {
        let err = value_from_slice(b"012").expect_err("leading zeros");
        assert_eq!(err.to_string(), "leading zeros are unsupported");
    }

    #[test]
    fn coerces_bool_to_string() {
        let value = Value::Bool(true);
        let text = from_value::<String>(value).expect("bool to string");
        assert_eq!(text, "true");
    }
}
