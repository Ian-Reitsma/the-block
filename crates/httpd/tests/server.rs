use httpd::{
    Method, Response, Router, ServerConfig, ServerTlsConfig, StatusCode, WebSocketResponse, serve,
    serve_tls,
};
use runtime::net::TcpListener;
use runtime::ws::{self, ClientStream, Message as WsMessage};
use runtime::{block_on, sleep, spawn};
use rustls::client::ClientConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConnection, RootCertStore};
use rustls_pemfile::{certs, pkcs8_private_keys, rsa_private_keys};
use serde_json::json;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream as StdTcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls")
}

fn load_certs(path: &Path) -> Vec<CertificateDer<'static>> {
    let mut reader = std::io::BufReader::new(File::open(path).expect("open cert"));
    certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .expect("read certs")
}

fn load_private_key(path: &Path) -> PrivateKeyDer<'static> {
    let mut reader = std::io::BufReader::new(File::open(path).expect("open key"));
    if let Some(key) = pkcs8_private_keys(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .expect("read pkcs8 keys")
        .into_iter()
        .next()
    {
        return PrivateKeyDer::from(key);
    }
    let mut reader = std::io::BufReader::new(File::open(path).expect("open rsa key"));
    rsa_private_keys(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .expect("read rsa keys")
        .into_iter()
        .next()
        .map(PrivateKeyDer::from)
        .expect("missing private key")
}

fn tls_config_with_client_auth() -> ServerTlsConfig {
    let fixtures = fixtures_dir();
    ServerTlsConfig::from_pem_files_with_client_auth(
        fixtures.join("server.pem"),
        fixtures.join("server-key.pem"),
        fixtures.join("ca.pem"),
    )
    .expect("tls config")
}

fn tls_config_no_client_auth() -> ServerTlsConfig {
    let fixtures = fixtures_dir();
    ServerTlsConfig::from_pem_files(fixtures.join("server.pem"), fixtures.join("server-key.pem"))
        .expect("tls config")
}

fn tls_config_optional_client_auth() -> ServerTlsConfig {
    let fixtures = fixtures_dir();
    ServerTlsConfig::from_pem_files_with_optional_client_auth(
        fixtures.join("server.pem"),
        fixtures.join("server-key.pem"),
        fixtures.join("ca.pem"),
    )
    .expect("tls config")
}

fn client_config_without_cert() -> Arc<ClientConfig> {
    let fixtures = fixtures_dir();
    let ca_cert = load_certs(&fixtures.join("ca.pem"))
        .into_iter()
        .next()
        .expect("ca cert");
    let mut roots = RootCertStore::empty();
    roots.add(ca_cert).expect("add root");
    Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

fn client_config_with_cert() -> Arc<ClientConfig> {
    let fixtures = fixtures_dir();
    let ca_cert = load_certs(&fixtures.join("ca.pem"))
        .into_iter()
        .next()
        .expect("ca cert");
    let mut roots = RootCertStore::empty();
    roots.add(ca_cert).expect("add root");
    let client_chain = load_certs(&fixtures.join("client.pem"));
    let client_key = load_private_key(&fixtures.join("client-key.pem"));
    Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_client_auth_cert(client_chain, client_key)
            .expect("client config"),
    )
}

const SECURE_REQUEST: &str = "GET /secure HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";

fn perform_tls_request(addr: SocketAddr, client_cfg: Arc<ClientConfig>, request: &str) -> String {
    let server_name = ServerName::try_from("localhost").expect("server name");
    let stream = StdTcpStream::connect(addr).expect("connect");
    let mut tls = ClientConnection::new(client_cfg, server_name)
        .map(|conn| rustls::StreamOwned::new(conn, stream))
        .expect("tls connection");
    while tls.conn.is_handshaking() {
        tls.conn
            .complete_io(&mut tls.sock)
            .expect("complete handshake");
    }
    write!(tls, "{}", request).expect("write request");
    tls.flush().expect("flush");
    let mut response = String::new();
    tls.read_to_string(&mut response).expect("read response");
    response
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
            Ok(Response::new(StatusCode::ACCEPTED).json(&json!({"ok": true}))?)
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
        let body: serde_json::Value = serde_json::from_slice(response.body()).expect("json body");
        assert_eq!(body, json!({"ok": true}));
    });
}

#[test]
fn response_json_sets_header_and_body() {
    let response = Response::new(StatusCode::OK)
        .json(&json!({"value": 42}))
        .expect("json response");
    assert_eq!(response.header("content-type"), Some("application/json"));
    let body: serde_json::Value = serde_json::from_slice(response.body()).expect("json body");
    assert_eq!(body, json!({"value": 42}));
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

#[test]
fn serve_plain_round_trip() {
    block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind plain listener");
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).get("/ping", |_req| async move {
            Ok(Response::new(StatusCode::OK)
                .with_body(b"pong".to_vec())
                .close())
        });
        let server = spawn(async move {
            serve(listener, router, ServerConfig::default())
                .await
                .expect("serve plain");
        });
        sleep(Duration::from_millis(50)).await;

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

        server.abort();
    });
}

#[test]
fn serve_tls_round_trip_without_client_auth() {
    block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind tls listener");
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).get("/secure", |_req| async move {
            Ok(Response::new(StatusCode::OK)
                .with_body(b"secure".to_vec())
                .close())
        });
        let tls_config = tls_config_no_client_auth();
        let server = spawn(async move {
            serve_tls(listener, router, ServerConfig::default(), tls_config)
                .await
                .expect("serve tls");
        });
        sleep(Duration::from_millis(50)).await;

        let client_cfg = client_config_without_cert();
        let response = perform_tls_request(addr, client_cfg, SECURE_REQUEST);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("secure"));

        server.abort();
    });
}

#[test]
fn serve_tls_accepts_clients_with_cert() {
    block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind tls listener");
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).get("/secure", |_req| async move {
            Ok(Response::new(StatusCode::OK)
                .with_body(b"secure".to_vec())
                .close())
        });
        let tls_config = tls_config_with_client_auth();
        let server = spawn(async move {
            serve_tls(listener, router, ServerConfig::default(), tls_config)
                .await
                .expect("serve tls");
        });
        sleep(Duration::from_millis(50)).await;

        let client_cfg = client_config_with_cert();
        let response = perform_tls_request(addr, client_cfg, SECURE_REQUEST);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("secure"));

        server.abort();
    });
}

#[test]
fn serve_tls_allows_optional_client_auth() {
    block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind tls listener");
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).get("/secure", |_req| async move {
            Ok(Response::new(StatusCode::OK)
                .with_body(b"secure".to_vec())
                .close())
        });
        let tls_config = tls_config_optional_client_auth();
        let server = spawn(async move {
            serve_tls(listener, router, ServerConfig::default(), tls_config)
                .await
                .expect("serve tls");
        });
        sleep(Duration::from_millis(50)).await;

        let client_cfg = client_config_without_cert();
        let response = perform_tls_request(addr, client_cfg, SECURE_REQUEST);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("secure"));

        server.abort();
    });
}

#[test]
fn serve_tls_rejects_clients_without_cert() {
    block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind tls listener");
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).get("/secure", |_req| async move {
            Ok(Response::new(StatusCode::OK)
                .with_body(b"secure".to_vec())
                .close())
        });
        let tls_config = tls_config_with_client_auth();
        let server = spawn(async move {
            serve_tls(listener, router, ServerConfig::default(), tls_config)
                .await
                .expect("serve tls");
        });
        sleep(Duration::from_millis(50)).await;

        let client_cfg = client_config_without_cert();
        let server_name = ServerName::try_from("localhost").expect("server name");
        let mut stream = StdTcpStream::connect(addr).expect("connect");
        let mut connection = ClientConnection::new(client_cfg, server_name).expect("client conn");
        let mut failed = false;
        for _ in 0..8 {
            match connection.complete_io(&mut stream) {
                Ok((read, written)) => {
                    if !connection.is_handshaking() {
                        break;
                    }
                    if read == 0 && written == 0 {
                        failed = true;
                        break;
                    }
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        if !failed {
            let mut tls = rustls::StreamOwned::new(connection, stream);
            let _ = tls.write(b"GET /secure HTTP/1.1\r\nHost: localhost\r\n\r\n");
            let mut buf = [0u8; 1];
            match tls.read(&mut buf) {
                Ok(_) => panic!("handshake unexpectedly succeeded without client cert"),
                Err(_) => {
                    failed = true;
                }
            }
        }
        assert!(failed, "handshake should fail without client cert");

        server.abort();
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

#[test]
fn websocket_upgrade_accepts_and_dispatches_handler() {
    block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).upgrade("/ws", |_req, _upgrade| async move {
            Ok(WebSocketResponse::accept(move |mut stream| async move {
                sleep(Duration::from_millis(10)).await;
                stream.send(WsMessage::Text("hello".into())).await?;
                stream.close().await?;
                Ok(())
            }))
        });
        let server = spawn(async move {
            serve(listener, router, ServerConfig::default())
                .await
                .expect("serve");
        });
        sleep(Duration::from_millis(50)).await;

        let mut stream = runtime::net::TcpStream::connect(addr)
            .await
            .expect("connect");
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
        match client.recv().await.expect("frame") {
            Some(WsMessage::Text(text)) => assert_eq!(text, "hello"),
            other => panic!("unexpected frame: {:?}", other),
        }

        server.abort();
    });
}

#[test]
fn websocket_upgrade_over_tls_dispatches_handler() {
    block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).upgrade("/ws", |_req, _upgrade| async move {
            Ok(WebSocketResponse::accept(|mut stream| async move {
                stream.send(WsMessage::Text("hello".into())).await?;
                stream.close().await?;
                Ok(())
            }))
        });
        let tls_config = tls_config_no_client_auth();
        let server = spawn(async move {
            serve_tls(listener, router, ServerConfig::default(), tls_config)
                .await
                .expect("serve tls");
        });
        sleep(Duration::from_millis(50)).await;

        let addr = addr;
        let join = std::thread::spawn(move || {
            let client_cfg = client_config_without_cert();
            let server_name = ServerName::try_from("localhost").expect("server name");
            let stream = StdTcpStream::connect(addr).expect("connect");
            let mut tls = ClientConnection::new(client_cfg, server_name)
                .map(|conn| rustls::StreamOwned::new(conn, stream))
                .expect("tls connection");
            while tls.conn.is_handshaking() {
                tls.conn
                    .complete_io(&mut tls.sock)
                    .expect("complete handshake");
            }
            let key = ws::handshake_key();
            let request = format!(
                "GET /ws HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
            );
            tls.write_all(request.as_bytes()).expect("write request");
            tls.flush().expect("flush");

            let mut buf = [0u8; 64];
            let mut response = Vec::new();
            while !response.windows(4).any(|w| w == b"\r\n\r\n") {
                let read = tls.read(&mut buf).expect("read handshake");
                assert!(read > 0, "unexpected eof during handshake");
                response.extend_from_slice(&buf[..read]);
            }
            let header_end = response
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .expect("handshake terminator")
                + 4;
            let headers = std::str::from_utf8(&response[..header_end]).expect("utf8 response");
            assert!(headers.starts_with("HTTP/1.1 101"));
            let accept = ws::handshake_accept(&key).expect("handshake accept");
            assert!(headers.lines().any(|line| {
                line.trim()
                    .eq_ignore_ascii_case(&format!("Sec-WebSocket-Accept: {accept}"))
            }));

            let mut frame_bytes = response[header_end..].to_vec();
            while frame_bytes.len() < 2 {
                let read = tls.read(&mut buf).expect("read frame header");
                assert!(read > 0, "unexpected eof before frame header");
                frame_bytes.extend_from_slice(&buf[..read]);
            }
            let len = (frame_bytes[1] & 0x7F) as usize;
            while frame_bytes.len() < 2 + len {
                let read = tls.read(&mut buf).expect("read frame payload");
                assert!(read > 0, "unexpected eof before frame payload");
                frame_bytes.extend_from_slice(&buf[..read]);
            }
            assert_eq!(frame_bytes[0], 0x81, "opcode");
            let payload = &frame_bytes[2..2 + len];
            assert_eq!(payload, b"hello");
        });

        join.join().expect("client thread");
        server.abort();
    });
}

#[test]
fn websocket_upgrade_rejects_with_response() {
    block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("addr");
        let router = Router::new(()).upgrade("/ws", |_req, _upgrade| async move {
            Ok(WebSocketResponse::reject(
                Response::new(StatusCode::FORBIDDEN).with_body(b"denied".to_vec()),
            ))
        });
        let server = spawn(async move {
            serve(listener, router, ServerConfig::default())
                .await
                .expect("serve");
        });
        sleep(Duration::from_millis(50)).await;

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

        server.abort();
    });
}
