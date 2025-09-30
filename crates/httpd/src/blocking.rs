use crate::Method;
use crate::client::{Client, ClientConfig, ClientError, ClientResponse, RequestBuilder};
use runtime::RuntimeHandle;
use serde::Serialize;
use std::time::Duration;

/// Blocking wrapper around the async HTTP client.
#[derive(Clone)]
pub struct BlockingClient {
    inner: Client,
    handle: RuntimeHandle,
}

impl BlockingClient {
    /// Create a new client using the provided configuration knobs.
    pub fn new(config: ClientConfig) -> Self {
        Self {
            inner: Client::new(config),
            handle: runtime::handle(),
        }
    }

    /// Construct a client with the default configuration.
    pub fn default() -> Self {
        Self::new(ClientConfig::default())
    }

    /// Prepare a blocking request to the provided URL using the supplied method.
    pub fn request(
        &self,
        method: Method,
        url: &str,
    ) -> Result<BlockingRequestBuilder<'_>, ClientError> {
        let builder = self.inner.request(method, url)?;
        Ok(BlockingRequestBuilder {
            builder,
            handle: self.handle.clone(),
        })
    }
}

/// Builder that dispatches requests synchronously via the global runtime handle.
pub struct BlockingRequestBuilder<'a> {
    builder: RequestBuilder<'a>,
    handle: RuntimeHandle,
}

impl<'a> BlockingRequestBuilder<'a> {
    /// Attach a header to the outbound request.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.builder = self.builder.header(name, value);
        self
    }

    /// Override the body bytes to send with the request.
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.builder = self.builder.body(body);
        self
    }

    /// Serialize `value` using the canonical JSON configuration.
    pub fn json<T: Serialize>(mut self, value: &T) -> Result<Self, ClientError> {
        self.builder = self.builder.json(value)?;
        Ok(self)
    }

    /// Override the timeout for this request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.builder = self.builder.timeout(timeout);
        self
    }

    /// Execute the request, blocking on the runtime until completion.
    pub fn send(self) -> Result<ClientResponse, ClientError> {
        self.handle.block_on(self.builder.send())
    }
}
