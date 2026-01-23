use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::hex;
use httpd::{BlockingClient, Method, StatusCode};
use std::{env, fs, path::PathBuf, time::Duration};

pub struct DriveStore {
    base_dir: PathBuf,
    peers: Vec<String>,
    allow_peer_fetch: bool,
    timeout: Duration,
    gateway_url: String,
}

impl DriveStore {
    pub fn from_env() -> Self {
        let base_dir = env::var("TB_DRIVE_BASE_DIR").unwrap_or_else(|_| "blobstore/drive".into());
        let peers = env::var("TB_DRIVE_PEERS")
            .unwrap_or_default()
            .split(',')
            .map(|entry| entry.trim().to_string())
            .filter(|entry| !entry.is_empty())
            .collect();
        let allow_peer_fetch = env::var("TB_DRIVE_ALLOW_PEER_FETCH")
            .map(|value| value != "0")
            .unwrap_or(true);
        let timeout_ms = env::var("TB_DRIVE_FETCH_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(3000);
        let gateway_url =
            env::var("TB_GATEWAY_URL").unwrap_or_else(|_| "http://localhost:9000".into());
        Self::new(
            PathBuf::from(base_dir),
            peers,
            allow_peer_fetch,
            Duration::from_millis(timeout_ms),
            gateway_url,
        )
    }

    fn new(
        base_dir: PathBuf,
        peers: Vec<String>,
        allow_peer_fetch: bool,
        timeout: Duration,
        gateway_url: String,
    ) -> Self {
        Self {
            base_dir,
            peers,
            allow_peer_fetch,
            timeout,
            gateway_url,
        }
    }

    pub fn with_base(base_dir: PathBuf) -> Self {
        Self::new(
            base_dir,
            Vec::new(),
            false,
            Duration::from_secs(1),
            "http://localhost:9000".into(),
        )
    }

    pub fn store(&self, data: &[u8]) -> Result<String, String> {
        let mut hasher = Hasher::new();
        hasher.update(data);
        let id = hex::encode(hasher.finalize().as_bytes());
        let path = self.local_path(&id);
        if path.exists() {
            return Ok(id);
        }
        fs::create_dir_all(&self.base_dir).map_err(|err| err.to_string())?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, data).map_err(|err| err.to_string())?;
        fs::rename(&tmp, &path).map_err(|err| err.to_string())?;
        Ok(id)
    }

    pub fn fetch(&self, object_id: &str) -> Option<Vec<u8>> {
        if !is_valid_object_id(object_id) {
            return None;
        }
        if let Some(bytes) = self.read_local(object_id) {
            return Some(bytes);
        }
        if self.allow_peer_fetch {
            if let Some(bytes) = self.fetch_from_peers(object_id) {
                let _ = self.store(&bytes);
                return Some(bytes);
            }
        }
        None
    }

    pub fn share_url(&self, object_id: &str) -> String {
        let base = self.gateway_url.trim_end_matches('/');
        format!("{}/drive/{}", base, object_id)
    }

    fn local_path(&self, object_id: &str) -> PathBuf {
        self.base_dir.join(object_id)
    }

    fn read_local(&self, object_id: &str) -> Option<Vec<u8>> {
        fs::read(self.local_path(object_id)).ok()
    }

    fn fetch_from_peers(&self, object_id: &str) -> Option<Vec<u8>> {
        for peer in &self.peers {
            if peer.is_empty() {
                continue;
            }
            let url = format!("{}/drive/{}", peer.trim_end_matches('/'), object_id);
            let client = BlockingClient::default();
            let builder = match client.request(Method::Get, &url) {
                Ok(builder) => builder,
                Err(_) => continue,
            };
            let response = builder
                .timeout(self.timeout)
                .header("accept", "application/octet-stream")
                .send();
            if let Ok(resp) = response {
                if resp.status() == StatusCode::OK {
                    return Some(resp.body().to_vec());
                }
            }
        }
        None
    }
}

fn is_valid_object_id(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::tempfile::tempdir;

    #[test]
    fn store_and_fetch_round_trip() {
        let dir = tempdir().expect("tempdir");
        let store = DriveStore::with_base(dir.path().join("drive"));
        let payload = b"drive-datastream";
        let object_id = store.store(payload).expect("store object");
        assert_eq!(store.fetch(&object_id).as_deref(), Some(payload.as_ref()));
        let share_url = store.share_url(&object_id);
        assert!(share_url.contains(&object_id));
        assert!(store.fetch("deadbeef").is_none());
        assert!(store
            .fetch("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz")
            .is_none());
    }
}
