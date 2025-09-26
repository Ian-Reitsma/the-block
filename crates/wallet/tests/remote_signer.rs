use crypto_suite::signatures::ed25519::{Signature, SIGNATURE_LENGTH};
use ledger::crypto::remote_tag;
use serial_test::serial;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tiny_http::{Response, Server};
use tungstenite::Message;
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
        let mut buf = [0u8; 512];
        let _ = tls.read(&mut buf).unwrap();
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
        let tls = rustls::StreamOwned::new(conn, stream);
        let mut ws = tungstenite::accept(tls).unwrap();
        let msg = ws.read().unwrap();
        let txt = match msg {
            Message::Text(t) => t,
            _ => panic!(),
        };
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
        ws.send(Message::Text(serde_json::to_string(&resp).unwrap()))
            .unwrap();
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
