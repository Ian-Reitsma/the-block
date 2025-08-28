use the_block::{Blockchain, SnapshotManager};

mod util;

#[test]
fn create_and_apply_snapshot() {
    let dir = util::temp::temp_dir("snapshot_cli");
    let bc = Blockchain::new(dir.path().to_str().unwrap());
    let mgr = SnapshotManager::new(dir.path().to_str().unwrap().to_string(), 1);
    let root = mgr.write_snapshot(bc.block_height, bc.accounts()).unwrap();
    let loaded = mgr.load_latest().unwrap().unwrap();
    assert_eq!(loaded.2, root);
}
