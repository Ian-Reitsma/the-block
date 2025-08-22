use once_cell::sync::Lazy;
use sled::Tree;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct BanStore {
    tree: Tree,
}

impl BanStore {
    pub fn open(path: &str) -> Self {
        let db = sled::open(path).unwrap_or_else(|e| panic!("open ban db: {e}"));
        let tree = db
            .open_tree("bans")
            .unwrap_or_else(|e| panic!("open bans tree: {e}"));
        Self { tree }
    }

    pub fn ban(&self, pk: &[u8; 32], until: u64) {
        let _ = self.tree.insert(pk, &until.to_be_bytes());
        #[cfg(feature = "telemetry")]
        tracing::info!(peer = %hex::encode(pk), until, "peer banned");
        self.update_metric();
    }

    pub fn unban(&self, pk: &[u8; 32]) {
        let _ = self.tree.remove(pk);
        #[cfg(feature = "telemetry")]
        tracing::info!(peer = %hex::encode(pk), "peer unbanned");
        self.update_metric();
    }

    pub fn is_banned(&self, pk: &[u8; 32]) -> bool {
        self.purge_expired();
        if let Ok(Some(v)) = self.tree.get(pk) {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&v);
            let ts = u64::from_be_bytes(arr);
            let now = current_ts();
            if ts > now {
                return true;
            }
        }
        false
    }

    pub fn purge_expired(&self) {
        let now = current_ts();
        let keys: Vec<Vec<u8>> = self
            .tree
            .iter()
            .filter_map(|res| res.ok())
            .filter_map(|(k, v)| {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&v);
                let ts = u64::from_be_bytes(arr);
                if ts <= now {
                    Some(k.to_vec())
                } else {
                    None
                }
            })
            .collect();
        for k in keys {
            let _ = self.tree.remove(k);
        }
        self.update_metric();
    }

    pub fn list(&self) -> Vec<(String, u64)> {
        self.purge_expired();
        self.tree
            .iter()
            .filter_map(|res| res.ok())
            .map(|(k, v)| {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&v);
                let ts = u64::from_be_bytes(arr);
                (hex::encode(k.as_ref()), ts)
            })
            .collect()
    }

    fn update_metric(&self) {
        #[cfg(feature = "telemetry")]
        {
            crate::telemetry::BANNED_PEERS_TOTAL.set(self.tree.len() as i64);
            crate::telemetry::BANNED_PEER_EXPIRATION.reset();
            for res in self.tree.iter().filter_map(|r| r.ok()) {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&res.1);
                let ts = u64::from_be_bytes(arr);
                crate::telemetry::BANNED_PEER_EXPIRATION
                    .with_label_values(&[&hex::encode(res.0.as_ref())])
                    .set(ts as i64);
            }
        }
    }
}

fn current_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|e| panic!("time error: {e}"))
        .as_secs()
}

pub static BAN_STORE: Lazy<Mutex<BanStore>> = Lazy::new(|| {
    let path = std::env::var("TB_BAN_DB").unwrap_or_else(|_| "ban_db".into());
    Mutex::new(BanStore::open(&path))
});
