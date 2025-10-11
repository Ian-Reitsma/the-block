use std::{collections::HashMap, fs, path::Path};

use diagnostics::anyhow as diag_anyhow;
use diagnostics::anyhow::{Context, Result};

use crate::model::{DependencyEntry, DependencyRegistry, ViolationReport};
use foundation_serialization::json;
use runtime::telemetry::{IntGaugeVec, Opts, Registry};

pub fn write_registry_json(registry: &DependencyRegistry, out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let path = out_dir.join("dependency-registry.json");
    let file =
        fs::File::create(&path).with_context(|| format!("unable to create {}", path.display()))?;
    json::to_writer_pretty(file, registry)
        .map_err(|err| diag_anyhow::anyhow!(err))
        .with_context(|| format!("unable to serialise registry to {}", path.display()))?;
    Ok(())
}

pub fn write_snapshot(registry: &DependencyRegistry, snapshot_path: &Path) -> Result<()> {
    if let Some(parent) = snapshot_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let snapshot = registry.comparison_key();
    let file = fs::File::create(snapshot_path)
        .with_context(|| format!("unable to create {}", snapshot_path.display()))?;
    json::to_writer_pretty(file, &snapshot)
        .map_err(|err| diag_anyhow::anyhow!(err))
        .with_context(|| {
            format!(
                "unable to serialise dependency snapshot to {}",
                snapshot_path.display()
            )
        })?;
    Ok(())
}

pub fn write_markdown(registry: &DependencyRegistry, markdown_path: &Path) -> Result<()> {
    if let Some(parent) = markdown_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create markdown directory {}", parent.display()))?;
    }

    let mut rows = String::new();
    rows.push_str("# Dependency Inventory\n\n");
    rows.push_str("| Tier | Crate | Version | Origin | License | Depth |\n");
    rows.push_str("| --- | --- | --- | --- | --- | --- |\n");

    let mut sorted = registry.entries.clone();
    sorted.sort_by(|a, b| {
        a.tier
            .cmp(&b.tier)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.version.cmp(&b.version))
    });

    for entry in sorted {
        rows.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} |\n",
            entry.tier,
            entry.name,
            entry.version,
            entry.origin,
            entry.license.unwrap_or_else(|| "—".to_string()),
            entry.depth
        ));
    }

    fs::write(markdown_path, rows)
        .with_context(|| format!("unable to write {}", markdown_path.display()))
}

pub fn write_crate_manifest(registry: &DependencyRegistry, manifest_path: &Path) -> Result<()> {
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut names = registry
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();

    let buffer = names.join("\n");
    fs::write(manifest_path, buffer)
        .with_context(|| format!("unable to write {}", manifest_path.display()))
}

pub fn write_violations(report: &ViolationReport, out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let path = out_dir.join("dependency-violations.json");
    let file =
        fs::File::create(&path).with_context(|| format!("unable to create {}", path.display()))?;
    json::to_writer_pretty(file, report)
        .map_err(|err| diag_anyhow::anyhow!(err))
        .with_context(|| format!("unable to serialise violations to {}", path.display()))?;
    Ok(())
}

pub fn write_telemetry_metrics(report: &ViolationReport, out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let path = out_dir.join("dependency-metrics.telemetry");
    let registry = Registry::new();
    let gauge_vec = IntGaugeVec::new(
        Opts::new(
            "dependency_policy_violation",
            "Policy violations grouped by crate",
        ),
        &["crate", "version", "kind", "detail", "depth"],
    )
    .map_err(|err| diag_anyhow::anyhow!(err))?;
    registry
        .register(Box::new(gauge_vec.clone()))
        .map_err(|err| diag_anyhow::anyhow!(err))?;
    let total = registry
        .register_counter(
            "dependency_policy_violation_total",
            "Total dependency policy violations",
        )
        .map_err(|err| diag_anyhow::anyhow!(err))?;
    total.reset();

    for entry in &report.entries {
        let kind_owned = entry.kind.to_string();
        let detail_owned = entry.detail.clone();
        let depth_owned = entry
            .depth
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string());
        let labels = [
            entry.name.as_str(),
            entry.version.as_str(),
            kind_owned.as_str(),
            detail_owned.as_str(),
            depth_owned.as_str(),
        ];
        let gauge = gauge_vec
            .ensure_handle_for_label_values(&labels)
            .expect(runtime::telemetry::LABEL_REGISTRATION_ERR);
        gauge.set(1);
        total.inc();
    }

    fs::write(&path, registry.render_bytes())
        .with_context(|| format!("unable to write {}", path.display()))
}

pub fn diff_registries(old_path: &Path, new_path: &Path) -> Result<()> {
    let old = load_registry(old_path)?;
    let new = load_registry(new_path)?;

    let old_map = index_by_crate(&old.entries);
    let new_map = index_by_crate(&new.entries);

    let mut has_changes = false;

    for key in new_map.keys() {
        if !old_map.contains_key(key) {
            has_changes = true;
            let entry = new_map.get(key).unwrap();
            println!(
                "+ {} {} (tier: {}, origin: {}, license: {})",
                entry.name,
                entry.version,
                entry.tier,
                entry.origin,
                entry.license.clone().unwrap_or_else(|| "—".to_string())
            );
        }
    }

    for key in old_map.keys() {
        if !new_map.contains_key(key) {
            has_changes = true;
            let entry = old_map.get(key).unwrap();
            println!(
                "- {} {} (tier: {}, origin: {}, license: {})",
                entry.name,
                entry.version,
                entry.tier,
                entry.origin,
                entry.license.clone().unwrap_or_else(|| "—".to_string())
            );
        }
    }

    for (key, old_entry) in &old_map {
        if let Some(new_entry) = new_map.get(key) {
            if old_entry != new_entry {
                has_changes = true;
                print_field_diff(
                    "tier",
                    old_entry.tier.to_string(),
                    new_entry.tier.to_string(),
                    &old_entry.name,
                    &old_entry.version,
                );
                print_field_diff(
                    "origin",
                    old_entry.origin.clone(),
                    new_entry.origin.clone(),
                    &old_entry.name,
                    &old_entry.version,
                );
                print_field_diff(
                    "license",
                    old_entry.license.clone().unwrap_or_else(|| "—".to_string()),
                    new_entry.license.clone().unwrap_or_else(|| "—".to_string()),
                    &old_entry.name,
                    &old_entry.version,
                );
            }
        }
    }

    if !has_changes {
        println!(
            "no differences detected between {} and {}",
            old_path.display(),
            new_path.display()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ViolationEntry, ViolationKind};
    use sys::tempfile::tempdir;

    #[test]
    fn telemetry_metrics_empty_report_emits_zero_total() {
        let dir = tempdir().unwrap();
        let report = ViolationReport::default();
        write_telemetry_metrics(&report, dir.path()).expect("write metrics");
        let contents = std::fs::read_to_string(dir.path().join("dependency-metrics.telemetry"))
            .expect("read metrics");
        assert!(contents.contains("# TYPE dependency_policy_violation gauge"));
        assert!(contents.contains("dependency_policy_violation_total 0"));
    }

    #[test]
    fn telemetry_metrics_include_labels() {
        let dir = tempdir().unwrap();
        let mut report = ViolationReport::default();
        report.push(ViolationEntry {
            name: "serde".into(),
            version: "1.0.0".into(),
            kind: ViolationKind::License,
            detail: "GPL".into(),
            depth: Some(2),
        });
        write_telemetry_metrics(&report, dir.path()).expect("write metrics");
        let contents = std::fs::read_to_string(dir.path().join("dependency-metrics.telemetry"))
            .expect("read metrics");
        assert!(contents.contains(
            "dependency_policy_violation{crate=\"serde\",version=\"1.0.0\",kind=\"license\",detail=\"GPL\",depth=\"2\"} 1"
        ));
        assert!(contents.contains("dependency_policy_violation_total 1"));
    }
}

pub fn explain_crate(crate_name: &str, registry_path: &Path) -> Result<()> {
    let registry = load_registry(registry_path)?;
    let mut found = false;
    for entry in registry
        .entries
        .iter()
        .filter(|entry| entry.name == crate_name)
    {
        found = true;
        println!("crate: {}", entry.name);
        println!("version: {}", entry.version);
        println!("tier: {}", entry.tier);
        println!("origin: {}", entry.origin);
        println!(
            "license: {}",
            entry.license.clone().unwrap_or_else(|| "—".to_string())
        );
        println!("depth: {}", entry.depth);
        if !entry.dependencies.is_empty() {
            println!("dependencies:");
            for dep in &entry.dependencies {
                println!("  - {} {}", dep.name, dep.version);
            }
        }
        if !entry.dependents.is_empty() {
            println!("dependents:");
            for dep in &entry.dependents {
                println!("  - {} {}", dep.name, dep.version);
            }
        }
        println!();
    }

    if !found {
        return Err(diag_anyhow::anyhow!(
            "crate {} not present in registry {}",
            crate_name,
            registry_path.display()
        ));
    }

    Ok(())
}

pub fn load_registry(path: &Path) -> Result<DependencyRegistry> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("unable to read registry at {}", path.display()))?;
    let registry: DependencyRegistry = json::from_slice(contents.as_bytes())
        .map_err(|err| diag_anyhow::anyhow!(err))
        .with_context(|| format!("unable to parse registry at {}", path.display()))?;
    Ok(registry)
}

fn index_by_crate(entries: &[DependencyEntry]) -> HashMap<(String, String), DependencyEntry> {
    let mut map = HashMap::new();
    for entry in entries {
        map.insert((entry.name.clone(), entry.version.clone()), entry.clone());
    }
    map
}

fn print_field_diff(field: &str, old: String, new: String, name: &str, version: &str) {
    if old != new {
        println!(
            "~ {} {} {} changed: '{}' -> '{}'",
            name, version, field, old, new
        );
    }
}
