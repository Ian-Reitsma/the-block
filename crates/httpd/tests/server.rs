use base64_fp;
use crypto_suite::encryption::symmetric::{decrypt_aes256_cbc, encrypt_aes256_cbc};
use crypto_suite::encryption::x25519::{PublicKey as X25519Public, SecretKey as X25519Secret};
use crypto_suite::mac::hmac_sha256;
use crypto_suite::signatures::ed25519::{Signature as Ed25519Signature, SigningKey, VerifyingKey};
use foundation_serialization::json::{self as json_module, Value};
use httpd::{
    Method, Response, Router, ServerConfig, ServerTlsConfig, StatusCode, WebSocketResponse, serve,
    serve_tls,
};
use rand::RngCore;
use rand::rngs::OsRng;
use runtime::net::TcpListener;
use runtime::ws::{self, ClientStream, Message as WsMessage};
use runtime::{block_on, sleep, spawn};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream as StdTcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

const HANDSHAKE_MAGIC: &[u8; 4] = b"TBHS";
const HANDSHAKE_VERSION: u8 = 1;
const AES_BLOCK: usize = 16;
const MAC_LEN: usize = 32;
const SESSION_INFO: &[u8] = b"tb-httpd-session-keys";
const CLIENT_AUTH_INFO: &[u8] = b"tb-httpd-client-auth";
const SECURE_REQUEST: &str = "GET /secure HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";

fn slow_server_config() -> ServerConfig {
    let mut cfg = ServerConfig::default();
    cfg.request_timeout = Duration::from_secs(60);
    cfg
}

struct ServerHelloFrame {
    server_ephemeral: [u8; 32],
    server_nonce: [u8; 32],
    certificate: Vec<u8>,
    signature: Vec<u8>,
    _client_auth_required: bool,
}

struct Identity {
    tempdir: sys::tempfile::TempDir,
    cert_path: PathBuf,
    key_path: PathBuf,
    verifying: VerifyingKey,
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
            verifying,
        }
    }

    fn cert_path(&self) -> &Path {
        &self.cert_path
    }

    fn key_path(&self) -> &Path {
        &self.key_path
    }

    fn verifying(&self) -> &VerifyingKey {
        &self.verifying
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

fn build_server_transcript(
    client_ephemeral: &[u8; 32],
    client_nonce: &[u8; 32],
    server_ephemeral: &[u8; 32],
    server_nonce: &[u8; 32],
) -> Vec<u8> {
    let mut transcript = Vec::with_capacity(32 * 4);
    transcript.extend_from_slice(CLIENT_AUTH_INFO);
    transcript.extend_from_slice(client_ephemeral);
    transcript.extend_from_slice(client_nonce);
    transcript.extend_from_slice(server_ephemeral);
    transcript.extend_from_slice(server_nonce);
    transcript
}

struct SessionKeyMaterial {
    server_write: [u8; 32],
    client_write: [u8; 32],
    server_mac: [u8; 32],
    client_mac: [u8; 32],
}

fn derive_session_keys(
    shared: &[u8; 32],
    client_nonce: &[u8; 32],
    server_nonce: &[u8; 32],
) -> SessionKeyMaterial {
    let mut material = Vec::with_capacity(shared.len() + client_nonce.len() + server_nonce.len());
    material.extend_from_slice(shared);
    material.extend_from_slice(client_nonce);
    material.extend_from_slice(server_nonce);
    let mut out = [0u8; 128];
    crypto_suite::key_derivation::inhouse::derive_key_material(
        None,
        SESSION_INFO,
        &material,
        &mut out,
    );
    let mut server_write = [0u8; 32];
    let mut client_write = [0u8; 32];
    let mut server_mac = [0u8; 32];
    let mut client_mac = [0u8; 32];
    server_write.copy_from_slice(&out[..32]);
    client_write.copy_from_slice(&out[32..64]);
    server_mac.copy_from_slice(&out[64..96]);
    client_mac.copy_from_slice(&out[96..128]);
    SessionKeyMaterial {
        server_write,
        client_write,
        server_mac,
        client_mac,
    }
}

fn encode_client_hello(
    client_ephemeral: &[u8; 32],
    client_nonce: &[u8; 32],
    certificate: Option<&[u8]>,
    signature: Option<&[u8]>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 1 + 32 + 32 + 1);
    out.extend_from_slice(HANDSHAKE_MAGIC);
    out.push(HANDSHAKE_VERSION);
    out.extend_from_slice(client_ephemeral);
    out.extend_from_slice(client_nonce);
    let mut flags = 0u8;
    if certificate.is_some() {
        flags |= 0x01;
    }
    if signature.is_some() {
        flags |= 0x02;
    }
    out.push(flags);
    if let Some(cert) = certificate {
        out.extend_from_slice(&(cert.len() as u32).to_be_bytes());
        out.extend_from_slice(cert);
    }
    if let Some(sig) = signature {
        out.extend_from_slice(&(sig.len() as u32).to_be_bytes());
        out.extend_from_slice(sig);
    }
    out
}

fn decode_server_hello(frame: &[u8]) -> io::Result<ServerHelloFrame> {
    let mut cursor = 0usize;
    if frame.len() < 4 + 1 + 32 + 32 + 4 + 4 + 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "server hello too small",
        ));
    }
    let mut magic = [0u8; 4];
    magic.copy_from_slice(&frame[cursor..cursor + 4]);
    cursor += 4;
    if magic != *HANDSHAKE_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid server hello magic",
        ));
    }
    let version = frame[cursor];
    cursor += 1;
    if version != HANDSHAKE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported server hello version",
        ));
    }
    let mut server_ephemeral = [0u8; 32];
    server_ephemeral.copy_from_slice(&frame[cursor..cursor + 32]);
    cursor += 32;
    let mut server_nonce = [0u8; 32];
    server_nonce.copy_from_slice(&frame[cursor..cursor + 32]);
    cursor += 32;
    let cert_len = read_handshake_len(frame, &mut cursor)?;
    let certificate = read_handshake_bytes(frame, &mut cursor, cert_len, "server certificate")?;
    let sig_len = read_handshake_len(frame, &mut cursor)?;
    let signature = read_handshake_bytes(frame, &mut cursor, sig_len, "server signature")?;
    if cursor >= frame.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing client auth flag",
        ));
    }
    let auth_flag = frame[cursor];
    cursor += 1;
    if cursor != frame.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "extra server hello bytes",
        ));
    }
    Ok(ServerHelloFrame {
        server_ephemeral,
        server_nonce,
        certificate,
        signature,
        _client_auth_required: auth_flag != 0,
    })
}

fn read_handshake_len(frame: &[u8], cursor: &mut usize) -> io::Result<usize> {
    if frame.len() < *cursor + 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated handshake length",
        ));
    }
    let len = u32::from_be_bytes(frame[*cursor..*cursor + 4].try_into().unwrap()) as usize;
    *cursor += 4;
    Ok(len)
}

fn read_handshake_bytes(
    frame: &[u8],
    cursor: &mut usize,
    len: usize,
    label: &str,
) -> io::Result<Vec<u8>> {
    if frame.len() < *cursor + len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("truncated {label}"),
        ));
    }
    let bytes = frame[*cursor..*cursor + len].to_vec();
    *cursor += len;
    Ok(bytes)
}

fn parse_certificate_fields(bytes: &[u8]) -> io::Result<(String, String)> {
    let value: Value = json_module::from_slice(bytes).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid certificate json: {err}"),
        )
    })?;
    let map = match value {
        Value::Object(map) => map,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "certificate must be an object",
            ));
        }
    };
    let algorithm = match map.get("algorithm") {
        Some(Value::String(s)) => s.clone(),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "certificate missing algorithm",
            ));
        }
    };
    let public_key = match map.get("public_key") {
        Some(Value::String(s)) => s.clone(),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "certificate missing public_key",
            ));
        }
    };
    Ok((algorithm, public_key))
}

struct TlsClient {
    stream: StdTcpStream,
    client_write: [u8; 32],
    client_mac: [u8; 32],
    server_write: [u8; 32],
    server_mac: [u8; 32],
    write_seq: u64,
    read_seq: u64,
    pending: Vec<u8>,
}

impl TlsClient {
    fn connect(
        addr: SocketAddr,
        identity: &Identity,
        client_signing: Option<&SigningKey>,
    ) -> io::Result<Self> {
        let mut stream = StdTcpStream::connect(addr)?;
        stream.set_nodelay(true).ok();
        let mut rng = OsRng::default();
        let client_secret = X25519Secret::generate(&mut rng);
        let client_ephemeral = client_secret.public_key().to_bytes();
        let mut client_nonce = [0u8; 32];
        rng.fill_bytes(&mut client_nonce);
        let (certificate, signature) = if let Some(signing) = client_signing {
            let verifying = signing.verifying_key();
            let cert = render_certificate_json(&base64_fp::encode_standard(&verifying.to_bytes()));
            let mut msg = Vec::with_capacity(64);
            msg.extend_from_slice(&client_ephemeral);
            msg.extend_from_slice(&client_nonce);
            let sig = signing.sign(&msg);
            (Some(cert), Some(sig.to_bytes().to_vec()))
        } else {
            (None, None)
        };
        let payload = encode_client_hello(
            &client_ephemeral,
            &client_nonce,
            certificate.as_deref(),
            signature.as_deref(),
        );
        stream.write_all(&(payload.len() as u32).to_be_bytes())?;
        stream.write_all(&payload)?;
        stream.flush()?;

        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf)?;
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        let mut frame = vec![0u8; frame_len];
        stream.read_exact(&mut frame)?;
        let server = decode_server_hello(&frame)?;
        let (algorithm, public_key) = parse_certificate_fields(&server.certificate)?;
        if algorithm != "ed25519" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported certificate algorithm",
            ));
        }
        let decoded = base64_fp::decode_standard(&public_key).expect("decode server key");
        let mut pk = [0u8; 32];
        pk.copy_from_slice(&decoded);
        let server_key = VerifyingKey::from_bytes(&pk).expect("server verifying key");
        assert_eq!(server_key.to_bytes(), identity.verifying().to_bytes());
        let sig_bytes: [u8; 64] = server
            .signature
            .clone()
            .try_into()
            .expect("server signature length");
        let signature = Ed25519Signature::from_bytes(&sig_bytes);
        let transcript = build_server_transcript(
            &client_ephemeral,
            &client_nonce,
            &server.server_ephemeral,
            &server.server_nonce,
        );
        server_key
            .verify(&transcript, &signature)
            .expect("verify server signature");
        let server_public =
            X25519Public::from_bytes(&server.server_ephemeral).expect("server ephemeral");
        let shared = client_secret.diffie_hellman(&server_public).to_bytes();
        let keys = derive_session_keys(&shared, &client_nonce, &server.server_nonce);
        Ok(TlsClient {
            stream,
            client_write: keys.client_write,
            client_mac: keys.client_mac,
            server_write: keys.server_write,
            server_mac: keys.server_mac,
            write_seq: 0,
            read_seq: 0,
            pending: Vec::new(),
        })
    }

    fn write_all(&mut self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            let chunk = buf.len().min(16 * 1024);
            let frame = encrypt_record(
                &self.client_write,
                &self.client_mac,
                self.write_seq,
                &buf[..chunk],
            )?;
            self.stream.write_all(&frame)?;
            self.write_seq = self.write_seq.wrapping_add(1);
            buf = &buf[chunk..];
        }
        self.stream.flush()?;
        Ok(())
    }

    fn read_record(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut header = [0u8; 12];
        match self.stream.read_exact(&mut header) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(err) => return Err(err),
        }
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let padded = ((length / AES_BLOCK) + 1) * AES_BLOCK;
        let mut iv = [0u8; AES_BLOCK];
        self.stream.read_exact(&mut iv)?;
        let mut ciphertext = vec![0u8; padded];
        self.stream.read_exact(&mut ciphertext)?;
        let mut mac = [0u8; MAC_LEN];
        self.stream.read_exact(&mut mac)?;
        let mut frame = Vec::with_capacity(12 + AES_BLOCK + padded + MAC_LEN);
        frame.extend_from_slice(&header);
        frame.extend_from_slice(&iv);
        frame.extend_from_slice(&ciphertext);
        frame.extend_from_slice(&mac);
        let plain = decrypt_record(&self.server_write, &self.server_mac, self.read_seq, &frame)?;
        self.read_seq = self.read_seq.wrapping_add(1);
        Ok(Some(plain))
    }

    fn read_all(&mut self) -> io::Result<Vec<u8>> {
        let mut body = Vec::new();
        if !self.pending.is_empty() {
            let chunk = std::mem::take(&mut self.pending);
            if chunk.is_empty() {
                return Ok(body);
            }
            body.extend_from_slice(&chunk);
        }
        while let Some(chunk) = self.read_record()? {
            if chunk.is_empty() {
                break;
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    fn read_http_response(&mut self) -> io::Result<String> {
        let mut buffer = if self.pending.is_empty() {
            Vec::new()
        } else {
            std::mem::take(&mut self.pending)
        };
        loop {
            if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                let split = pos + 4;
                let tail = buffer.split_off(split);
                self.pending = tail;
                return String::from_utf8(buffer)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err));
            }
            match self.read_record()? {
                Some(chunk) if chunk.is_empty() => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "incomplete http response",
                    ));
                }
                Some(chunk) => buffer.extend_from_slice(&chunk),
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "connection closed before response",
                    ));
                }
            }
        }
    }

    fn request(mut self, request: &str) -> io::Result<String> {
        self.write_all(request.as_bytes())?;
        let bytes = self.read_all()?;
        String::from_utf8(bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
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
            serve(listener, router, slow_server_config())
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

#[testkit::tb_serial]
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
        let (identity, tls_config) = tls_config_no_client_auth();
        let server_config = tls_config.clone();
        let server = spawn(async move {
            serve_tls(listener, router, slow_server_config(), server_config)
                .await
                .expect("serve tls");
        });
        sleep(Duration::from_millis(50)).await;

        let response = perform_tls_request(addr, &identity, None, SECURE_REQUEST);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("secure"));

        server.abort();
    });
}

#[testkit::tb_serial]
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
        let (identity, tls_config, client_key) = tls_config_with_client_auth();
        let server_config = tls_config.clone();
        let server = spawn(async move {
            serve_tls(listener, router, slow_server_config(), server_config)
                .await
                .expect("serve tls");
        });
        sleep(Duration::from_millis(50)).await;

        let response = perform_tls_request(addr, &identity, Some(&client_key), SECURE_REQUEST);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("secure"));

        server.abort();
    });
}

#[testkit::tb_serial]
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
        let (identity, tls_config, client_key) = tls_config_optional_client_auth();
        let server_config = tls_config.clone();
        let server = spawn(async move {
            serve_tls(listener, router, slow_server_config(), server_config)
                .await
                .expect("serve tls");
        });
        sleep(Duration::from_millis(50)).await;

        let response = perform_tls_request(addr, &identity, None, SECURE_REQUEST);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("secure"));

        let response = perform_tls_request(addr, &identity, Some(&client_key), SECURE_REQUEST);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("secure"));

        server.abort();
    });
}

#[testkit::tb_serial]
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
        let (identity, tls_config, _client_key) = tls_config_with_client_auth();
        let server_config = tls_config.clone();
        let server = spawn(async move {
            serve_tls(listener, router, slow_server_config(), server_config)
                .await
                .expect("serve tls");
        });
        sleep(Duration::from_millis(50)).await;

        let result = TlsClient::connect(addr, &identity, None);
        assert!(result.is_err(), "handshake should fail without client cert");

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

#[testkit::tb_serial]
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
            serve(listener, router, slow_server_config())
                .await
                .expect("serve");
        });
        sleep(Duration::from_millis(50)).await;

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

        server.abort();
    });
}

#[testkit::tb_serial]
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
        let (identity, tls_config) = tls_config_no_client_auth();
        let server_config = tls_config.clone();
        let server = spawn(async move {
            serve_tls(listener, router, slow_server_config(), server_config)
                .await
                .expect("serve tls");
        });
        sleep(Duration::from_millis(50)).await;

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
        server.abort();
    });
}

#[testkit::tb_serial]
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
            serve(listener, router, slow_server_config())
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

fn perform_tls_request(
    addr: SocketAddr,
    identity: &Identity,
    client_signing: Option<&SigningKey>,
    request: &str,
) -> String {
    let client = TlsClient::connect(addr, identity, client_signing).expect("tls client");
    client.request(request).expect("tls response")
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

fn encrypt_record(
    key: &[u8; 32],
    mac_key: &[u8; 32],
    sequence: u64,
    plaintext: &[u8],
) -> io::Result<Vec<u8>> {
    let mut rng = OsRng::default();
    let mut iv = [0u8; AES_BLOCK];
    rng.fill_bytes(&mut iv);
    let ciphertext = encrypt_aes256_cbc(key, &iv, plaintext);
    let mut header = Vec::with_capacity(12);
    header.extend_from_slice(&(plaintext.len() as u32).to_be_bytes());
    header.extend_from_slice(&sequence.to_be_bytes());
    let mut mac_input = header.clone();
    mac_input.extend_from_slice(&iv);
    mac_input.extend_from_slice(&ciphertext);
    let mac = hmac_sha256(mac_key, &mac_input);
    let mut out = header;
    out.extend_from_slice(&iv);
    out.extend_from_slice(&ciphertext);
    out.extend_from_slice(&mac);
    Ok(out)
}

fn decrypt_record(
    key: &[u8; 32],
    mac_key: &[u8; 32],
    expected_sequence: u64,
    frame: &[u8],
) -> io::Result<Vec<u8>> {
    if frame.len() < 4 + 8 + AES_BLOCK + MAC_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "record too small",
        ));
    }
    let length = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    let sequence = u64::from_be_bytes(frame[4..12].try_into().unwrap());
    if sequence != expected_sequence {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sequence mismatch",
        ));
    }
    let iv_start = 12;
    let iv_end = iv_start + AES_BLOCK;
    let mac_start = frame.len() - MAC_LEN;
    let mac_input = frame[..mac_start].to_vec();
    let mac = hmac_sha256(mac_key, &mac_input);
    if mac.as_slice() != &frame[mac_start..] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "record mac mismatch",
        ));
    }
    let mut iv = [0u8; AES_BLOCK];
    iv.copy_from_slice(&frame[iv_start..iv_end]);
    let ciphertext = &frame[iv_end..mac_start];
    let plaintext = decrypt_aes256_cbc(key, &iv, ciphertext)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    if plaintext.len() < length {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "plaintext shorter than advertised",
        ));
    }
    Ok(plaintext[..length].to_vec())
}
