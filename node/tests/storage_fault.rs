use std::os::unix::fs::PermissionsExt;

use tempfile::tempdir;
use the_block::SimpleDb;

#[test]
fn disk_full_simulation() {
    let dir = tempdir().unwrap();
    let mut db = SimpleDb::open(dir.path().to_str().unwrap());
    db.set_byte_limit(4);
    assert!(db.try_insert("k", vec![0; 16]).is_err());
}

#[test]
fn permission_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ro");
    std::fs::create_dir(&path).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o500);
    std::fs::set_permissions(&path, perms).unwrap();
    let res = std::panic::catch_unwind(|| {
        let mut db = SimpleDb::open(path.to_str().unwrap());
        db.insert("k", b"v".to_vec());
    });
    assert!(res.is_err());
}

