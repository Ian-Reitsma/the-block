use diagnostics::anyhow::{anyhow, Result};
use httpd::ServerTlsConfig;
use std::env;

fn resolve_env(arg: Option<String>, name: &str) -> Option<String> {
    arg.or_else(|| env::var(name).ok())
}

/// Build a TLS configuration using CLI overrides and environment variables.
pub fn build_tls_config(
    cert_arg: Option<String>,
    key_arg: Option<String>,
    client_ca_arg: Option<String>,
    client_ca_optional_arg: Option<String>,
) -> Result<Option<ServerTlsConfig>> {
    let cert = resolve_env(cert_arg, "TB_GATEWAY_TLS_CERT");
    let key = resolve_env(key_arg, "TB_GATEWAY_TLS_KEY");
    let client_ca = resolve_env(client_ca_arg, "TB_GATEWAY_TLS_CLIENT_CA");
    let client_ca_optional =
        resolve_env(client_ca_optional_arg, "TB_GATEWAY_TLS_CLIENT_CA_OPTIONAL");

    if cert.is_none() && key.is_none() && client_ca.is_none() && client_ca_optional.is_none() {
        return Ok(None);
    }

    let cert = cert.ok_or_else(|| {
        anyhow!("tls identity requires both a certificate and private key; missing certificate")
    })?;
    let key = key.ok_or_else(|| {
        anyhow!("tls identity requires both a certificate and private key; missing private key")
    })?;

    if client_ca.is_some() && client_ca_optional.is_some() {
        return Err(anyhow!(
            "only one of TB_GATEWAY_TLS_CLIENT_CA or TB_GATEWAY_TLS_CLIENT_CA_OPTIONAL may be set"
        ));
    }

    let config = if let Some(ca) = client_ca {
        ServerTlsConfig::from_identity_files_with_client_auth(&cert, &key, ca)?
    } else if let Some(ca) = client_ca_optional {
        ServerTlsConfig::from_identity_files_with_optional_client_auth(&cert, &key, ca)?
    } else {
        ServerTlsConfig::from_identity_files(&cert, &key)?
    };
    Ok(Some(config))
}
