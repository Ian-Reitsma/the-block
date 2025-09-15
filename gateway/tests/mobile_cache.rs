#[path = "../../node/src/gateway/mobile_cache.rs"]
mod mobile_cache;
use mobile_cache::MobileCache;
use std::time::Duration;

#[test]
fn cache_and_queue() {
    let mut c = MobileCache::new(Duration::from_secs(1));
    c.insert("k".into(), "v".into());
    assert_eq!(c.get("k"), Some("v".into()));
    c.queue_tx("tx".into());
    c.drain_queue(|_| {});
}
