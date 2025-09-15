use blake3::Hasher;
use light_client::{StateChunk, StateStream};
use std::collections::HashMap;

fn root(accts: &[(String, u64)]) -> [u8; 32] {
    let mut h = Hasher::new();
    for (a, b) in accts.iter() {
        h.update(a.as_bytes());
        h.update(&b.to_le_bytes());
    }
    h.finalize().into()
}

#[test]
fn dropped_packet_and_resync() {
    let mut stream = StateStream::new();
    let a1 = vec![("alice".to_string(), 1)];
    let c0 = StateChunk {
        seq: 0,
        tip_height: 1,
        accounts: a1.clone(),
        root: root(&a1),
        proof: vec![],
        compressed: false,
    };
    assert!(stream.apply_chunk(c0).is_ok());
    // simulate dropped seq=1
    let a2 = vec![("bob".to_string(), 2)];
    let c2 = StateChunk {
        seq: 2,
        tip_height: 2,
        accounts: a2.clone(),
        root: root(&a2),
        proof: vec![],
        compressed: false,
    };
    assert!(stream.apply_chunk(c2).is_err());
    // resync via compressed snapshot
    let mut map = HashMap::new();
    map.insert("alice".to_string(), 1u64);
    map.insert("bob".to_string(), 2u64);
    let bytes = bincode::serialize(&map).unwrap();
    let snapshot = zstd::encode_all(bytes.as_slice(), 0).unwrap();
    stream.apply_snapshot(&snapshot, true).unwrap();
    // after snapshot sequence resets to 0
    let c0b = StateChunk {
        seq: 0,
        tip_height: 3,
        accounts: Vec::new(),
        root: root(&[]),
        proof: vec![],
        compressed: false,
    };
    assert!(stream.apply_chunk(c0b).is_ok());
}
