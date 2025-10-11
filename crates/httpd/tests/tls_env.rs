use base64_fp::encode_standard;
use crypto_suite::signatures::ed25519::{SigningKey, VerifyingKey};
use httpd::{
    HttpClient, Method, Response, Router, ServerConfig, ServerTlsConfig, StatusCode,
    TlsConnectorError, serve_tls, tls_connector_from_env,
};
use runtime::net::TcpListener;
use runtime::{block_on, sleep, spawn};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
use sys::tempfile;

struct EnvScope {
    original: HashMap<String, Option<String>>,
}

impl EnvScope {
    fn new(assignments: &[(&str, Option<&str>)]) -> Self {
        let mut original = HashMap::new();
        for (key, value) in assignments {
            let key = key.to_string();
            original.insert(key.clone(), env::var(&key).ok());
            set_env(&key, *value);
        }
        EnvScope { original }
    }
}

impl Drop for EnvScope {
    fn drop(&mut self) {
        for (key, value) in self.original.drain() {
            set_env(&key, value.as_deref());
        }
    }
}

fn set_env(key: &str, value: Option<&str>) {
    match value {
        Some(val) => unsafe { env::set_var(key, val) },
        None => unsafe { env::remove_var(key) },
    }
}

struct Identity {
    tempdir: tempfile::TempDir,
    cert_path: PathBuf,
    key_path: PathBuf,
    verifying: VerifyingKey,
}

impl Identity {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().expect("identity tempdir");
        let signing = SigningKey::generate(&mut rand::rngs::OsRng::default());
        let verifying = signing.verifying_key();
        let cert_path = tempdir.path().join("identity-cert.json");
        let key_path = tempdir.path().join("identity-key.json");
        fs::write(&cert_path, render_certificate_json(&verifying)).expect("write cert");
        fs::write(&key_path, render_key_json(&signing)).expect("write key");
        Identity {
            tempdir,
            cert_path,
            key_path,
            verifying,
        }
    }

    fn cert_path(&self) -> &Path {
        &self.cert_path
    }

    fn key_path(&self) -> &Path {
        &self.key_path
    }

    fn write_anchor(&self, name: &str) -> PathBuf {
        let path = self.tempdir.path().join(name);
        fs::write(&path, render_certificate_json(&self.verifying)).expect("write anchor");
        path
    }
}

fn render_certificate_json(verifying: &VerifyingKey) -> Vec<u8> {
    let encoded = encode_standard(&verifying.to_bytes());
    format!(
        "{{\"version\":1,\"algorithm\":\"ed25519\",\"public_key\":\"{}\"}}",
        encoded
    )
    .into_bytes()
}

fn render_key_json(signing: &SigningKey) -> Vec<u8> {
    let encoded = encode_standard(&signing.to_bytes());
    format!(
        "{{\"version\":1,\"algorithm\":\"ed25519\",\"private_key\":\"{}\"}}",
        encoded
    )
    .into_bytes()
}

async fn start_tls_server(identity: &Identity) -> (String, runtime::JoinHandle<io::Result<()>>) {
    let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("addr");
    let router = Router::new(()).get("/ping", |_req| async move {
        Ok(Response::new(StatusCode::OK)
            .with_header("content-type", "text/plain")
            .with_body(b"pong".to_vec()))
    });
    let tls = ServerTlsConfig::from_identity_files(identity.cert_path(), identity.key_path())
        .expect("tls config");
    let handle =
        spawn(async move { serve_tls(listener, router, ServerConfig::default(), tls).await });
    (format!("https://{}", addr), handle)
}

#[test]
fn env_tls_client_prefers_first_prefix() {
    block_on(async {
        let server_identity = Identity::new();
        let (base, handle) = start_tls_server(&server_identity).await;
        sleep(Duration::from_millis(50)).await;

        let anchor = server_identity.write_anchor("anchor.json");
        let _guard = EnvScope::new(&[
            ("TB_PRIMARY_TLS_CA", Some(anchor.to_str().unwrap())),
            ("TB_SECONDARY_TLS_CA", None),
            ("TB_HTTP_TLS_CA", None),
            ("TB_HTTP_TLS_CERT", None),
            ("TB_HTTP_TLS_KEY", None),
        ]);

        let client = HttpClient::with_tls_from_env(&["TB_PRIMARY_TLS", "TB_SECONDARY_TLS"])
            .expect("primary tls client");
        let response = client
            .request(Method::Get, &format!("{}/ping", base))
            .expect("request")
            .send()
            .await
            .expect("response");
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.text().expect("text"), "pong");

        handle.abort();
    });
}

#[test]
fn env_tls_client_falls_back_to_secondary_prefix() {
    block_on(async {
        let server_identity = Identity::new();
        let (base, handle) = start_tls_server(&server_identity).await;
        sleep(Duration::from_millis(50)).await;

        let anchor = server_identity.write_anchor("fallback-anchor.json");
        let _guard = EnvScope::new(&[
            ("TB_PRIMARY_TLS_CA", None),
            ("TB_SECONDARY_TLS_CA", Some(anchor.to_str().unwrap())),
            ("TB_HTTP_TLS_CA", None),
            ("TB_HTTP_TLS_CERT", None),
            ("TB_HTTP_TLS_KEY", None),
        ]);

        let client = HttpClient::with_tls_from_env(&["TB_PRIMARY_TLS", "TB_SECONDARY_TLS"])
            .expect("fallback tls client");
        let response = client
            .request(Method::Get, &format!("{}/ping", base))
            .expect("request")
            .send()
            .await
            .expect("response");
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.text().expect("text"), "pong");

        handle.abort();
    });
}

#[test]
fn env_tls_connector_errors_on_half_identity() {
    let identity = Identity::new();
    let _guard = EnvScope::new(&[
        (
            "TB_BROKEN_TLS_CERT",
            Some(identity.cert_path().to_str().unwrap()),
        ),
        ("TB_BROKEN_TLS_KEY", None),
    ]);

    let err =
        tls_connector_from_env("TB_BROKEN_TLS").expect_err("connector should error on missing key");
    assert!(matches!(err, TlsConnectorError::InvalidIdentity(_)));
}

#[test]
fn env_tls_connector_allows_missing_ca_with_identity() {
    block_on(async {
        let server_identity = Identity::new();
        let client_identity = Identity::new();
        let (base, handle) = start_tls_server(&server_identity).await;
        sleep(Duration::from_millis(50)).await;

        let _guard = EnvScope::new(&[
            (
                "TB_NO_CA_TLS_CERT",
                Some(client_identity.cert_path().to_str().unwrap()),
            ),
            (
                "TB_NO_CA_TLS_KEY",
                Some(client_identity.key_path().to_str().unwrap()),
            ),
            ("TB_NO_CA_TLS_CA", None),
        ]);

        let client =
            HttpClient::with_tls_from_env(&["TB_NO_CA_TLS"]).expect("tls client without ca");
        let response = client
            .request(Method::Get, &format!("{}/ping", base))
            .expect("request")
            .send()
            .await
            .expect("response");
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.text().expect("text"), "pong");

        handle.abort();
    });
}
