mod support;

use std::time::Duration;

use httpd::StatusCode;
use ledger::crypto::remote_tag;
use wallet::{remote_signer::RemoteSigner, WalletError, WalletSigner};

use support::HttpSignerMock;

struct RemoteSignerTimeout(Option<String>);

impl RemoteSignerTimeout {
    fn set_ms(ms: &str) -> Self {
        let previous = std::env::var("REMOTE_SIGNER_TIMEOUT_MS").ok();
        std::env::set_var("REMOTE_SIGNER_TIMEOUT_MS", ms);
        RemoteSignerTimeout(previous)
    }
}

impl Drop for RemoteSignerTimeout {
    fn drop(&mut self) {
        if let Some(value) = self.0.take() {
            std::env::set_var("REMOTE_SIGNER_TIMEOUT_MS", value);
        } else {
            std::env::remove_var("REMOTE_SIGNER_TIMEOUT_MS");
        }
    }
}

#[test]
#[testkit::tb_serial]
fn multisig_success_and_failure() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let _timeout_guard = RemoteSignerTimeout::set_ms("30000");

    let signer_a = HttpSignerMock::success();
    let signer_b = HttpSignerMock::success();
    let endpoints = vec![signer_a.url().to_string(), signer_b.url().to_string()];
    let signer = RemoteSigner::connect_multi(&endpoints, 2).expect("connect");
    let msg = b"hello";
    let approvals = signer.sign_multisig(msg).expect("sign");
    for (pk, sig) in &approvals {
        pk.verify(&remote_tag(msg), sig).unwrap();
    }

    let signer =
        RemoteSigner::connect_multi(&vec![signer_a.url().to_string()], 1).expect("connect");
    let approvals = signer.sign_multisig(b"fail").expect("single signer");
    assert_eq!(approvals.len(), 1);
}

#[test]
#[testkit::tb_serial]
fn multisig_threshold_fails_when_signer_returns_error() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let _timeout_guard = RemoteSignerTimeout::set_ms("30000");

    let good = HttpSignerMock::success();
    let bad = HttpSignerMock::failing(StatusCode::INTERNAL_SERVER_ERROR);
    let endpoints = vec![good.url().to_string(), bad.url().to_string()];
    let signer = RemoteSigner::connect_multi(&endpoints, 2).expect("connect");
    let err = signer
        .sign_multisig(b"threshold")
        .expect_err("threshold err");
    assert!(
        matches!(err, WalletError::Failure(_)),
        "unexpected error: {err:?}"
    );
}

#[test]
#[testkit::tb_serial]
fn multisig_rejects_invalid_signatures() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let _timeout_guard = RemoteSignerTimeout::set_ms("30000");

    let good = HttpSignerMock::success();
    let bad = HttpSignerMock::invalid_signature();
    let endpoints = vec![good.url().to_string(), bad.url().to_string()];
    let signer = RemoteSigner::connect_multi(&endpoints, 2).expect("connect");
    let err = signer
        .sign_multisig(b"invalid")
        .expect_err("invalid signature rejection");
    assert!(
        matches!(err, WalletError::Failure(_)),
        "unexpected error: {err:?}"
    );
}

#[test]
#[testkit::tb_serial]
fn multisig_times_out_slow_signers() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let _timeout_guard = RemoteSignerTimeout::set_ms("50");

    let fast = HttpSignerMock::success();
    let slow = HttpSignerMock::delayed(Duration::from_millis(200));
    let endpoints = vec![fast.url().to_string(), slow.url().to_string()];
    let signer = RemoteSigner::connect_multi(&endpoints, 2).expect("connect");
    let err = signer
        .sign_multisig(b"timeout")
        .expect_err("timeout enforced");
    assert!(matches!(err, WalletError::Timeout));
}
