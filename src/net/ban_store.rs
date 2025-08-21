use once_cell::sync::Lazy;
use sled::Tree;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct BanStore {
    tree: Tree,
}

impl BanStore {
    pub fn open(path: &str) -> Self {
        let db = sled::open(path).unwrap();
        let tree = db.open_tree("bans").unwrap();
        Self { tree }
    }

    pub fn ban(&self, pk: &[u8; 32], until: u64) {
        let _ = self.tree.insert(pk, &until.to_be_bytes());
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

    fn update_metric(&self) {
        #[cfg(feature = "telemetry")]
        {
            crate::telemetry::BANNED_PEERS_TOTAL.set(self.tree.len() as i64);
        }
    }
}

fn current_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub static BAN_STORE: Lazy<Mutex<BanStore>> = Lazy::new(|| {
    let path = std::env::var("TB_BAN_DB").unwrap_or_else(|_| "ban_db".into());
    Mutex::new(BanStore::open(&path))
});
