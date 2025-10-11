use std::sync::{Arc, Mutex};

use foundation_serialization::{binary, Deserialize, Serialize};
use light_client::{
    account_state_value, AccountChunk, StateChunk, StateStream, StateStreamBuilder,
    StateStreamError,
};
use state::MerkleTrie;
use sys::tempfile::tempdir;

#[derive(Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct SnapshotAccount {
    address: String,
    balance: u64,
    seq: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct SnapshotPayload {
    accounts: Vec<SnapshotAccount>,
    next_seq: u64,
}

fn build_chunk(seq: u64, tip_height: u64, entries: &[(&str, u64, u64)]) -> StateChunk {
    let mut trie = MerkleTrie::new();
    for (address, balance, account_seq) in entries.iter().copied() {
        let value = account_state_value(balance, account_seq);
        trie.insert(address.as_bytes(), &value);
    }
    let root = trie.root_hash();
    let accounts = entries
        .iter()
        .map(|(address, balance, account_seq)| AccountChunk {
            address: (*address).to_string(),
            balance: *balance,
            account_seq: *account_seq,
            proof: trie
                .prove(address.as_bytes())
                .expect("proof must exist for inserted leaf"),
        })
        .collect();
    StateChunk {
        seq,
        tip_height,
        accounts,
        root,
        compressed: false,
    }
}

#[test]
fn validates_proofs_and_rejects_stale_updates() {
    let dir = tempdir().unwrap();
    let cache_path = dir.path().join("cache.bin");
    let mut stream = StateStream::builder().cache_path(cache_path).build();

    let chunk0 = build_chunk(0, 1, &[("alice", 50, 10)]);
    stream.apply_chunk(chunk0).expect("first chunk");

    let mut invalid_chunk = build_chunk(1, 2, &[("alice", 55, 11)]);
    invalid_chunk.root = [1u8; 32];
    match stream.apply_chunk(invalid_chunk) {
        Err(StateStreamError::InvalidProof { address }) => assert_eq!(address, "alice"),
        other => panic!("expected invalid proof error, got {other:?}"),
    }

    let stale_chunk = build_chunk(1, 2, &[("alice", 60, 9)]);
    match stream.apply_chunk(stale_chunk) {
        Err(StateStreamError::StaleAccountUpdate {
            address,
            cached_seq,
            update_seq,
        }) => {
            assert_eq!(address, "alice");
            assert_eq!(cached_seq, 10);
            assert_eq!(update_seq, 9);
        }
        other => panic!("expected stale update error, got {other:?}"),
    }
}

#[test]
fn gap_recovery_with_callback() {
    let dir = tempdir().unwrap();
    let cache_path = dir.path().join("cache.bin");

    let missing_chunk = build_chunk(0, 1, &[("alice", 5, 1)]);
    let delivered_chunk = build_chunk(1, 2, &[("bob", 7, 3)]);

    let invocations = Arc::new(Mutex::new(0u32));
    let fetch_invocations = invocations.clone();
    let missing_clone = missing_chunk.clone();

    let mut stream = StateStreamBuilder::new()
        .cache_path(cache_path.clone())
        .gap_fetcher(move |from, to| {
            let mut calls = fetch_invocations.lock().unwrap();
            *calls += 1;
            assert_eq!(from, 0);
            assert_eq!(to, 1);
            Ok(vec![missing_clone.clone()])
        })
        .build();

    stream
        .apply_chunk(delivered_chunk)
        .expect("gap should be filled by callback");

    assert_eq!(*invocations.lock().unwrap(), 1);
    assert_eq!(stream.next_seq(), 2);
    assert_eq!(stream.cached_balance("alice"), Some((5, 1)));
    assert_eq!(stream.cached_balance("bob"), Some((7, 3)));
}

#[test]
fn snapshot_resume_persists_state() {
    let dir = tempdir().unwrap();
    let cache_path = dir.path().join("cache.bin");
    let mut stream = StateStream::builder()
        .cache_path(cache_path.clone())
        .build();

    let snapshot = SnapshotPayload {
        accounts: vec![SnapshotAccount {
            address: "alice".to_string(),
            balance: 99,
            seq: 42,
        }],
        next_seq: 5,
    };
    let bytes = binary::encode(&snapshot).unwrap();
    stream
        .apply_snapshot(&bytes, false)
        .expect("snapshot should load");
    assert_eq!(stream.next_seq(), 5);
    assert_eq!(stream.cached_balance("alice"), Some((99, 42)));

    drop(stream);

    let restored = StateStream::builder().cache_path(cache_path).build();
    assert_eq!(restored.next_seq(), 5);
    assert_eq!(restored.cached_balance("alice"), Some((99, 42)));
}

#[cfg(unix)]
#[test]
fn chunk_persist_failure_rolls_back_state() {
    use std::fs;

    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join("ro");
    fs::create_dir_all(&cache_dir).unwrap();
    let cache_path = cache_dir.join("cache.bin");

    let mut stream = StateStream::builder()
        .cache_path(cache_path.clone())
        .build();
    let chunk = build_chunk(0, 0, &[("alice", 1, 1)]);

    fs::create_dir_all(&cache_path).unwrap();

    let err = stream
        .apply_chunk(chunk)
        .expect_err("persist failure should bubble up");
    assert!(matches!(err, StateStreamError::Io(_)));
    assert_eq!(stream.next_seq(), 0);
    assert_eq!(stream.cached_balance("alice"), None);
    fs::remove_dir_all(&cache_path).unwrap();
}

#[cfg(unix)]
#[test]
fn snapshot_persist_failure_rolls_back_state() {
    use std::fs;

    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join("persist");
    fs::create_dir_all(&cache_dir).unwrap();
    let cache_path = cache_dir.join("cache.bin");

    let mut stream = StateStream::builder()
        .cache_path(cache_path.clone())
        .build();
    let chunk = build_chunk(0, 1, &[("carol", 4, 2)]);
    stream
        .apply_chunk(chunk)
        .expect("initial chunk should persist");

    fs::remove_file(&cache_path).unwrap();
    fs::create_dir_all(&cache_path).unwrap();

    let snapshot = SnapshotPayload {
        accounts: vec![SnapshotAccount {
            address: "dave".to_string(),
            balance: 99,
            seq: 10,
        }],
        next_seq: 4,
    };
    let bytes = binary::encode(&snapshot).unwrap();
    let err = stream
        .apply_snapshot(&bytes, false)
        .expect_err("snapshot persist failure should bubble up");
    assert!(matches!(err, StateStreamError::Io(_)));
    assert_eq!(stream.next_seq(), 1);
    assert_eq!(stream.cached_balance("carol"), Some((4, 2)));
    assert_eq!(stream.cached_balance("dave"), None);
    fs::remove_dir_all(&cache_path).unwrap();
}
