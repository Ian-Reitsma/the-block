use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

fn snapshot_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let mut base = std::env::temp_dir();
        base.push(format!("testkit-snap-{}", std::process::id()));
        base.push("snapshot");
        fs::create_dir_all(&base).unwrap();
        base
    })
    .clone()
}

#[test]
fn snapshot_roundtrip_and_mismatch() {
    let dir = snapshot_dir();
    std::env::set_var("TB_SNAPSHOT_DIR", &dir);
    std::env::set_var("TB_UPDATE_SNAPSHOTS", "1");
    testkit::tb_snapshot!("roundtrip", "first line\n");
    std::env::remove_var("TB_UPDATE_SNAPSHOTS");

    testkit::tb_snapshot!("roundtrip", "first line\n");

    let result = std::panic::catch_unwind(|| {
        testkit::tb_snapshot!("roundtrip", "different");
    });
    assert!(result.is_err());

    std::env::remove_var("TB_SNAPSHOT_DIR");
    std::env::remove_var("TB_UPDATE_SNAPSHOTS");
}
