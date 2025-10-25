use std::fmt;

use crate::Serialize;
use serde::de::DeserializeOwned;

use crate::json_impl::{self, Map as JsonMap, Number as JsonNumber, Value as JsonValue};

/// Representation of a TOML value using the crate's JSON backing types.
pub type Value = JsonValue;

/// Representation of a TOML table.
pub type Table = JsonMap;

/// Representation of a TOML number.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    Message(String),
    UnexpectedToken {
        expected: String,
        found: Option<char>,
    },
    DuplicateKey(String),
    InvalidRoot,
    Json(json_impl::Error),
}

impl Error {
    fn message<T: fmt::Display>(message: T) -> Self {
        Self {
            kind: ErrorKind::Message(message.to_string()),
        }
    }

    fn unexpected(expected: impl Into<String>, found: Option<char>) -> Self {
        Self {
            kind: ErrorKind::UnexpectedToken {
                expected: expected.into(),
                found,
            },
        }
    }

    fn duplicate_key(key: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::DuplicateKey(key.into()),
        }
    }

    fn invalid_root() -> Self {
        Self {
            kind: ErrorKind::InvalidRoot,
        }
    }

    fn json(err: json_impl::Error) -> Self {
        Self {
            kind: ErrorKind::Json(err),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Message(msg) => f.write_str(msg),
            ErrorKind::UnexpectedToken { expected, found } => match found {
                Some(ch) => write!(f, "expected {expected}, found '{ch}'"),
                None => write!(f, "expected {expected}, found end of input"),
            },
            ErrorKind::DuplicateKey(key) => write!(f, "duplicate key '{key}'"),
            ErrorKind::InvalidRoot => f.write_str("TOML documents must be tables"),
            ErrorKind::Json(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::Json(err) => Some(err),
            _ => None,
        }
    }
}

pub fn from_str<T: DeserializeOwned>(input: &str) -> Result<T> {
    let value = parse_value(input)?;
    json_impl::from_value(value).map_err(Error::json)
}

/// Parse a TOML document into a [`Value`].
pub fn parse_value(input: &str) -> Result<Value> {
    Ok(Value::Object(parse_table(input)?))
}

/// Parse a TOML document into a [`Table`].
pub fn parse_table(input: &str) -> Result<Table> {
    Parser::new(input).parse()
}

pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    let json_value = json_impl::to_value(value).map_err(Error::json)?;
    render_document_compact(json_value)
}

pub fn to_string_pretty<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    let json_value = json_impl::to_value(value).map_err(Error::json)?;
    render_document(json_value)
}

pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    Ok(to_string(value)?.into_bytes())
}

fn render_document(value: JsonValue) -> Result<String> {
    match value {
        JsonValue::Object(map) => {
            let mut renderer = Renderer::new(true);
            renderer.render_table(Vec::new(), &map)?;
            Ok(renderer.finish())
        }
        JsonValue::Null => Ok(String::new()),
        _ => Err(Error::invalid_root()),
    }
}

fn render_document_compact(value: JsonValue) -> Result<String> {
    match value {
        JsonValue::Object(map) => {
            let mut renderer = Renderer::new(false);
            renderer.render_table(Vec::new(), &map)?;
            Ok(renderer.finish())
        }
        JsonValue::Null => Ok(String::new()),
        _ => Err(Error::invalid_root()),
    }
}

struct Renderer {
    output: String,
    pretty: bool,
}

impl Renderer {
    fn new(pretty: bool) -> Self {
        Self {
            output: String::new(),
            pretty,
        }
    }

    fn finish(self) -> String {
        self.output
    }

    fn render_table(&mut self, path: Vec<String>, map: &JsonMap) -> Result<()> {
        let mut scalars: Vec<(&String, &JsonValue)> = Vec::new();
        let mut tables: Vec<(&String, &JsonValue)> = Vec::new();
        let mut table_arrays: Vec<(&String, &[JsonValue])> = Vec::new();

        for (key, value) in map {
            match value {
                JsonValue::Null => {}
                JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {
                    scalars.push((key, value));
                }
                JsonValue::Array(values) => {
                    if values.is_empty()
                        || values
                            .iter()
                            .any(|entry| !matches!(entry, JsonValue::Object(_)))
                    {
                        scalars.push((key, value));
                    } else {
                        table_arrays.push((key, values.as_slice()));
                    }
                }
                JsonValue::Object(child) => {
                    if child.is_empty() {
                        scalars.push((key, value));
                    } else {
                        tables.push((key, value));
                    }
                }
            }
        }

        scalars.sort_by(|a, b| a.0.cmp(b.0));
        tables.sort_by(|a, b| a.0.cmp(b.0));
        table_arrays.sort_by(|a, b| a.0.cmp(b.0));

        for (key, value) in &scalars {
            if !self.output.is_empty() && !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            self.write_key(key);
            self.output.push_str(" = ");
            self.render_inline(value)?;
        }

        for (index, (key, value)) in tables.iter().enumerate() {
            if !self.output.is_empty() {
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                if self.pretty && (index > 0 || !scalars.is_empty()) {
                    self.output.push('\n');
                }
            }
            let mut full_path = path.clone();
            full_path.push((*key).clone());
            self.output.push('[');
            self.output.push_str(&full_path.join("."));
            self.output.push_str("]\n");

            if let JsonValue::Object(child) = value {
                self.render_table(full_path, child)?;
            }
        }

        let mut rendered_arrays = 0usize;
        for (key, values) in &table_arrays {
            for (entry_index, value) in values.iter().enumerate() {
                if !self.output.is_empty() {
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    if self.pretty
                        && (!scalars.is_empty()
                            || !tables.is_empty()
                            || rendered_arrays > 0
                            || entry_index > 0)
                    {
                        self.output.push('\n');
                    }
                }
                rendered_arrays += 1;

                let mut full_path = path.clone();
                full_path.push((*key).clone());
                self.output.push_str("[[");
                self.output.push_str(&full_path.join("."));
                self.output.push_str("]]\n");

                if let JsonValue::Object(child) = value {
                    self.render_table(full_path, child)?;
                }
            }
        }

        Ok(())
    }

    fn write_key(&mut self, key: &str) {
        self.output.push_str(key);
    }

    fn render_inline(&mut self, value: &JsonValue) -> Result<()> {
        match value {
            JsonValue::Null => Err(Error::message("null is not valid in TOML")),
            JsonValue::Bool(true) => {
                self.output.push_str("true");
                Ok(())
            }
            JsonValue::Bool(false) => {
                self.output.push_str("false");
                Ok(())
            }
            JsonValue::Number(number) => {
                let rendered = json_impl::to_string(number).map_err(Error::json)?;
                self.output.push_str(&rendered);
                Ok(())
            }
            JsonValue::String(s) => {
                self.output.push('"');
                for ch in s.chars() {
                    match ch {
                        '\\' => self.output.push_str("\\\\"),
                        '"' => self.output.push_str("\\\""),
                        '\n' => self.output.push_str("\\n"),
                        '\r' => self.output.push_str("\\r"),
                        '\t' => self.output.push_str("\\t"),
                        _ => self.output.push(ch),
                    }
                }
                self.output.push('"');
                Ok(())
            }
            JsonValue::Array(values) => {
                self.output.push('[');
                let mut first = true;
                for value in values {
                    if !first {
                        self.output.push_str(", ");
                    }
                    first = false;
                    self.render_inline(value)?;
                }
                self.output.push(']');
                Ok(())
            }
            JsonValue::Object(map) => {
                self.output.push('{');
                let mut first = true;
                for (key, value) in map {
                    if matches!(value, JsonValue::Null) {
                        continue;
                    }
                    if !first {
                        self.output.push_str(", ");
                    }
                    first = false;
                    self.output.push_str(key);
                    self.output.push_str(" = ");
                    self.render_inline(value)?;
                }
                self.output.push('}');
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug)]
enum TableSegment {
    Table(String),
    Array { key: String, index: usize },
}

#[derive(Debug)]
enum TableKind {
    Standard,
    Array,
}

#[derive(Debug)]
struct TableHeader {
    path: Vec<String>,
    kind: TableKind,
}

struct Parser<'a> {
    input: &'a [u8],
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            index: 0,
        }
    }

    fn parse(mut self) -> Result<JsonMap> {
        let mut root = JsonMap::new();
        let mut table_path: Vec<TableSegment> = Vec::new();

        loop {
            self.skip_trivia();
            if self.eof() {
                break;
            }

            match self.peek_char() {
                Some('[') => {
                    let header = self.parse_table_header()?;
                    table_path = apply_table_header(&mut root, header)?;
                }
                Some(_) => {
                    let key_path = self.parse_key_path()?;
                    self.skip_trivia();
                    self.expect_char('=')?;
                    self.skip_trivia();
                    let value = self.parse_value()?;
                    self.skip_to_line_end();
                    insert_value(&mut root, &table_path, key_path, value)?;
                }
                None => break,
            }
        }

        Ok(root)
    }

    fn parse_table_header(&mut self) -> Result<TableHeader> {
        self.expect_char('[')?;
        let is_array = if self.peek_char() == Some('[') {
            self.index += 1;
            true
        } else {
            false
        };
        self.skip_trivia();
        let mut path = Vec::new();
        loop {
            let key = self.parse_key_component()?;
            path.push(key);
            self.skip_whitespace();
            match self.peek_char() {
                Some('.') => {
                    self.index += 1;
                    self.skip_trivia();
                }
                Some(']') => {
                    self.index += 1;
                    if is_array {
                        self.skip_whitespace();
                        self.expect_char(']')?;
                    }
                    break;
                }
                other => return Err(Error::unexpected("]", other)),
            }
        }
        self.skip_to_line_end();
        Ok(TableHeader {
            path,
            kind: if is_array {
                TableKind::Array
            } else {
                TableKind::Standard
            },
        })
    }

    fn parse_key_path(&mut self) -> Result<Vec<String>> {
        let mut path = Vec::new();
        loop {
            let key = self.parse_key_component()?;
            path.push(key);
            self.skip_whitespace();
            match self.peek_char() {
                Some('.') => {
                    self.index += 1;
                    self.skip_trivia();
                }
                _ => break,
            }
        }
        Ok(path)
    }

    fn parse_key_component(&mut self) -> Result<String> {
        match self.peek_char() {
            Some('"') => self.parse_quoted_string(),
            Some(ch) if is_key_char(ch) => self.parse_bare_key(),
            other => Err(Error::unexpected("a key", other)),
        }
    }

    fn parse_bare_key(&mut self) -> Result<String> {
        let start = self.index;
        while let Some(ch) = self.peek_char() {
            if is_key_char(ch) {
                self.index += 1;
            } else {
                break;
            }
        }
        Ok(String::from_utf8(self.input[start..self.index].to_vec()).unwrap())
    }

    fn parse_quoted_string(&mut self) -> Result<String> {
        self.expect_char('"')?;
        let mut result = String::new();
        while let Some(ch) = self.next_char() {
            match ch {
                '"' => return Ok(result),
                '\\' => {
                    let escaped = self
                        .next_char()
                        .ok_or_else(|| Error::unexpected("escape", None))?;
                    match escaped {
                        '"' => result.push('"'),
                        '\\' => result.push('\\'),
                        'n' => result.push('\n'),
                        'r' => result.push('\r'),
                        't' => result.push('\t'),
                        other => {
                            return Err(Error::message(format!(
                                "unsupported escape sequence \\{other}"
                            )))
                        }
                    }
                }
                other => result.push(other),
            }
        }
        Err(Error::unexpected("\"", None))
    }

    fn parse_value(&mut self) -> Result<JsonValue> {
        match self.peek_char() {
            Some('"') => Ok(JsonValue::String(self.parse_quoted_string()?)),
            Some('t') if self.peek_keyword("true") && self.keyword_boundary(4) => {
                self.index += 4;
                Ok(JsonValue::Bool(true))
            }
            Some('f') if self.peek_keyword("false") && self.keyword_boundary(5) => {
                self.index += 5;
                Ok(JsonValue::Bool(false))
            }
            Some('[') => self.parse_array(),
            Some('{') => self.parse_inline_table(),
            Some(ch) if ch == '-' || ch.is_ascii_digit() => self.parse_number(),
            other => Err(Error::unexpected("a value", other)),
        }
    }

    fn parse_array(&mut self) -> Result<JsonValue> {
        self.expect_char('[')?;
        let mut values = Vec::new();
        loop {
            self.skip_trivia();
            if self.peek_char() == Some(']') {
                self.index += 1;
                break;
            }
            let value = self.parse_value()?;
            values.push(value);
            self.skip_trivia();
            match self.peek_char() {
                Some(',') => {
                    self.index += 1;
                    continue;
                }
                Some(']') => {
                    self.index += 1;
                    break;
                }
                other => return Err(Error::unexpected("]", other)),
            }
        }
        Ok(JsonValue::Array(values))
    }

    fn parse_inline_table(&mut self) -> Result<JsonValue> {
        self.expect_char('{')?;
        let mut map = JsonMap::new();
        loop {
            self.skip_trivia();
            if self.peek_char() == Some('}') {
                self.index += 1;
                break;
            }
            let key_path = self.parse_key_path()?;
            self.skip_trivia();
            self.expect_char('=')?;
            self.skip_trivia();
            let value = self.parse_value()?;
            insert_value(&mut map, &[], key_path, value)?;
            self.skip_trivia();
            match self.peek_char() {
                Some(',') => {
                    self.index += 1;
                }
                Some('}') => {
                    self.index += 1;
                    break;
                }
                other => return Err(Error::unexpected("}", other)),
            }
        }
        Ok(JsonValue::Object(map))
    }

    fn parse_number(&mut self) -> Result<JsonValue> {
        let start = self.index;
        if self.peek_char() == Some('-') {
            self.index += 1;
        }
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() || ch == '_' {
                self.index += 1;
            } else {
                break;
            }
        }
        let mut is_float = false;
        if self.peek_char() == Some('.') {
            is_float = true;
            self.index += 1;
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_digit() || ch == '_' {
                    self.index += 1;
                } else {
                    break;
                }
            }
        }
        if let Some(ch) = self.peek_char() {
            if ch == 'e' || ch == 'E' {
                is_float = true;
                self.index += 1;
                if matches!(self.peek_char(), Some('+') | Some('-')) {
                    self.index += 1;
                }
                while let Some(ch) = self.peek_char() {
                    if ch.is_ascii_digit() || ch == '_' {
                        self.index += 1;
                    } else {
                        break;
                    }
                }
            }
        }

        let slice = &self.input[start..self.index];
        if !validate_number_literal(slice) {
            return Err(Error::message("invalid numeric literal"));
        }
        let text = std::str::from_utf8(slice).unwrap().replace('_', "");
        if is_float {
            let value: f64 = text
                .parse()
                .map_err(|_| Error::message("invalid float literal"))?;
            let number =
                JsonNumber::from_f64(value).ok_or_else(|| Error::message("non-finite float"))?;
            Ok(JsonValue::Number(number))
        } else if let Ok(value) = text.parse::<i64>() {
            Ok(JsonValue::Number(JsonNumber::from(value)))
        } else {
            let value = text
                .parse::<u64>()
                .map_err(|_| Error::message("invalid integer literal"))?;
            Ok(JsonValue::Number(JsonNumber::from(value)))
        }
    }

    fn skip_to_line_end(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch == '\n' {
                self.index += 1;
                break;
            } else if ch == '#' {
                while let Some(next) = self.next_char() {
                    if next == '\n' {
                        break;
                    }
                }
                break;
            } else if ch.is_whitespace() {
                self.index += 1;
            } else {
                break;
            }
        }
    }

    fn peek_keyword(&self, keyword: &str) -> bool {
        self.input[self.index..].starts_with(keyword.as_bytes())
    }

    fn keyword_boundary(&self, len: usize) -> bool {
        match self.input.get(self.index + len) {
            Some(b) => {
                let ch = *b as char;
                ch.is_whitespace() || matches!(ch, ',' | ']' | '}' | '#')
            }
            None => true,
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            self.skip_whitespace();
            match self.peek_char() {
                Some('#') => {
                    while let Some(ch) = self.next_char() {
                        if ch == '\n' {
                            break;
                        }
                    }
                }
                Some('\n') => {
                    self.index += 1;
                }
                _ => break,
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek_char(), Some(' ' | '\t' | '\r')) {
            self.index += 1;
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input.get(self.index).map(|b| *b as char)
    }

    fn next_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.index += 1;
        Some(ch)
    }

    fn expect_char(&mut self, expected: char) -> Result<()> {
        match self.next_char() {
            Some(ch) if ch == expected => Ok(()),
            other => Err(Error::unexpected(expected.to_string(), other)),
        }
    }

    fn eof(&self) -> bool {
        self.index >= self.input.len()
    }
}

fn insert_value(
    root: &mut JsonMap,
    table_path: &[TableSegment],
    key_path: Vec<String>,
    value: JsonValue,
) -> Result<()> {
    if key_path.is_empty() && table_path.is_empty() {
        return Err(Error::invalid_root());
    }

    let mut path_segments = table_path.to_vec();
    let last_index = key_path.len().saturating_sub(1);

    for (index, key) in key_path.into_iter().enumerate() {
        if index == last_index {
            let table = resolve_table_mut(root, &path_segments)?;
            if table.insert(key.clone(), value).is_some() {
                return Err(Error::duplicate_key(key));
            }
            return Ok(());
        } else {
            let table = resolve_table_mut(root, &path_segments)?;
            let entry = table
                .entry(key.clone())
                .or_insert_with(|| JsonValue::Object(JsonMap::new()));
            match entry {
                JsonValue::Object(_) => {
                    path_segments.push(TableSegment::Table(key));
                }
                _ => return Err(Error::message("non-table key used as table")),
            }
        }
    }

    Ok(())
}

fn is_key_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'
}

fn apply_table_header(root: &mut JsonMap, header: TableHeader) -> Result<Vec<TableSegment>> {
    if header.path.is_empty() {
        return Err(Error::invalid_root());
    }

    match header.kind {
        TableKind::Standard => {
            let mut segments = Vec::new();
            let mut current = root;
            for key in header.path {
                let entry = current
                    .entry(key.clone())
                    .or_insert_with(|| JsonValue::Object(JsonMap::new()));
                match entry {
                    JsonValue::Object(map) => {
                        segments.push(TableSegment::Table(key));
                        current = map;
                    }
                    _ => return Err(Error::message("table path conflicts with non-table value")),
                }
            }
            Ok(segments)
        }
        TableKind::Array => {
            let mut segments = Vec::new();
            let mut current = root;
            let last_index = header.path.len() - 1;
            for (index, key) in header.path.into_iter().enumerate() {
                if index == last_index {
                    let entry = current
                        .entry(key.clone())
                        .or_insert_with(|| JsonValue::Array(Vec::new()));
                    match entry {
                        JsonValue::Array(array) => {
                            array.push(JsonValue::Object(JsonMap::new()));
                            let new_index = array.len() - 1;
                            segments.push(TableSegment::Array {
                                key,
                                index: new_index,
                            });
                        }
                        _ => {
                            return Err(Error::message(
                                "table array path conflicts with non-array value",
                            ))
                        }
                    }
                } else {
                    let entry = current
                        .entry(key.clone())
                        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
                    match entry {
                        JsonValue::Object(map) => {
                            segments.push(TableSegment::Table(key));
                            current = map;
                        }
                        _ => {
                            return Err(Error::message("table path conflicts with non-table value"))
                        }
                    }
                }
            }
            Ok(segments)
        }
    }
}

fn resolve_table_mut<'a>(root: &'a mut JsonMap, path: &[TableSegment]) -> Result<&'a mut JsonMap> {
    let mut current = root;
    for segment in path {
        match segment {
            TableSegment::Table(key) => {
                let entry = current
                    .get_mut(key)
                    .ok_or_else(|| Error::message("table path missing"))?;
                match entry {
                    JsonValue::Object(map) => {
                        current = map;
                    }
                    _ => return Err(Error::message("non-table key used as table")),
                }
            }
            TableSegment::Array { key, index } => {
                let entry = current
                    .get_mut(key)
                    .ok_or_else(|| Error::message("table array missing"))?;
                match entry {
                    JsonValue::Array(values) => {
                        if *index >= values.len() {
                            return Err(Error::message("table array index out of bounds"));
                        }
                        match values.get_mut(*index).expect("index bounds checked above") {
                            JsonValue::Object(map) => {
                                current = map;
                            }
                            _ => return Err(Error::message("table array element is not a table")),
                        }
                    }
                    _ => return Err(Error::message("non-array key used as table array")),
                }
            }
        }
    }
    Ok(current)
}

fn validate_number_literal(slice: &[u8]) -> bool {
    let mut prev: Option<u8> = None;
    for (i, &b) in slice.iter().enumerate() {
        if b == b'_' {
            if i == 0 {
                return false;
            }
            match prev {
                Some(prev_b)
                    if prev_b == b'_' || matches!(prev_b, b'-' | b'+' | b'.' | b'e' | b'E') =>
                {
                    return false;
                }
                None => return false,
                _ => {}
            }
        }
        prev = Some(b);
    }
    prev != Some(b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ser::SerializeStruct;
    use crate::{de, defaults, ser, Deserialize, Serialize};
    use core::{fmt, result::Result as StdResult};

    #[derive(Debug, PartialEq)]
    struct Sample {
        enabled: bool,
        port: u16,
        name: String,
        values: Vec<u32>,
        nested: Option<Nested>,
    }

    impl Serialize for Sample {
        fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            let mut state = serializer.serialize_struct("Sample", 5)?;
            state.serialize_field("enabled", &self.enabled)?;
            state.serialize_field("port", &self.port)?;
            state.serialize_field("name", &self.name)?;
            state.serialize_field("values", &self.values)?;
            state.serialize_field("nested", &self.nested)?;
            state.end()
        }
    }

    impl<'de> Deserialize<'de> for Sample {
        fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            enum Field {
                Enabled,
                Port,
                Name,
                Values,
                Nested,
            }

            struct FieldVisitor;

            impl<'de> de::Visitor<'de> for FieldVisitor {
                type Value = Field;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("`enabled`, `port`, `name`, `values`, or `nested`")
                }

                fn visit_str<E>(self, value: &str) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        "enabled" => Ok(Field::Enabled),
                        "port" => Ok(Field::Port),
                        "name" => Ok(Field::Name),
                        "values" => Ok(Field::Values),
                        "nested" => Ok(Field::Nested),
                        other => Err(de::Error::unknown_field(other, FIELDS)),
                    }
                }

                fn visit_bytes<E>(self, value: &[u8]) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        b"enabled" => Ok(Field::Enabled),
                        b"port" => Ok(Field::Port),
                        b"name" => Ok(Field::Name),
                        b"values" => Ok(Field::Values),
                        b"nested" => Ok(Field::Nested),
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
                    let mut enabled = None;
                    let mut port = None;
                    let mut name = None;
                    let mut values = None;
                    let mut nested: Option<Option<Nested>> = None;
                    while let Some(field) = map.next_key::<Field>()? {
                        match field {
                            Field::Enabled => {
                                if enabled.is_some() {
                                    return Err(de::Error::duplicate_field("enabled"));
                                }
                                enabled = Some(map.next_value()?);
                            }
                            Field::Port => {
                                if port.is_some() {
                                    return Err(de::Error::duplicate_field("port"));
                                }
                                port = Some(map.next_value()?);
                            }
                            Field::Name => {
                                if name.is_some() {
                                    return Err(de::Error::duplicate_field("name"));
                                }
                                name = Some(map.next_value()?);
                            }
                            Field::Values => {
                                if values.is_some() {
                                    return Err(de::Error::duplicate_field("values"));
                                }
                                values = Some(map.next_value()?);
                            }
                            Field::Nested => {
                                if nested.is_some() {
                                    return Err(de::Error::duplicate_field("nested"));
                                }
                                nested = Some(map.next_value()?);
                            }
                        }
                    }
                    Ok(Sample {
                        enabled: enabled.ok_or_else(|| de::Error::missing_field("enabled"))?,
                        port: port.ok_or_else(|| de::Error::missing_field("port"))?,
                        name: name.ok_or_else(|| de::Error::missing_field("name"))?,
                        values: values.ok_or_else(|| de::Error::missing_field("values"))?,
                        nested: nested.unwrap_or_else(defaults::default),
                    })
                }
            }

            const FIELDS: &[&str] = &["enabled", "port", "name", "values", "nested"];
            deserializer.deserialize_struct("Sample", FIELDS, SampleVisitor)
        }
    }

    #[derive(Debug, PartialEq)]
    struct Nested {
        threshold: f64,
        note: String,
    }

    impl Serialize for Nested {
        fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            let mut state = serializer.serialize_struct("Nested", 2)?;
            state.serialize_field("threshold", &self.threshold)?;
            state.serialize_field("note", &self.note)?;
            state.end()
        }
    }

    impl<'de> Deserialize<'de> for Nested {
        fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            enum Field {
                Threshold,
                Note,
            }

            struct FieldVisitor;

            impl<'de> de::Visitor<'de> for FieldVisitor {
                type Value = Field;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("`threshold` or `note`")
                }

                fn visit_str<E>(self, value: &str) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        "threshold" => Ok(Field::Threshold),
                        "note" => Ok(Field::Note),
                        other => Err(de::Error::unknown_field(other, FIELDS)),
                    }
                }

                fn visit_bytes<E>(self, value: &[u8]) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        b"threshold" => Ok(Field::Threshold),
                        b"note" => Ok(Field::Note),
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

            struct NestedVisitor;

            impl<'de> de::Visitor<'de> for NestedVisitor {
                type Value = Nested;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("Nested struct")
                }

                fn visit_map<M>(self, mut map: M) -> StdResult<Nested, M::Error>
                where
                    M: de::MapAccess<'de>,
                {
                    let mut threshold = None;
                    let mut note = None;
                    while let Some(field) = map.next_key::<Field>()? {
                        match field {
                            Field::Threshold => {
                                if threshold.is_some() {
                                    return Err(de::Error::duplicate_field("threshold"));
                                }
                                threshold = Some(map.next_value()?);
                            }
                            Field::Note => {
                                if note.is_some() {
                                    return Err(de::Error::duplicate_field("note"));
                                }
                                note = Some(map.next_value()?);
                            }
                        }
                    }
                    Ok(Nested {
                        threshold: threshold
                            .ok_or_else(|| de::Error::missing_field("threshold"))?,
                        note: note.ok_or_else(|| de::Error::missing_field("note"))?,
                    })
                }
            }

            const FIELDS: &[&str] = &["threshold", "note"];
            deserializer.deserialize_struct("Nested", FIELDS, NestedVisitor)
        }
    }

    #[test]
    fn parse_simple_tables() {
        let text = r#"
            enabled = true
            port = 9000
            name = "demo"
            values = [1, 2, 3]

            [nested]
            threshold = 0.5
            note = "hi"
        "#;

        let parsed: Sample = from_str(text).expect("parse sample");
        assert_eq!(parsed.enabled, true);
        assert_eq!(parsed.port, 9000);
        assert_eq!(parsed.name, "demo");
        assert_eq!(parsed.values, vec![1, 2, 3]);
        assert_eq!(parsed.nested.unwrap().threshold, 0.5);
    }

    #[test]
    fn roundtrip_pretty() {
        let sample = Sample {
            enabled: true,
            port: 7000,
            name: "roundtrip".into(),
            values: vec![4, 5, 6],
            nested: Some(Nested {
                threshold: 1.25,
                note: "ok".into(),
            }),
        };

        let rendered = to_string_pretty(&sample).expect("serialize");
        let reparsed: Sample = from_str(&rendered).expect("reparse");
        assert_eq!(sample, reparsed);
    }

    #[derive(Debug, PartialEq)]
    struct DeviceConfig {
        devices: Vec<Device>,
    }

    impl Serialize for DeviceConfig {
        fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            let mut state = serializer.serialize_struct("DeviceConfig", 1)?;
            state.serialize_field("devices", &self.devices)?;
            state.end()
        }
    }

    impl<'de> Deserialize<'de> for DeviceConfig {
        fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            enum Field {
                Devices,
            }

            struct FieldVisitor;

            impl<'de> de::Visitor<'de> for FieldVisitor {
                type Value = Field;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("`devices`")
                }

                fn visit_str<E>(self, value: &str) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        "devices" => Ok(Field::Devices),
                        other => Err(de::Error::unknown_field(other, FIELDS)),
                    }
                }

                fn visit_bytes<E>(self, value: &[u8]) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        b"devices" => Ok(Field::Devices),
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

            struct DeviceConfigVisitor;

            impl<'de> de::Visitor<'de> for DeviceConfigVisitor {
                type Value = DeviceConfig;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("DeviceConfig struct")
                }

                fn visit_map<M>(self, mut map: M) -> StdResult<DeviceConfig, M::Error>
                where
                    M: de::MapAccess<'de>,
                {
                    let mut devices = None;
                    while let Some(field) = map.next_key::<Field>()? {
                        match field {
                            Field::Devices => {
                                if devices.is_some() {
                                    return Err(de::Error::duplicate_field("devices"));
                                }
                                devices = Some(map.next_value()?);
                            }
                        }
                    }
                    Ok(DeviceConfig {
                        devices: devices.ok_or_else(|| de::Error::missing_field("devices"))?,
                    })
                }
            }

            const FIELDS: &[&str] = &["devices"];
            deserializer.deserialize_struct("DeviceConfig", FIELDS, DeviceConfigVisitor)
        }
    }

    #[derive(Debug, PartialEq)]
    struct Device {
        name: String,
        enabled: bool,
    }

    impl Serialize for Device {
        fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            let mut state = serializer.serialize_struct("Device", 2)?;
            state.serialize_field("name", &self.name)?;
            state.serialize_field("enabled", &self.enabled)?;
            state.end()
        }
    }

    impl<'de> Deserialize<'de> for Device {
        fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            enum Field {
                Name,
                Enabled,
            }

            struct FieldVisitor;

            impl<'de> de::Visitor<'de> for FieldVisitor {
                type Value = Field;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("`name` or `enabled`")
                }

                fn visit_str<E>(self, value: &str) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        "name" => Ok(Field::Name),
                        "enabled" => Ok(Field::Enabled),
                        other => Err(de::Error::unknown_field(other, FIELDS)),
                    }
                }

                fn visit_bytes<E>(self, value: &[u8]) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        b"name" => Ok(Field::Name),
                        b"enabled" => Ok(Field::Enabled),
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

            struct DeviceVisitor;

            impl<'de> de::Visitor<'de> for DeviceVisitor {
                type Value = Device;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("Device struct")
                }

                fn visit_map<M>(self, mut map: M) -> StdResult<Device, M::Error>
                where
                    M: de::MapAccess<'de>,
                {
                    let mut name = None;
                    let mut enabled = None;
                    while let Some(field) = map.next_key::<Field>()? {
                        match field {
                            Field::Name => {
                                if name.is_some() {
                                    return Err(de::Error::duplicate_field("name"));
                                }
                                name = Some(map.next_value()?);
                            }
                            Field::Enabled => {
                                if enabled.is_some() {
                                    return Err(de::Error::duplicate_field("enabled"));
                                }
                                enabled = Some(map.next_value()?);
                            }
                        }
                    }
                    Ok(Device {
                        name: name.ok_or_else(|| de::Error::missing_field("name"))?,
                        enabled: enabled.ok_or_else(|| de::Error::missing_field("enabled"))?,
                    })
                }
            }

            const FIELDS: &[&str] = &["name", "enabled"];
            deserializer.deserialize_struct("Device", FIELDS, DeviceVisitor)
        }
    }

    #[test]
    fn parse_array_of_tables() {
        let text = r#"
            [[devices]]
            name = "alpha"
            enabled = true

            [[devices]]
            name = "beta"
            enabled = false
        "#;

        let parsed: DeviceConfig = from_str(text).expect("parse devices");
        assert_eq!(parsed.devices.len(), 2);
        assert_eq!(parsed.devices[0].name, "alpha");
        assert!(!parsed.devices[1].enabled);

        let compact = to_string(&parsed).expect("serialize compact");
        let pretty = to_string_pretty(&parsed).expect("serialize pretty");
        assert!(pretty.contains("[[devices]]"));
        assert!(pretty.contains("\n\n[[devices]]"));
        assert!(compact.contains("[[devices]]"));
        assert!(!compact.contains("\n\n[[devices]]"));
    }

    #[derive(Debug)]
    struct Flags {
        value: i64,
        flag: bool,
    }

    impl<'de> Deserialize<'de> for Flags {
        fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            enum Field {
                Value,
                Flag,
            }

            struct FieldVisitor;

            impl<'de> de::Visitor<'de> for FieldVisitor {
                type Value = Field;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("`value` or `flag`")
                }

                fn visit_str<E>(self, value: &str) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        "value" => Ok(Field::Value),
                        "flag" => Ok(Field::Flag),
                        other => Err(de::Error::unknown_field(other, FIELDS)),
                    }
                }

                fn visit_bytes<E>(self, value: &[u8]) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        b"value" => Ok(Field::Value),
                        b"flag" => Ok(Field::Flag),
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

            struct FlagsVisitor;

            impl<'de> de::Visitor<'de> for FlagsVisitor {
                type Value = Flags;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("Flags struct")
                }

                fn visit_map<M>(self, mut map: M) -> StdResult<Flags, M::Error>
                where
                    M: de::MapAccess<'de>,
                {
                    let mut value = None;
                    let mut flag = None;
                    while let Some(field) = map.next_key::<Field>()? {
                        match field {
                            Field::Value => {
                                if value.is_some() {
                                    return Err(de::Error::duplicate_field("value"));
                                }
                                value = Some(map.next_value()?);
                            }
                            Field::Flag => {
                                if flag.is_some() {
                                    return Err(de::Error::duplicate_field("flag"));
                                }
                                flag = Some(map.next_value()?);
                            }
                        }
                    }
                    Ok(Flags {
                        value: value.ok_or_else(|| de::Error::missing_field("value"))?,
                        flag: flag.ok_or_else(|| de::Error::missing_field("flag"))?,
                    })
                }
            }

            const FIELDS: &[&str] = &["value", "flag"];
            deserializer.deserialize_struct("Flags", FIELDS, FlagsVisitor)
        }
    }

    #[test]
    fn parse_comments_and_trivia() {
        let text = "value = 10 # trailing\nflag = true\n";
        let parsed: Flags = from_str(text).expect("parse with comments");
        assert_eq!(parsed.value, 10);
        assert!(parsed.flag);
    }

    #[test]
    fn reject_invalid_numeric_literal() {
        #[derive(Debug)]
        struct Number {
            #[allow(dead_code)]
            value: i32,
        }

        impl<'de> Deserialize<'de> for Number {
            fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
            where
                D: de::Deserializer<'de>,
            {
                enum Field {
                    Value,
                }

                struct FieldVisitor;

                impl<'de> de::Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str("`value`")
                    }

                    fn visit_str<E>(self, value: &str) -> StdResult<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "value" => Ok(Field::Value),
                            other => Err(de::Error::unknown_field(other, FIELDS)),
                        }
                    }

                    fn visit_bytes<E>(self, value: &[u8]) -> StdResult<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            b"value" => Ok(Field::Value),
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

                struct NumberVisitor;

                impl<'de> de::Visitor<'de> for NumberVisitor {
                    type Value = Number;

                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str("Number struct")
                    }

                    fn visit_map<M>(self, mut map: M) -> StdResult<Number, M::Error>
                    where
                        M: de::MapAccess<'de>,
                    {
                        let mut value = None;
                        while let Some(field) = map.next_key::<Field>()? {
                            match field {
                                Field::Value => {
                                    if value.is_some() {
                                        return Err(de::Error::duplicate_field("value"));
                                    }
                                    value = Some(map.next_value()?);
                                }
                            }
                        }
                        Ok(Number {
                            value: value.ok_or_else(|| de::Error::missing_field("value"))?,
                        })
                    }
                }

                const FIELDS: &[&str] = &["value"];
                deserializer.deserialize_struct("Number", FIELDS, NumberVisitor)
            }
        }

        let err = from_str::<Number>("value = 1__0").expect_err("underscore error");
        assert!(format!("{err}").contains("invalid numeric literal"));
    }

    #[test]
    fn reject_boolean_without_boundary() {
        #[derive(Debug)]
        struct Flag {
            #[allow(dead_code)]
            flag: bool,
        }

        impl<'de> Deserialize<'de> for Flag {
            fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
            where
                D: de::Deserializer<'de>,
            {
                enum Field {
                    Flag,
                }

                struct FieldVisitor;

                impl<'de> de::Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str("`flag`")
                    }

                    fn visit_str<E>(self, value: &str) -> StdResult<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "flag" => Ok(Field::Flag),
                            other => Err(de::Error::unknown_field(other, FIELDS)),
                        }
                    }

                    fn visit_bytes<E>(self, value: &[u8]) -> StdResult<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            b"flag" => Ok(Field::Flag),
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

                struct FlagVisitor;

                impl<'de> de::Visitor<'de> for FlagVisitor {
                    type Value = Flag;

                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str("Flag struct")
                    }

                    fn visit_map<M>(self, mut map: M) -> StdResult<Flag, M::Error>
                    where
                        M: de::MapAccess<'de>,
                    {
                        let mut flag = None;
                        while let Some(field) = map.next_key::<Field>()? {
                            match field {
                                Field::Flag => {
                                    if flag.is_some() {
                                        return Err(de::Error::duplicate_field("flag"));
                                    }
                                    flag = Some(map.next_value()?);
                                }
                            }
                        }
                        Ok(Flag {
                            flag: flag.ok_or_else(|| de::Error::missing_field("flag"))?,
                        })
                    }
                }

                const FIELDS: &[&str] = &["flag"];
                deserializer.deserialize_struct("Flag", FIELDS, FlagVisitor)
            }
        }

        let err = from_str::<Flag>("flag = trueish").expect_err("boundary");
        assert!(format!("{err}").contains("expected a value"));
    }
}
