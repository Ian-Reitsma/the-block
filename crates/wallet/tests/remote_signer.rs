mod support;

use crypto_suite::signatures::ed25519::{Signature, SIGNATURE_LENGTH};
use httpd::{ServerTlsConfig, StatusCode};
use ledger::crypto::remote_tag;
use serial_test::serial;
use std::time::Duration;
use wallet::{remote_signer::RemoteSigner, Wallet, WalletError, WalletSigner};

use support::{HttpSignerMock, TlsWebSocketSignerMock};

#[test]
#[serial]
fn remote_signer_roundtrip() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let server = HttpSignerMock::success();
    let url = server.url().to_string();
    let signer = RemoteSigner::connect_multi(&vec![url.clone()], 1).expect("connect");
    let msg = b"hello";
    let sig = signer.sign(msg).expect("sign");
    signer.public_key().verify(&remote_tag(msg), &sig).unwrap();
}

#[test]
#[serial]
fn remote_signer_signature_roundtrip_bytes() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let server = HttpSignerMock::success();
    let url = server.url().to_string();
    let signer = RemoteSigner::connect(&url).expect("connect");
    let msg = b"suite-bytes";
    let sig = signer.sign(msg).expect("sign");
    let sig_bytes: [u8; SIGNATURE_LENGTH] = sig.into();
    let sig_roundtrip = Signature::from_bytes(&sig_bytes);
    signer
        .public_key()
        .verify(&remote_tag(msg), &sig_roundtrip)
        .expect("verify");
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
    let good = HttpSignerMock::success();
    let bad = HttpSignerMock::failing(StatusCode::INTERNAL_SERVER_ERROR);
    let signer =
        RemoteSigner::connect_multi(&vec![good.url().to_string(), bad.url().to_string()], 2)
            .expect("connect");
    let res = signer.sign_multisig(b"data");
    assert!(res.is_err());
}

#[test]
#[serial]
fn remote_signer_invalid_signature() {
    std::env::remove_var("REMOTE_SIGNER_TIMEOUT_MS");
    let server = HttpSignerMock::invalid_signature();
    let signer = RemoteSigner::connect(server.url()).expect("connect");
    let res = signer.sign(b"data");
    assert!(res.is_err());
}

#[test]
#[serial]
fn remote_signer_timeout() {
    std::env::set_var("REMOTE_SIGNER_TIMEOUT_MS", "100");
    let server = HttpSignerMock::delayed(Duration::from_secs(2));
    let signer = RemoteSigner::connect(server.url()).expect("connect");
    let res = signer.sign(b"data");
    assert!(matches!(res, Err(WalletError::Timeout)));
    std::env::remove_var("REMOTE_SIGNER_TIMEOUT_MS");
}

#[test]
#[serial]
fn remote_signer_mtls_ws() {
    use rcgen::{BasicConstraints, Certificate, CertificateParams, IsCa};

    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = Certificate::from_params(ca_params).unwrap();

    let server_params = CertificateParams::new(vec!["127.0.0.1".to_string()]);
    let server_cert = Certificate::from_params(server_params).unwrap();
    let client_params = CertificateParams::new(vec!["client".to_string()]);
    let client_cert = Certificate::from_params(client_params).unwrap();

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

    let server_cert_pem = server_cert
        .serialize_pem_with_signer(&ca)
        .expect("server cert pem");
    let server_key_pem = server_cert.serialize_private_key_pem();
    let server_cert_file = tempfile::NamedTempFile::new().unwrap();
    let server_key_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(server_cert_file.path(), server_cert_pem).unwrap();
    std::fs::write(server_key_file.path(), server_key_pem).unwrap();
    let tls = ServerTlsConfig::from_pem_files_with_client_auth(
        server_cert_file.path(),
        server_key_file.path(),
        ca_file.path(),
    )
    .unwrap();

    let wallet = Wallet::generate();
    let server = TlsWebSocketSignerMock::new(wallet, tls);
    let endpoint = server.url().to_string();

    let signer = RemoteSigner::connect(&endpoint).expect("connect");
    let msg = b"mtls";
    let sig = signer.sign(msg).expect("sign");
    signer.public_key().verify(&remote_tag(msg), &sig).unwrap();

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
