#[path = "../../node/src/gateway/mobile_cache.rs"]
mod mobile_cache;
use mobile_cache::{MobileCache, MobileCacheConfig};
use rand::rngs::OsRng;
use rand::RngCore;
use std::time::Duration;

#[test]
fn cache_and_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    let cfg = MobileCacheConfig::ephemeral(tmp.path(), Duration::from_secs(10), key);
    let mut cache = MobileCache::open(cfg).expect("open cache");
    cache
        .insert("k".into(), "v".into())
        .expect("insert cache entry");
    assert_eq!(
        cache.get("k").expect("cache get"),
        Some(String::from("v"))
    );
    cache.queue_tx("tx".into()).expect("queue tx");
    cache
        .drain_queue(|_| {})
        .expect("drain offline queue");
}
