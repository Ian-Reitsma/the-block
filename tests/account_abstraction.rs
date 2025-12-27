use crypto::session::SessionKey;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use the_block::{
    transaction::{sign_tx, RawTxPayload},
    Account, Blockchain, TokenBalance, TxAdmissionError,
};

#[test]
fn session_nonce_and_expiry() {
    let mut bc = Blockchain::default();
    bc.accounts.insert(
        "alice".into(),
        Account {
            address: "alice".into(),
            balance: TokenBalance {
                consumer: 100,
                industrial: 0,
            },
            nonce: 0,
            pending_consumer: 0,
            pending_industrial: 0,
            pending_nonce: 0,
            pending_nonces: HashSet::new(),
            sessions: Vec::new(),
        },
    );

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let sess = SessionKey::generate(now + 60).expect("session key");
    bc.issue_session_key("alice".into(), sess.public_key.clone(), sess.expires_at).unwrap();
    let payload = RawTxPayload {
        from_: "alice".into(),
        to: "bob".into(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee: 0,
        pct: 100,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx1 = sign_tx(&sess.secret.to_bytes(), &payload).unwrap();
    bc.submit_transaction(tx1.clone()).unwrap();
    // replay should fail
    let tx2 = sign_tx(&sess.secret.to_bytes(), &payload).unwrap();
    assert_eq!(bc.submit_transaction(tx2), Err(TxAdmissionError::Duplicate));

    // expired session fails
    let expired = SessionKey::generate(now - 1).expect("expired session key");
    bc.issue_session_key("alice".into(), expired.public_key.clone(), expired.expires_at).unwrap();
    let payload2 = RawTxPayload { nonce: 2, ..payload };
    let tx3 = sign_tx(&expired.secret.to_bytes(), &payload2).unwrap();
    assert_eq!(bc.submit_transaction(tx3), Err(TxAdmissionError::SessionExpired));
}
