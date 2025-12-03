use crate::launch_governor::DecisionPayload;
use crypto_suite::hashing::blake3;
use crypto_suite::hex;
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_serialization::json::{self, Map as JsonMap, Value as JsonValue};
use std::env;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

fn snapshot_dir(base: &str) -> PathBuf {
    Path::new(base).join("governor").join("decisions")
}

fn snapshot_path(base: &str, epoch: u64) -> (PathBuf, PathBuf) {
    let dir = snapshot_dir(base);
    let name = format!("epoch-{epoch:020}.json");
    let path = dir.join(&name);
    let sig_path = dir.join(format!("{name}.sig"));
    (path, sig_path)
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
    } else {
        Ok(())
    }
}

pub fn persist_snapshot(base: &str, payload: &DecisionPayload, epoch: u64) -> io::Result<[u8; 32]> {
    let (path, sig_path) = snapshot_path(base, epoch);
    ensure_parent(&path)?;
    let json_value = payload.to_json();
    let bytes = json::to_vec(&json_value)
        .map_err(|err| io::Error::new(ErrorKind::Other, err.to_string()))?;
    fs::write(&path, &bytes)?;
    let digest = blake3::hash(&bytes);
    if should_sign() {
        if let Some(sk) = signing_key_from_env() {
            let sig = sk.sign(digest.as_bytes());
            let mut sidecar = JsonMap::new();
            sidecar.insert(
                "pubkey_hex".into(),
                JsonValue::String(hex::encode(sk.verifying_key().to_bytes())),
            );
            sidecar.insert(
                "payload_hash_hex".into(),
                JsonValue::String(digest.to_hex().to_string()),
            );
            sidecar.insert(
                "signature_hex".into(),
                JsonValue::String(hex::encode(sig.to_bytes())),
            );
            let sig_bytes = json::to_vec(&JsonValue::Object(sidecar))
                .map_err(|err| io::Error::new(ErrorKind::Other, err.to_string()))?;
            let mut file = fs::File::create(&sig_path)?;
            file.write_all(&sig_bytes)?;
        }
    }
    Ok(*digest.as_bytes())
}

fn should_sign() -> bool {
    env::var("TB_GOVERNOR_SIGN")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn signing_key_from_env() -> Option<SigningKey> {
    let key_hex = env::var("TB_NODE_KEY_HEX").ok()?;
    let bytes = hex::decode(key_hex).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Some(SigningKey::from_bytes(&arr))
}

pub fn load_snapshot(base: &str, epoch: u64) -> Option<JsonValue> {
    let (path, sig_path) = snapshot_path(base, epoch);
    let payload = fs::read(&path).ok()?;
    let mut root: JsonMap = json::from_slice(&payload).ok()?;
    if let Ok(side_bytes) = fs::read(&sig_path) {
        if let Ok(JsonValue::Object(sig)) = json::from_slice::<JsonValue>(&side_bytes) {
            root.insert("attestation".into(), JsonValue::Object(sig));
        }
    }
    Some(JsonValue::Object(root))
}

pub fn list_snapshots(base: &str, start_epoch: u64, end_epoch: u64) -> Vec<JsonValue> {
    let mut items = Vec::new();
    for epoch in start_epoch..=end_epoch {
        if let Some(val) = load_snapshot(base, epoch) {
            items.push(val);
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch_governor::{GateAction, IntentMetrics};
    use foundation_serialization::json::Value;
    use std::collections::VecDeque;
    use sys::tempfile::TempDir;

    fn sample_payload() -> DecisionPayload {
        DecisionPayload {
            gate: "operational".into(),
            action: GateAction::Enter,
            reason: "tests".into(),
            intent_id: "op-1".into(),
            epoch: 7,
            metrics: IntentMetrics {
                summary: Value::String("ok".into()),
                raw: Value::Null,
            },
            params_patch: Value::Object(JsonMap::new()),
        }
    }

    #[test]
    fn signing_sidecar_round_trip() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().to_str().unwrap();
        env::set_var("TB_GOVERNOR_SIGN", "1");
        env::set_var("TB_NODE_KEY_HEX", hex::encode([5u8; 32]));
        let payload = sample_payload();
        let digest = persist_snapshot(path, &payload, payload.epoch).expect("snapshot");
        let loaded = load_snapshot(path, payload.epoch).expect("load snapshot");
        let obj = loaded.as_object().expect("object");
        assert_eq!(
            obj.get("gate").and_then(|v| v.as_str()),
            Some("operational")
        );
        let attestation = obj
            .get("attestation")
            .and_then(|v| v.as_object())
            .expect("attestation");
        let digest_hex = hex::encode(digest);
        assert_eq!(
            attestation.get("payload_hash_hex").and_then(|v| v.as_str()),
            Some(digest_hex.as_str())
        );
        env::remove_var("TB_GOVERNOR_SIGN");
        env::remove_var("TB_NODE_KEY_HEX");
    }

    #[test]
    fn list_snapshots_empty_ok() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().to_str().unwrap();
        let list = list_snapshots(path, 1, 3);
        assert!(list.is_empty());
    }
}
