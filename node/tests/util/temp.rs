use tempfile::{Builder, TempDir};
use the_block::Blockchain;

pub fn temp_dir(prefix: &str) -> TempDir {
    Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("create temp dir")
}

#[allow(dead_code)]
pub fn temp_blockchain(prefix: &str) -> (TempDir, Blockchain) {
    let dir = temp_dir(prefix);
    let bc = Blockchain::new(dir.path().to_str().unwrap());
    (dir, bc)
}
