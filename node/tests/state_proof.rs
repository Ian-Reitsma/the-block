#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use crypto_suite::hashing::blake3::Hasher;
use the_block::Blockchain;

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
}

#[test]
fn account_proof_roundtrip() {
    init();
    let dir = temp_dir("proof_db");
    let mut bc = Blockchain::with_difficulty(dir.path().to_str().unwrap(), 0).unwrap();
    bc.recompute_difficulty();
    bc.add_account("alice".into(), 10, 0).unwrap();
    bc.add_account("bob".into(), 5, 0).unwrap();
    bc.mine_block("miner").unwrap();
    let (root, proof) = bc.account_proof("alice".into()).unwrap();
    let acc = bc.accounts.get("alice").unwrap();
    let mut h = Hasher::new();
    h.update("alice".as_bytes());
    h.update(&acc.balance.consumer.to_le_bytes());
    h.update(&acc.balance.industrial.to_le_bytes());
    h.update(&acc.nonce.to_le_bytes());
    let mut leaf = *h.finalize().as_bytes();
    for (sib_hex, is_left) in proof {
        let sib = crypto_suite::hex::decode(sib_hex).unwrap();
        let mut hh = Hasher::new();
        if is_left {
            hh.update(&sib);
            hh.update(&leaf);
        } else {
            hh.update(&leaf);
            hh.update(&sib);
        }
        leaf = *hh.finalize().as_bytes();
    }
    assert_eq!(crypto_suite::hex::encode(leaf), root);
}
