use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::fmt;
use std::str;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Unsigned(u64),
    Text(String),
    Bytes(Vec<u8>),
    Bool(bool),
    Array(Vec<Value>),
    Map(BTreeMap<String, Value>),
    Null,
}

impl Value {
    pub fn as_map(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Map(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text.as_str()),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Unsigned(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(bytes) => Some(bytes.as_slice()),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(values) => Some(values.as_slice()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug, Clone, PartialEq)]
enum ErrorKind {
    UnexpectedEof,
    TrailingBytes,
    InvalidType(&'static str),
    Unsupported(&'static str),
    InvalidUtf8(str::Utf8Error),
    LengthOverflow,
}

impl Error {
    pub(crate) fn unexpected_eof() -> Self {
        Self {
            kind: ErrorKind::UnexpectedEof,
        }
    }

    pub(crate) fn trailing_bytes() -> Self {
        Self {
            kind: ErrorKind::TrailingBytes,
        }
    }

    pub(crate) fn invalid_type(msg: &'static str) -> Self {
        Self {
            kind: ErrorKind::InvalidType(msg),
        }
    }

    pub(crate) fn unsupported(msg: &'static str) -> Self {
        Self {
            kind: ErrorKind::Unsupported(msg),
        }
    }

    pub(crate) fn invalid_utf8(err: str::Utf8Error) -> Self {
        Self {
            kind: ErrorKind::InvalidUtf8(err),
        }
    }

    pub(crate) fn length_overflow() -> Self {
        Self {
            kind: ErrorKind::LengthOverflow,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::UnexpectedEof => write!(f, "unexpected end of CBOR input"),
            ErrorKind::TrailingBytes => write!(f, "trailing bytes after CBOR value"),
            ErrorKind::InvalidType(msg) => write!(f, "invalid CBOR type: {msg}"),
            ErrorKind::Unsupported(msg) => write!(f, "unsupported CBOR feature: {msg}"),
            ErrorKind::InvalidUtf8(err) => write!(f, "invalid UTF-8 string: {err}"),
            ErrorKind::LengthOverflow => write!(f, "length exceeds usize::MAX"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::InvalidUtf8(err) => Some(err),
            _ => None,
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

pub fn parse(bytes: &[u8]) -> Result<Value> {
    let mut reader = Reader::new(bytes);
    let value = reader.read_value()?;
    if reader.is_complete() {
        Ok(value)
    } else {
        Err(Error::trailing_bytes())
    }
}

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn is_complete(&self) -> bool {
        self.position == self.data.len()
    }

    fn read_value(&mut self) -> Result<Value> {
        let (major, additional) = self.read_type()?;
        match major {
            0 => self.read_unsigned(additional).map(Value::Unsigned),
            1 => Err(Error::unsupported("negative integers")),
            2 => self.read_bytes(additional).map(Value::Bytes),
            3 => self.read_text(additional).map(Value::Text),
            4 => self.read_array(additional).map(Value::Array),
            5 => self.read_map(additional).map(Value::Map),
            6 => {
                let _tag = self.read_unsigned(additional)?;
                self.read_value()
            }
            7 => self.read_simple(additional),
            _ => Err(Error::invalid_type("unknown major type")),
        }
    }

    fn read_simple(&mut self, additional: u8) -> Result<Value> {
        match additional {
            20 => Ok(Value::Bool(false)),
            21 => Ok(Value::Bool(true)),
            22 | 23 => Ok(Value::Null),
            24 => {
                let _ = self.read_byte()?;
                Ok(Value::Null)
            }
            25 => Err(Error::unsupported("half precision floats")),
            26 | 27 => Err(Error::unsupported("floating point values")),
            _ => Err(Error::invalid_type("unknown simple value")),
        }
    }

    fn read_array(&mut self, additional: u8) -> Result<Vec<Value>> {
        let len = self.read_length(additional)?;
        let mut values = Vec::new();
        match len {
            Length::Finite(count) => {
                for _ in 0..count {
                    values.push(self.read_value()?);
                }
            }
            Length::Indefinite => loop {
                match self.peek_byte()? {
                    0xff => {
                        self.read_byte()?;
                        break;
                    }
                    _ => values.push(self.read_value()?),
                }
            },
        }
        Ok(values)
    }

    fn read_map(&mut self, additional: u8) -> Result<BTreeMap<String, Value>> {
        let len = self.read_length(additional)?;
        let mut map = BTreeMap::new();
        match len {
            Length::Finite(count) => {
                for _ in 0..count {
                    let (major, add) = self.read_type()?;
                    if major != 3 {
                        return Err(Error::invalid_type("map keys must be text"));
                    }
                    let key = self.read_text(add)?;
                    let value = self.read_value()?;
                    map.insert(key, value);
                }
            }
            Length::Indefinite => loop {
                match self.peek_byte()? {
                    0xff => {
                        self.read_byte()?;
                        break;
                    }
                    _ => {
                        let (major, add) = self.read_type()?;
                        if major != 3 {
                            return Err(Error::invalid_type("map keys must be text"));
                        }
                        let key = self.read_text(add)?;
                        let value = self.read_value()?;
                        map.insert(key, value);
                    }
                }
            },
        }
        Ok(map)
    }

    fn read_text(&mut self, additional: u8) -> Result<String> {
        match self.read_length(additional)? {
            Length::Finite(len) => {
                let bytes = self.read_exact(len)?;
                Ok(str::from_utf8(bytes)
                    .map_err(Error::invalid_utf8)?
                    .to_owned())
            }
            Length::Indefinite => {
                let mut result = String::new();
                loop {
                    match self.peek_byte()? {
                        0xff => {
                            self.read_byte()?;
                            break;
                        }
                        _ => {
                            let (major, add) = self.read_type()?;
                            if major != 3 {
                                return Err(Error::invalid_type(
                                    "indefinite text chunks must be text",
                                ));
                            }
                            let chunk_len = match self.read_length(add)? {
                                Length::Finite(len) => len,
                                Length::Indefinite => {
                                    return Err(Error::invalid_type(
                                        "nested indefinite strings not supported",
                                    ))
                                }
                            };
                            let bytes = self.read_exact(chunk_len)?;
                            result.push_str(str::from_utf8(bytes).map_err(Error::invalid_utf8)?);
                        }
                    }
                }
                Ok(result)
            }
        }
    }

    fn read_bytes(&mut self, additional: u8) -> Result<Vec<u8>> {
        match self.read_length(additional)? {
            Length::Finite(len) => self.read_exact(len).map(|slice| slice.to_vec()),
            Length::Indefinite => {
                let mut result = Vec::new();
                loop {
                    match self.peek_byte()? {
                        0xff => {
                            self.read_byte()?;
                            break;
                        }
                        _ => {
                            let (major, add) = self.read_type()?;
                            if major != 2 {
                                return Err(Error::invalid_type(
                                    "indefinite byte strings must use byte chunks",
                                ));
                            }
                            let chunk_len = match self.read_length(add)? {
                                Length::Finite(len) => len,
                                Length::Indefinite => {
                                    return Err(Error::invalid_type(
                                        "nested indefinite byte strings not supported",
                                    ))
                                }
                            };
                            let chunk = self.read_exact(chunk_len)?;
                            result.extend_from_slice(chunk);
                        }
                    }
                }
                Ok(result)
            }
        }
    }

    fn read_unsigned(&mut self, additional: u8) -> Result<u64> {
        match additional {
            n @ 0..=23 => Ok(n as u64),
            24 => self.read_byte().map(|b| b as u64),
            25 => {
                let mut buf = [0u8; 2];
                buf.copy_from_slice(self.read_exact(2)?);
                Ok(u16::from_be_bytes(buf) as u64)
            }
            26 => {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(self.read_exact(4)?);
                Ok(u32::from_be_bytes(buf) as u64)
            }
            27 => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(self.read_exact(8)?);
                Ok(u64::from_be_bytes(buf))
            }
            _ => Err(Error::unsupported("additional integer width")),
        }
    }

    fn read_length(&mut self, additional: u8) -> Result<Length> {
        if additional == 31 {
            return Ok(Length::Indefinite);
        }
        let len = self.read_unsigned(additional)?;
        let len = usize::try_from(len).map_err(|_| Error::length_overflow())?;
        Ok(Length::Finite(len))
    }

    fn read_type(&mut self) -> Result<(u8, u8)> {
        let byte = self.read_byte()?;
        Ok((byte >> 5, byte & 0x1f))
    }

    fn read_byte(&mut self) -> Result<u8> {
        let byte = *self
            .data
            .get(self.position)
            .ok_or_else(Error::unexpected_eof)?;
        self.position += 1;
        Ok(byte)
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(len)
            .ok_or_else(Error::length_overflow)?;
        let slice = self
            .data
            .get(self.position..end)
            .ok_or_else(Error::unexpected_eof)?;
        self.position = end;
        Ok(slice)
    }

    fn peek_byte(&self) -> Result<u8> {
        self.data
            .get(self.position)
            .copied()
            .ok_or_else(Error::unexpected_eof)
    }
}

enum Length {
    Finite(usize),
    Indefinite,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_trailing_bytes() {
        let data = [0xf6, 0x00]; // null followed by trailing byte
        let err = parse(&data).expect_err("should detect trailing bytes");
        assert_eq!(err, Error::trailing_bytes());
    }

    #[test]
    fn parses_indefinite_map() {
        let data = [
            0xbf, // start indefinite map
            0x63, b'o', b'n', b'e', 0x01, 0x63, b't', b'w', b'o', 0x82, 0x01,
            0x02, // array value
            0xff,
        ];
        let value = parse(&data).expect("parse indefinite map");
        let map = value.as_map().expect("map");
        assert_eq!(map.get("one"), Some(&Value::Unsigned(1)));
        assert_eq!(
            map.get("two"),
            Some(&Value::Array(vec![Value::Unsigned(1), Value::Unsigned(2)]))
        );
    }
}
