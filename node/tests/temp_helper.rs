#![cfg(feature = "integration-tests")]
#[path = "util/temp.rs"]
mod temp;
use temp::temp_dir;

#[test]
fn temp_dir_auto_cleans_on_drop() {
    let dir = temp_dir("temp_dir_cleanup");
    let path = dir.path().to_path_buf();
    assert!(
        path.exists(),
        "temp dir should exist while TempDir is alive"
    );
    drop(dir);
    assert!(
        !path.exists(),
        "temp dir should be removed after TempDir is dropped"
    );
}
