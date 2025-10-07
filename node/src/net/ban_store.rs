use once_cell::sync::OnceCell;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::simple_db::{names, SimpleDb};

pub struct BanStore {
    db: Mutex<SimpleDb>,
}

/// Minimal trait so callers (and tests) can provide an alternate backend
/// without touching the on-disk `sled` database.
pub trait BanStoreLike {
    fn ban(&self, pk: &[u8; 32], until: u64);
    fn unban(&self, pk: &[u8; 32]);
    fn list(&self) -> Vec<(String, u64)>;
}

impl BanStore {
    pub fn open(path: &str) -> Self {
        let db = SimpleDb::open_named(names::NET_BANS, path);
        Self { db: Mutex::new(db) }
    }

    pub fn ban(&self, pk: &[u8; 32], until: u64) {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let key = key_for(pk);
        db.put(key.as_bytes(), &until.to_be_bytes())
            .unwrap_or_else(|e| panic!("store ban {key}: {e}"));
        drop(db);
        #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
        tracing::info!(peer = %hex::encode(pk), until, "peer banned");
        self.update_metric();
    }

    pub fn unban(&self, pk: &[u8; 32]) {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let key = key_for(pk);
        let _ = db.try_remove(&key);
        drop(db);
        #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
        tracing::info!(peer = %hex::encode(pk), "peer unbanned");
        self.update_metric();
    }

    pub fn is_banned(&self, pk: &[u8; 32]) -> bool {
        self.purge_expired();
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let key = key_for(pk);
        if let Some(ts) = db.get(&key).and_then(as_timestamp) {
            return ts > current_ts();
        }
        false
    }

    pub fn purge_expired(&self) {
        let now = current_ts();
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let keys: Vec<String> = db
            .keys_with_prefix("")
            .into_iter()
            .filter(|key| {
                db.get(key)
                    .and_then(as_timestamp)
                    .map(|ts| ts <= now)
                    .unwrap_or(false)
            })
            .collect();
        for key in keys {
            let _ = db.try_remove(&key);
        }
        drop(db);
        self.update_metric();
    }

    pub fn list(&self) -> Vec<(String, u64)> {
        self.purge_expired();
        self.entries()
    }

    fn update_metric(&self) {
        #[cfg(feature = "telemetry")]
        {
            let entries = self.entries();
            crate::telemetry::BANNED_PEERS_TOTAL.set(entries.len() as i64);
            crate::telemetry::BANNED_PEER_EXPIRATION.reset();
            for (peer, ts) in entries {
                crate::telemetry::BANNED_PEER_EXPIRATION
                    .with_label_values(&[&peer])
                    .set(ts as i64);
            }
        }
    }

    fn entries(&self) -> Vec<(String, u64)> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.keys_with_prefix("")
            .into_iter()
            .filter_map(|key| db.get(&key).and_then(as_timestamp).map(|ts| (key, ts)))
            .collect()
    }
}

impl BanStoreLike for BanStore {
    fn ban(&self, pk: &[u8; 32], until: u64) {
        BanStore::ban(self, pk, until);
    }

    fn unban(&self, pk: &[u8; 32]) {
        BanStore::unban(self, pk);
    }

    fn list(&self) -> Vec<(String, u64)> {
        BanStore::list(self)
    }
}

fn current_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|e| panic!("time error: {e}"))
        .as_secs()
}

fn key_for(pk: &[u8; 32]) -> String {
    hex::encode(pk)
}

fn as_timestamp(bytes: Vec<u8>) -> Option<u64> {
    if bytes.len() != 8 {
        return None;
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes);
    Some(u64::from_be_bytes(arr))
}

static BAN_STORE: OnceCell<Mutex<BanStore>> = OnceCell::new();

/// Obtain the global ban store.
pub fn store() -> &'static Mutex<BanStore> {
    BAN_STORE.get_or_init(|| {
        let path = std::env::var("TB_BAN_DB").unwrap_or_else(|_| "ban_db".into());
        Mutex::new(BanStore::open(&path))
    })
}

/// Replace the global ban store with one backed by `path`.
/// Primarily used by tests to isolate state.
pub fn init(path: &str) {
    if let Some(store) = BAN_STORE.get() {
        let mut guard = store.lock().unwrap_or_else(|e| e.into_inner());
        *guard = BanStore::open(path);
    } else {
        let _ = BAN_STORE.set(Mutex::new(BanStore::open(path)));
    }
}
