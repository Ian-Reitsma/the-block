use httpd::{BlockingClient, HttpClient, ServerTlsConfig, TlsConnectorError};
use std::env;
use std::io::{self, ErrorKind};

/// Wrapper around [`BlockingClient::with_tls_from_env`] that falls back to the
/// default client when TLS configuration fails, emitting a consistent log
/// message tagged with the provided `component` label.
pub fn blocking_client(prefixes: &[&str], component: &str) -> BlockingClient {
    match BlockingClient::with_tls_from_env(prefixes) {
        Ok(client) => client,
        Err(err) => {
            eprintln!(
                "{component}: falling back to default blocking HTTP client after TLS configuration error: {err}"
            );
            BlockingClient::default()
        }
    }
}

/// Wrapper around [`HttpClient::with_tls_from_env`] mirroring the behaviour of
/// [`blocking_client`].
pub fn http_client(prefixes: &[&str], component: &str) -> HttpClient {
    match HttpClient::with_tls_from_env(prefixes) {
        Ok(client) => client,
        Err(err) => {
            eprintln!(
                "{component}: falling back to default async HTTP client after TLS configuration error: {err}"
            );
            HttpClient::default()
        }
    }
}

/// Result returned by [`server_tls_from_env`] describing the loaded
/// configuration and whether legacy environment variables were used.
#[derive(Clone)]
pub struct ServerTlsResult {
    pub config: ServerTlsConfig,
    pub source_prefix: String,
    pub legacy_env: bool,
}

impl ServerTlsResult {
    pub fn new(config: ServerTlsConfig, source_prefix: String, legacy_env: bool) -> Self {
        Self {
            config,
            source_prefix,
            legacy_env,
        }
    }
}

/// Attempt to construct a [`ServerTlsConfig`] using the provided environment
/// `prefix`. When no variables are present and `legacy_prefix` is supplied, the
/// loader falls back to the legacy prefix (e.g. `AGGREGATOR` for
/// `TB_AGGREGATOR_TLS`).
///
/// The environment may specify:
/// * `<PREFIX>_CERT` and `<PREFIX>_KEY` – required when any TLS variables for
///   the prefix are provided.
/// * `<PREFIX>_CLIENT_CA` – path to a required-client-auth registry.
/// * `<PREFIX>_CLIENT_CA_OPTIONAL` – path to an optional-client-auth registry.
///
/// Providing both client auth options or only one side of the identity pair
/// produces an `InvalidInput` error.
pub fn server_tls_from_env(
    prefix: &str,
    legacy_prefix: Option<&str>,
) -> io::Result<Option<ServerTlsResult>> {
    if let Some(config) = load_server_tls(prefix)? {
        return Ok(Some(ServerTlsResult::new(
            config,
            prefix.to_string(),
            false,
        )));
    }

    if let Some(legacy) = legacy_prefix {
        if let Some(config) = load_server_tls(legacy)? {
            return Ok(Some(ServerTlsResult::new(config, legacy.to_string(), true)));
        }
    }

    Ok(None)
}

fn load_server_tls(prefix: &str) -> io::Result<Option<ServerTlsConfig>> {
    let cert_var = format!("{prefix}_CERT");
    let key_var = format!("{prefix}_KEY");
    let client_ca_var = format!("{prefix}_CLIENT_CA");
    let client_ca_optional_var = format!("{prefix}_CLIENT_CA_OPTIONAL");

    let cert = env::var(&cert_var).ok();
    let key = env::var(&key_var).ok();
    let client_ca = env::var(&client_ca_var).ok();
    let client_ca_optional = env::var(&client_ca_optional_var).ok();

    if cert.is_none() && key.is_none() && client_ca.is_none() && client_ca_optional.is_none() {
        return Ok(None);
    }

    if cert.is_none() || key.is_none() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("TLS configuration for prefix {prefix} requires both {cert_var} and {key_var}"),
        ));
    }

    if client_ca.is_some() && client_ca_optional.is_some() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "TLS configuration for prefix {prefix} cannot set both {client_ca_var} and {client_ca_optional_var}"
            ),
        ));
    }

    let cert_path = cert.unwrap();
    let key_path = key.unwrap();

    let config = if let Some(ca) = client_ca {
        ServerTlsConfig::from_identity_files_with_client_auth(&cert_path, &key_path, ca)?
    } else if let Some(ca) = client_ca_optional {
        ServerTlsConfig::from_identity_files_with_optional_client_auth(&cert_path, &key_path, ca)?
    } else {
        ServerTlsConfig::from_identity_files(&cert_path, &key_path)?
    };

    Ok(Some(config))
}

/// Convenience re-export for callers that need to bubble the raw error.
pub type ClientTlsError = TlsConnectorError;

/// Build a blocking client without handling fallbacks, returning the raw error
/// so higher layers can customise reporting.
pub fn try_blocking_client(prefixes: &[&str]) -> Result<BlockingClient, TlsConnectorError> {
    BlockingClient::with_tls_from_env(prefixes)
}

/// Build an async client without handling fallbacks, returning the raw error so
/// higher layers can customise reporting.
pub fn try_http_client(prefixes: &[&str]) -> Result<HttpClient, TlsConnectorError> {
    HttpClient::with_tls_from_env(prefixes)
}
