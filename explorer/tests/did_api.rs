use crypto_suite::signatures::ed25519::SigningKey;
use explorer::{did_view, Explorer, MetricPoint};
use std::convert::TryInto;
use sys::tempfile;
use the_block::generate_keypair;
use the_block::governance::GovStore;
use the_block::identity::DidRegistry;
use the_block::transaction::TxDidAnchor;

fn build_anchor(doc: &str, nonce: u64, sk: &SigningKey) -> TxDidAnchor {
    let pk = sk.verifying_key();
    let pk_bytes = pk.to_bytes();
    let mut tx = TxDidAnchor {
        address: crypto_suite::hex::encode(pk_bytes),
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
    let dir = tempfile::tempdir().unwrap();
    let did_path = dir.path().join("did.db");
    std::env::set_var("TB_DID_DB_PATH", did_path.to_string_lossy().as_ref());
    let gov = GovStore::open(dir.path().join("gov.db"));

    let (sk_bytes, pk_bytes) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes.try_into().unwrap());
    let address = crypto_suite::hex::encode(pk_bytes);

    let explorer_db = dir.path().join("explorer.db");

    let (record_one, record_two) = {
        let mut registry = DidRegistry::open(&did_path);
        let anchor_one = build_anchor("{\"id\":1}", 1, &sk);
        let record_one = registry
            .anchor(&anchor_one, Some(&gov))
            .expect("first anchor");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let anchor_two = build_anchor("{\"id\":2}", 2, &sk);
        let record_two = registry
            .anchor(&anchor_two, Some(&gov))
            .expect("second anchor");
        (record_one, record_two)
    };

    let expected_wallet = format!("/wallets/{}", address);
    let ex = Explorer::open(&explorer_db).expect("open explorer");
    ex.record_did_anchor(&record_one.clone().into())
        .expect("seed explorer");

    let record = ex
        .did_document(&address)
        .expect("did record available after seeding");
    assert_eq!(record.nonce, 1);
    assert_eq!(record.document, "{\"id\":1}");

    let history = did_view::by_address(&ex, &address).expect("history by address");
    assert!(
        !history.is_empty(),
        "did history should include at least the most recent anchor"
    );
    assert_eq!(
        history[0].wallet_url.as_deref(),
        Some(expected_wallet.as_str())
    );
    assert!(
        history.iter().any(|row| row.hash == record.hash),
        "history should contain the initial anchor"
    );

    ex.record_did_anchor(&record_two.clone().into())
        .expect("update explorer");

    let updated = ex.did_document(&address).expect("cached resolve");
    assert_eq!(updated.nonce, 2);

    let history = did_view::by_address(&ex, &address).expect("history refreshed");
    assert!(!history.is_empty());

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
