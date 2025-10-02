use std::str::FromStr;

use thiserror::Error;

#[derive(Clone, Debug)]
pub enum ParsedValue {
    Flag(bool),
    Single(String),
    Many(Vec<String>),
    None,
}

impl ParsedValue {
    pub fn is_present(&self) -> bool {
        !matches!(self, ParsedValue::None)
    }

    pub fn as_bool(&self) -> bool {
        match self {
            ParsedValue::Flag(value) => *value,
            ParsedValue::Single(value) => value == "true",
            ParsedValue::Many(values) => values.iter().any(|v| v == "true"),
            ParsedValue::None => false,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            ParsedValue::Single(value) => Some(value.as_str()),
            ParsedValue::Many(values) => values.first().map(|v| v.as_str()),
            _ => None,
        }
    }

    pub fn as_strings(&self) -> Vec<&str> {
        match self {
            ParsedValue::Single(value) => vec![value.as_str()],
            ParsedValue::Many(values) => values.iter().map(|v| v.as_str()).collect(),
            _ => Vec::new(),
        }
    }

    pub fn parse<T>(&self) -> Result<T, ParseError>
    where
        T: FromStr,
        T::Err: std::error::Error + Send + Sync + 'static,
    {
        match self.as_str() {
            Some(value) => value.parse::<T>().map_err(|err| ParseError::InvalidValue {
                value: value.to_string(),
                source: Box::new(err),
            }),
            None => Err(ParseError::MissingValue),
        }
    }
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("missing value")]
    MissingValue,
    #[error("invalid value '{value}'")]
    InvalidValue {
        value: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum ValueType {
    Bool,
    String,
    Integer,
    Float,
}
