use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use crypto_suite::signatures::ed25519::{Signature, SIGNATURE_LENGTH};
use ledger::crypto::remote_tag;
use serial_test::serial;
use sha1::{Digest, Sha1};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tiny_http::{Response, Server};
use wallet::{remote_signer::RemoteSigner, Wallet, WalletError, WalletSigner};

fn spawn_failing_signer() -> (String, thread::JoinHandle<()>) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", server.server_addr());
    let wallet = Wallet::generate();
    let pk_hex = hex::encode(wallet.public_key().to_bytes());
    let handle = thread::spawn(move || {
        for request in server.incoming_requests() {
            match request.url() {
                "/pubkey" => {
                    let resp = Response::from_string(format!("{{\"pubkey\":\"{}\"}}", pk_hex));
                    let _ = request.respond(resp);
                }
                "/sign" => {
                    let _ = request.respond(Response::empty(500));
                    break;
                }
                _ => {
                    let _ = request.respond(Response::empty(404));
                }
            }
        }
    });
    (addr, handle)
}

fn read_http_request(stream: &mut impl Read) -> std::io::Result<String> {
    let mut buf = Vec::with_capacity(512);
    let mut tmp = [0u8; 128];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 8192 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

fn websocket_accept(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    BASE64.encode(hasher.finalize())
}

fn websocket_handshake(stream: &mut impl Write, request: &str) -> std::io::Result<()> {
    let key = request
        .lines()
        .find_map(|line| {
            if line.to_ascii_lowercase().starts_with("sec-websocket-key:") {
                line.splitn(2, ':').nth(1).map(|v| v.trim().to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "missing websocket key")
        })?;
    let accept = websocket_accept(&key);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream.write_all(response.as_bytes())
}

fn read_ws_payload(stream: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header)?;
    let opcode = header[0] & 0x0F;
    let masked = header[1] & 0x80 != 0;
    if !masked {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "client frames must be masked",
        ));
    }
    if opcode == 0x8 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionAborted,
            "client closed websocket",
        ));
    }
    if opcode != 0x1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unexpected websocket opcode",
        ));
    }
    let mut len = (header[1] & 0x7F) as u64;
    if len == 126 {
        let mut extended = [0u8; 2];
        stream.read_exact(&mut extended)?;
        len = u16::from_be_bytes(extended) as u64;
    } else if len == 127 {
        let mut extended = [0u8; 8];
        stream.read_exact(&mut extended)?;
        len = u64::from_be_bytes(extended);
    }
    let mut mask = [0u8; 4];
    stream.read_exact(&mut mask)?;
    let mut payload = vec![0u8; len as usize];
    if len > 0 {
        stream.read_exact(&mut payload)?;
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[i % 4];
        }
    }
    Ok(payload)
}

fn write_ws_payload(stream: &mut impl Write, payload: &[u8]) -> std::io::Result<()> {
    stream.write_all(&[0x81])?;
    if payload.len() < 126 {
        stream.write_all(&[payload.len() as u8])?;
    } else if payload.len() <= u16::MAX as usize {
        stream.write_all(&[126])?;
        stream.write_all(&(payload.len() as u16).to_be_bytes())?;
    } else {
        stream.write_all(&[127])?;
        stream.write_all(&(payload.len() as u64).to_be_bytes())?;
    }
    stream.write_all(payload)
}

fn spawn_mock_signer() -> (String, thread::JoinHandle<()>) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", server.server_addr());
    let wallet = Wallet::generate();
    let pk_hex = hex::encode(wallet.public_key().to_bytes());
    let handle = thread::spawn(move || {
        for mut request in server.incoming_requests() {
            match request.url() {
                "/pubkey" => {
                    let resp = Response::from_string(format!("{{\"pubkey\":\"{}\"}}", pk_hex));
                    let _ = request.respond(resp);
                }
                "/sign" => {
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                    let msg_hex = v["msg"].as_str().unwrap();
                    let msg = hex::decode(msg_hex).unwrap();
                    let sig = wallet.sign(&msg).unwrap();
                    let resp = Response::from_string(format!(
                        "{{\"sig\":\"{}\"}}",
                        hex::encode(sig.to_bytes())
                    ));
                    let _ = request.respond(resp);
                    break;
                }
                _ => {
                    let _ = request.respond(Response::empty(404));
                }
            }
        }
    });
    (addr, handle)
}

#[test]
#[serial]
fn remote_signer_roundtrip() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let (url, handle) = spawn_mock_signer();
    let signer = RemoteSigner::connect_multi(&vec![url.clone()], 1).expect("connect");
    let msg = b"hello";
    let sig = signer.sign(msg).expect("sign");
    signer.public_key().verify(&remote_tag(msg), &sig).unwrap();
    handle.join().unwrap();
}

#[test]
#[serial]
fn remote_signer_signature_roundtrip_bytes() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let (url, handle) = spawn_mock_signer();
    let signer = RemoteSigner::connect(&url).expect("connect");
    let msg = b"suite-bytes";
    let sig = signer.sign(msg).expect("sign");
    let sig_bytes: [u8; SIGNATURE_LENGTH] = sig.into();
    let sig_roundtrip = Signature::from_bytes(&sig_bytes);
    signer
        .public_key()
        .verify(&remote_tag(msg), &sig_roundtrip)
        .expect("verify");
    handle.join().unwrap();
}

#[test]
#[serial]
fn remote_signer_connect_error() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let res = RemoteSigner::connect("http://127.0.0.1:1");
    assert!(res.is_err());
}

#[test]
#[serial]
fn remote_signer_threshold_error() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let (good, h1) = spawn_mock_signer();
    let (bad, h2) = spawn_failing_signer();
    let signer = RemoteSigner::connect_multi(&vec![good, bad], 2).expect("connect");
    let res = signer.sign_multisig(b"data");
    assert!(res.is_err());
    h1.join().unwrap();
    h2.join().unwrap();
}

fn spawn_invalid_signer() -> (String, thread::JoinHandle<()>) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", server.server_addr());
    let wallet = Wallet::generate();
    let pk_hex = hex::encode(wallet.public_key().to_bytes());
    let handle = thread::spawn(move || {
        for request in server.incoming_requests() {
            match request.url() {
                "/pubkey" => {
                    let resp = Response::from_string(format!("{{\"pubkey\":\"{}\"}}", pk_hex));
                    let _ = request.respond(resp);
                }
                "/sign" => {
                    let resp = Response::from_string("{\"sig\":\"00\"}");
                    let _ = request.respond(resp);
                    break;
                }
                _ => {
                    let _ = request.respond(Response::empty(404));
                }
            }
        }
    });
    (addr, handle)
}

fn spawn_timeout_signer() -> (String, thread::JoinHandle<()>) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", server.server_addr());
    let wallet = Wallet::generate();
    let pk_hex = hex::encode(wallet.public_key().to_bytes());
    let handle = thread::spawn(move || {
        for request in server.incoming_requests() {
            match request.url() {
                "/pubkey" => {
                    let resp = Response::from_string(format!("{{\"pubkey\":\"{}\"}}", pk_hex));
                    let _ = request.respond(resp);
                }
                "/sign" => {
                    std::thread::sleep(Duration::from_secs(2));
                    break;
                }
                _ => {
                    let _ = request.respond(Response::empty(404));
                }
            }
        }
    });
    (addr, handle)
}

#[test]
#[serial]
fn remote_signer_invalid_signature() {
    std::env::remove_var("REMOTE_SIGNER_TIMEOUT_MS");
    let (url, handle) = spawn_invalid_signer();
    let signer = RemoteSigner::connect(&url).expect("connect");
    let res = signer.sign(b"data");
    assert!(res.is_err());
    handle.join().unwrap();
}

#[test]
#[serial]
fn remote_signer_timeout() {
    std::env::set_var("REMOTE_SIGNER_TIMEOUT_MS", "100");
    let (url, handle) = spawn_timeout_signer();
    let signer = RemoteSigner::connect(&url).expect("connect");
    let res = signer.sign(b"data");
    assert!(matches!(res, Err(WalletError::Timeout)));
    handle.join().unwrap();
    std::env::remove_var("REMOTE_SIGNER_TIMEOUT_MS");
}

#[test]
#[serial]
fn remote_signer_mtls_ws() {
    use rcgen::{BasicConstraints, Certificate, CertificateParams, IsCa};
    use rustls::server::AllowAnyAuthenticatedClient;
    use rustls::{Certificate as RustlsCert, PrivateKey, RootCertStore, ServerConfig};

    // Generate CA, server cert, and client cert
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = Certificate::from_params(ca_params).unwrap();

    let server_params = CertificateParams::new(vec!["127.0.0.1".to_string()]);
    let server_cert = Certificate::from_params(server_params).unwrap();
    let server_der = server_cert.serialize_der_with_signer(&ca).unwrap();
    let server_key = server_cert.serialize_private_key_der();

    let client_params = CertificateParams::new(vec!["client".to_string()]);
    let client_cert = Certificate::from_params(client_params).unwrap();

    // Write client cert, key, and CA to temp files and set env vars
    let cert_file = tempfile::NamedTempFile::new().unwrap();
    let key_file = tempfile::NamedTempFile::new().unwrap();
    let ca_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        cert_file.path(),
        client_cert.serialize_pem_with_signer(&ca).unwrap(),
    )
    .unwrap();
    std::fs::write(key_file.path(), client_cert.serialize_private_key_pem()).unwrap();
    std::fs::write(ca_file.path(), ca.serialize_pem().unwrap()).unwrap();
    std::env::set_var("REMOTE_SIGNER_TLS_CERT", cert_file.path());
    std::env::set_var("REMOTE_SIGNER_TLS_KEY", key_file.path());
    std::env::set_var("REMOTE_SIGNER_TLS_CA", ca_file.path());

    // Build server config requiring client cert
    let mut roots = RootCertStore::empty();
    roots.add(&RustlsCert(ca.serialize_der().unwrap())).unwrap();
    let verifier = AllowAnyAuthenticatedClient::new(roots);
    let server_cfg = ServerConfig::builder()
        .with_safe_defaults()
        .with_client_cert_verifier(Arc::new(verifier))
        .with_single_cert(
            vec![RustlsCert(server_der.clone())],
            PrivateKey(server_key.clone()),
        )
        .unwrap();
    let server_cfg = Arc::new(server_cfg);
    let server_cfg_pub = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(vec![RustlsCert(server_der)], PrivateKey(server_key))
        .unwrap();
    let server_cfg_pub = Arc::new(server_cfg_pub);

    // Start TLS WebSocket server
    let wallet = Wallet::generate();
    let pk_hex = hex::encode(wallet.public_key().to_bytes());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let endpoint = format!("wss://{}", addr);

    let handle = thread::spawn(move || {
        // First connection for pubkey
        let (stream, _) = listener.accept().unwrap();
        let conn = rustls::ServerConnection::new(server_cfg_pub.clone()).unwrap();
        let mut tls = rustls::StreamOwned::new(conn, stream);
        let _request = read_http_request(&mut tls).unwrap();
        let body = format!("{{\"pubkey\":\"{}\"}}", pk_hex);
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        tls.write_all(resp.as_bytes()).unwrap();

        // Second connection for signing
        let (stream, _) = listener.accept().unwrap();
        let conn = rustls::ServerConnection::new(server_cfg.clone()).unwrap();
        let mut tls = rustls::StreamOwned::new(conn, stream);
        let request = read_http_request(&mut tls).unwrap();
        websocket_handshake(&mut tls, &request).unwrap();
        let payload = read_ws_payload(&mut tls).unwrap();
        let txt = String::from_utf8(payload).unwrap();
        #[derive(serde::Deserialize)]
        struct Req {
            msg: String,
        }
        let req: Req = serde_json::from_str(&txt).unwrap();
        let data = hex::decode(req.msg).unwrap();
        let sig = wallet.sign(&data).unwrap();
        #[derive(serde::Serialize)]
        struct Resp {
            sig: String,
        }
        let resp = Resp {
            sig: hex::encode(sig.to_bytes()),
        };
        let payload = serde_json::to_vec(&resp).unwrap();
        write_ws_payload(&mut tls, &payload).unwrap();
    });

    // Client signing
    let signer = RemoteSigner::connect(&endpoint).expect("connect");
    let msg = b"mtls";
    let sig = signer.sign(msg).expect("sign");
    signer.public_key().verify(&remote_tag(msg), &sig).unwrap();

    handle.join().unwrap();
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
}

#[test]
#[ignore]
fn external_signer_manual() {
    if let Ok(url) = std::env::var("REMOTE_SIGNER_URL") {
        let signer = RemoteSigner::connect(&url).unwrap();
        let sig = signer.sign(b"ping").unwrap();
        assert_eq!(sig.to_bytes().len(), 64);
    }
}
