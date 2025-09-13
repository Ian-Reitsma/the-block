use light_client::Header;
use std::time::Duration;
use tb_sim::mobile_sync::measure_sync_latency;

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
        let v = u64::from_le_bytes(h.hash()[..8].try_into().unwrap());
        if v <= u64::MAX / h.difficulty {
            break;
        }
        h.nonce = h.nonce.wrapping_add(1);
    }
    h
}

#[test]
fn measures_sync_latency() {
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
    let h1 = make_header(&genesis, 1);
    let h2 = make_header(&h1, 2);
    let headers = vec![genesis, h1, h2];
    let dur = measure_sync_latency(headers, Duration::from_millis(2));
    assert!(dur.as_millis() >= 4);
}
