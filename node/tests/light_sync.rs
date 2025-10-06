#![cfg(feature = "integration-tests")]
use the_block::light_client::{sync_background, Header, LightClient, SyncOptions};

fn mine(prev: &Header, height: u64) -> Header {
    let mut h = Header {
        height,
        prev_hash: prev.hash(),
        merkle_root: [0u8; 32],
        checkpoint_hash: [0u8; 32],
        validator_key: None,
        checkpoint_sig: None,
        nonce: 0,
        difficulty: 1,
        timestamp_millis: 0,
        l2_roots: Vec::new(),
        l2_sizes: Vec::new(),
        vdf_commit: [0u8; 32],
        vdf_output: [0u8; 32],
        vdf_proof: Vec::new(),
    };
    loop {
        let hash = h.hash();
        let v = u64::from_le_bytes(hash[..8].try_into().unwrap());
        if v <= u64::MAX / h.difficulty {
            break;
        }
        h.nonce = h.nonce.wrapping_add(1);
    }
    h
}

#[test]
fn partial_to_full_sync() {
    runtime::block_on(async {
        let genesis = Header {
            height: 0,
            prev_hash: [0u8; 32],
            merkle_root: [0u8; 32],
            checkpoint_hash: [0u8; 32],
            validator_key: None,
            checkpoint_sig: None,
            nonce: 0,
            difficulty: 1,
            timestamp_millis: 0,
            l2_roots: Vec::new(),
            l2_sizes: Vec::new(),
            vdf_commit: [0u8; 32],
            vdf_output: [0u8; 32],
            vdf_proof: Vec::new(),
        };
        let h1 = mine(&genesis, 1);
        let h2 = mine(&h1, 2);
        let mut lc = LightClient::new(genesis);
        lc.verify_and_append(h1).unwrap();
        let remaining = vec![h2];
        let fetch = move |start: u64, _batch: usize| {
            let remaining = remaining.clone();
            async move {
                remaining
                    .into_iter()
                    .filter(|h| h.height >= start)
                    .collect()
            }
        };
        let opts = SyncOptions {
            wifi_only: false,
            require_charging: false,
            min_battery: 0.0,
            ..SyncOptions::default()
        };
        sync_background(&mut lc, opts, fetch).await.unwrap();
        assert_eq!(lc.tip_height(), 2);
    });
}
