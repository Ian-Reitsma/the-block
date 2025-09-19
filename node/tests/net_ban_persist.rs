#![cfg(feature = "integration-tests")]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use the_block::net::ban_store::BanStore;

#[test]
fn ban_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bans");
    let store = BanStore::open(path.to_str().unwrap());
    let pk = [1u8; 32];
    let until = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 60;
    store.ban(&pk, until);
    assert!(store.is_banned(&pk));
    drop(store);
    let store2 = BanStore::open(path.to_str().unwrap());
    assert!(store2.is_banned(&pk));
}

#[test]
fn ban_expires() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bans");
    let store = BanStore::open(path.to_str().unwrap());
    let pk = [2u8; 32];
    let until = SystemTime::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    store.ban(&pk, until);
    store.purge_expired();
    assert!(!store.is_banned(&pk));
}
