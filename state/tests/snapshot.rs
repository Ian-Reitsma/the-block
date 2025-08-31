use state::{MerkleTrie, SnapshotManager};
use std::path::PathBuf;

#[test]
fn snapshot_roundtrip() {
    let mut trie = MerkleTrie::new();
    trie.insert(b"a", b"1");
    trie.insert(b"b", b"2");
    let root = trie.root_hash();
    let dir = PathBuf::from("/tmp/state_snap");
    let mgr = SnapshotManager::new(dir.clone(), 2);
    let path = mgr.snapshot(&trie).expect("snapshot");
    let restored = mgr.restore(&path).expect("restore");
    assert_eq!(root, restored.root_hash());
}

#[test]
fn proof_verification() {
    let mut trie = MerkleTrie::new();
    trie.insert(b"key", b"value");
    let root = trie.root_hash();
    let proof = trie.prove(b"key").expect("proof");
    assert!(MerkleTrie::verify_proof(root, b"key", b"value", &proof));
    assert!(!MerkleTrie::verify_proof(root, b"key", b"bad", &proof));
}
