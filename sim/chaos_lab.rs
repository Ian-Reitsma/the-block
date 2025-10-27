#![forbid(unsafe_code)]

use crypto_suite::hex;
use crypto_suite::signatures::ed25519::{SigningKey, SECRET_KEY_LENGTH};
use foundation_serialization::json::{self, Map, Value};
use foundation_time::UtcDateTime;
use httpd::{BlockingClient, Method};
use monitoring_build::{
    sign_attestation, ChaosAttestation, ChaosReadinessSnapshot, ChaosSiteReadiness,
};
use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tb_sim::chaos::{ChaosModule, ChaosProviderKind, ChaosSite};
use tb_sim::Simulation;

const STATUS_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

fn signing_key_from_env() -> Result<SigningKey, Box<dyn Error>> {
    match env::var("TB_CHAOS_SIGNING_KEY") {
        Ok(hex_key) => {
            let key_bytes = hex::decode_array::<SECRET_KEY_LENGTH>(&hex_key)
                .map_err(|_| "TB_CHAOS_SIGNING_KEY must be a valid hex-encoded ed25519 secret")?;
            Ok(SigningKey::from_bytes(&key_bytes))
        }
        Err(_) => {
            use rand::rngs::OsRng;
            eprintln!("[chaos-lab] TB_CHAOS_SIGNING_KEY missing; generating ephemeral signing key");
            let mut rng = OsRng::default();
            Ok(SigningKey::generate(&mut rng))
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let steps = env::var("TB_CHAOS_STEPS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(120);
    let nodes = env::var("TB_CHAOS_NODE_COUNT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(256);
    let dashboard_path = env::var("TB_CHAOS_DASHBOARD").ok();
    let attestation_path =
        env::var("TB_CHAOS_ATTESTATIONS").unwrap_or_else(|_| "chaos_attestations.json".to_string());
    let status_snapshot_path = env::var("TB_CHAOS_STATUS_SNAPSHOT")
        .ok()
        .filter(|value| !value.is_empty());
    let diff_path_env = env::var("TB_CHAOS_STATUS_DIFF")
        .ok()
        .filter(|value| !value.is_empty());
    let baseline_path_env = env::var("TB_CHAOS_STATUS_BASELINE")
        .ok()
        .filter(|value| !value.is_empty());
    let overlay_path_env = env::var("TB_CHAOS_OVERLAY_READINESS")
        .ok()
        .filter(|value| !value.is_empty());
    let provider_failover_env = env::var("TB_CHAOS_PROVIDER_FAILOVER")
        .ok()
        .filter(|value| !value.is_empty());
    let status_endpoint = env::var("TB_CHAOS_STATUS_ENDPOINT")
        .ok()
        .filter(|value| !value.is_empty());
    let require_diff = env::var("TB_CHAOS_REQUIRE_DIFF")
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(false);
    let signing_key = signing_key_from_env()?;

    let baseline_from_endpoint = if let Some(ref endpoint) = status_endpoint {
        let baseline = fetch_status_snapshot(endpoint)?;
        if let Some(ref path) = baseline_path_env {
            persist_status_snapshot(path, &baseline)?;
            eprintln!("[chaos-lab] fetched chaos/status baseline from {endpoint} into {path}");
        } else {
            eprintln!("[chaos-lab] fetched chaos/status baseline from {endpoint}");
        }
        Some(baseline)
    } else {
        None
    };

    let baseline_from_file = if baseline_from_endpoint.is_none() {
        match baseline_path_env.as_ref() {
            Some(path) => {
                let baseline = load_status_snapshot(path)?;
                eprintln!("[chaos-lab] loaded chaos/status baseline from {path}");
                Some(baseline)
            }
            None => None,
        }
    } else {
        None
    };

    let mut sim = Simulation::new(nodes);
    apply_site_overrides(&mut sim);
    if let Some(ref path) = dashboard_path {
        sim.run(steps, path)?;
    } else {
        sim.drive(steps);
    }

    let issued_at = UtcDateTime::now().unix_timestamp().unwrap_or_default() as u64;
    let drafts = sim.chaos_attestation_drafts(issued_at);
    let attestations: Vec<ChaosAttestation> = drafts
        .into_iter()
        .map(|draft| sign_attestation(draft, &signing_key))
        .collect();

    let snapshots: Vec<ChaosReadinessSnapshot> = attestations
        .iter()
        .map(ChaosReadinessSnapshot::from)
        .collect();

    if let Some(ref path) = status_snapshot_path {
        persist_status_snapshot(path, &snapshots)?;
        eprintln!("[chaos-lab] wrote chaos status snapshot to {path}");
    }

    let baseline_for_diff = baseline_from_endpoint
        .as_ref()
        .map(|entries| entries.as_slice())
        .or_else(|| {
            baseline_from_file
                .as_ref()
                .map(|entries| entries.as_slice())
        });

    if let Some(ref path) = overlay_path_env {
        let total = persist_overlay_readiness(path, &snapshots, baseline_for_diff)?;
        eprintln!("[chaos-lab] wrote {total} overlay readiness entries to {path}");
    }

    let diff_path = diff_path_env
        .clone()
        .unwrap_or_else(|| "chaos_status_diff.json".to_string());

    if let Some(baseline) = baseline_for_diff {
        let diffs = compute_status_diff(baseline, &snapshots);
        persist_status_diff(&diff_path, &diffs)?;
        eprintln!(
            "[chaos-lab] chaos status diff entries={} path={}",
            diffs.len(),
            diff_path
        );
        if require_diff && diffs.is_empty() {
            return Err("expected chaos/status diff but none detected".into());
        }
    } else {
        let empty: Vec<StatusDiffEntry> = Vec::new();
        persist_status_diff(&diff_path, &empty)?;
        eprintln!(
            "[chaos-lab] no baseline provided; wrote empty chaos/status diff to {}",
            diff_path
        );
        if require_diff {
            return Err("TB_CHAOS_REQUIRE_DIFF set but no baseline was available".into());
        }
    }

    let provider_failover_path = provider_failover_env
        .clone()
        .unwrap_or_else(|| "chaos_provider_failover.json".to_string());
    let provider_outcome =
        provider_failover_reports(baseline_for_diff.unwrap_or(&snapshots), &snapshots);
    persist_provider_failover_reports(&provider_failover_path, &provider_outcome.reports)?;
    for report in &provider_outcome.reports {
        if report.scenarios.is_empty() {
            eprintln!(
                "[chaos-lab] provider failover provider={} skipped (no overlay sites)",
                report.provider
            );
        } else {
            eprintln!(
                "[chaos-lab] provider failover provider={} scenarios={} diff_entries={}",
                report.provider,
                report.scenarios.len(),
                report.total_diff_entries
            );
        }
    }
    if !provider_outcome.failures.is_empty() {
        return Err(provider_outcome.failures.join("; ").into());
    }

    persist_attestations(&attestation_path, &attestations)?;
    eprintln!(
        "[chaos-lab] captured {} attestations for modules: {}",
        attestations.len(),
        format_modules(&attestations)
    );
    eprintln!(
        "[chaos-lab] verifier={}",
        hex::encode(signing_key.verifying_key().to_bytes())
    );
    Ok(())
}

fn persist_attestations(
    path: &str,
    attestations: &[ChaosAttestation],
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut file = File::create(path)?;
    let payload = Value::Array(attestations.iter().map(|att| att.to_value()).collect());
    let data = json::to_vec_pretty(&payload)?;
    file.write_all(&data)?;
    file.flush()?;
    Ok(())
}

fn persist_status_snapshot(
    path: &str,
    snapshots: &[ChaosReadinessSnapshot],
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut file = File::create(path)?;
    let payload = Value::Array(
        snapshots
            .iter()
            .map(|snapshot| snapshot.to_value())
            .collect(),
    );
    let data = json::to_vec_pretty(&payload)?;
    file.write_all(&data)?;
    file.flush()?;
    Ok(())
}

fn load_status_snapshot(path: &str) -> Result<Vec<ChaosReadinessSnapshot>, Box<dyn Error>> {
    let data = fs::read(path)?;
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let snapshots: Vec<ChaosReadinessSnapshot> = json::from_slice(&data)?;
    Ok(snapshots)
}

fn persist_status_diff(path: &str, diffs: &[StatusDiffEntry]) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut file = File::create(path)?;
    let payload = Value::Array(diffs.iter().map(StatusDiffEntry::to_value).collect());
    let data = json::to_vec_pretty(&payload)?;
    file.write_all(&data)?;
    file.flush()?;
    Ok(())
}

struct OverlayReadinessRecord {
    scenario: String,
    module: String,
    site: String,
    provider: String,
    readiness: f64,
    scenario_readiness: f64,
    readiness_before: Option<f64>,
    provider_before: Option<String>,
    window_start: u64,
    window_end: u64,
    issued_at: u64,
    breaches: u64,
    sla_threshold: f64,
}

impl OverlayReadinessRecord {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("scenario".into(), Value::String(self.scenario.clone()));
        map.insert("module".into(), Value::String(self.module.clone()));
        map.insert("site".into(), Value::String(self.site.clone()));
        map.insert("provider".into(), Value::String(self.provider.clone()));
        map.insert("readiness".into(), Value::from(self.readiness));
        map.insert(
            "scenario_readiness".into(),
            Value::from(self.scenario_readiness),
        );
        if let Some(value) = self.readiness_before {
            map.insert("readiness_before".into(), Value::from(value));
        }
        if let Some(provider) = &self.provider_before {
            map.insert("provider_before".into(), Value::String(provider.clone()));
        }
        map.insert("window_start".into(), Value::from(self.window_start));
        map.insert("window_end".into(), Value::from(self.window_end));
        map.insert("issued_at".into(), Value::from(self.issued_at));
        map.insert("breaches".into(), Value::from(self.breaches));
        map.insert("sla_threshold".into(), Value::from(self.sla_threshold));
        Value::Object(map)
    }
}

struct ProviderDrillScenarioReport {
    scenario: String,
    module: ChaosModule,
    impacted_sites: usize,
    readiness_before: f64,
    readiness_after: f64,
    diff_entries: usize,
}

impl ProviderDrillScenarioReport {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("scenario".into(), Value::String(self.scenario.clone()));
        map.insert(
            "module".into(),
            Value::String(self.module.as_str().to_string()),
        );
        map.insert(
            "impacted_sites".into(),
            Value::from(self.impacted_sites as u64),
        );
        map.insert(
            "readiness_before".into(),
            Value::from(self.readiness_before),
        );
        map.insert("readiness_after".into(), Value::from(self.readiness_after));
        map.insert("diff_entries".into(), Value::from(self.diff_entries as u64));
        Value::Object(map)
    }
}

struct ProviderDrillReport {
    provider: String,
    scenarios: Vec<ProviderDrillScenarioReport>,
    total_diff_entries: usize,
}

impl ProviderDrillReport {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("provider".into(), Value::String(self.provider.clone()));
        map.insert(
            "total_diff_entries".into(),
            Value::from(self.total_diff_entries as u64),
        );
        map.insert(
            "scenarios".into(),
            Value::Array(self.scenarios.iter().map(|s| s.to_value()).collect()),
        );
        Value::Object(map)
    }
}

struct ProviderDrillOutcome {
    reports: Vec<ProviderDrillReport>,
    failures: Vec<String>,
}

fn persist_overlay_readiness(
    path: &str,
    snapshots: &[ChaosReadinessSnapshot],
    baseline: Option<&[ChaosReadinessSnapshot]>,
) -> Result<usize, Box<dyn Error>> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let baseline_map = baseline.map(snapshot_map);
    let mut records = Vec::new();

    for snapshot in snapshots
        .iter()
        .filter(|entry| entry.module == ChaosModule::Overlay)
    {
        let baseline_sites = baseline_map
            .as_ref()
            .and_then(|map| map.get(&(snapshot.scenario.clone(), snapshot.module)));
        for site in &snapshot.site_readiness {
            let (readiness_before, provider_before) = baseline_sites
                .and_then(|summary| summary.sites.get(&site.site))
                .map(|value| {
                    (
                        Some(value.readiness),
                        Some(value.provider_kind.as_str().to_string()),
                    )
                })
                .unwrap_or((None, None));
            records.push(OverlayReadinessRecord {
                scenario: snapshot.scenario.clone(),
                module: snapshot.module.as_str().to_string(),
                site: site.site.clone(),
                provider: site.provider_kind.as_str().to_string(),
                readiness: site.readiness,
                scenario_readiness: snapshot.readiness,
                readiness_before,
                provider_before,
                window_start: snapshot.window_start,
                window_end: snapshot.window_end,
                issued_at: snapshot.issued_at,
                breaches: snapshot.breaches,
                sla_threshold: snapshot.sla_threshold,
            });
        }
    }

    records.sort_by(|a, b| {
        a.scenario
            .cmp(&b.scenario)
            .then(a.module.cmp(&b.module))
            .then(a.site.cmp(&b.site))
    });

    let mut file = File::create(path)?;
    let payload = Value::Array(
        records
            .iter()
            .map(OverlayReadinessRecord::to_value)
            .collect(),
    );
    let data = json::to_vec_pretty(&payload)?;
    file.write_all(&data)?;
    file.flush()?;
    Ok(records.len())
}

fn provider_failover_reports(
    baseline: &[ChaosReadinessSnapshot],
    current: &[ChaosReadinessSnapshot],
) -> ProviderDrillOutcome {
    let providers = collect_overlay_providers(current);
    let baseline_map = snapshot_map(baseline);
    let current_map = snapshot_map(current);
    let mut reports = Vec::new();
    let mut failures = Vec::new();

    for provider in providers {
        let (mutated, impacted) = synthesize_provider_failover(current, provider);
        if impacted.is_empty() {
            reports.push(ProviderDrillReport {
                provider: provider.as_str().to_string(),
                scenarios: Vec::new(),
                total_diff_entries: 0,
            });
            continue;
        }
        let failover_map = snapshot_map(&mutated);
        let diffs = compute_status_diff(baseline, &mutated);
        let mut scenarios = Vec::new();
        for ((scenario, module), site_count) in impacted {
            let diff_entries = diffs
                .iter()
                .filter(|entry| entry.module == module && entry.scenario == scenario)
                .count();
            let readiness_before = current_map
                .get(&(scenario.clone(), module))
                .map(|summary| summary.readiness)
                .or_else(|| {
                    baseline_map
                        .get(&(scenario.clone(), module))
                        .map(|summary| summary.readiness)
                })
                .unwrap_or(1.0);
            let readiness_after = failover_map
                .get(&(scenario.clone(), module))
                .map(|summary| summary.readiness)
                .unwrap_or(readiness_before);
            scenarios.push(ProviderDrillScenarioReport {
                scenario: scenario.clone(),
                module,
                impacted_sites: site_count,
                readiness_before,
                readiness_after,
                diff_entries,
            });
            if diff_entries == 0 {
                failures.push(format!(
                    "provider '{}' failover did not register diff for scenario '{}'",
                    provider.as_str(),
                    scenario
                ));
            } else if !(readiness_after + STATUS_EPSILON < readiness_before) {
                failures.push(format!(
                    "provider '{}' failover for scenario '{}' did not lower readiness (before {:.4} after {:.4})",
                    provider.as_str(),
                    scenario,
                    readiness_before,
                    readiness_after
                ));
            }
        }
        scenarios.sort_by(|a, b| a.scenario.cmp(&b.scenario));
        let total_diff_entries = diffs
            .iter()
            .filter(|entry| entry.module == ChaosModule::Overlay)
            .count();
        if total_diff_entries == 0 {
            failures.push(format!(
                "provider '{}' failover produced no chaos/status diff entries",
                provider.as_str()
            ));
        }
        reports.push(ProviderDrillReport {
            provider: provider.as_str().to_string(),
            scenarios,
            total_diff_entries,
        });
    }

    reports.sort_by(|a, b| a.provider.cmp(&b.provider));
    ProviderDrillOutcome { reports, failures }
}

fn persist_provider_failover_reports(
    path: &str,
    reports: &[ProviderDrillReport],
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut file = File::create(path)?;
    let payload = Value::Array(reports.iter().map(|r| r.to_value()).collect());
    let data = json::to_vec_pretty(&payload)?;
    file.write_all(&data)?;
    file.flush()?;
    Ok(())
}

fn collect_overlay_providers(snapshots: &[ChaosReadinessSnapshot]) -> Vec<ChaosProviderKind> {
    let mut providers = HashSet::new();
    for snapshot in snapshots
        .iter()
        .filter(|entry| entry.module == ChaosModule::Overlay)
    {
        for site in &snapshot.site_readiness {
            providers.insert(site.provider_kind);
        }
    }
    let mut providers: Vec<_> = providers.into_iter().collect();
    providers.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    providers
}

fn synthesize_provider_failover(
    snapshots: &[ChaosReadinessSnapshot],
    provider: ChaosProviderKind,
) -> (
    Vec<ChaosReadinessSnapshot>,
    HashMap<(String, ChaosModule), usize>,
) {
    let mut mutated = snapshots.to_vec();
    let mut impacted: HashMap<(String, ChaosModule), usize> = HashMap::new();
    for snapshot in &mut mutated {
        if snapshot.module != ChaosModule::Overlay {
            continue;
        }
        let mut count = 0usize;
        for site in &mut snapshot.site_readiness {
            if site.provider_kind == provider {
                site.readiness = 0.0;
                count = count.saturating_add(1);
            }
        }
        if count > 0 {
            snapshot.readiness = snapshot
                .site_readiness
                .iter()
                .map(|site| site.readiness)
                .fold(1.0, f64::min);
            snapshot.breaches = snapshot.breaches.saturating_add(1);
            impacted.insert((snapshot.scenario.clone(), snapshot.module), count);
        }
    }
    (mutated, impacted)
}

fn fetch_status_snapshot(endpoint: &str) -> Result<Vec<ChaosReadinessSnapshot>, Box<dyn Error>> {
    let client = BlockingClient::default();
    let response = client
        .request(Method::Get, endpoint)?
        .timeout(STATUS_FETCH_TIMEOUT)
        .send()?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("chaos/status fetch failed with status {}", status.as_u16()).into());
    }
    let body = response.into_body();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let payload: Value = json::from_slice(&body)?;
    let snapshots =
        decode_status_payload(payload).map_err(|err| Box::new(err) as Box<dyn Error>)?;
    Ok(snapshots)
}

fn decode_status_payload(value: Value) -> Result<Vec<ChaosReadinessSnapshot>, SnapshotDecodeError> {
    let entries = value
        .as_array()
        .ok_or_else(|| SnapshotDecodeError::new("chaos/status payload must be an array"))?;
    let mut snapshots = Vec::with_capacity(entries.len());
    for entry in entries {
        snapshots.push(snapshot_from_value(entry)?);
    }
    Ok(snapshots)
}

fn snapshot_from_value(value: &Value) -> Result<ChaosReadinessSnapshot, SnapshotDecodeError> {
    let map = value
        .as_object()
        .ok_or_else(|| SnapshotDecodeError::new("chaos/status entry must be an object"))?;
    let scenario = read_string(map, "scenario")?;
    let module = read_module(map, "module")?;
    let readiness = read_f64(map, "readiness")?;
    let sla_threshold = read_f64(map, "sla_threshold")?;
    let breaches = read_u64(map, "breaches")?;
    let window_start = read_u64(map, "window_start")?;
    let window_end = read_u64(map, "window_end")?;
    let issued_at = read_u64(map, "issued_at")?;
    let signer = read_bytes::<32>(map, "signer")?;
    let digest = read_bytes::<32>(map, "digest")?;
    let site_readiness = match map.get("site_readiness") {
        Some(Value::Array(entries)) => {
            let mut sites = Vec::with_capacity(entries.len());
            for entry in entries {
                sites.push(site_from_value(entry)?);
            }
            sites
        }
        Some(_) => {
            return Err(SnapshotDecodeError::new(
                "site_readiness must be an array when present",
            ))
        }
        None => Vec::new(),
    };
    Ok(ChaosReadinessSnapshot {
        scenario,
        module,
        readiness,
        sla_threshold,
        breaches,
        window_start,
        window_end,
        issued_at,
        signer,
        digest,
        site_readiness,
    })
}

fn site_from_value(value: &Value) -> Result<ChaosSiteReadiness, SnapshotDecodeError> {
    let map = value
        .as_object()
        .ok_or_else(|| SnapshotDecodeError::new("site_readiness entries must be objects"))?;
    let site = read_string(map, "site")?;
    let readiness = read_f64(map, "readiness")?;
    let provider_kind = match map.get("provider_kind") {
        Some(value) => {
            let text = value.as_str().ok_or_else(|| {
                SnapshotDecodeError::new("site_readiness.provider_kind must be a string")
            })?;
            ChaosProviderKind::from_str(text).ok_or_else(|| {
                SnapshotDecodeError::new(format!(
                    "invalid provider kind '{text}' in site_readiness"
                ))
            })?
        }
        None => ChaosProviderKind::Unknown,
    };
    Ok(ChaosSiteReadiness {
        site,
        readiness,
        provider_kind,
    })
}

fn read_string(map: &Map, field: &'static str) -> Result<String, SnapshotDecodeError> {
    map.get(field)
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .ok_or_else(|| SnapshotDecodeError::new(format!("missing or invalid {field}")))
}

fn read_f64(map: &Map, field: &'static str) -> Result<f64, SnapshotDecodeError> {
    map.get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| SnapshotDecodeError::new(format!("missing or invalid {field}")))
}

fn read_u64(map: &Map, field: &'static str) -> Result<u64, SnapshotDecodeError> {
    map.get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| SnapshotDecodeError::new(format!("missing or invalid {field}")))
}

fn read_module(map: &Map, field: &'static str) -> Result<ChaosModule, SnapshotDecodeError> {
    let value = map
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| SnapshotDecodeError::new(format!("missing or invalid {field}")))?;
    ChaosModule::from_str(value).ok_or_else(|| {
        SnapshotDecodeError::new(format!("unknown module '{value}' in chaos/status"))
    })
}

fn read_bytes<const N: usize>(
    map: &Map,
    field: &'static str,
) -> Result<[u8; N], SnapshotDecodeError> {
    let value = map
        .get(field)
        .ok_or_else(|| SnapshotDecodeError::new(format!("missing {field}")))?;
    let array = value
        .as_array()
        .ok_or_else(|| SnapshotDecodeError::new(format!("{field} must be an array of bytes")))?;
    if array.len() != N {
        return Err(SnapshotDecodeError::new(format!(
            "{field} must contain {N} entries"
        )));
    }
    let mut bytes = [0u8; N];
    for (index, entry) in array.iter().enumerate() {
        let value = entry
            .as_u64()
            .ok_or_else(|| SnapshotDecodeError::new(format!("{field}[{index}] must be a byte")))?;
        if value > u8::MAX as u64 {
            return Err(SnapshotDecodeError::new(format!(
                "{field}[{index}] must be within 0-255"
            )));
        }
        bytes[index] = value as u8;
    }
    Ok(bytes)
}

#[derive(Debug)]
struct SnapshotDecodeError(String);

impl SnapshotDecodeError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for SnapshotDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for SnapshotDecodeError {}

const STATUS_EPSILON: f64 = 1e-6;

fn compute_status_diff(
    baseline: &[ChaosReadinessSnapshot],
    current: &[ChaosReadinessSnapshot],
) -> Vec<StatusDiffEntry> {
    let baseline_map = snapshot_map(baseline);
    let current_map = snapshot_map(current);
    let mut keys: HashSet<(String, ChaosModule)> = HashSet::new();
    keys.extend(baseline_map.keys().cloned());
    keys.extend(current_map.keys().cloned());
    let mut keys: Vec<_> = keys.into_iter().collect();
    keys.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.as_str().cmp(b.1.as_str())));

    let mut diffs = Vec::new();
    for (scenario, module) in keys {
        let baseline_entry = baseline_map.get(&(scenario.clone(), module));
        let current_entry = current_map.get(&(scenario.clone(), module));
        let readiness_before = baseline_entry.map(|entry| entry.readiness);
        let readiness_after = current_entry.map(|entry| entry.readiness);

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();

        match (baseline_entry, current_entry) {
            (Some(before), Some(after)) => {
                for (site, value) in &after.sites {
                    match before.sites.get(site) {
                        Some(prev) => {
                            let readiness_changed = !approx_equal(prev.readiness, value.readiness);
                            let provider_changed = prev.provider_kind != value.provider_kind;
                            if readiness_changed || provider_changed {
                                changed.push(SiteChange {
                                    site: site.clone(),
                                    before: Some(prev.readiness),
                                    after: Some(value.readiness),
                                    provider_before: Some(prev.provider_kind),
                                    provider_after: Some(value.provider_kind),
                                });
                            }
                        }
                        None => added.push(SiteEntry {
                            site: site.clone(),
                            provider_kind: value.provider_kind,
                        }),
                    }
                }
                for (site, prev) in &before.sites {
                    if !after.sites.contains_key(site) {
                        removed.push(SiteEntry {
                            site: site.clone(),
                            provider_kind: prev.provider_kind,
                        });
                    }
                }
            }
            (Some(before), None) => {
                removed.extend(before.sites.iter().map(|(site, prev)| SiteEntry {
                    site: site.clone(),
                    provider_kind: prev.provider_kind,
                }));
            }
            (None, Some(after)) => {
                added.extend(after.sites.iter().map(|(site, value)| SiteEntry {
                    site: site.clone(),
                    provider_kind: value.provider_kind,
                }));
            }
            (None, None) => {}
        }

        let readiness_changed = match (readiness_before, readiness_after) {
            (Some(before), Some(after)) => !approx_equal(before, after),
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };

        if added.is_empty() && removed.is_empty() && changed.is_empty() && !readiness_changed {
            continue;
        }

        diffs.push(StatusDiffEntry {
            module,
            scenario,
            readiness_before,
            readiness_after,
            site_added: added,
            site_removed: removed,
            site_changed: changed,
        });
    }

    diffs.sort_by(|a, b| {
        a.scenario
            .cmp(&b.scenario)
            .then(a.module.as_str().cmp(b.module.as_str()))
    });
    diffs
}

fn approx_equal(lhs: f64, rhs: f64) -> bool {
    (lhs - rhs).abs() <= STATUS_EPSILON
}

#[derive(Clone)]
struct SiteSummary {
    readiness: f64,
    provider_kind: ChaosProviderKind,
}

#[derive(Clone)]
struct SnapshotSummary {
    readiness: f64,
    sites: HashMap<String, SiteSummary>,
}

fn snapshot_map(
    snapshots: &[ChaosReadinessSnapshot],
) -> HashMap<(String, ChaosModule), SnapshotSummary> {
    let mut map = HashMap::new();
    for snapshot in snapshots {
        let sites = snapshot
            .site_readiness
            .iter()
            .map(|entry| {
                (
                    entry.site.clone(),
                    SiteSummary {
                        readiness: entry.readiness,
                        provider_kind: entry.provider_kind,
                    },
                )
            })
            .collect();
        map.insert(
            (snapshot.scenario.clone(), snapshot.module),
            SnapshotSummary {
                readiness: snapshot.readiness,
                sites,
            },
        );
    }
    map
}

struct StatusDiffEntry {
    module: ChaosModule,
    scenario: String,
    readiness_before: Option<f64>,
    readiness_after: Option<f64>,
    site_added: Vec<SiteEntry>,
    site_removed: Vec<SiteEntry>,
    site_changed: Vec<SiteChange>,
}

impl StatusDiffEntry {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("scenario".into(), Value::String(self.scenario.clone()));
        map.insert(
            "module".into(),
            Value::String(self.module.as_str().to_string()),
        );
        if let Some(value) = self.readiness_before {
            map.insert("readiness_before".into(), Value::from(value));
        }
        if let Some(value) = self.readiness_after {
            map.insert("readiness_after".into(), Value::from(value));
        }
        map.insert(
            "site_added".into(),
            Value::Array(self.site_added.iter().map(SiteEntry::to_value).collect()),
        );
        map.insert(
            "site_removed".into(),
            Value::Array(self.site_removed.iter().map(SiteEntry::to_value).collect()),
        );
        map.insert(
            "site_changed".into(),
            Value::Array(self.site_changed.iter().map(SiteChange::to_value).collect()),
        );
        Value::Object(map)
    }
}

struct SiteEntry {
    site: String,
    provider_kind: ChaosProviderKind,
}

impl SiteEntry {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("site".into(), Value::String(self.site.clone()));
        map.insert(
            "provider_kind".into(),
            Value::String(self.provider_kind.as_str().into()),
        );
        Value::Object(map)
    }
}

struct SiteChange {
    site: String,
    before: Option<f64>,
    after: Option<f64>,
    provider_before: Option<ChaosProviderKind>,
    provider_after: Option<ChaosProviderKind>,
}

impl SiteChange {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("site".into(), Value::String(self.site.clone()));
        if let Some(value) = self.before {
            map.insert("before".into(), Value::from(value));
        }
        if let Some(value) = self.after {
            map.insert("after".into(), Value::from(value));
        }
        if let Some(provider) = self.provider_before {
            map.insert(
                "provider_before".into(),
                Value::String(provider.as_str().into()),
            );
        }
        if let Some(provider) = self.provider_after {
            map.insert(
                "provider_after".into(),
                Value::String(provider.as_str().into()),
            );
        }
        Value::Object(map)
    }
}

fn format_modules(attestations: &[ChaosAttestation]) -> String {
    let mut modules: Vec<&'static str> =
        attestations.iter().map(|att| att.module.as_str()).collect();
    modules.sort();
    modules.dedup();
    modules.join(",")
}

fn apply_site_overrides(sim: &mut Simulation) {
    let Ok(spec) = env::var("TB_CHAOS_SITE_TOPOLOGY") else {
        return;
    };
    match parse_site_topology(&spec) {
        Ok(map) => {
            let harness = sim.chaos_harness_mut();
            for (module, sites) in map {
                harness.configure_sites(module, sites);
            }
        }
        Err(err) => {
            eprintln!("[chaos-lab] invalid TB_CHAOS_SITE_TOPOLOGY: {err}");
        }
    }
}

fn parse_site_topology(spec: &str) -> Result<HashMap<ChaosModule, Vec<ChaosSite>>, String> {
    let mut map: HashMap<ChaosModule, Vec<ChaosSite>> = HashMap::new();
    for module_entry in spec.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let mut parts = module_entry.splitn(2, '=');
        let module_key = parts
            .next()
            .ok_or_else(|| "missing module identifier".to_string())?
            .trim();
        let sites_spec = parts
            .next()
            .ok_or_else(|| format!("missing site list for module '{module_key}'"))?
            .trim();
        let Some(module) = ChaosModule::from_str(module_key) else {
            return Err(format!("unknown chaos module '{module_key}'"));
        };
        let mut sites = Vec::new();
        for site_entry in sites_spec
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let mut fields = site_entry.split(':');
            let name = fields
                .next()
                .ok_or_else(|| format!("invalid site entry '{site_entry}'"))?
                .trim();
            let weight_str = fields.next().unwrap_or("1.0").trim();
            let latency_str = fields.next().unwrap_or("0.0").trim();
            let provider_str = fields.next().unwrap_or("").trim();
            let weight = weight_str
                .parse::<f64>()
                .map_err(|_| format!("invalid weight '{weight_str}' for site '{name}'"))?;
            let latency = latency_str
                .parse::<f64>()
                .map_err(|_| format!("invalid latency '{latency_str}' for site '{name}'"))?;
            let provider_kind = if provider_str.is_empty() {
                ChaosProviderKind::Unknown
            } else {
                ChaosProviderKind::from_str(provider_str).ok_or_else(|| {
                    format!("invalid provider kind '{provider_str}' for site '{name}'")
                })?
            };
            sites.push(ChaosSite::with_kind(name, weight, latency, provider_kind));
        }
        if !sites.is_empty() {
            map.entry(module).or_default().extend(sites);
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitoring_build::ChaosSiteReadiness;
    use sys::tempfile;

    fn snapshot(
        module: ChaosModule,
        scenario: &str,
        readiness: f64,
        sites: &[(&str, f64, ChaosProviderKind)],
    ) -> ChaosReadinessSnapshot {
        ChaosReadinessSnapshot {
            scenario: scenario.to_string(),
            module,
            readiness,
            sla_threshold: 0.9,
            breaches: 0,
            window_start: 0,
            window_end: 1,
            issued_at: 1,
            signer: [0u8; 32],
            digest: [0u8; 32],
            site_readiness: sites
                .iter()
                .map(|(name, value, provider)| ChaosSiteReadiness {
                    site: (*name).to_string(),
                    readiness: *value,
                    provider_kind: *provider,
                })
                .collect(),
        }
    }

    #[test]
    fn diff_detects_removed_and_changed_sites() {
        let baseline = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-test",
            0.9,
            &[
                ("site-a", 0.9, ChaosProviderKind::Foundation),
                ("site-b", 0.88, ChaosProviderKind::Partner),
            ],
        )];
        let current = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-test",
            0.92,
            &[("site-b", 0.91, ChaosProviderKind::Partner)],
        )];
        let diffs = compute_status_diff(&baseline, &current);
        assert_eq!(diffs.len(), 1);
        let entry = &diffs[0];
        assert_eq!(entry.scenario, "overlay-test");
        assert_eq!(entry.module, ChaosModule::Overlay);
        assert!(entry.site_added.is_empty());
        assert_eq!(entry.site_removed.len(), 1);
        assert_eq!(entry.site_removed[0].site, "site-a");
        assert_eq!(
            entry.site_removed[0].provider_kind,
            ChaosProviderKind::Foundation
        );
        assert_eq!(entry.site_changed.len(), 1);
        assert_eq!(entry.site_changed[0].site, "site-b");
        assert_eq!(entry.site_changed[0].before, Some(0.88));
        assert_eq!(entry.site_changed[0].after, Some(0.91));
        assert_eq!(
            entry.site_changed[0].provider_before,
            Some(ChaosProviderKind::Partner)
        );
        assert_eq!(
            entry.site_changed[0].provider_after,
            Some(ChaosProviderKind::Partner)
        );
        assert_eq!(entry.readiness_before, Some(0.9));
        assert_eq!(entry.readiness_after, Some(0.92));
    }

    #[test]
    fn diff_ignores_identical_snapshots() {
        let baseline = vec![snapshot(
            ChaosModule::Compute,
            "compute-test",
            0.95,
            &[("site-a", 0.95, ChaosProviderKind::Unknown)],
        )];
        let current = baseline.clone();
        let diffs = compute_status_diff(&baseline, &current);
        assert!(diffs.is_empty());
    }

    #[test]
    fn diff_flags_readiness_only_changes() {
        let baseline = vec![snapshot(ChaosModule::Storage, "storage-test", 0.85, &[])];
        let current = vec![snapshot(ChaosModule::Storage, "storage-test", 0.8, &[])];
        let diffs = compute_status_diff(&baseline, &current);
        assert_eq!(diffs.len(), 1);
        let entry = &diffs[0];
        assert!(entry.site_added.is_empty());
        assert!(entry.site_removed.is_empty());
        assert_eq!(entry.site_changed.len(), 0);
        assert_eq!(entry.readiness_before, Some(0.85));
        assert_eq!(entry.readiness_after, Some(0.8));
    }

    #[test]
    fn diff_detects_provider_changes() {
        let baseline = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-test",
            0.9,
            &[("site-a", 0.9, ChaosProviderKind::Foundation)],
        )];
        let current = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-test",
            0.9,
            &[("site-a", 0.9, ChaosProviderKind::Partner)],
        )];
        let diffs = compute_status_diff(&baseline, &current);
        assert_eq!(diffs.len(), 1);
        let entry = &diffs[0];
        assert!(entry.site_added.is_empty());
        assert!(entry.site_removed.is_empty());
        assert_eq!(entry.site_changed.len(), 1);
        let change = &entry.site_changed[0];
        assert_eq!(change.site, "site-a");
        assert_eq!(change.before, Some(0.9));
        assert_eq!(change.after, Some(0.9));
        assert_eq!(change.provider_before, Some(ChaosProviderKind::Foundation));
        assert_eq!(change.provider_after, Some(ChaosProviderKind::Partner));
    }

    #[test]
    fn overlay_readiness_serializes_records() {
        let baseline = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-soak",
            0.91,
            &[
                ("site-a", 0.9, ChaosProviderKind::Foundation),
                ("site-b", 0.88, ChaosProviderKind::Partner),
            ],
        )];
        let current = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-soak",
            0.93,
            &[
                ("site-a", 0.92, ChaosProviderKind::Foundation),
                ("site-c", 0.87, ChaosProviderKind::Community),
            ],
        )];
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("overlay.json");
        let count =
            persist_overlay_readiness(path.to_str().expect("utf8 path"), &current, Some(&baseline))
                .expect("persist overlay readiness");
        assert_eq!(count, 2);
        let data = fs::read(&path).expect("overlay data");
        let value: Value = json::from_slice(&data).expect("overlay json");
        let entries = value.as_array().expect("entries array");
        assert_eq!(entries.len(), 2);
        let site_a = entries
            .iter()
            .find(|entry| entry.get("site").and_then(Value::as_str) == Some("site-a"))
            .expect("site-a entry");
        assert_eq!(
            site_a.get("provider").and_then(Value::as_str),
            Some("foundation")
        );
        assert_eq!(
            site_a.get("readiness_before").and_then(Value::as_f64),
            Some(0.9)
        );
        let site_c = entries
            .iter()
            .find(|entry| entry.get("site").and_then(Value::as_str) == Some("site-c"))
            .expect("site-c entry");
        assert!(site_c.get("readiness_before").is_none());
        assert_eq!(
            site_c.get("provider").and_then(Value::as_str),
            Some("community")
        );
    }

    #[test]
    fn fetch_status_snapshot_downloads_data() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let snapshots = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-soak",
            0.94,
            &[("site-a", 0.94, ChaosProviderKind::Foundation)],
        )];
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let payload = json::to_vec(&Value::Array(
            snapshots
                .iter()
                .map(ChaosReadinessSnapshot::to_value)
                .collect(),
        ))
        .expect("serialize snapshots");
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                    payload.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&payload);
            }
        });

        let endpoint = format!("http://{}/chaos/status", addr);
        let fetched = fetch_status_snapshot(&endpoint).expect("fetch status");
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].scenario, snapshots[0].scenario);
        assert_eq!(fetched[0].module, snapshots[0].module);
        assert_eq!(fetched[0].site_readiness.len(), 1);
        handle.join().expect("server thread");
    }

    #[test]
    fn provider_failover_reports_detects_outage() {
        let baseline = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-soak",
            0.95,
            &[
                ("site-a", 0.95, ChaosProviderKind::Foundation),
                ("site-b", 0.92, ChaosProviderKind::Partner),
            ],
        )];
        let outcome = provider_failover_reports(&baseline, &baseline);
        assert!(
            outcome.failures.is_empty(),
            "failures: {:?}",
            outcome.failures
        );
        let report = outcome
            .reports
            .iter()
            .find(|report| report.provider == "foundation")
            .expect("foundation report");
        assert_eq!(report.total_diff_entries, 1);
        assert_eq!(report.scenarios.len(), 1);
        let scenario = &report.scenarios[0];
        assert_eq!(scenario.impacted_sites, 1);
        assert!(scenario.readiness_after + STATUS_EPSILON < scenario.readiness_before);
    }
}
