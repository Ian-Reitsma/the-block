use light_client::{sync_background, Header, LightClient, SyncOptions};

fn make_header(prev: &Header, height: u64) -> Header {
    let mut h = Header {
        height,
        prev_hash: prev.hash(),
        merkle_root: [0u8; 32],
        checkpoint_hash: [0u8; 32],
        nonce: 0,
        difficulty: 1,
        timestamp_millis: 0,
        l2_roots: vec![],
        l2_sizes: vec![],
        vdf_commit: [0u8; 32],
        vdf_output: [0u8; 32],
        vdf_proof: vec![],
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
fn syncs_to_chain_tip() {
    let genesis = Header {
        height: 0,
        prev_hash: [0u8; 32],
        merkle_root: [0u8; 32],
        checkpoint_hash: [0u8; 32],
        nonce: 0,
        difficulty: 1,
        timestamp_millis: 0,
        l2_roots: vec![],
        l2_sizes: vec![],
        vdf_commit: [0u8; 32],
        vdf_output: [0u8; 32],
        vdf_proof: vec![],
    };
    let mut lc = LightClient::new(genesis.clone());
    let h1 = make_header(&genesis, 1);
    let h2 = make_header(&h1, 2);
    let fetch = move |start: u64| match start {
        1 => vec![h1.clone(), h2.clone()],
        _ => Vec::new(),
    };
    sync_background(
        &mut lc,
        SyncOptions {
            wifi_only: false,
            require_charging: false,
            min_battery: 0.0,
        },
        fetch,
    );
    assert_eq!(lc.tip_height(), 2);
}
