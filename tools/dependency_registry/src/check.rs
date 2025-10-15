use std::collections::{HashMap, HashSet};

use crate::model::{ComparisonRegistry, CrateRef, DependencyEntry};

#[derive(Debug, Default, Clone)]
pub struct DriftSummary {
    pub additions: Vec<DependencyEntry>,
    pub removals: Vec<DependencyEntry>,
    pub entry_changes: Vec<FieldChange>,
    pub policy_changes: Vec<PolicyChange>,
    pub root_additions: Vec<String>,
    pub root_removals: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FieldChange {
    pub name: String,
    pub version: String,
    pub field: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone)]
pub struct PolicyChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DriftCounts {
    pub additions: usize,
    pub removals: usize,
    pub field_changes: usize,
    pub policy_changes: usize,
    pub root_additions: usize,
    pub root_removals: usize,
}

impl DriftSummary {
    pub fn is_empty(&self) -> bool {
        self.additions.is_empty()
            && self.removals.is_empty()
            && self.entry_changes.is_empty()
            && self.policy_changes.is_empty()
            && self.root_additions.is_empty()
            && self.root_removals.is_empty()
    }

    pub fn counts(&self) -> DriftCounts {
        DriftCounts {
            additions: self.additions.len(),
            removals: self.removals.len(),
            field_changes: self.entry_changes.len(),
            policy_changes: self.policy_changes.len(),
            root_additions: self.root_additions.len(),
            root_removals: self.root_removals.len(),
        }
    }
}

pub fn compute(old: &ComparisonRegistry, new: &ComparisonRegistry) -> Option<DriftSummary> {
    let mut summary = DriftSummary::default();

    let mut old_entries: HashMap<(String, String), &DependencyEntry> = HashMap::new();
    for entry in &old.entries {
        old_entries.insert((entry.name.clone(), entry.version.clone()), entry);
    }

    let mut new_entries: HashMap<(String, String), &DependencyEntry> = HashMap::new();
    for entry in &new.entries {
        let key = (entry.name.clone(), entry.version.clone());
        if !old_entries.contains_key(&key) {
            summary.additions.push(entry.clone());
        }
        new_entries.insert(key, entry);
    }

    for entry in &old.entries {
        let key = (entry.name.clone(), entry.version.clone());
        if !new_entries.contains_key(&key) {
            summary.removals.push(entry.clone());
        }
    }

    for ((name, version), new_entry) in &new_entries {
        if let Some(old_entry) = old_entries.get(&(name.clone(), version.clone())) {
            summary
                .entry_changes
                .extend(diff_entry(old_entry, new_entry));
        }
    }

    summary.additions.sort_by(|a, b| compare_entry(a, b));
    summary.removals.sort_by(|a, b| compare_entry(a, b));

    let old_roots: HashSet<String> = old.root_packages.iter().cloned().collect();
    let new_roots: HashSet<String> = new.root_packages.iter().cloned().collect();
    summary
        .root_additions
        .extend(new_roots.difference(&old_roots).cloned());
    summary
        .root_removals
        .extend(old_roots.difference(&new_roots).cloned());
    summary.root_additions.sort();
    summary.root_removals.sort();

    summary
        .policy_changes
        .extend(diff_policy(&old.policy, &new.policy));

    if summary.is_empty() {
        None
    } else {
        Some(summary)
    }
}

fn diff_entry(old: &DependencyEntry, new: &DependencyEntry) -> Vec<FieldChange> {
    let mut changes = Vec::new();

    if old.tier != new.tier {
        changes.push(FieldChange {
            name: new.name.clone(),
            version: new.version.clone(),
            field: "tier".to_string(),
            before: old.tier.to_string(),
            after: new.tier.to_string(),
        });
    }

    if old.origin != new.origin {
        changes.push(FieldChange {
            name: new.name.clone(),
            version: new.version.clone(),
            field: "origin".to_string(),
            before: old.origin.clone(),
            after: new.origin.clone(),
        });
    }

    if old.license != new.license {
        changes.push(FieldChange {
            name: new.name.clone(),
            version: new.version.clone(),
            field: "license".to_string(),
            before: render_license(old),
            after: render_license(new),
        });
    }

    if old.depth != new.depth {
        changes.push(FieldChange {
            name: new.name.clone(),
            version: new.version.clone(),
            field: "depth".to_string(),
            before: old.depth.to_string(),
            after: new.depth.to_string(),
        });
    }

    if !same_refs(&old.dependencies, &new.dependencies) {
        changes.push(FieldChange {
            name: new.name.clone(),
            version: new.version.clone(),
            field: "dependencies".to_string(),
            before: render_refs(&old.dependencies),
            after: render_refs(&new.dependencies),
        });
    }

    if !same_refs(&old.dependents, &new.dependents) {
        changes.push(FieldChange {
            name: new.name.clone(),
            version: new.version.clone(),
            field: "dependents".to_string(),
            before: render_refs(&old.dependents),
            after: render_refs(&new.dependents),
        });
    }

    changes
}

fn diff_policy(
    old: &crate::model::PolicySummary,
    new: &crate::model::PolicySummary,
) -> Vec<PolicyChange> {
    let mut changes = Vec::new();
    if old.config_path != new.config_path {
        changes.push(PolicyChange {
            field: "config_path".to_string(),
            before: old.config_path.clone(),
            after: new.config_path.clone(),
        });
    }
    if old.max_depth != new.max_depth {
        changes.push(PolicyChange {
            field: "max_depth".to_string(),
            before: old.max_depth.to_string(),
            after: new.max_depth.to_string(),
        });
    }

    let mut old_licenses = old.forbidden_licenses.clone();
    let mut new_licenses = new.forbidden_licenses.clone();
    old_licenses.sort();
    new_licenses.sort();
    if old_licenses != new_licenses {
        changes.push(PolicyChange {
            field: "forbidden_licenses".to_string(),
            before: format_list(&old_licenses),
            after: format_list(&new_licenses),
        });
    }
    changes
}

fn same_refs(left: &[CrateRef], right: &[CrateRef]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let lhs = render_ref_vec(left);
    let rhs = render_ref_vec(right);
    lhs == rhs
}

fn render_refs(entries: &[CrateRef]) -> String {
    let rendered = render_ref_vec(entries);
    rendered.join(", ")
}

fn render_ref_vec(entries: &[CrateRef]) -> Vec<String> {
    let mut values: Vec<String> = entries
        .iter()
        .map(|reference| format!("{} {}", reference.name, reference.version))
        .collect();
    values.sort();
    values
}

fn render_license(entry: &DependencyEntry) -> String {
    entry.license.clone().unwrap_or_else(|| "—".to_string())
}

fn format_list(values: &[String]) -> String {
    if values.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", values.join(", "))
    }
}

fn compare_entry(a: &DependencyEntry, b: &DependencyEntry) -> std::cmp::Ordering {
    a.name
        .cmp(&b.name)
        .then_with(|| a.version.cmp(&b.version))
        .then_with(|| a.tier.cmp(&b.tier))
        .then_with(|| a.origin.cmp(&b.origin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ComparisonRegistry, CrateRef, DependencyEntry, PolicySummary, RiskTier};

    fn make_entry(index: usize, depth: usize) -> DependencyEntry {
        DependencyEntry {
            name: format!("crate_{index}"),
            version: "0.1.0".to_string(),
            tier: RiskTier::Unclassified,
            origin: "crates.io".to_string(),
            license: Some("MIT".to_string()),
            depth,
            dependencies: vec![CrateRef::new("base_dep".to_string(), "0.1.0".to_string())],
            dependents: Vec::new(),
        }
    }

    fn build_registry(
        entries: Vec<DependencyEntry>,
        max_depth: usize,
        roots: Vec<String>,
    ) -> ComparisonRegistry {
        ComparisonRegistry {
            root_packages: roots,
            policy: PolicySummary {
                config_path: "config/dependency_policies.toml".to_string(),
                max_depth,
                forbidden_licenses: Vec::new(),
            },
            entries,
        }
    }

    #[test]
    fn computes_drift_for_large_registries() {
        const ENTRY_COUNT: usize = 400;
        const ADDITION_COUNT: usize = 40;

        let mut old_entries = Vec::new();
        let mut new_entries = Vec::new();

        let mut expected_additions = 0;
        let mut expected_removals = 0;
        let mut expected_field_changes = 0;

        for i in 0..ENTRY_COUNT {
            let base = make_entry(i, 1);
            if i % 30 == 0 {
                old_entries.push(base);
                expected_removals += 1;
                continue;
            }

            let mut updated = base.clone();
            if i % 25 == 0 {
                updated.depth = base.depth + 1;
                expected_field_changes += 1;
            }

            old_entries.push(base);
            new_entries.push(updated);
        }

        for i in ENTRY_COUNT..(ENTRY_COUNT + ADDITION_COUNT) {
            new_entries.push(make_entry(i, 2));
            expected_additions += 1;
        }

        let old_registry = build_registry(old_entries, 5, vec!["root".to_string()]);
        let new_registry = build_registry(
            new_entries,
            6,
            vec!["root".to_string(), "extra_root".to_string()],
        );

        let summary = compute(&old_registry, &new_registry).expect("drift expected");
        assert_eq!(summary.additions.len(), expected_additions);
        assert_eq!(summary.removals.len(), expected_removals);
        assert_eq!(summary.entry_changes.len(), expected_field_changes);
        assert_eq!(summary.policy_changes.len(), 1, "max_depth change detected");
        assert_eq!(summary.root_additions.len(), 1);
        assert_eq!(summary.root_removals.len(), 0);

        let counts = summary.counts();
        assert_eq!(counts.additions, expected_additions);
        assert_eq!(counts.removals, expected_removals);
        assert_eq!(counts.field_changes, expected_field_changes);
        assert_eq!(counts.policy_changes, 1);
        assert_eq!(counts.root_additions, 1);
        assert_eq!(counts.root_removals, 0);
    }
}
