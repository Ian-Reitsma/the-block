#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use state::{MerkleTrie, Proof};
use the_block::Blockchain;

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {});
}

#[test]
fn account_proof_roundtrip() {
    init();
    let dir = temp_dir("proof_db");
    let mut bc = Blockchain::with_difficulty(dir.path().to_str().unwrap(), 0).unwrap();
    bc.recompute_difficulty();
    bc.add_account("alice".into(), 10).unwrap();
    bc.add_account("bob".into(), 5).unwrap();
    bc.mine_block("miner").unwrap();
    let (root, proof) = bc.account_proof("alice".into()).unwrap();
    let acc = bc.accounts.get("alice").unwrap();
    let mut value = Vec::new();
    value.extend_from_slice(&acc.balance.amount.to_le_bytes());
    value.extend_from_slice(&0u64.to_le_bytes());
    value.extend_from_slice(&acc.nonce.to_le_bytes());

    let proof = Proof(
        proof
            .into_iter()
            .map(|(sib_hex, is_left)| {
                let bytes = crypto_suite::hex::decode(sib_hex).unwrap();
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                (arr, is_left)
            })
            .collect(),
    );
    let root_bytes = {
        let bytes = crypto_suite::hex::decode(root).unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        arr
    };
    assert!(MerkleTrie::verify_proof(
        root_bytes,
        "alice".as_bytes(),
        &value,
        &proof
    ));
}
