use ed25519_dalek::{Signer, SigningKey};
use explorer::{did_view, Explorer, MetricPoint};
use hex;
use std::convert::TryInto;
use tempfile::tempdir;
use the_block::generate_keypair;
use the_block::governance::GovStore;
use the_block::identity::DidRegistry;
use the_block::transaction::TxDidAnchor;

fn build_anchor(doc: &str, nonce: u64, sk: &SigningKey) -> TxDidAnchor {
    let pk = sk.verifying_key();
    let pk_bytes = pk.to_bytes();
    let mut tx = TxDidAnchor {
        address: hex::encode(pk_bytes),
        public_key: pk_bytes.to_vec(),
        document: doc.to_string(),
        nonce,
        signature: Vec::new(),
        remote_attestation: None,
    };
    let sig = sk.sign(tx.owner_digest().as_ref());
    tx.signature = sig.to_bytes().to_vec();
    tx
}

#[test]
fn explorer_indexes_recent_dids_and_metrics() {
    let dir = tempdir().unwrap();
    let did_path = dir.path().join("did.db");
    std::env::set_var("TB_DID_DB_PATH", did_path.to_string_lossy().as_ref());
    let mut registry = DidRegistry::open(&did_path);
    let gov = GovStore::open(dir.path().join("gov.db"));

    let (sk_bytes, pk_bytes) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes.try_into().unwrap());
    let address = hex::encode(pk_bytes);

    let mut anchor = build_anchor("{\"id\":1}", 1, &sk);
    registry.anchor(&anchor, Some(&gov)).expect("first anchor");

    let explorer_db = dir.path().join("explorer.db");
    let ex = Explorer::open(&explorer_db).expect("open explorer");

    let record = ex
        .did_document(&address)
        .expect("did record available after seeding");
    assert_eq!(record.nonce, 1);
    assert_eq!(record.document, "{\"id\":1}");

    let mut history = did_view::by_address(&ex, &address).expect("history by address");
    assert_eq!(history.len(), 1);
    let expected_wallet = format!("/wallets/{}", address);
    assert_eq!(
        history[0].wallet_url.as_deref(),
        Some(expected_wallet.as_str())
    );

    // Apply an update and ensure both cache and SQLite history are refreshed.
    anchor.nonce = 2;
    anchor.document = "{\"id\":2}".to_string();
    let sig = sk.sign(anchor.owner_digest().as_ref());
    anchor.signature = sig.to_bytes().to_vec();
    registry.anchor(&anchor, Some(&gov)).expect("second anchor");

    let updated = ex.did_document(&address).expect("cached resolve");
    assert_eq!(updated.nonce, 2);

    history = did_view::by_address(&ex, &address).expect("history refreshed");
    assert!(history.len() >= 2);

    let recent = did_view::recent(&ex, 8).expect("recent anchors");
    assert!(!recent.is_empty());

    ex.archive_metric(&MetricPoint {
        name: "did_anchor_total".to_string(),
        ts: 100,
        value: 5.0,
    })
    .expect("archive metric");
    ex.archive_metric(&MetricPoint {
        name: "did_anchor_total".to_string(),
        ts: 160,
        value: 11.0,
    })
    .expect("archive metric");

    let rates = did_view::anchor_rate(&ex).expect("anchor rate");
    let rate = rates.last().unwrap().value;
    assert!((rate - (11.0 - 5.0) / 60.0).abs() < 1e-9);

    std::env::remove_var("TB_DID_DB_PATH");
}
