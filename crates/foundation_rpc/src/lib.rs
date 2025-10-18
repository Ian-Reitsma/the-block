#![forbid(unsafe_code)]

//! First-party RPC envelope shared across the workspace.
//!
//! The helpers in this crate intentionally mirror the wire contract exposed by
//! the node while avoiding any dependency on `jsonrpc-core`.  Request and
//! response structures round-trip through `foundation_serialization` and provide
//! utilities for bridging to the in-house `httpd` layer.

use foundation_serialization::de::DeserializeOwned;
use foundation_serialization::json::{self, Map, Value};
use foundation_serialization::{Deserialize, Serialize};
use httpd::{HttpError, Request as HttpRequest, Response as HttpResponse, StatusCode};
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
    #[serde(default = "foundation_serialization::defaults::default")]
    pub params: Params,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub id: Option<Value>,
    #[serde(default = "foundation_serialization::defaults::default")]
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

    /// Attach an identifier to the request and return the updated envelope.
    pub fn with_id(mut self, id: impl Into<Value>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Attach a badge to the request envelope.
    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    /// Override the parameters carried by this request.
    pub fn with_params(mut self, params: impl Into<Params>) -> Self {
        self.params = params.into();
        self
    }

    /// Borrow the identifier associated with this request, when present.
    pub fn id(&self) -> Option<&Value> {
        self.id.as_ref()
    }

    /// Borrow the parameters embedded in the request.
    pub fn params(&self) -> &Params {
        &self.params
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
        #[serde(default = "foundation_serialization::defaults::default")]
        id: Option<Value>,
    },
    Error {
        #[serde(rename = "jsonrpc")]
        version: String,
        error: RpcError,
        #[serde(default = "foundation_serialization::defaults::default")]
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

    /// Borrow the identifier attached to this response, if present.
    pub fn id(&self) -> Option<&Value> {
        match self {
            Response::Result { id, .. } | Response::Error { id, .. } => id.as_ref(),
        }
    }

    /// Convert this response into a typed payload, decoding the success branch
    /// into `T` while preserving RPC errors.
    pub fn into_payload<T>(self) -> Result<ResponsePayload<T>, foundation_serialization::Error>
    where
        T: DeserializeOwned,
    {
        match self {
            Response::Result { result, id, .. } => {
                let typed = json::from_value(result)?;
                Ok(ResponsePayload::Success { id, result: typed })
            }
            Response::Error { error, id, .. } => Ok(ResponsePayload::Error { id, error }),
        }
    }

    /// Convert the response into an [`httpd::Response`] with the supplied status.
    pub fn into_http(self, status: StatusCode) -> Result<HttpResponse, HttpError> {
        HttpResponse::new(status).json(&self)
    }
}

/// Typed representation of a JSON-RPC response payload.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponsePayload<T> {
    /// Successful response carrying a typed result.
    Success { id: Option<Value>, result: T },
    /// Error response carrying the RPC error.
    Error { id: Option<Value>, error: RpcError },
}

impl<T> ResponsePayload<T> {
    /// Borrow the identifier associated with this payload, when present.
    pub fn id(&self) -> Option<&Value> {
        match self {
            ResponsePayload::Success { id, .. } | ResponsePayload::Error { id, .. } => id.as_ref(),
        }
    }

    /// Map the success payload into a different type.
    pub fn map<U, F>(self, func: F) -> ResponsePayload<U>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            ResponsePayload::Success { id, result } => ResponsePayload::Success {
                id,
                result: func(result),
            },
            ResponsePayload::Error { id, error } => ResponsePayload::Error { id, error },
        }
    }

    /// Convert the payload into a `Result`, propagating RPC errors.
    pub fn into_result(self) -> Result<T, RpcError> {
        match self {
            ResponsePayload::Success { result, .. } => Ok(result),
            ResponsePayload::Error { error, .. } => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_payload_decodes_success() {
        let response = Response::success(json::Value::from(42u64), Some(json::Value::from(7u64)));
        let payload = response.into_payload::<u64>().expect("decode");
        match payload {
            ResponsePayload::Success { id, result } => {
                assert_eq!(id, Some(json::Value::from(7u64)));
                assert_eq!(result, 42);
            }
            ResponsePayload::Error { .. } => panic!("expected success payload"),
        }
    }

    #[test]
    fn into_payload_preserves_error() {
        let err = RpcError::new(-32000, "kaboom");
        let response = Response::error(err.clone(), None);
        let payload = response.into_payload::<u64>().expect("decode");
        match payload {
            ResponsePayload::Success { .. } => panic!("expected error payload"),
            ResponsePayload::Error { id, error } => {
                assert_eq!(id, None);
                assert_eq!(error, err);
            }
        }
    }

    #[test]
    fn into_payload_reports_decode_errors() {
        let response = Response::success(json::Value::from("not a number"), None);
        let err = response
            .into_payload::<u64>()
            .expect_err("decode should fail");
        match err {
            foundation_serialization::Error::Json(_) => {}
            other => panic!("unexpected error variant: {other:?}"),
        }
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
