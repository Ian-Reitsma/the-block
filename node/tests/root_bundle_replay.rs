use crypto_suite::hashing::blake3::Hasher;
use the_block::{transaction::BlobTx, Blockchain};

fn make_root(seed: u8) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(&[seed]);
    h.finalize().into()
}

#[test]
fn root_bundles_replay_deterministically() {
    let miner = "miner";
    let blobs = vec![
        BlobTx::new("owner-1".into(), [1u8; 32], make_root(1), 1024, 1, None),
        BlobTx::new("owner-2".into(), [2u8; 32], make_root(2), 2048, 1, None),
    ];

    let mut bc_a = Blockchain::default();
    for tx in &blobs {
        bc_a.submit_blob_tx(tx.clone()).expect("blob enqueued");
    }
    // First cadence boundary (4s) should emit consistent bundles across nodes.
    let block_a = bc_a.mine_block_at(miner, 4_000).expect("block mined");

    let mut bc_b = Blockchain::default();
    for tx in blobs {
        bc_b.submit_blob_tx(tx).expect("blob enqueued");
    }
    let block_b = bc_b.mine_block_at(miner, 4_000).expect("block mined");

    assert_eq!(block_a.root_bundles, block_b.root_bundles);
}
