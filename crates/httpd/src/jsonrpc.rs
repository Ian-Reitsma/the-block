use crate::{HttpError, JSON_CODEC, Request, Response, Router, StatusCode};
use codec;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

const JSON_VERSION: &str = "2.0";

type JsonRpcFuture = Pin<Box<dyn Future<Output = Result<Value, JsonRpcError>> + Send + 'static>>;

type JsonRpcHandler<State> = Arc<dyn Fn(JsonRpcRequest<State>) -> JsonRpcFuture + Send + Sync>;

/// Builder that wires JSON-RPC handlers onto the HTTP router.
pub struct JsonRpcRouter<State> {
    methods: Arc<HashMap<String, JsonRpcHandler<State>>>,
}

impl<State> JsonRpcRouter<State>
where
    State: Send + Sync + 'static,
{
    /// Construct an empty JSON-RPC router.
    pub fn new() -> Self {
        Self {
            methods: Arc::new(HashMap::new()),
        }
    }

    /// Register a JSON-RPC method handler.
    pub fn with_method<F, Fut>(self, name: &str, handler: F) -> Self
    where
        F: Fn(JsonRpcRequest<State>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, JsonRpcError>> + Send + 'static,
    {
        let mut methods = (*self.methods).clone();
        methods.insert(
            name.to_string(),
            Arc::new(move |request| {
                let fut = handler(request);
                Box::pin(fut) as JsonRpcFuture
            }),
        );
        Self {
            methods: Arc::new(methods),
        }
    }

    /// Mount the registered methods onto the provided HTTP router at `path`.
    pub fn mount(self, router: Router<State>, path: &str) -> Router<State> {
        let service = Arc::new(JsonRpcService::new(self.methods));
        router.post(path, move |request| {
            let service = service.clone();
            async move { service.handle(request).await }
        })
    }
}

impl<State> Clone for JsonRpcRouter<State> {
    fn clone(&self) -> Self {
        Self {
            methods: self.methods.clone(),
        }
    }
}

struct JsonRpcService<State> {
    methods: Arc<HashMap<String, JsonRpcHandler<State>>>,
    _marker: std::marker::PhantomData<State>,
}

impl<State> JsonRpcService<State>
where
    State: Send + Sync + 'static,
{
    fn new(methods: Arc<HashMap<String, JsonRpcHandler<State>>>) -> Self {
        Self {
            methods,
            _marker: std::marker::PhantomData,
        }
    }

    async fn handle(&self, request: Request<State>) -> Result<Response, HttpError> {
        let context = JsonRpcContext {
            state: request.state().clone(),
            remote_addr: request.remote_addr(),
            headers: request.headers().clone(),
        };
        let body = request.body_bytes();
        if body.is_empty() {
            return self.error_response(None, JsonRpcError::invalid_request());
        }
        let envelope = match codec::deserialize::<JsonRpcEnvelope>(JSON_CODEC, body) {
            Ok(payload) => payload,
            Err(_) => return self.error_response(None, JsonRpcError::parse_error()),
        };
        match envelope {
            JsonRpcEnvelope::Single(call) => self.handle_single(call, &context).await,
            JsonRpcEnvelope::Batch(calls) => self.handle_batch(calls, &context).await,
        }
    }

    async fn handle_single(
        &self,
        call: JsonRpcCall,
        context: &JsonRpcContext<State>,
    ) -> Result<Response, HttpError> {
        match self.dispatch(call, context).await {
            Some(reply) => self.response_for(reply),
            None => Ok(Response::new(StatusCode::NO_CONTENT)),
        }
    }

    async fn handle_batch(
        &self,
        calls: Vec<JsonRpcCall>,
        context: &JsonRpcContext<State>,
    ) -> Result<Response, HttpError> {
        if calls.is_empty() {
            return self.error_response(None, JsonRpcError::invalid_request());
        }
        let mut replies = Vec::new();
        for call in calls {
            if let Some(reply) = self.dispatch(call, context).await {
                replies.push(reply);
            }
        }
        if replies.is_empty() {
            Ok(Response::new(StatusCode::NO_CONTENT))
        } else {
            self.batch_response(replies)
        }
    }

    async fn dispatch(
        &self,
        call: JsonRpcCall,
        context: &JsonRpcContext<State>,
    ) -> Option<JsonRpcReply> {
        let id = call.id.clone();
        let is_notification = id.is_none();
        if call
            .jsonrpc
            .as_deref()
            .map(|value| value != JSON_VERSION)
            .unwrap_or(false)
        {
            return if is_notification {
                None
            } else {
                Some(JsonRpcReply::error(id, JsonRpcError::invalid_request()))
            };
        }
        let handler = match self.methods.get(&call.method) {
            Some(handler) => handler.clone(),
            None => {
                return if is_notification {
                    None
                } else {
                    Some(JsonRpcReply::error(id, JsonRpcError::method_not_found()))
                };
            }
        };
        let request = JsonRpcRequest::new(
            context.state.clone(),
            call.params,
            id.clone(),
            call.badge,
            context.remote_addr,
            context.headers.clone(),
        );
        match handler(request).await {
            Ok(result) => {
                if is_notification {
                    None
                } else {
                    Some(JsonRpcReply::success(id, result))
                }
            }
            Err(err) => {
                if is_notification {
                    None
                } else {
                    Some(JsonRpcReply::error(id, err))
                }
            }
        }
    }

    fn response_for(&self, reply: JsonRpcReply) -> Result<Response, HttpError> {
        let envelope = reply.into_envelope();
        let body = codec::serialize(JSON_CODEC, &envelope)?;
        Ok(Response::new(StatusCode::OK)
            .with_body(body)
            .with_header("content-type", "application/json"))
    }

    fn batch_response(&self, replies: Vec<JsonRpcReply>) -> Result<Response, HttpError> {
        let envelopes: Vec<JsonRpcResponseEnvelope> = replies
            .into_iter()
            .map(JsonRpcReply::into_envelope)
            .collect();
        let body = codec::serialize(JSON_CODEC, &envelopes)?;
        Ok(Response::new(StatusCode::OK)
            .with_body(body)
            .with_header("content-type", "application/json"))
    }

    fn error_response(
        &self,
        id: Option<Value>,
        error: JsonRpcError,
    ) -> Result<Response, HttpError> {
        let identifier = id.or_else(|| Some(Value::Null));
        self.response_for(JsonRpcReply::error(identifier, error))
    }
}

struct JsonRpcContext<State> {
    state: Arc<State>,
    remote_addr: SocketAddr,
    headers: HashMap<String, String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonRpcEnvelope {
    Single(JsonRpcCall),
    Batch(Vec<JsonRpcCall>),
}

#[derive(Deserialize)]
struct JsonRpcCall {
    #[serde(default)]
    jsonrpc: Option<String>,
    method: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    badge: Option<String>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum JsonRpcResponseEnvelope {
    Success(JsonRpcSuccess),
    Error(JsonRpcFailure),
}

#[derive(Serialize)]
struct JsonRpcSuccess {
    jsonrpc: &'static str,
    result: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
}

#[derive(Serialize)]
struct JsonRpcFailure {
    jsonrpc: &'static str,
    error: JsonRpcErrorPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
}

#[derive(Serialize)]
struct JsonRpcErrorPayload {
    code: i32,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

enum JsonRpcReply {
    Success {
        id: Option<Value>,
        result: Value,
    },
    Error {
        id: Option<Value>,
        error: JsonRpcError,
    },
}

impl JsonRpcReply {
    fn success(id: Option<Value>, result: Value) -> Self {
        JsonRpcReply::Success { id, result }
    }

    fn error(id: Option<Value>, error: JsonRpcError) -> Self {
        JsonRpcReply::Error { id, error }
    }

    fn into_envelope(self) -> JsonRpcResponseEnvelope {
        match self {
            JsonRpcReply::Success { id, result } => {
                JsonRpcResponseEnvelope::Success(JsonRpcSuccess {
                    jsonrpc: JSON_VERSION,
                    result,
                    id,
                })
            }
            JsonRpcReply::Error { id, error } => JsonRpcResponseEnvelope::Error(JsonRpcFailure {
                jsonrpc: JSON_VERSION,
                error: error.payload(),
                id,
            }),
        }
    }
}

/// Error type surfaced by JSON-RPC handlers.
#[derive(Clone, Debug)]
pub struct JsonRpcError {
    code: i32,
    message: &'static str,
    data: Option<Value>,
}

impl JsonRpcError {
    pub const fn new(code: i32, message: &'static str) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    pub const fn code(&self) -> i32 {
        self.code
    }

    pub const fn message(&self) -> &'static str {
        self.message
    }

    pub fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }

    pub const fn invalid_request() -> Self {
        Self::new(-32600, "invalid request")
    }

    pub const fn method_not_found() -> Self {
        Self::new(-32601, "method not found")
    }

    pub const fn invalid_params() -> Self {
        Self::new(-32602, "invalid params")
    }

    pub const fn internal_error() -> Self {
        Self::new(-32603, "internal error")
    }

    pub const fn parse_error() -> Self {
        Self::new(-32700, "parse error")
    }

    fn payload(&self) -> JsonRpcErrorPayload {
        JsonRpcErrorPayload {
            code: self.code,
            message: self.message,
            data: self.data.clone(),
        }
    }
}

/// JSON-RPC invocation made available to handlers.
pub struct JsonRpcRequest<State> {
    params: Value,
    id: Option<Value>,
    badge: Option<String>,
    state: Arc<State>,
    remote_addr: SocketAddr,
    headers: HashMap<String, String>,
}

impl<State> JsonRpcRequest<State> {
    fn new(
        state: Arc<State>,
        params: Value,
        id: Option<Value>,
        badge: Option<String>,
        remote_addr: SocketAddr,
        headers: HashMap<String, String>,
    ) -> Self {
        Self {
            params,
            id,
            badge,
            state,
            remote_addr,
            headers,
        }
    }

    pub fn params(&self) -> &Value {
        &self.params
    }

    pub fn params_as<T: DeserializeOwned>(&self) -> Result<T, JsonRpcError> {
        let bytes = codec::serialize(JSON_CODEC, &self.params)
            .map_err(|_| JsonRpcError::invalid_params())?;
        codec::deserialize(JSON_CODEC, &bytes).map_err(|_| JsonRpcError::invalid_params())
    }

    pub fn id(&self) -> Option<&Value> {
        self.id.as_ref()
    }

    pub fn badge(&self) -> Option<&str> {
        self.badge.as_deref()
    }

    pub fn state(&self) -> Arc<State> {
        self.state.clone()
    }

    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|value| value.as_str())
    }
}
