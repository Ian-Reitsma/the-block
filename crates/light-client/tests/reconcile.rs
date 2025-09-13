use light_client::{sync_background, Header, LightClient, SyncOptions};

fn make_header(prev: &Header, height: u64) -> Header {
    let mut h = Header { height, prev_hash: prev.hash(), merkle_root: [0;32], checkpoint_hash: [0;32], validator_key: None, checkpoint_sig: None, nonce:0, difficulty:1, timestamp_millis:0, l2_roots:vec![], l2_sizes:vec![], vdf_commit:[0;32], vdf_output:[0;32], vdf_proof:vec![] };
    loop {
        let v = u64::from_le_bytes(h.hash()[..8].try_into().unwrap());
        if v <= u64::MAX / h.difficulty { break; }
        h.nonce = h.nonce.wrapping_add(1);
    }
    h
}

#[test]
fn reconciles_partial_and_full_sync() {
    let genesis = Header { height:0, prev_hash:[0;32], merkle_root:[0;32], checkpoint_hash:[0;32], validator_key:None, checkpoint_sig:None, nonce:0, difficulty:1, timestamp_millis:0, l2_roots:vec![], l2_sizes:vec![], vdf_commit:[0;32], vdf_output:[0;32], vdf_proof:vec![] };
    let h1 = make_header(&genesis,1);
    let h2 = make_header(&h1,2);

    let mut partial = LightClient::new(genesis.clone());
    partial.verify_and_append(h1.clone()).unwrap();
    let fetch = move |start:u64| match start { 2 => vec![h2.clone()], _ => vec![] };
    sync_background(&mut partial, SyncOptions { wifi_only:false, require_charging:false, min_battery:0.0 }, fetch);

    let mut full = LightClient::new(genesis);
    full.verify_and_append(h1).unwrap();
    full.verify_and_append(h2).unwrap();

    assert_eq!(partial.chain, full.chain);
}
