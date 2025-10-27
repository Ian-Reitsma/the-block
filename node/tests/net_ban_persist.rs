#![cfg(feature = "integration-tests")]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use the_block::net::ban_store::BanStore;

#[test]
fn ban_persists_across_reopen() {
    let dir = sys::tempfile::tempdir().unwrap();
    let path = dir.path().join("bans");
    let store = BanStore::open(path.to_str().unwrap());
    let pk = [1u8; 32];
    let until = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 60;
    store.ban(&pk, until).expect("ban");
    assert!(store.is_banned(&pk).expect("is banned"));
    drop(store);
    let store2 = BanStore::open(path.to_str().unwrap());
    assert!(store2.is_banned(&pk).expect("is banned"));
}

#[test]
fn ban_expires() {
    let dir = sys::tempfile::tempdir().unwrap();
    let path = dir.path().join("bans");
    let store = BanStore::open(path.to_str().unwrap());
    let pk = [2u8; 32];
    let until = SystemTime::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    store.ban(&pk, until).expect("ban");
    store.purge_expired().expect("purge");
    assert!(!store.is_banned(&pk).expect("is banned"));
}
