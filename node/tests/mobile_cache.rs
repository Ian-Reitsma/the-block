use rand::rngs::OsRng;
use rand::RngCore;
use std::thread::sleep;
use std::time::Duration;
use sys::tempfile::tempdir;
use the_block::gateway::mobile_cache::{MobileCache, MobileCacheConfig};

fn persistent_config(dir: &std::path::Path, key: [u8; 32]) -> MobileCacheConfig {
    MobileCacheConfig {
        ttl: Duration::from_secs(60),
        sweep_interval: Duration::from_secs(1),
        max_entries: 16,
        max_payload_bytes: 4 * 1024,
        max_queue: 8,
        db_path: dir.join("mobile_cache.db"),
        encryption_key: key,
        temporary: false,
    }
}

#[test]
fn mobile_cache_persists_across_restarts() {
    let tmp = tempdir().expect("temp dir");
    let mut key = [0u8; 32];
    OsRng::default().fill_bytes(&mut key);
    let cfg = persistent_config(tmp.path(), key);

    {
        let mut cache = MobileCache::open(cfg.clone()).expect("open cache");
        cache
            .insert("domain".into(), "{\"record\":true}".into())
            .expect("cache insert");
        cache.queue_tx("offline-tx".into()).expect("queue tx");
    }

    {
        let mut cache = MobileCache::open(cfg.clone()).expect("reopen cache");
        let stored = cache
            .get("domain")
            .expect("read cache")
            .expect("missing cached value");
        assert_eq!(stored, "{\"record\":true}".to_string());
        let drained = cache.drain_queue(|_| {}).expect("drain queue after reopen");
        assert_eq!(drained, 1);
    }
}

#[cfg(feature = "telemetry")]
#[test]
fn mobile_cache_updates_telemetry_counters() {
    use the_block::telemetry::{
        MOBILE_CACHE_ENTRY_TOTAL, MOBILE_CACHE_HIT_TOTAL, MOBILE_CACHE_MISS_TOTAL,
        MOBILE_CACHE_QUEUE_TOTAL,
    };

    let tmp = tempdir().expect("temp dir");
    let mut key = [0u8; 32];
    OsRng::default().fill_bytes(&mut key);
    let cfg = persistent_config(tmp.path(), key);

    let mut cache = MobileCache::open(cfg).expect("open cache");
    let base_hits = MOBILE_CACHE_HIT_TOTAL.value();
    let base_miss = MOBILE_CACHE_MISS_TOTAL.value();
    let _ = cache.get("unknown").expect("cache miss lookup");
    assert!(MOBILE_CACHE_MISS_TOTAL.value() > base_miss);
    cache
        .insert("tracked".into(), "value".into())
        .expect("insert tracked");
    let _ = cache.get("tracked").expect("cache hit");
    assert!(MOBILE_CACHE_HIT_TOTAL.value() > base_hits);
    assert!(MOBILE_CACHE_ENTRY_TOTAL.value() >= 1);
    cache.queue_tx("queued".into()).expect("queue tx");
    assert!(MOBILE_CACHE_QUEUE_TOTAL.value() >= 1);
}

#[test]
fn expired_entries_are_swept() {
    let tmp = tempdir().expect("temp dir");
    let mut key = [0u8; 32];
    OsRng::default().fill_bytes(&mut key);
    let cfg = MobileCacheConfig::ephemeral(tmp.path(), Duration::from_millis(50), key);

    let mut cache = MobileCache::open(cfg).expect("open cache");
    cache
        .insert("short".into(), "value".into())
        .expect("insert value");
    sleep(Duration::from_millis(120));
    assert!(cache.get("short").expect("lookup after ttl").is_none());
    assert_eq!(cache.status().totals.entry_count, 0);
}
