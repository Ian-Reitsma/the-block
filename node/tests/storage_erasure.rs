use hex::encode;
use rand::{rngs::OsRng, RngCore};
use tempfile::tempdir;
use the_block::storage::pipeline::{Provider, StoragePipeline};

#[derive(Clone)]
struct LocalProvider {
    id: String,
}

impl Provider for LocalProvider {
    fn id(&self) -> &str {
        &self.id
    }
}

#[test]
fn recovers_from_missing_shard() {
    let dir = tempdir().unwrap();
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let prov = LocalProvider { id: "p1".into() };
    let mut data = vec![0u8; 1024];
    OsRng.fill_bytes(&mut data);
    let receipt = pipe.put_object(&data, "lane", &[&prov]).expect("store");
    // delete first shard
    let manifest = pipe.get_manifest(&receipt.manifest_hash).unwrap();
    let parity = format!("chunk/{}", encode(manifest.chunks[1].id));
    pipe.db_mut().remove(&parity);
    let out = pipe.get_object(&receipt.manifest_hash).expect("recover");
    assert_eq!(out, data);
}
