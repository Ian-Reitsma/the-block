use httpd::{BlockingClient, HttpClient, ServerTlsConfig, TlsConnectorError};
use std::env;
use std::fmt;
use std::io::{self, ErrorKind};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

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

impl fmt::Debug for ServerTlsResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerTlsResult")
            .field("source_prefix", &self.source_prefix)
            .field("legacy_env", &self.legacy_env)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsEnvWarning {
    pub prefix: String,
    pub code: &'static str,
    pub detail: String,
    pub variables: Vec<String>,
}

impl fmt::Display for TlsEnvWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TLS_ENV_WARNING prefix={} code={} detail=\"{}\"",
            self.prefix, self.code, self.detail
        )?;
        if !self.variables.is_empty() {
            write!(f, " vars={}", self.variables.join(","))?;
        }
        Ok(())
    }
}

enum WarningHandler {
    Diagnostics,
    Stderr,
    Custom(Arc<dyn Fn(&TlsEnvWarning) + Send + Sync + 'static>),
}

impl WarningHandler {
    fn call(&self, warning: &TlsEnvWarning) {
        match self {
            WarningHandler::Diagnostics => log_warning(warning),
            WarningHandler::Stderr => eprintln!("{}", warning),
            WarningHandler::Custom(callback) => callback(warning),
        }
    }
}

impl Clone for WarningHandler {
    fn clone(&self) -> Self {
        match self {
            WarningHandler::Diagnostics => WarningHandler::Diagnostics,
            WarningHandler::Stderr => WarningHandler::Stderr,
            WarningHandler::Custom(callback) => WarningHandler::Custom(callback.clone()),
        }
    }
}

struct WarningCell {
    handler: Mutex<WarningHandler>,
}

static WARNING_CELL: OnceLock<WarningCell> = OnceLock::new();

fn warning_cell() -> &'static WarningCell {
    WARNING_CELL.get_or_init(|| WarningCell {
        handler: Mutex::new(WarningHandler::Diagnostics),
    })
}

pub struct TlsEnvWarningGuard {
    previous: WarningHandler,
}

impl Drop for TlsEnvWarningGuard {
    fn drop(&mut self) {
        if let Ok(mut handler) = warning_cell().handler.lock() {
            *handler = self.previous.clone();
        }
    }
}

fn set_warning_handler(handler: WarningHandler) -> TlsEnvWarningGuard {
    let cell = warning_cell();
    let mut lock = cell.handler.lock().expect("tls warning handler");
    let previous = lock.clone();
    *lock = handler;
    TlsEnvWarningGuard { previous }
}

pub fn install_tls_warning_handler<F>(handler: F) -> TlsEnvWarningGuard
where
    F: Fn(&TlsEnvWarning) + Send + Sync + 'static,
{
    set_warning_handler(WarningHandler::Custom(Arc::new(handler)))
}

pub fn install_tls_warning_observer<F>(observer: F) -> TlsEnvWarningGuard
where
    F: Fn(&TlsEnvWarning) + Send + Sync + 'static,
{
    install_tls_warning_handler(move |warning| {
        observer(warning);
        log_warning(warning);
    })
}

pub fn redirect_tls_warnings_to_stderr() -> TlsEnvWarningGuard {
    set_warning_handler(WarningHandler::Stderr)
}

fn emit_tls_warning(
    prefix: &str,
    code: &'static str,
    detail: impl Into<String>,
    variables: Vec<String>,
) {
    let warning = TlsEnvWarning {
        prefix: prefix.to_string(),
        code,
        detail: detail.into(),
        variables,
    };

    // ensure handler resolution happens outside the mutex to avoid reentrancy concerns
    let handler = {
        let cell = warning_cell();
        cell.handler
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or(WarningHandler::Stderr)
    };

    handler.call(&warning);
}

fn log_warning(warning: &TlsEnvWarning) {
    diagnostics::warn!(
        target: "http_env.tls_env",
        prefix = %warning.prefix,
        code = warning.code,
        detail = %warning.detail,
        variables = ?warning.variables,
        "tls_env_warning"
    );
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
        let mut missing = Vec::new();
        if cert.is_none() {
            missing.push(cert_var.clone());
        }
        if key.is_none() {
            missing.push(key_var.clone());
        }
        let detail = format!(
            "identity requires both {cert_var} and {key_var}; missing {}",
            missing.join(", ")
        );
        emit_tls_warning(
            prefix,
            "missing_identity_component",
            detail.clone(),
            missing.clone(),
        );
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("tls env error [missing_identity_component] for prefix {prefix}: {detail}"),
        ));
    }

    if client_ca.is_some() && client_ca_optional.is_some() {
        let detail = format!("only one of {client_ca_var} or {client_ca_optional_var} may be set");
        emit_tls_warning(
            prefix,
            "conflicting_client_ca",
            detail.clone(),
            vec![client_ca_var.clone(), client_ca_optional_var.clone()],
        );
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("tls env error [conflicting_client_ca] for prefix {prefix}: {detail}"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use base64_fp as base64;
    use crypto_suite::signatures::ed25519::{SigningKey, VerifyingKey};
    use rand::rngs::OsRng;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn write_identity(dir: &PathBuf) -> (PathBuf, PathBuf, VerifyingKey) {
        let mut rng = OsRng::default();
        let signing = SigningKey::generate(&mut rng);
        let verifying = signing.verifying_key();
        let cert_path = dir.join("cert.json");
        let key_path = dir.join("key.json");
        let cert_json = format!(
            "{{\"version\":1,\"algorithm\":\"ed25519\",\"public_key\":\"{}\"}}",
            base64::encode_standard(&verifying.to_bytes())
        );
        let key_json = format!(
            "{{\"version\":1,\"algorithm\":\"ed25519\",\"private_key\":\"{}\"}}",
            base64::encode_standard(&signing.to_bytes())
        );
        fs::write(&cert_path, cert_json).expect("cert write");
        fs::write(&key_path, key_json).expect("key write");
        (cert_path, key_path, verifying)
    }

    fn write_registry(dir: &PathBuf, verifying: &VerifyingKey) -> PathBuf {
        let path = dir.join("registry.json");
        let json = format!(
            "{{\"version\":1,\"allowed\":[{{\"algorithm\":\"ed25519\",\"public_key\":\"{}\"}}]}}",
            base64::encode_standard(&verifying.to_bytes())
        );
        fs::write(&path, json).expect("registry write");
        path
    }

    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let mut dir = std::env::temp_dir();
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let idx = COUNTER.fetch_add(1, Ordering::Relaxed);
        dir.push(format!("http_env_test_{seed}_{idx}"));
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn unset(vars: &[&str]) {
        for var in vars {
            env::remove_var(var);
        }
    }

    #[test]
    fn server_tls_from_env_returns_none_when_empty() {
        let _env = ENV_LOCK.lock().unwrap();
        unset(&["TB_TEST_TLS_CERT", "TB_TEST_TLS_KEY"]);
        let result = server_tls_from_env("TB_TEST_TLS", None).expect("load env");
        assert!(result.is_none());
    }

    #[test]
    fn server_tls_from_env_prefers_primary_prefix() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let (cert, key, _) = write_identity(&dir);
        env::set_var("TB_TEST_TLS_CERT", &cert);
        env::set_var("TB_TEST_TLS_KEY", &key);
        let loaded = server_tls_from_env("TB_TEST_TLS", Some("LEGACY")).expect("load");
        assert!(loaded.is_some());
        let result = loaded.unwrap();
        assert_eq!(result.source_prefix, "TB_TEST_TLS");
        assert!(!result.legacy_env);
        unset(&["TB_TEST_TLS_CERT", "TB_TEST_TLS_KEY"]);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn server_tls_from_env_falls_back_to_legacy_prefix() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let (cert, key, _) = write_identity(&dir);
        env::remove_var("TB_TEST_TLS_CERT");
        env::remove_var("TB_TEST_TLS_KEY");
        env::set_var("LEGACY_CERT", &cert);
        env::set_var("LEGACY_KEY", &key);
        let loaded = server_tls_from_env("TB_TEST_TLS", Some("LEGACY")).expect("load");
        let result = loaded.expect("legacy result");
        assert_eq!(result.source_prefix, "LEGACY");
        assert!(result.legacy_env);
        unset(&["LEGACY_CERT", "LEGACY_KEY"]);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn conflicting_client_ca_variables_emit_warning() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let (cert, key, verifying) = write_identity(&dir);
        let registry = write_registry(&dir, &verifying);
        env::set_var("TB_TEST_TLS_CERT", &cert);
        env::set_var("TB_TEST_TLS_KEY", &key);
        env::set_var("TB_TEST_TLS_CLIENT_CA", &registry);
        env::set_var("TB_TEST_TLS_CLIENT_CA_OPTIONAL", &registry);

        let warnings = Arc::new(Mutex::new(Vec::new()));
        let captured = warnings.clone();
        let _guard = install_tls_warning_handler(move |warning| {
            captured.lock().unwrap().push(warning.clone());
        });

        let err = server_tls_from_env("TB_TEST_TLS", None).expect_err("conflict");
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(format!("{err}").contains("conflicting_client_ca"));

        let collected = warnings.lock().unwrap();
        assert_eq!(collected.len(), 1);
        let warning = &collected[0];
        assert_eq!(warning.code, "conflicting_client_ca");
        assert!(warning
            .variables
            .contains(&"TB_TEST_TLS_CLIENT_CA".to_string()));
        assert!(warning
            .variables
            .contains(&"TB_TEST_TLS_CLIENT_CA_OPTIONAL".to_string()));

        unset(&[
            "TB_TEST_TLS_CERT",
            "TB_TEST_TLS_KEY",
            "TB_TEST_TLS_CLIENT_CA",
            "TB_TEST_TLS_CLIENT_CA_OPTIONAL",
        ]);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn warning_observer_chains_logging() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let (cert, _key, _) = write_identity(&dir);
        env::set_var("TB_TEST_TLS_CERT", &cert);
        env::remove_var("TB_TEST_TLS_KEY");

        let warnings = Arc::new(Mutex::new(Vec::new()));
        let captured = warnings.clone();
        {
            let _guard = install_tls_warning_observer(move |warning| {
                captured.lock().unwrap().push(warning.clone());
            });
            let err = server_tls_from_env("TB_TEST_TLS", None).expect_err("missing key");
            assert_eq!(err.kind(), ErrorKind::InvalidInput);
        }

        let collected = warnings.lock().unwrap();
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].code, "missing_identity_component");

        unset(&["TB_TEST_TLS_CERT"]);
        fs::remove_dir_all(dir).ok();
    }
}
