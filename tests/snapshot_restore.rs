#[path = "../tools/snapshot.rs"]
mod snapshot_tool;
use snapshot_tool::{create_snapshot, restore_snapshot};
#[test]
fn legacy_snapshot_helpers_now_fail() {
    let dir = std::path::PathBuf::from("/tmp/unused");
    let snap = dir.join("snap.zst");
    assert!(create_snapshot(&dir, &snap).is_err());
    assert!(restore_snapshot(&snap, &dir).is_err());
}
