#![cfg(feature = "integration-tests")]
use std::thread;
use std::time::Duration;

use tempfile::tempdir;
use the_block::net::{peer_metrics_store, PeerMetrics};

#[test]
fn persist_and_prune() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("db");
    peer_metrics_store::init(path.to_str().unwrap());
    let pk = [1u8; 32];
    {
        let store = peer_metrics_store::store().unwrap();
        let mut m = PeerMetrics::default();
        m.requests = 1;
        store.insert(&pk, &m, 1);
        store.flush().unwrap();
    }
    thread::sleep(Duration::from_secs(2));
    {
        let store = peer_metrics_store::store().unwrap();
        let mut m2 = PeerMetrics::default();
        m2.requests = 2;
        store.insert(&pk, &m2, 1);
        store.flush().unwrap();
    }
    let store2 = peer_metrics_store::store().unwrap();
    let loaded = store2.load(10);
    assert_eq!(loaded.get(&pk).unwrap().requests, 2);
    assert_eq!(store2.count(), 1);
}

#[test]
fn concurrent_inserts() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("db2");
    peer_metrics_store::init(path.to_str().unwrap());
    let pk = [2u8; 32];
    let handles: Vec<_> = (0..10)
        .map(|_| {
            thread::spawn({
                let pk = pk;
                move || {
                    let mut m = PeerMetrics::default();
                    m.requests = 1;
                    if let Some(store) = peer_metrics_store::store() {
                        store.insert(&pk, &m, 60);
                    }
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let store = peer_metrics_store::store().unwrap();
    assert!(!store.load(60).is_empty());
}
