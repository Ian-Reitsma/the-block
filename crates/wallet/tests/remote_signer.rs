mod support;

use crypto_suite::signatures::ed25519::{Signature, SigningKey, SIGNATURE_LENGTH};
use httpd::{ServerTlsConfig, StatusCode};
use ledger::crypto::remote_tag;
use std::time::Duration;
use sys::tempfile::NamedTempFile;
use wallet::{remote_signer::RemoteSigner, Wallet, WalletError, WalletSigner};

use support::{HttpSignerMock, TlsWebSocketSignerMock};

#[test]
#[testkit::tb_serial]
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
#[testkit::tb_serial]
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
#[testkit::tb_serial]
fn remote_signer_connect_error() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let res = RemoteSigner::connect("http://127.0.0.1:1");
    assert!(res.is_err());
}

#[test]
#[testkit::tb_serial]
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
#[testkit::tb_serial]
fn remote_signer_invalid_signature() {
    std::env::remove_var("REMOTE_SIGNER_TIMEOUT_MS");
    let server = HttpSignerMock::invalid_signature();
    let signer = RemoteSigner::connect(server.url()).expect("connect");
    let res = signer.sign(b"data");
    assert!(res.is_err());
}

#[test]
#[testkit::tb_serial]
fn remote_signer_timeout() {
    std::env::set_var("REMOTE_SIGNER_TIMEOUT_MS", "100");
    let server = HttpSignerMock::delayed(Duration::from_secs(2));
    let signer = RemoteSigner::connect(server.url()).expect("connect");
    let res = signer.sign(b"data");
    assert!(matches!(res, Err(WalletError::Timeout)));
    std::env::remove_var("REMOTE_SIGNER_TIMEOUT_MS");
}

#[test]
#[testkit::tb_serial]
fn remote_signer_mtls_ws() {
    use base64_fp::encode_standard;
    use foundation_time::{Duration as TimeDuration, UtcDateTime};
    use foundation_tls::{sign_with_ca_ed25519, SelfSignedCertParams};
    use rand::rngs::OsRng;
    use rand::RngCore;

    fn der_to_pem(label: &str, der: &[u8]) -> String {
        let mut pem = String::new();
        pem.push_str(&format!("-----BEGIN {label}-----\n"));
        let encoded = encode_standard(der);
        for chunk in encoded.as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(chunk).unwrap());
            pem.push('\n');
        }
        pem.push_str(&format!("-----END {label}-----\n"));
        pem
    }

    fn random_serial() -> [u8; 16] {
        let mut serial = [0u8; 16];
        OsRng::default().fill_bytes(&mut serial);
        serial[0] &= 0x7F;
        serial
    }

    let mut rng = OsRng::default();
    let ca_key = SigningKey::generate(&mut rng);
    let ca_params = SelfSignedCertParams::builder()
        .subject_cn("wallet-test-ca")
        .validity(
            UtcDateTime::now() - TimeDuration::hours(1),
            UtcDateTime::now() + TimeDuration::days(7),
        )
        .serial(random_serial())
        .ca(true)
        .build()
        .unwrap();
    let cert_file = NamedTempFile::new().unwrap();
    let key_file = NamedTempFile::new().unwrap();
    let client_key = SigningKey::generate(&mut rng);
    let client_params = SelfSignedCertParams::builder()
        .subject_cn("client")
        .validity(
            UtcDateTime::now() - TimeDuration::hours(1),
            UtcDateTime::now() + TimeDuration::days(7),
        )
        .serial(random_serial())
        .build()
        .unwrap();
    let client_cert_der =
        sign_with_ca_ed25519(&ca_key, ca_params.subject_cn(), &client_key, &client_params).unwrap();
    let client_cert_pem = der_to_pem("CERTIFICATE", &client_cert_der);
    let client_key_pem = der_to_pem(
        "PRIVATE KEY",
        client_key.to_pkcs8_der().expect("client pkcs8").as_bytes(),
    );
    std::fs::write(cert_file.path(), client_cert_pem).unwrap();
    std::fs::write(key_file.path(), client_key_pem).unwrap();
    std::env::set_var("REMOTE_SIGNER_TLS_CERT", cert_file.path());
    std::env::set_var("REMOTE_SIGNER_TLS_KEY", key_file.path());

    let server_key = SigningKey::generate(&mut rng);
    let server_cert_file = NamedTempFile::new().unwrap();
    let server_key_file = NamedTempFile::new().unwrap();
    let server_cert_json = format!(
        "{{\"version\":1,\"algorithm\":\"ed25519\",\"public_key\":\"{}\"}}",
        encode_standard(&server_key.verifying_key().to_bytes())
    );
    let server_key_json = format!(
        "{{\"version\":1,\"algorithm\":\"ed25519\",\"private_key\":\"{}\"}}",
        encode_standard(&server_key.to_bytes())
    );
    std::fs::write(server_cert_file.path(), server_cert_json).unwrap();
    std::fs::write(server_key_file.path(), server_key_json).unwrap();

    let client_registry_file = NamedTempFile::new().unwrap();
    let client_registry = format!(
        "{{\"version\":1,\"allowed\":[{{\"algorithm\":\"ed25519\",\"public_key\":\"{}\"}}]}}",
        encode_standard(&client_key.verifying_key().to_bytes())
    );
    std::fs::write(client_registry_file.path(), client_registry).unwrap();

    let trust_anchor_file = NamedTempFile::new().unwrap();
    std::fs::write(
        trust_anchor_file.path(),
        format!(
            "{{\"version\":1,\"algorithm\":\"ed25519\",\"public_key\":\"{}\"}}",
            encode_standard(&server_key.verifying_key().to_bytes())
        ),
    )
    .unwrap();
    std::env::set_var("REMOTE_SIGNER_TLS_CA", trust_anchor_file.path());

    let tls = ServerTlsConfig::from_identity_files_with_client_auth(
        server_cert_file.path(),
        server_key_file.path(),
        client_registry_file.path(),
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
