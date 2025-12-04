#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{Error as IoError, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ad_market::{DomainTier, DistributionPolicy, MarketplaceHandle};
use crypto_suite::{encoding::hex, hashing::blake3, signatures::ed25519::SigningKey};
use foundation_serialization::json::{
    self, Map as JsonMap, Number as JsonNumber, Value as JsonValue,
};

fn dir_for(base: &str) -> PathBuf {
    Path::new(base).join("ad_policy")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn number_from_u64(v: u64) -> JsonNumber {
    JsonNumber::from(v)
}

fn snapshot_path(base: &str, epoch: u64) -> (PathBuf, PathBuf) {
    let dir = dir_for(base);
    (
        dir.join(format!("{}.json", epoch)),
        dir.join(format!("{}.sig", epoch)),
    )
}

const DEFAULT_SNAPSHOT_RETENTION: usize = 336;
const PPM_SCALE: u64 = 1_000_000;

fn snapshot_retention_limit() -> usize {
    env::var("TB_AD_POLICY_SNAPSHOT_KEEP")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|limit| *limit > 0)
        .unwrap_or(DEFAULT_SNAPSHOT_RETENTION)
}

fn prune_snapshots(base: &str, limit: usize) {
    if limit == 0 {
        return;
    }
    let dir = dir_for(base);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let mut epochs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            if let Ok(epoch) = stem.parse::<u64>() {
                epochs.push(epoch);
            }
        }
    }
    if epochs.len() <= limit {
        return;
    }
    epochs.sort_unstable();
    let prune_count = epochs.len() - limit;
    for epoch in epochs.into_iter().take(prune_count) {
        let (json_path, sig_path) = snapshot_path(base, epoch);
        let _ = fs::remove_file(json_path);
        let _ = fs::remove_file(sig_path);
    }
}

fn previous_snapshot_distribution(base: &str, current_epoch: u64) -> Option<[u64; 5]> {
    if current_epoch == 0 {
        return None;
    }
    let dir = dir_for(base);
    let entries = fs::read_dir(&dir).ok()?;
    let mut candidate: Option<u64> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let stem = path.file_stem()?.to_str()?;
        let Ok(epoch) = stem.parse::<u64>() else {
            continue;
        };
        if epoch < current_epoch {
            candidate = match candidate {
                Some(existing) if existing >= epoch => Some(existing),
                _ => Some(epoch),
            };
        }
    }
    let prev_epoch = candidate?;
    load_snapshot(base, prev_epoch).and_then(|value| distribution_array(&value))
}

fn distribution_array(value: &JsonValue) -> Option<[u64; 5]> {
    let obj = value.as_object()?;
    let dist = obj.get("distribution")?.as_object()?;
    Some([
        dist.get("viewer_percent")?.as_u64()?,
        dist.get("host_percent")?.as_u64()?,
        dist.get("hardware_percent")?.as_u64()?,
        dist.get("verifier_percent")?.as_u64()?,
        dist.get("liquidity_percent")?.as_u64()?,
    ])
}

fn compute_drift_ppm(current: &[u64; 5], previous: &[u64; 5]) -> [i64; 5] {
    let mut out = [0i64; 5];
    for (idx, value) in out.iter_mut().enumerate() {
        let before = previous[idx] as i64;
        let after = current[idx] as i64;
        *value = (after - before) * (PPM_SCALE as i64 / 100);
    }
    out
}

pub fn persist_snapshot(base: &str, market: &MarketplaceHandle, epoch: u64) -> std::io::Result<()> {
    let (storage, verifier, host) = market.cost_medians_usd_micros();
    let policy: DistributionPolicy = market.distribution();
    let mut root = JsonMap::new();
    root.insert("epoch".into(), JsonValue::Number(number_from_u64(epoch)));
    root.insert(
        "generated_at".into(),
        JsonValue::Number(number_from_u64(now_secs())),
    );
    let mut pol = JsonMap::new();
    pol.insert(
        "viewer_percent".into(),
        JsonValue::Number(number_from_u64(policy.viewer_percent)),
    );
    pol.insert(
        "host_percent".into(),
        JsonValue::Number(number_from_u64(policy.host_percent)),
    );
    pol.insert(
        "hardware_percent".into(),
        JsonValue::Number(number_from_u64(policy.hardware_percent)),
    );
    pol.insert(
        "verifier_percent".into(),
        JsonValue::Number(number_from_u64(policy.verifier_percent)),
    );
    pol.insert(
        "liquidity_percent".into(),
        JsonValue::Number(number_from_u64(policy.liquidity_percent)),
    );
    pol.insert(
        "liquidity_split_ct_ppm".into(),
        JsonValue::Number(JsonNumber::from(policy.liquidity_split_ct_ppm as u64)),
    );
    pol.insert(
        "normalized_liquidity_ppm".into(),
        JsonValue::Number(number_from_u64(
            (policy.liquidity_split_ct_ppm.min(PPM_SCALE as u32)) as u64,
        )),
    );
    pol.insert(
        "dual_token_settlement_enabled".into(),
        JsonValue::Bool(policy.dual_token_settlement_enabled),
    );
    root.insert("distribution".into(), JsonValue::Object(pol));
    let mut med = JsonMap::new();
    med.insert(
        "storage_price_per_mib_usd_micros".into(),
        JsonValue::Number(number_from_u64(storage)),
    );
    med.insert(
        "verifier_cost_usd_micros".into(),
        JsonValue::Number(number_from_u64(verifier)),
    );
    med.insert(
        "host_fee_usd_micros".into(),
        JsonValue::Number(number_from_u64(host)),
    );
    root.insert("medians".into(), JsonValue::Object(med));

    // Compute domain tier supply from cohort prices
    let cohort_prices = market.cohort_prices();
    let mut domain_tier_supply: std::collections::HashMap<DomainTier, u64> =
        std::collections::HashMap::new();
    let mut interest_tag_supply: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    let mut presence_bucket_count = 0u64;
    let mut presence_ready_slots = 0u64;

    for cohort in &cohort_prices {
        // Aggregate by domain tier
        *domain_tier_supply.entry(cohort.domain_tier).or_insert(0) += 1;

        // Aggregate by interest tags
        for tag in &cohort.interest_tags {
            *interest_tag_supply.entry(tag.clone()).or_insert(0) += 1;
        }

        // Count presence buckets
        if cohort.presence_bucket.is_some() {
            presence_bucket_count += 1;
            // Placeholder: in production, ready slots would be computed from readiness snapshot
            presence_ready_slots += 1;
        }
    }

    let total_cohorts = cohort_prices.len().max(1) as u64;

    // Domain tier supply in ppm
    let mut tier_supply_map = JsonMap::new();
    for tier in [
        DomainTier::Premium,
        DomainTier::Reserved,
        DomainTier::Community,
        DomainTier::Unverified,
    ] {
        let count = domain_tier_supply.get(&tier).copied().unwrap_or(0);
        let ppm = (count * 1_000_000) / total_cohorts;
        tier_supply_map.insert(
            tier.as_str().into(),
            JsonValue::Number(JsonNumber::from(ppm)),
        );
    }
    root.insert("domain_tier_supply_ppm".into(), JsonValue::Object(tier_supply_map));

    // Interest tag supply in ppm (top 20 tags only to limit snapshot size)
    let mut tag_supply: Vec<_> = interest_tag_supply.into_iter().collect();
    tag_supply.sort_by(|a, b| b.1.cmp(&a.1));
    let mut tag_supply_map = JsonMap::new();
    for (tag, count) in tag_supply.into_iter().take(20) {
        let ppm = (count * 1_000_000) / total_cohorts;
        tag_supply_map.insert(tag, JsonValue::Number(JsonNumber::from(ppm)));
    }
    root.insert(
        "interest_tag_supply_ppm".into(),
        JsonValue::Object(tag_supply_map),
    );

    // Presence bucket metrics
    let mut presence_map = JsonMap::new();
    let presence_ppm = (presence_bucket_count * 1_000_000) / total_cohorts;
    presence_map.insert(
        "bucket_count".into(),
        JsonValue::Number(JsonNumber::from(presence_bucket_count)),
    );
    presence_map.insert(
        "bucket_supply_ppm".into(),
        JsonValue::Number(JsonNumber::from(presence_ppm)),
    );
    presence_map.insert(
        "ready_slots".into(),
        JsonValue::Number(JsonNumber::from(presence_ready_slots)),
    );
    root.insert("presence_bucket_stats".into(), JsonValue::Object(presence_map));

    // Selectors version
    let selectors_version = cohort_prices
        .first()
        .map(|c| c.selectors_version)
        .unwrap_or(1);
    root.insert(
        "selectors_version".into(),
        JsonValue::Number(JsonNumber::from(selectors_version as u64)),
    );

    if let Some(previous) = previous_snapshot_distribution(base, epoch) {
        let current = [
            policy.viewer_percent,
            policy.host_percent,
            policy.hardware_percent,
            policy.verifier_percent,
            policy.liquidity_percent,
        ];
        let drift = compute_drift_ppm(&current, &previous);
        let mut drift_map = JsonMap::new();
        drift_map.insert(
            "viewer_ppm".into(),
            JsonValue::Number(JsonNumber::from(drift[0])),
        );
        drift_map.insert(
            "host_ppm".into(),
            JsonValue::Number(JsonNumber::from(drift[1])),
        );
        drift_map.insert(
            "hardware_ppm".into(),
            JsonValue::Number(JsonNumber::from(drift[2])),
        );
        drift_map.insert(
            "verifier_ppm".into(),
            JsonValue::Number(JsonNumber::from(drift[3])),
        );
        drift_map.insert(
            "liquidity_ppm".into(),
            JsonValue::Number(JsonNumber::from(drift[4])),
        );
        root.insert(
            "distribution_drift_ppm".into(),
            JsonValue::Object(drift_map),
        );
        root.insert(
            "liquidity_drift_ppm".into(),
            JsonValue::Number(JsonNumber::from(drift[4])),
        );
    }

    // Ensure directory
    let (path, sig_path) = snapshot_path(base, epoch);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let payload = json::to_vec(&JsonValue::Object(root.clone()))
        .map_err(|e| IoError::new(ErrorKind::Other, e.to_string()))?;
    fs::write(&path, &payload)?;

    // Optional signing using TB_NODE_KEY_HEX
    if let Ok(key_hex) = env::var("TB_NODE_KEY_HEX") {
        if key_hex.len() == 64 {
            if let Ok(bytes) = hex::decode(&key_hex) {
                if bytes.len() == 32 {
                    let sk = SigningKey::from_bytes(&bytes.try_into().unwrap());
                    let digest = blake3::hash(&payload);
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
                    let bytes = json::to_vec(&JsonValue::Object(sidecar))
                        .map_err(|e| IoError::new(ErrorKind::Other, e.to_string()))?;
                    let mut f = fs::File::create(&sig_path)?;
                    f.write_all(&bytes)?;
                }
            }
        }
    }
    prune_snapshots(base, snapshot_retention_limit());
    Ok(())
}

pub fn load_snapshot(base: &str, epoch: u64) -> Option<JsonValue> {
    let (path, sig_path) = snapshot_path(base, epoch);
    let payload = fs::read(&path).ok()?;
    let mut root: JsonMap = json::from_slice(&payload).ok()?;
    if let Ok(side) = fs::read(&sig_path) {
        if !side.is_empty() {
            if let Ok(JsonValue::Object(obj)) = json::from_slice::<JsonValue>(&side) {
                root.insert("attestation".into(), JsonValue::Object(obj));
            }
        }
    }
    Some(JsonValue::Object(root))
}

pub fn list_snapshots(base: &str, start_epoch: u64, end_epoch: u64) -> Vec<JsonValue> {
    let mut items = Vec::new();
    for e in start_epoch..=end_epoch {
        if let Some(v) = load_snapshot(base, e) {
            items.push(v);
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use ad_market::{InMemoryMarketplace, MarketplaceConfig};
    use crypto_suite::signatures::ed25519::{Signature, SigningKey, VerifyingKey};
    use foundation_serialization::json;
    use std::sync::Arc;
    use sys::tempfile::TempDir;

    fn fixed_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn snapshot_sidecar_signature_verifies() {
        let tmp = TempDir::new().expect("tempdir");
        let base = tmp.path().to_str().expect("utf8");
        let signing = fixed_signing_key();
        env::set_var("TB_NODE_KEY_HEX", hex::encode(signing.to_bytes()));
        let market: MarketplaceHandle =
            Arc::new(InMemoryMarketplace::new(MarketplaceConfig::default()));
        persist_snapshot(base, &market, 1).expect("persist snapshot");
        env::remove_var("TB_NODE_KEY_HEX");

        let (json_path, sig_path) = snapshot_path(base, 1);
        let payload = fs::read(json_path).expect("read snapshot");
        let digest = blake3::hash(&payload);
        let sidecar_bytes = fs::read(sig_path).expect("read sidecar");
        let sidecar: JsonValue = json::from_slice(&sidecar_bytes).expect("decode sidecar");
        let obj = sidecar.as_object().expect("sidecar object");
        let pub_hex = obj
            .get("pubkey_hex")
            .and_then(JsonValue::as_str)
            .expect("pubkey");
        let sig_hex = obj
            .get("signature_hex")
            .and_then(JsonValue::as_str)
            .expect("signature");
        let hash_hex = obj
            .get("payload_hash_hex")
            .and_then(JsonValue::as_str)
            .expect("hash");
        assert_eq!(hash_hex, digest.to_hex().to_string());
        let pub_vec = hex::decode(pub_hex).expect("decode pubkey");
        let pub_arr: [u8; 32] = pub_vec.try_into().expect("pubkey length");
        let verifying = VerifyingKey::from_bytes(&pub_arr).expect("verifying key");
        let sig_vec = hex::decode(sig_hex).expect("decode signature");
        let sig_arr: [u8; 64] = sig_vec.try_into().expect("signature length");
        let signature = Signature::from_bytes(&sig_arr);
        verifying
            .verify(digest.as_bytes(), &signature)
            .expect("signature valid");
    }

    #[test]
    fn snapshot_retention_prunes_old_entries() {
        let tmp = TempDir::new().expect("tempdir");
        let base = tmp.path().to_str().expect("utf8");
        env::set_var("TB_AD_POLICY_SNAPSHOT_KEEP", "3");
        let market: MarketplaceHandle =
            Arc::new(InMemoryMarketplace::new(MarketplaceConfig::default()));
        for epoch in 0..5u64 {
            persist_snapshot(base, &market, epoch).expect("persist snapshot");
        }
        env::remove_var("TB_AD_POLICY_SNAPSHOT_KEEP");
        let dir = dir_for(base);
        let mut epochs: Vec<u64> = fs::read_dir(dir)
            .expect("read dir")
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    return None;
                }
                path.file_stem()?.to_str()?.parse::<u64>().ok()
            })
            .collect();
        epochs.sort_unstable();
        assert_eq!(epochs, vec![2, 3, 4]);
    }
}
