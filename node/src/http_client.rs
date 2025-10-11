use http_env::{
    blocking_client as env_blocking_client, http_client as env_http_client,
    try_blocking_client as try_env_blocking_client, try_http_client as try_env_http_client,
};
use httpd::{BlockingClient, HttpClient};

const DEFAULT_BLOCKING_PREFIXES: &[&str] = &["TB_NODE_TLS", "TB_HTTP_TLS"];
const DEFAULT_ASYNC_PREFIXES: &[&str] = &["TB_NODE_TLS", "TB_HTTP_TLS"];

pub fn blocking_client() -> BlockingClient {
    env_blocking_client(DEFAULT_BLOCKING_PREFIXES, "node")
}

pub fn http_client() -> HttpClient {
    env_http_client(DEFAULT_ASYNC_PREFIXES, "node")
}

pub fn try_blocking_client() -> Result<BlockingClient, http_env::ClientTlsError> {
    try_env_blocking_client(DEFAULT_BLOCKING_PREFIXES)
}

pub fn try_http_client() -> Result<HttpClient, http_env::ClientTlsError> {
    try_env_http_client(DEFAULT_ASYNC_PREFIXES)
}
