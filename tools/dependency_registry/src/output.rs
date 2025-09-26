use std::{collections::HashMap, fs, path::Path};

use anyhow::{anyhow, Context, Result};

use crate::model::{DependencyEntry, DependencyRegistry, ViolationReport};

pub fn write_registry_json(registry: &DependencyRegistry, out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let path = out_dir.join("dependency-registry.json");
    let file =
        fs::File::create(&path).with_context(|| format!("unable to create {}", path.display()))?;
    serde_json::to_writer_pretty(file, registry)
        .with_context(|| format!("unable to serialise registry to {}", path.display()))?;
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

pub fn write_violations(report: &ViolationReport, out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let path = out_dir.join("dependency-violations.json");
    let file =
        fs::File::create(&path).with_context(|| format!("unable to create {}", path.display()))?;
    serde_json::to_writer_pretty(file, report)
        .with_context(|| format!("unable to serialise violations to {}", path.display()))?;
    Ok(())
}

pub fn write_prometheus_metrics(report: &ViolationReport, out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let path = out_dir.join("dependency-metrics.prom");
    let mut buffer = String::from(
        "# HELP dependency_policy_violation Policy violations grouped by crate\n# TYPE dependency_policy_violation gauge\n",
    );
    if report.entries.is_empty() {
        buffer.push_str("dependency_policy_violation_total 0\n");
    } else {
        for entry in &report.entries {
            let mut labels = vec![
                ("crate", escape_label(&entry.name)),
                ("version", escape_label(&entry.version)),
                ("kind", entry.kind.to_string()),
                ("detail", escape_label(&entry.detail)),
            ];
            if let Some(depth) = entry.depth {
                labels.push(("depth", depth.to_string()));
            }
            let formatted = labels
                .iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, v))
                .collect::<Vec<_>>()
                .join(",");
            buffer.push_str(&format!("dependency_policy_violation{{{}}} 1\n", formatted));
        }
        buffer.push_str(&format!(
            "dependency_policy_violation_total {}\n",
            report.entries.len()
        ));
    }
    fs::write(&path, buffer).with_context(|| format!("unable to write {}", path.display()))
}

fn escape_label(input: &str) -> String {
    input
        .replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace('\n', " ")
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
    use tempfile::tempdir;

    #[test]
    fn prometheus_metrics_empty_report_emits_zero_total() {
        let dir = tempdir().unwrap();
        let report = ViolationReport::default();
        write_prometheus_metrics(&report, dir.path()).expect("write metrics");
        let contents = std::fs::read_to_string(dir.path().join("dependency-metrics.prom"))
            .expect("read metrics");
        assert!(contents.contains("# TYPE dependency_policy_violation gauge"));
        assert!(contents.contains("dependency_policy_violation_total 0"));
    }

    #[test]
    fn prometheus_metrics_include_labels() {
        let dir = tempdir().unwrap();
        let mut report = ViolationReport::default();
        report.push(ViolationEntry {
            name: "serde".into(),
            version: "1.0.0".into(),
            kind: ViolationKind::License,
            detail: "GPL".into(),
            depth: Some(2),
        });
        write_prometheus_metrics(&report, dir.path()).expect("write metrics");
        let contents = std::fs::read_to_string(dir.path().join("dependency-metrics.prom"))
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
        return Err(anyhow!(
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
    let registry: DependencyRegistry = serde_json::from_str(&contents)
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
