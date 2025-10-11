use http_env::{
    blocking_client as env_blocking_client, try_blocking_client as try_env_blocking_client,
};
use httpd::BlockingClient;

const DEFAULT_PREFIXES: &[&str] = &["TB_RPC_TLS", "TB_HTTP_TLS"];

pub fn blocking_client() -> BlockingClient {
    env_blocking_client(DEFAULT_PREFIXES, "cli")
}

#[allow(dead_code)]
pub fn try_blocking_client() -> Result<BlockingClient, http_env::ClientTlsError> {
    try_env_blocking_client(DEFAULT_PREFIXES)
}
