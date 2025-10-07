use state::{MerkleTrie, SnapshotManager};
use std::collections::BTreeSet;
use std::thread;
use std::time::Duration;
use sys::temp;

#[test]
fn snapshot_roundtrip() {
    let mut trie = MerkleTrie::new();
    trie.insert(b"a", b"1");
    trie.insert(b"b", b"2");
    let root = trie.root_hash();
    let tmp = temp::tempdir().expect("tmpdir");
    let dir = tmp.path().to_path_buf();
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

#[test]
fn prune_keeps_newest_snapshots() {
    let tmp = temp::tempdir().expect("tmpdir");
    let dir = tmp.path().to_path_buf();
    let keep = 2;
    let mgr = SnapshotManager::new(dir.clone(), keep);
    let mut trie = MerkleTrie::new();

    let mut created = Vec::new();
    for i in 0..4 {
        let key = format!("key{i}");
        let value = format!("value{i}");
        trie.insert(key.as_bytes(), value.as_bytes());
        let path = mgr.snapshot(&trie).expect("snapshot");
        created.push(path.file_name().unwrap().to_string_lossy().to_string());
        thread::sleep(Duration::from_millis(10));
    }

    let survivors: BTreeSet<String> = std::fs::read_dir(&dir)
        .expect("read_dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert_eq!(survivors.len(), keep);

    let expected: BTreeSet<String> = created.iter().rev().take(keep).cloned().collect();

    assert_eq!(survivors, expected);
}

#[test]
fn prune_removes_all_snapshots_when_keep_zero() {
    let tmp = temp::tempdir().expect("tmpdir");
    let dir = tmp.path().to_path_buf();
    let mut trie = MerkleTrie::new();
    trie.insert(b"only", b"snapshot");
    let mgr = SnapshotManager::new(dir.clone(), 0);
    let _ = mgr.snapshot(&trie).expect("snapshot");

    let remaining: Vec<_> = std::fs::read_dir(&dir).expect("read_dir").collect();

    assert!(remaining.is_empty());
}
