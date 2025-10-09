#![forbid(unsafe_code)]

//! First-party RPC envelope shared across the workspace.
//!
//! The helpers in this crate intentionally mirror the wire contract exposed by
//! the node while avoiding any dependency on `jsonrpc-core`.  Request and
//! response structures round-trip through `foundation_serialization` and provide
//! utilities for bridging to the in-house `httpd` layer.

use foundation_serialization::json::{self, Map, Value};
use httpd::{HttpError, Request as HttpRequest, Response as HttpResponse, StatusCode};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use thiserror::Error;

/// JSON-RPC version identifier carried by every envelope.
pub const VERSION: &str = "2.0";

/// Error returned by the first-party RPC layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcError {
    pub code: i32,
    pub message: Cow<'static, str>,
}

impl RpcError {
    /// Construct a new error with the supplied code and message.
    pub fn new(code: i32, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Borrow the message as a string slice.
    pub fn message(&self) -> &str {
        self.message.as_ref()
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RpcError {}

/// Parameters embedded in an RPC request.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(transparent)]
pub struct Params(Value);

impl Params {
    /// Construct parameters from a JSON value.
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    /// Borrow the parameters as a JSON value.
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// Consume the parameters returning the underlying value.
    pub fn into_inner(self) -> Value {
        self.0
    }

    /// Return the parameters as an object map if present.
    pub fn as_map(&self) -> Option<&Map> {
        self.0.as_object()
    }

    /// Look up a field inside an object parameter map.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_map()?.get(key)
    }

    /// Returns `true` when the parameters are empty or null.
    pub fn is_empty(&self) -> bool {
        match &self.0 {
            Value::Null => true,
            Value::Object(map) => map.is_empty(),
            Value::Array(array) => array.is_empty(),
            _ => false,
        }
    }
}

impl From<Value> for Params {
    fn from(value: Value) -> Self {
        Self::new(value)
    }
}

impl From<Params> for Value {
    fn from(params: Params) -> Self {
        params.into_inner()
    }
}

/// Inbound RPC request envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Request {
    #[serde(rename = "jsonrpc", default)]
    pub version: Option<String>,
    pub method: String,
    #[serde(default)]
    pub params: Params,
    #[serde(default)]
    pub id: Option<Value>,
    #[serde(default)]
    pub badge: Option<String>,
}

impl Request {
    /// Construct a new request targeting `method` with the supplied parameters.
    pub fn new(method: impl Into<String>, params: impl Into<Params>) -> Self {
        Self {
            version: Some(VERSION.to_string()),
            method: method.into(),
            params: params.into(),
            id: None,
            badge: None,
        }
    }

    /// Parse a request from a slice of bytes using the first-party JSON codec.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, foundation_serialization::Error> {
        json::from_slice(bytes)
    }

    /// Serialise the request into a vector of bytes.
    pub fn to_vec(&self) -> Result<Vec<u8>, foundation_serialization::Error> {
        json::to_vec(self)
    }

    /// Attempt to parse a request from an [`httpd`] request, enforcing `max_body`.
    pub fn from_http_state<T>(
        request: &HttpRequest<T>,
        max_body: usize,
    ) -> Result<Self, RequestParseError> {
        let body = request.body_bytes();
        if body.len() > max_body {
            return Err(RequestParseError::BodyLimit {
                len: body.len(),
                limit: max_body,
            });
        }
        Ok(Self::from_slice(body)?)
    }
}

/// RPC response envelope dispatched to clients.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Response {
    Result {
        #[serde(rename = "jsonrpc")]
        version: String,
        result: Value,
        #[serde(default)]
        id: Option<Value>,
    },
    Error {
        #[serde(rename = "jsonrpc")]
        version: String,
        error: RpcError,
        #[serde(default)]
        id: Option<Value>,
    },
}

impl Response {
    /// Construct a success response wrapping `result`.
    pub fn success(result: Value, id: Option<Value>) -> Self {
        Self::Result {
            version: VERSION.to_string(),
            result,
            id,
        }
    }

    /// Construct an error response.
    pub fn error(error: RpcError, id: Option<Value>) -> Self {
        Self::Error {
            version: VERSION.to_string(),
            error,
            id,
        }
    }

    /// Serialise the response into a vector of bytes.
    pub fn to_vec(&self) -> Result<Vec<u8>, foundation_serialization::Error> {
        json::to_vec(self)
    }

    /// Convert the response into an [`httpd::Response`] with the supplied status.
    pub fn into_http(self, status: StatusCode) -> Result<HttpResponse, HttpError> {
        HttpResponse::new(status).json(&self)
    }
}

/// Errors encountered while parsing RPC requests from HTTP.
#[derive(Debug, Error)]
pub enum RequestParseError {
    #[error("request body exceeds limit ({len} > {limit})")]
    BodyLimit { len: usize, limit: usize },
    #[error(transparent)]
    Serialization(#[from] foundation_serialization::Error),
}

impl RequestParseError {
    /// Render the parse error as an RPC error payload.
    pub fn into_rpc_error(self) -> RpcError {
        match self {
            RequestParseError::BodyLimit { .. } => RpcError::new(-32600, "invalid request"),
            RequestParseError::Serialization(_) => RpcError::new(-32700, "parse error"),
        }
    }
}
