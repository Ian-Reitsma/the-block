use serial_test::serial;
use tempfile::tempdir;
use the_block::net::{export_peer_stats, record_request, set_metrics_export_dir};

#[test]
#[serial]
fn rejects_traversal_and_symlink() {
    let dir = tempdir().unwrap();
    set_metrics_export_dir(dir.path().to_str().unwrap().to_string());
    let pk = [1u8; 32];
    record_request(&pk);
    assert!(export_peer_stats(&pk, "../evil.json").is_err());
    let link_path = dir.path().join("out.json");
    std::os::unix::fs::symlink(dir.path().join("target"), &link_path).unwrap();
    assert!(export_peer_stats(&pk, "out.json").is_err());
}
