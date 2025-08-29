use rand::{rngs::OsRng, RngCore};
use tempfile::tempdir;
use the_block::storage::pipeline::{Provider, StoragePipeline};

#[derive(Clone, Copy)]
struct NoopProvider;

impl Provider for NoopProvider {
    fn id(&self) -> &str {
        "local"
    }
}

#[test]
fn put_and_get_roundtrip() {
    let dir = tempdir().unwrap();
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let provider = NoopProvider;
    let mut data = vec![0u8; 1024 * 1024];
    OsRng.fill_bytes(&mut data);
    let receipt = pipe
        .put_object(&data, "consumer", &[&provider])
        .expect("store");
    drop(pipe);
    let pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let out = pipe.get_object(&receipt.manifest_hash).expect("load");
    assert_eq!(out, data);
}
