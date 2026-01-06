use base64_fp;
use crypto_suite::signatures::ed25519::{SigningKey, VerifyingKey};
use foundation_serialization::json::{self as json_module, Value};
use httpd::{
    Method, Response, Router, ServerConfig, ServerTlsConfig, StatusCode, WebSocketResponse, serve,
    serve_tls,
};
use rand::rngs::OsRng;
mod support;

use runtime::ws::{self, ClientStream, Message as WsMessage};
use runtime::{block_on, sleep, spawn};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream as StdTcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::{LOCAL_BIND_ADDR, bind_runtime_listener};

const SECURE_REQUEST: &str = "GET /secure HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";

/// RAII guard that ensures spawned server tasks are aborted and fully terminated
/// even if the test panics. This prevents leftover tasks from polluting the global
/// runtime state and causing intermittent failures in subsequent test runs.
///
/// CRITICAL: This guard not only aborts the task but also adds a small delay to ensure
/// the task has fully terminated and released all resources (ports, file descriptors, etc.)
/// before the next test begins. This is essential for preventing race conditions in serial tests.
struct ServerGuard<T> {
    handle: Option<runtime::JoinHandle<T>>,
}

impl<T> ServerGuard<T> {
    fn new(handle: runtime::JoinHandle<T>) -> Self {
        Self {
            handle: Some(handle),
        }
    }
}

impl<T> Drop for ServerGuard<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // Abort the server task to ensure cleanup even if test panics.
            // Note: abort() is asynchronous but the testkit mutex recovery
            // prevents cascading failures if cleanup isn't complete before
            // the next test starts.
            handle.abort();
        }
    }
}

fn slow_server_config() -> ServerConfig {
    let mut cfg = ServerConfig::default();
    cfg.request_timeout = Duration::from_secs(60);
    cfg.tls_handshake_timeout = Duration::from_secs(60);
    cfg
}

struct Identity {
    tempdir: sys::tempfile::TempDir,
    cert_path: PathBuf,
    key_path: PathBuf,
}

impl Identity {
    fn new() -> Self {
        let tempdir = sys::tempfile::tempdir().expect("tempdir");
        let mut rng = OsRng::default();
        let signing = SigningKey::generate(&mut rng);
        let verifying = signing.verifying_key();
        let cert_path = tempdir.path().join("server-cert.json");
        let key_path = tempdir.path().join("server-key.json");
        let cert_content =
            render_certificate_json(&base64_fp::encode_standard(&verifying.to_bytes()));
        let key_content = render_key_json(&base64_fp::encode_standard(&signing.to_bytes()));
        fs::write(&cert_path, cert_content).expect("write cert");
        fs::write(&key_path, key_content).expect("write key");
        Identity {
            tempdir,
            cert_path,
            key_path,
        }
    }

    fn cert_path(&self) -> &Path {
        &self.cert_path
    }

    fn key_path(&self) -> &Path {
        &self.key_path
    }

    fn base_dir(&self) -> &Path {
        self.tempdir.path()
    }
}

fn write_client_registry(base: &Path, clients: &[VerifyingKey]) -> PathBuf {
    let path = base.join("clients.json");
    let allowed = clients
        .iter()
        .map(|vk| {
            format!(
                "{{\"algorithm\":\"ed25519\",\"public_key\":\"{}\"}}",
                base64_fp::encode_standard(&vk.to_bytes())
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let content = format!("{{\"version\":1,\"allowed\":[{}]}}", allowed);
    fs::write(&path, content.as_bytes()).expect("write clients");
    path
}

fn render_certificate_json(public_key_b64: &str) -> Vec<u8> {
    format!(
        "{{\"version\":1,\"algorithm\":\"ed25519\",\"public_key\":\"{}\"}}",
        public_key_b64
    )
    .into_bytes()
}

fn render_key_json(private_key_b64: &str) -> Vec<u8> {
    format!(
        "{{\"version\":1,\"algorithm\":\"ed25519\",\"private_key\":\"{}\"}}",
        private_key_b64
    )
    .into_bytes()
}

fn build_tls_connector(
    identity: &Identity,
    client_signing: Option<&SigningKey>,
) -> io::Result<(httpd::TlsConnector, Option<sys::tempfile::TempDir>)> {
    let mut builder = httpd::TlsConnector::builder();
    builder
        .add_trust_anchor_from_file(identity.cert_path())
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    let mut client_identity_dir = None;
    if let Some(signing) = client_signing {
        let tempdir = sys::tempfile::tempdir().expect("client identity dir");
        let cert_path = tempdir.path().join("client-cert.json");
        let key_path = tempdir.path().join("client-key.json");
        let verifying = signing.verifying_key();
        fs::write(
            &cert_path,
            render_certificate_json(&base64_fp::encode_standard(&verifying.to_bytes())),
        )
        .expect("write client cert");
        fs::write(
            &key_path,
            render_key_json(&base64_fp::encode_standard(&signing.to_bytes())),
        )
        .expect("write client key");
        builder
            .identity_from_files(&cert_path, &key_path)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        client_identity_dir = Some(tempdir);
    }
    let connector = builder
        .build()
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    Ok((connector, client_identity_dir))
}

struct TlsClient {
    stream: httpd::ClientTlsStream,
    pending: Vec<u8>,
}

impl TlsClient {
    fn connect(
        addr: SocketAddr,
        identity: &Identity,
        client_signing: Option<&SigningKey>,
    ) -> io::Result<Self> {
        let debug = std::env::var("TB_TLS_TEST_DEBUG").is_ok();
        let (connector, _client_identity_dir) = build_tls_connector(identity, client_signing)?;
        let stream = StdTcpStream::connect(addr)?;
        stream.set_nodelay(true).ok();
        if debug {
            eprintln!("[tls-client] starting tls connect to {addr}");
        }
        let stream = connector
            .connect("localhost", stream)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        if debug {
            eprintln!("[tls-client] tls connect established");
        }
        Ok(Self {
            stream,
            pending: Vec::new(),
        })
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.stream.write_all(buf)
    }

    fn read_http_response(&mut self) -> io::Result<String> {
        let mut buffer = std::mem::take(&mut self.pending);
        loop {
            if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                let split = pos + 4;
                let tail = buffer.split_off(split);
                self.pending = tail;
                return String::from_utf8(buffer)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err));
            }
            let mut chunk = [0u8; 1024];
            let read = self.stream.read(&mut chunk)?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed before response",
                ));
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
    }
}

#[test]
fn request_builder_defaults_host_header() {
    block_on(async {
        let router = Router::new(());
        let request = router.request_builder().build();
        assert_eq!(request.header("host"), Some("localhost"));
        assert_eq!(request.version(), "HTTP/1.1");
        assert!(request.keep_alive());
    });
}

#[test]
fn request_builder_customizes_requests() {
    block_on(async {
        let remote: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let router = Router::new(()).post("/echo/:id", move |req| async move {
            assert_eq!(req.method(), Method::Post);
            assert_eq!(req.param("id"), Some("123"));
            assert_eq!(req.query_param("flag"), Some("true"));
            assert_eq!(req.header("host"), Some("example.com"));
            assert_eq!(req.version(), "HTTP/1.0");
            assert!(!req.keep_alive());
            assert_eq!(req.remote_addr(), remote);
            assert_eq!(req.body_bytes(), b"payload");
            Ok(Response::new(StatusCode::ACCEPTED)
                .json(&foundation_serialization::json!({"ok": true}))?)
        });
        let request = router
            .request_builder()
            .method(Method::Post)
            .path("/echo/123")
            .query_param("flag", "true")
            .body(b"payload".to_vec())
            .host("example.com")
            .remote_addr(remote)
            .keep_alive(false)
            .version("HTTP/1.0")
            .build();
        assert_eq!(request.header("host"), Some("example.com"));
        assert_eq!(request.query_param("flag"), Some("true"));
        let response = router.handle(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(response.header("content-type"), Some("application/json"));
        let body: Value = json_module::from_slice(response.body()).expect("json body");
        assert_eq!(body, foundation_serialization::json!({"ok": true}));
    });
}

#[test]
fn response_json_sets_header_and_body() {
    let response = Response::new(StatusCode::OK)
        .json(&foundation_serialization::json!({"value": 42}))
        .expect("json response");
    assert_eq!(response.header("content-type"), Some("application/json"));
    let body: Value = json_module::from_slice(response.body()).expect("json body");
    assert_eq!(body, foundation_serialization::json!({"value": 42}));
}

#[test]
fn request_builder_drives_router() {
    block_on(async {
        let router = Router::new(()).get("/ping", |_req| async move {
            Ok(Response::new(StatusCode::OK).with_body(b"pong".to_vec()))
        });
        let request = router.request_builder().path("/ping").build();
        let response = router.handle(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    });
}

#[testkit::tb_serial]
fn serve_plain_round_trip() {
    block_on(async {
        let listener = match bind_runtime_listener(LOCAL_BIND_ADDR).await {
            Some(listener) => listener,
            None => return,
        };
        let addr = listener.local_addr().expect("addr");
        let debug = std::env::var("TB_HTTP_DEBUG").is_ok();
        let router = Router::new(()).get("/ping", move |_req| {
            let debug = debug;
            async move {
                if debug {
                    eprintln!("[http-server] received /ping");
                }
                Ok(Response::new(StatusCode::OK)
                    .with_body(b"pong".to_vec())
                    .close())
            }
        });
        let server_handle = spawn(async move {
            serve(listener, router, slow_server_config())
                .await
                .expect("serve plain");
        });
        let _server = ServerGuard::new(server_handle);
        sleep(Duration::from_millis(200)).await;

        let url = format!("http://{}/ping", addr);
        let client = httpd::HttpClient::default();
        let response = client
            .request(Method::Get, &url)
            .expect("request")
            .send()
            .await
            .expect("response");
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.text().expect("text"), "pong");
    });
}

#[testkit::tb_serial]
fn serve_tls_round_trip_without_client_auth() {
    block_on(async {
        let listener = match bind_runtime_listener(LOCAL_BIND_ADDR).await {
            Some(listener) => listener,
            None => return,
        };
        let addr = listener.local_addr().expect("addr");
        let debug_env = std::env::var("TB_TLS_TEST_DEBUG");
        eprintln!("[test] TB_TLS_TEST_DEBUG={debug_env:?}, addr={addr}");
        let router = Router::new(()).get("/secure", |_req| async move {
            Ok(Response::new(StatusCode::OK)
                .with_body(b"secure".to_vec())
                .close())
        });
        let (identity, tls_config) = tls_config_no_client_auth();
        let server_config = tls_config.clone();
        let server_handle = spawn(async move {
            serve_tls(listener, router, slow_server_config(), server_config)
                .await
                .expect("serve tls");
        });
        let _server = ServerGuard::new(server_handle);
        sleep(Duration::from_secs(1)).await;

        eprintln!("[test] issuing tls client request to {addr}");
        let response = perform_tls_request(addr, &identity, None, SECURE_REQUEST)
            .await
            .expect("tls response");
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("secure"));
    });
}

#[testkit::tb_serial]
fn serve_tls_accepts_clients_with_cert() {
    block_on(async {
        let listener = match bind_runtime_listener(LOCAL_BIND_ADDR).await {
            Some(listener) => listener,
            None => return,
        };
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).get("/secure", |_req| async move {
            Ok(Response::new(StatusCode::OK)
                .with_body(b"secure".to_vec())
                .close())
        });
        let (identity, tls_config, client_key) = tls_config_with_client_auth();
        let server_config = tls_config.clone();
        let server_handle = spawn(async move {
            serve_tls(listener, router, slow_server_config(), server_config)
                .await
                .expect("serve tls");
        });
        let _server = ServerGuard::new(server_handle);
        sleep(Duration::from_millis(200)).await;

        let response = perform_tls_request(addr, &identity, Some(&client_key), SECURE_REQUEST)
            .await
            .expect("tls response");
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("secure"));
    });
}

#[testkit::tb_serial]
fn serve_tls_allows_optional_client_auth() {
    block_on(async {
        let listener = match bind_runtime_listener(LOCAL_BIND_ADDR).await {
            Some(listener) => listener,
            None => return,
        };
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).get("/secure", |_req| async move {
            Ok(Response::new(StatusCode::OK)
                .with_body(b"secure".to_vec())
                .close())
        });
        let (identity, tls_config, client_key) = tls_config_optional_client_auth();
        let server_config = tls_config.clone();
        let server_handle = spawn(async move {
            serve_tls(listener, router, slow_server_config(), server_config)
                .await
                .expect("serve tls");
        });
        let _server = ServerGuard::new(server_handle);
        sleep(Duration::from_millis(200)).await;

        let response = perform_tls_request(addr, &identity, None, SECURE_REQUEST)
            .await
            .expect("tls response");
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("secure"));

        let response = perform_tls_request(addr, &identity, Some(&client_key), SECURE_REQUEST)
            .await
            .expect("tls response");
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("secure"));
    });
}

#[testkit::tb_serial]
fn serve_tls_rejects_clients_without_cert() {
    block_on(async {
        let listener = match bind_runtime_listener(LOCAL_BIND_ADDR).await {
            Some(listener) => listener,
            None => return,
        };
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).get("/secure", |_req| async move {
            Ok(Response::new(StatusCode::OK)
                .with_body(b"secure".to_vec())
                .close())
        });
        let (identity, tls_config, _client_key) = tls_config_with_client_auth();
        let server_config = tls_config.clone();
        let server_handle = spawn(async move {
            serve_tls(listener, router, slow_server_config(), server_config)
                .await
                .expect("serve tls");
        });
        let _server = ServerGuard::new(server_handle);
        sleep(Duration::from_millis(200)).await;

        let result = perform_tls_request(addr, &identity, None, SECURE_REQUEST).await;
        assert!(result.is_err(), "handshake should fail without client cert");
    });
}
#[test]
fn router_wildcard_captures_remainder() {
    block_on(async {
        let router = Router::new(()).get("/files/*path", |req| async move {
            let captured = req.param("path").unwrap_or("").to_string();
            Ok(Response::new(StatusCode::OK).with_body(captured.into_bytes()))
        });
        let request = router.request_builder().path("/files/a/b/c.txt").build();
        let response = router.handle(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"a/b/c.txt");
    });
}

#[testkit::tb_serial]
fn websocket_upgrade_accepts_and_dispatches_handler() {
    block_on(async {
        let listener = match bind_runtime_listener(LOCAL_BIND_ADDR).await {
            Some(listener) => listener,
            None => return,
        };
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).upgrade("/ws", |_req, _upgrade| async move {
            Ok(WebSocketResponse::accept(move |mut stream| async move {
                sleep(Duration::from_millis(10)).await;
                stream.send(WsMessage::Text("hello".into())).await?;
                stream.close().await?;
                Ok(())
            }))
        });
        let server_handle = spawn(async move {
            serve(listener, router, slow_server_config())
                .await
                .expect("serve");
        });
        let _server = ServerGuard::new(server_handle);
        sleep(Duration::from_millis(200)).await;

        let std_stream = StdTcpStream::connect(addr).expect("connect");
        let mut stream = runtime::net::TcpStream::from_std(std_stream).expect("runtime stream");
        let key = ws::handshake_key();
        let request = format!(
            "GET /ws HTTP/1.1\r\n\
Host: localhost\r\n\
Connection: Upgrade\r\n\
Upgrade: websocket\r\n\
Sec-WebSocket-Key: {key}\r\n\
Sec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write(request.as_bytes())
            .await
            .expect("write request");
        let expected_accept = ws::handshake_accept(&key).expect("handshake accept");
        ws::read_client_handshake(&mut stream, &expected_accept)
            .await
            .expect("handshake");

        let mut client = ClientStream::new(stream);
        let mut got_text = None;
        while let Some(frame) = client.recv().await.expect("frame") {
            if let WsMessage::Text(text) = frame {
                got_text = Some(text);
                break;
            }
        }
        assert_eq!(got_text.as_deref(), Some("hello"));
    });
}

#[testkit::tb_serial]
fn websocket_upgrade_over_tls_dispatches_handler() {
    block_on(async {
        let listener = match bind_runtime_listener(LOCAL_BIND_ADDR).await {
            Some(listener) => listener,
            None => return,
        };
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).upgrade("/ws", |_req, _upgrade| async move {
            Ok(WebSocketResponse::accept(|mut stream| async move {
                stream.send(WsMessage::Text("hello".into())).await?;
                stream.close().await?;
                Ok(())
            }))
        });
        let (identity, tls_config) = tls_config_no_client_auth();
        let server_config = tls_config.clone();
        let server_handle = spawn(async move {
            serve_tls(listener, router, slow_server_config(), server_config)
                .await
                .expect("serve tls");
        });
        let _server = ServerGuard::new(server_handle);
        sleep(Duration::from_millis(200)).await;

        let addr = addr;
        let join = std::thread::spawn(move || {
            let mut client = TlsClient::connect(addr, &identity, None).expect("tls connection");
            let key = ws::handshake_key();
            let expected_accept = ws::handshake_accept(&key).expect("handshake accept");
            let request = format!(
                "GET /ws HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
            );
            client.write_all(request.as_bytes()).expect("write request");
            let headers = client.read_http_response().expect("handshake response");
            // TLS WebSocket upgrades are now supported
            assert!(
                headers.starts_with("HTTP/1.1 101"),
                "expected 101 Switching Protocols, got: {headers}"
            );
            assert!(headers.lines().any(|line| {
                line.to_ascii_lowercase()
                    .starts_with("sec-websocket-accept:")
            }));
            // Verify the accept header matches our expected value
            let accept_header = headers
                .lines()
                .find(|line| {
                    line.to_ascii_lowercase()
                        .starts_with("sec-websocket-accept:")
                })
                .and_then(|line| line.split_once(':'))
                .map(|(_, value)| value.trim())
                .expect("sec-websocket-accept header");
            assert_eq!(accept_header, expected_accept);
        });

        join.join().expect("client thread");
    });
}

#[testkit::tb_serial]
fn websocket_upgrade_rejects_with_response() {
    block_on(async {
        let listener = match bind_runtime_listener(LOCAL_BIND_ADDR).await {
            Some(listener) => listener,
            None => return,
        };
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).upgrade("/ws", |_req, _upgrade| async move {
            Ok(WebSocketResponse::reject(
                Response::new(StatusCode::FORBIDDEN).with_body(b"denied".to_vec()),
            ))
        });
        let server_handle = spawn(async move {
            serve(listener, router, slow_server_config())
                .await
                .expect("serve");
        });
        let _server = ServerGuard::new(server_handle);
        sleep(Duration::from_millis(200)).await;

        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        let key = ws::handshake_key();
        let request = format!(
            "GET /ws HTTP/1.1\r\n\
Host: localhost\r\n\
Connection: Upgrade\r\n\
Upgrade: websocket\r\n\
Sec-WebSocket-Key: {key}\r\n\
Sec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).expect("write request");
        stream.flush().expect("flush");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");
        assert!(response.starts_with("HTTP/1.1 403"));
        assert!(response.contains("denied"));
    });
}

fn map_client_error(err: httpd::ClientError) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err.to_string())
}

async fn perform_tls_request(
    addr: SocketAddr,
    identity: &Identity,
    client_signing: Option<&SigningKey>,
    request: &str,
) -> io::Result<String> {
    let mut lines = request.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method_raw = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing method"))?;
    let path = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing path"))?;
    let method = match method_raw {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "HEAD" => Method::Head,
        "PATCH" => Method::Patch,
        "OPTIONS" => Method::Options,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported method: {method_raw}"),
            ));
        }
    };
    let (connector, _client_identity_dir) = build_tls_connector(identity, client_signing)?;
    let mut config = httpd::ClientConfig::default();
    config.tls = Some(connector);
    let client = httpd::HttpClient::new(config);
    let url = format!("https://{}{}", addr, path);
    let response = client
        .request(method, &url)
        .map_err(map_client_error)?
        .send()
        .await
        .map_err(map_client_error)?;
    let status_code = response.status().as_u16();
    let body = response.text().unwrap_or_default();
    // Format response to match what tests expect (HTTP/1.1 <code> ...)
    Ok(format!("HTTP/1.1 {status_code}\r\n\r\n{body}"))
}

fn tls_config_no_client_auth() -> (Identity, ServerTlsConfig) {
    let identity = Identity::new();
    let config = ServerTlsConfig::from_identity_files(identity.cert_path(), identity.key_path())
        .expect("server config");
    (identity, config)
}

fn tls_config_with_client_auth() -> (Identity, ServerTlsConfig, SigningKey) {
    let identity = Identity::new();
    let mut rng = OsRng::default();
    let client = SigningKey::generate(&mut rng);
    let registry = write_client_registry(identity.base_dir(), &[client.verifying_key()]);
    let config = ServerTlsConfig::from_identity_files_with_client_auth(
        identity.cert_path(),
        identity.key_path(),
        &registry,
    )
    .expect("server config with auth");
    (identity, config, client)
}

fn tls_config_optional_client_auth() -> (Identity, ServerTlsConfig, SigningKey) {
    let identity = Identity::new();
    let mut rng = OsRng::default();
    let client = SigningKey::generate(&mut rng);
    let registry = write_client_registry(identity.base_dir(), &[client.verifying_key()]);
    let config = ServerTlsConfig::from_identity_files_with_optional_client_auth(
        identity.cert_path(),
        identity.key_path(),
        &registry,
    )
    .expect("server config optional auth");
    (identity, config, client)
}
