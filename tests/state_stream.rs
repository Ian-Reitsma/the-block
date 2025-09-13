use light_client::{StateChunk, StateStream};
use blake3::Hasher;
use std::collections::HashMap;

fn root_for(accts: &[(String, u64)]) -> [u8; 32] {
    let mut h = Hasher::new();
    for (a, b) in accts.iter() {
        h.update(a.as_bytes());
        h.update(&b.to_le_bytes());
    }
    h.finalize().into()
}

#[test]
fn resync_on_gap() {
    let mut stream = StateStream::new();
    let accounts0 = vec![("alice".into(), 1u64)];
    let chunk0 = StateChunk {
        seq: 0,
        tip_height: 0,
        accounts: accounts0.clone(),
        root: root_for(&accounts0),
        proof: Vec::new(),
        compressed: false,
    };
    assert!(stream.apply_chunk(chunk0).is_ok());
    // Missing seq 1, jump to 2 triggers error
    let accounts2: Vec<(String, u64)> = Vec::new();
    let chunk2 = StateChunk {
        seq: 2,
        tip_height: 2,
        accounts: accounts2.clone(),
        root: root_for(&accounts2),
        proof: Vec::new(),
        compressed: false,
    };
    assert!(stream.apply_chunk(chunk2).is_err());
    // Resync via snapshot
    let map: HashMap<String, u64> = HashMap::new();
    let snap = bincode::serialize(&map).unwrap();
    assert!(stream.apply_snapshot(&snap, false).is_ok());
}
