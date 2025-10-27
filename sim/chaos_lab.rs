#![forbid(unsafe_code)]

use crypto_suite::hex;
use crypto_suite::signatures::ed25519::{SigningKey, SECRET_KEY_LENGTH};
use foundation_serialization::json::{self, Map, Value};
use foundation_time::UtcDateTime;
use monitoring_build::{sign_attestation, ChaosAttestation, ChaosReadinessSnapshot};
use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use tb_sim::chaos::{ChaosModule, ChaosProviderKind, ChaosSite};
use tb_sim::Simulation;

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
    let signing_key = signing_key_from_env()?;

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

    if let Some(path) = env::var("TB_CHAOS_STATUS_SNAPSHOT")
        .ok()
        .filter(|value| !value.is_empty())
    {
        persist_status_snapshot(&path, &snapshots)?;
        eprintln!("[chaos-lab] wrote chaos status snapshot to {path}");
    }

    if let Some(baseline_path) = env::var("TB_CHAOS_STATUS_BASELINE")
        .ok()
        .filter(|value| !value.is_empty())
    {
        let baseline = load_status_snapshot(&baseline_path)?;
        let diffs = compute_status_diff(&baseline, &snapshots);
        let diff_path = env::var("TB_CHAOS_STATUS_DIFF")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "chaos_status_diff.json".to_string());
        persist_status_diff(&diff_path, &diffs)?;
        eprintln!(
            "[chaos-lab] chaos status diff entries={} path={}",
            diffs.len(),
            diff_path
        );
        let require_diff = env::var("TB_CHAOS_REQUIRE_DIFF")
            .ok()
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "True"))
            .unwrap_or(false);
        if require_diff && diffs.is_empty() {
            return Err("expected chaos/status diff but none detected".into());
        }
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
}
