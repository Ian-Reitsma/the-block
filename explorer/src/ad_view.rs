#![forbid(unsafe_code)]

use foundation_serialization::{
    json::{self, Map as JsonMap, Value as JsonValue},
    Deserialize, Serialize,
};
use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::Path;
use the_block::{
    ad_policy_snapshot,
    ad_readiness::{
        AdReadinessCohortUtilization, AdReadinessConfig, AdReadinessHandle, AdReadinessSnapshot,
        AdReadinessUtilizationSummary,
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdPolicyDistributionView {
    pub viewer_percent: u64,
    pub host_percent: u64,
    pub hardware_percent: u64,
    pub verifier_percent: u64,
    pub liquidity_percent: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DistributionDriftView {
    pub viewer_ppm: i64,
    pub host_ppm: i64,
    pub hardware_ppm: i64,
    pub verifier_ppm: i64,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub liquidity_ppm: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdPolicyCostView {
    pub storage_price_per_mib_usd_micros: u64,
    pub verifier_cost_usd_micros: u64,
    pub host_fee_usd_micros: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdPolicySnapshotSummary {
    pub epoch: u64,
    pub generated_at: u64,
    pub distribution: AdPolicyDistributionView,
    pub medians: AdPolicyCostView,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub drift: Option<DistributionDriftView>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub liquidity_drift_ppm: Option<i64>,
    pub has_attestation: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdPolicyAttestationView {
    pub pubkey_hex: String,
    pub payload_hash_hex: String,
    pub signature_hex: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdPolicySnapshotDetail {
    #[serde(flatten)]
    pub summary: AdPolicySnapshotSummary,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub attestation: Option<AdPolicyAttestationView>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdReadinessConfigView {
    pub window_secs: u64,
    pub min_unique_viewers: u64,
    pub min_host_count: u64,
    pub min_provider_count: u64,
    pub use_percentile_thresholds: bool,
    pub viewer_percentile: u8,
    pub host_percentile: u8,
    pub provider_percentile: u8,
    pub ema_smoothing_ppm: u32,
    pub floor_unique_viewers: u64,
    pub floor_host_count: u64,
    pub floor_provider_count: u64,
    pub cap_unique_viewers: u64,
    pub cap_host_count: u64,
    pub cap_provider_count: u64,
    pub percentile_buckets: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdReadinessSnapshotView {
    pub window_secs: u64,
    pub min_unique_viewers: u64,
    pub min_host_count: u64,
    pub min_provider_count: u64,
    pub unique_viewers: u64,
    pub host_count: u64,
    pub provider_count: u64,
    pub ready: bool,
    #[serde(default)]
    pub blockers: Vec<String>,
    pub last_updated: u64,
    pub total_usd_micros: u64,
    pub settlement_count: u64,
    pub price_usd_micros: u64,
    pub market_price_usd_micros: u64,
    #[serde(default)]
    pub cohort_utilization: Vec<AdReadinessCohortUtilization>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub utilization_summary: Option<AdReadinessUtilizationSummary>,
    pub ready_streak_windows: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdReadinessStatusView {
    pub config: AdReadinessConfigView,
    pub snapshot: AdReadinessSnapshotView,
    pub rehearsal_enabled: bool,
    pub rehearsal_required_windows: u64,
}

fn parse_u64(map: &JsonMap, key: &str) -> Option<u64> {
    map.get(key).and_then(JsonValue::as_u64)
}

fn parse_i64(map: &JsonMap, key: &str) -> Option<i64> {
    map.get(key).and_then(JsonValue::as_i64)
}

fn parse_distribution(map: &JsonMap) -> AdPolicyDistributionView {
    AdPolicyDistributionView {
        viewer_percent: parse_u64(map, "viewer_percent").unwrap_or_default(),
        host_percent: parse_u64(map, "host_percent").unwrap_or_default(),
        hardware_percent: parse_u64(map, "hardware_percent").unwrap_or_default(),
        verifier_percent: parse_u64(map, "verifier_percent").unwrap_or_default(),
        liquidity_percent: parse_u64(map, "liquidity_percent").unwrap_or_default(),
    }
}

fn parse_drift(map: &JsonMap) -> Option<DistributionDriftView> {
    let obj = map.get("distribution_drift_ppm")?.as_object()?;
    Some(DistributionDriftView {
        viewer_ppm: parse_i64(obj, "viewer_ppm").unwrap_or_default(),
        host_ppm: parse_i64(obj, "host_ppm").unwrap_or_default(),
        hardware_ppm: parse_i64(obj, "hardware_ppm").unwrap_or_default(),
        verifier_ppm: parse_i64(obj, "verifier_ppm").unwrap_or_default(),
        liquidity_ppm: parse_i64(obj, "liquidity_ppm"),
    })
}

fn parse_medians(map: &JsonMap) -> AdPolicyCostView {
    AdPolicyCostView {
        storage_price_per_mib_usd_micros: parse_u64(map, "storage_price_per_mib_usd_micros")
            .unwrap_or_default(),
        verifier_cost_usd_micros: parse_u64(map, "verifier_cost_usd_micros").unwrap_or_default(),
        host_fee_usd_micros: parse_u64(map, "host_fee_usd_micros").unwrap_or_default(),
    }
}

fn parse_attestation(value: &JsonValue) -> Option<AdPolicyAttestationView> {
    let obj = value.as_object()?;
    Some(AdPolicyAttestationView {
        pubkey_hex: obj
            .get("pubkey_hex")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string(),
        payload_hash_hex: obj
            .get("payload_hash_hex")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string(),
        signature_hex: obj
            .get("signature_hex")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

pub fn parse_policy_snapshot(value: &JsonValue) -> Option<AdPolicySnapshotDetail> {
    let obj = value.as_object()?;
    let epoch = parse_u64(obj, "epoch")?;
    let generated_at = parse_u64(obj, "generated_at").unwrap_or_default();
    let distribution = obj
        .get("distribution")
        .and_then(JsonValue::as_object)
        .map(parse_distribution)
        .unwrap_or_else(|| AdPolicyDistributionView {
            viewer_percent: 0,
            host_percent: 0,
            hardware_percent: 0,
            verifier_percent: 0,
            liquidity_percent: 0,
        });
    let medians = obj
        .get("medians")
        .and_then(JsonValue::as_object)
        .map(parse_medians)
        .unwrap_or_else(|| AdPolicyCostView {
            storage_price_per_mib_usd_micros: 0,
            verifier_cost_usd_micros: 0,
            host_fee_usd_micros: 0,
        });
    let drift = parse_drift(obj);
    let liquidity_drift_ppm = parse_i64(obj, "liquidity_drift_ppm");

    let attestation = obj.get("attestation").and_then(parse_attestation);

    let summary = AdPolicySnapshotSummary {
        epoch,
        generated_at,
        distribution,
        medians,
        drift,
        liquidity_drift_ppm,
        has_attestation: attestation.is_some(),
    };

    Some(AdPolicySnapshotDetail {
        summary,
        attestation,
    })
}

pub fn read_policy_snapshot(
    base_dir: &Path,
    epoch: u64,
) -> io::Result<Option<AdPolicySnapshotDetail>> {
    let normalized_base: &Path = match base_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == "ad_policy")
    {
        Some(true) => base_dir.parent().unwrap_or(base_dir),
        _ => base_dir,
    };
    let base_str = normalized_base
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "policy path not UTF-8"))?;
    Ok(ad_policy_snapshot::load_snapshot(base_str, epoch)
        .and_then(|value| parse_policy_snapshot(&value)))
}

pub fn list_policy_snapshots(
    base_dir: &Path,
    start_epoch: Option<u64>,
    end_epoch: Option<u64>,
    limit: usize,
) -> io::Result<Vec<AdPolicySnapshotSummary>> {
    let mut epochs = available_policy_epochs(base_dir)?;
    if let Some(start) = start_epoch {
        epochs.retain(|epoch| *epoch >= start);
    }
    if let Some(end) = end_epoch {
        epochs.retain(|epoch| *epoch <= end);
    }
    epochs.sort_unstable_by(|a, b| b.cmp(a));

    let mut out = Vec::new();
    for epoch in epochs.into_iter().take(limit) {
        if let Some(detail) = read_policy_snapshot(base_dir, epoch)? {
            out.push(detail.summary);
        }
    }
    Ok(out)
}

fn available_policy_epochs(base_dir: &Path) -> io::Result<Vec<u64>> {
    let mut epochs = Vec::new();
    if !base_dir.exists() {
        return Ok(epochs);
    }
    for entry in fs::read_dir(base_dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(ext) = path.extension() {
            if ext != "json" {
                continue;
            }
        } else {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if let Ok(epoch) = stem.parse::<u64>() {
                epochs.push(epoch);
            }
        }
    }
    epochs.sort_unstable_by(|a, b| {
        if a == b {
            Ordering::Equal
        } else if a < b {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    });
    epochs.dedup();
    Ok(epochs)
}

pub fn readiness_status_view(
    config: &AdReadinessConfig,
    snapshot: &AdReadinessSnapshot,
    rehearsal_enabled: bool,
    rehearsal_required_windows: u64,
) -> AdReadinessStatusView {
    AdReadinessStatusView {
        config: AdReadinessConfigView {
            window_secs: config.window_secs,
            min_unique_viewers: config.min_unique_viewers,
            min_host_count: config.min_host_count,
            min_provider_count: config.min_provider_count,
            use_percentile_thresholds: config.use_percentile_thresholds,
            viewer_percentile: config.viewer_percentile,
            host_percentile: config.host_percentile,
            provider_percentile: config.provider_percentile,
            ema_smoothing_ppm: config.ema_smoothing_ppm,
            floor_unique_viewers: config.floor_unique_viewers,
            floor_host_count: config.floor_host_count,
            floor_provider_count: config.floor_provider_count,
            cap_unique_viewers: config.cap_unique_viewers,
            cap_host_count: config.cap_host_count,
            cap_provider_count: config.cap_provider_count,
            percentile_buckets: config.percentile_buckets,
        },
        snapshot: AdReadinessSnapshotView {
            window_secs: snapshot.window_secs,
            min_unique_viewers: snapshot.min_unique_viewers,
            min_host_count: snapshot.min_host_count,
            min_provider_count: snapshot.min_provider_count,
            unique_viewers: snapshot.unique_viewers,
            host_count: snapshot.host_count,
            provider_count: snapshot.provider_count,
            ready: snapshot.ready,
            blockers: snapshot.blockers.clone(),
            last_updated: snapshot.last_updated,
            total_usd_micros: snapshot.total_usd_micros,
            settlement_count: snapshot.settlement_count,
            price_usd_micros: snapshot.price_usd_micros,
            market_price_usd_micros: snapshot.market_price_usd_micros,
            cohort_utilization: snapshot.cohort_utilization.clone(),
            utilization_summary: snapshot.utilization_summary.clone(),
            ready_streak_windows: snapshot.ready_streak_windows,
        },
        rehearsal_enabled,
        rehearsal_required_windows,
    }
}

pub fn summary_to_json(summary: &AdPolicySnapshotSummary) -> JsonValue {
    let mut root = JsonMap::new();
    root.insert("epoch".into(), JsonValue::Number(summary.epoch.into()));
    root.insert(
        "generated_at".into(),
        JsonValue::Number(summary.generated_at.into()),
    );
    // distribution
    let mut dist = JsonMap::new();
    dist.insert(
        "viewer_percent".into(),
        JsonValue::Number(summary.distribution.viewer_percent.into()),
    );
    dist.insert(
        "host_percent".into(),
        JsonValue::Number(summary.distribution.host_percent.into()),
    );
    dist.insert(
        "hardware_percent".into(),
        JsonValue::Number(summary.distribution.hardware_percent.into()),
    );
    dist.insert(
        "verifier_percent".into(),
        JsonValue::Number(summary.distribution.verifier_percent.into()),
    );
    dist.insert(
        "liquidity_percent".into(),
        JsonValue::Number(summary.distribution.liquidity_percent.into()),
    );
    root.insert("distribution".into(), JsonValue::Object(dist));
    // medians
    let mut med = JsonMap::new();
    med.insert(
        "storage_price_per_mib_usd_micros".into(),
        JsonValue::Number(summary.medians.storage_price_per_mib_usd_micros.into()),
    );
    med.insert(
        "verifier_cost_usd_micros".into(),
        JsonValue::Number(summary.medians.verifier_cost_usd_micros.into()),
    );
    med.insert(
        "host_fee_usd_micros".into(),
        JsonValue::Number(summary.medians.host_fee_usd_micros.into()),
    );
    root.insert("medians".into(), JsonValue::Object(med));
    // drift
    if let Some(d) = &summary.drift {
        let mut drift = JsonMap::new();
        drift.insert("viewer_ppm".into(), JsonValue::Number(d.viewer_ppm.into()));
        drift.insert("host_ppm".into(), JsonValue::Number(d.host_ppm.into()));
        drift.insert(
            "hardware_ppm".into(),
            JsonValue::Number(d.hardware_ppm.into()),
        );
        drift.insert(
            "verifier_ppm".into(),
            JsonValue::Number(d.verifier_ppm.into()),
        );
        if let Some(lp) = d.liquidity_ppm {
            drift.insert("liquidity_ppm".into(), JsonValue::Number(lp.into()));
        }
        root.insert("distribution_drift_ppm".into(), JsonValue::Object(drift));
    }
    if let Some(liq) = summary.liquidity_drift_ppm {
        root.insert("liquidity_drift_ppm".into(), JsonValue::Number(liq.into()));
    }
    root.insert(
        "has_attestation".into(),
        JsonValue::Bool(summary.has_attestation),
    );
    JsonValue::Object(root)
}

pub fn detail_to_json(detail: &AdPolicySnapshotDetail) -> JsonValue {
    let mut root = match summary_to_json(&detail.summary) {
        JsonValue::Object(map) => map,
        _ => JsonMap::new(),
    };
    if let Some(att) = &detail.attestation {
        let mut obj = JsonMap::new();
        obj.insert(
            "pubkey_hex".into(),
            JsonValue::String(att.pubkey_hex.clone()),
        );
        obj.insert(
            "payload_hash_hex".into(),
            JsonValue::String(att.payload_hash_hex.clone()),
        );
        obj.insert(
            "signature_hex".into(),
            JsonValue::String(att.signature_hex.clone()),
        );
        root.insert("attestation".into(), JsonValue::Object(obj));
    }
    JsonValue::Object(root)
}

pub fn readiness_to_json(view: &AdReadinessStatusView) -> JsonValue {
    let mut root = JsonMap::new();
    // config
    let mut cfg = JsonMap::new();
    cfg.insert(
        "window_secs".into(),
        JsonValue::Number(view.config.window_secs.into()),
    );
    cfg.insert(
        "min_unique_viewers".into(),
        JsonValue::Number(view.config.min_unique_viewers.into()),
    );
    cfg.insert(
        "min_host_count".into(),
        JsonValue::Number(view.config.min_host_count.into()),
    );
    cfg.insert(
        "min_provider_count".into(),
        JsonValue::Number(view.config.min_provider_count.into()),
    );
    cfg.insert(
        "use_percentile_thresholds".into(),
        JsonValue::Bool(view.config.use_percentile_thresholds),
    );
    cfg.insert(
        "viewer_percentile".into(),
        JsonValue::Number(view.config.viewer_percentile.into()),
    );
    cfg.insert(
        "host_percentile".into(),
        JsonValue::Number(view.config.host_percentile.into()),
    );
    cfg.insert(
        "provider_percentile".into(),
        JsonValue::Number(view.config.provider_percentile.into()),
    );
    cfg.insert(
        "ema_smoothing_ppm".into(),
        JsonValue::Number(view.config.ema_smoothing_ppm.into()),
    );
    cfg.insert(
        "floor_unique_viewers".into(),
        JsonValue::Number(view.config.floor_unique_viewers.into()),
    );
    cfg.insert(
        "floor_host_count".into(),
        JsonValue::Number(view.config.floor_host_count.into()),
    );
    cfg.insert(
        "floor_provider_count".into(),
        JsonValue::Number(view.config.floor_provider_count.into()),
    );
    cfg.insert(
        "cap_unique_viewers".into(),
        JsonValue::Number(view.config.cap_unique_viewers.into()),
    );
    cfg.insert(
        "cap_host_count".into(),
        JsonValue::Number(view.config.cap_host_count.into()),
    );
    cfg.insert(
        "cap_provider_count".into(),
        JsonValue::Number(view.config.cap_provider_count.into()),
    );
    cfg.insert(
        "percentile_buckets".into(),
        JsonValue::Number(view.config.percentile_buckets.into()),
    );
    root.insert("config".into(), JsonValue::Object(cfg));

    // snapshot
    let mut snap = JsonMap::new();
    snap.insert(
        "window_secs".into(),
        JsonValue::Number(view.snapshot.window_secs.into()),
    );
    snap.insert(
        "min_unique_viewers".into(),
        JsonValue::Number(view.snapshot.min_unique_viewers.into()),
    );
    snap.insert(
        "min_host_count".into(),
        JsonValue::Number(view.snapshot.min_host_count.into()),
    );
    snap.insert(
        "min_provider_count".into(),
        JsonValue::Number(view.snapshot.min_provider_count.into()),
    );
    snap.insert(
        "unique_viewers".into(),
        JsonValue::Number(view.snapshot.unique_viewers.into()),
    );
    snap.insert(
        "host_count".into(),
        JsonValue::Number(view.snapshot.host_count.into()),
    );
    snap.insert(
        "provider_count".into(),
        JsonValue::Number(view.snapshot.provider_count.into()),
    );
    snap.insert("ready".into(), JsonValue::Bool(view.snapshot.ready));
    snap.insert(
        "last_updated".into(),
        JsonValue::Number(view.snapshot.last_updated.into()),
    );
    snap.insert(
        "total_usd_micros".into(),
        JsonValue::Number(view.snapshot.total_usd_micros.into()),
    );
    snap.insert(
        "settlement_count".into(),
        JsonValue::Number(view.snapshot.settlement_count.into()),
    );
    snap.insert(
        "price_usd_micros".into(),
        JsonValue::Number(view.snapshot.price_usd_micros.into()),
    );
    snap.insert(
        "market_price_usd_micros".into(),
        JsonValue::Number(view.snapshot.market_price_usd_micros.into()),
    );
    // blockers array
    let blockers = view
        .snapshot
        .blockers
        .iter()
        .cloned()
        .map(JsonValue::String)
        .collect::<Vec<_>>();
    snap.insert("blockers".into(), JsonValue::Array(blockers));
    // utilization
    let cohorts = view
        .snapshot
        .cohort_utilization
        .iter()
        .map(|c| {
            let mut m = JsonMap::new();
            m.insert("domain".into(), JsonValue::String(c.domain.clone()));
            if let Some(p) = &c.provider {
                m.insert("provider".into(), JsonValue::String(p.clone()));
            }
            m.insert(
                "badges".into(),
                JsonValue::Array(c.badges.iter().cloned().map(JsonValue::String).collect()),
            );
            m.insert(
                "price_per_mib_usd_micros".into(),
                JsonValue::Number(c.price_per_mib_usd_micros.into()),
            );
            m.insert(
                "target_utilization_ppm".into(),
                JsonValue::Number(c.target_utilization_ppm.into()),
            );
            m.insert(
                "observed_utilization_ppm".into(),
                JsonValue::Number(c.observed_utilization_ppm.into()),
            );
            JsonValue::Object(m)
        })
        .collect::<Vec<_>>();
    snap.insert("cohort_utilization".into(), JsonValue::Array(cohorts));
    if let Some(us) = &view.snapshot.utilization_summary {
        let mut m = JsonMap::new();
        m.insert(
            "cohort_count".into(),
            JsonValue::Number(us.cohort_count.into()),
        );
        m.insert("mean_ppm".into(), JsonValue::Number(us.mean_ppm.into()));
        m.insert("min_ppm".into(), JsonValue::Number(us.min_ppm.into()));
        m.insert("max_ppm".into(), JsonValue::Number(us.max_ppm.into()));
        m.insert(
            "last_updated".into(),
            JsonValue::Number(us.last_updated.into()),
        );
        snap.insert("utilization_summary".into(), JsonValue::Object(m));
    } else {
        snap.insert("utilization_summary".into(), JsonValue::Null);
    }
    snap.insert(
        "ready_streak_windows".into(),
        JsonValue::Number(view.snapshot.ready_streak_windows.into()),
    );
    root.insert("snapshot".into(), JsonValue::Object(snap));

    // rehearsal flags
    root.insert(
        "rehearsal_enabled".into(),
        JsonValue::Bool(view.rehearsal_enabled),
    );
    root.insert(
        "rehearsal_required_windows".into(),
        JsonValue::Number(view.rehearsal_required_windows.into()),
    );
    JsonValue::Object(root)
}

pub fn load_readiness_status(
    data_dir: &Path,
    rehearsal_enabled: bool,
    rehearsal_windows: u64,
) -> io::Result<Option<AdReadinessStatusView>> {
    let readiness_dir = data_dir.join("ad_readiness");
    if !readiness_dir.exists() {
        return Ok(None);
    }
    let path_str = readiness_dir.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "ad_readiness path not UTF-8")
    })?;
    let handle = AdReadinessHandle::open_with_storage(path_str, AdReadinessConfig::default());
    let snapshot = handle.snapshot();
    let config = handle.config();
    Ok(Some(readiness_status_view(
        &config,
        &snapshot,
        rehearsal_enabled,
        rehearsal_windows,
    )))
}

pub fn load_param_history(path: &Path) -> io::Result<Vec<(the_block::governance::ParamKey, i64)>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(path)?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let value: JsonValue =
        json::from_slice(&bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let arr = match value.as_array() {
        Some(a) => a,
        None => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    for entry in arr {
        if let Some(obj) = entry.as_object() {
            if let (Some(key_str), Some(new_value)) = (
                obj.get("key").and_then(JsonValue::as_str),
                obj.get("new_value").and_then(JsonValue::as_i64),
            ) {
                // Map only the keys we need for explorer readiness stitching.
                let key_opt = match key_str {
                    "AdRehearsalEnabled" => {
                        Some(the_block::governance::ParamKey::AdRehearsalEnabled)
                    }
                    "AdRehearsalStabilityWindows" => {
                        Some(the_block::governance::ParamKey::AdRehearsalStabilityWindows)
                    }
                    _ => None,
                };
                if let Some(key) = key_opt {
                    out.push((key, new_value));
                }
            }
        }
    }
    Ok(out)
}
