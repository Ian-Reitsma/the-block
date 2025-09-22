use std::{
    collections::{HashMap, VecDeque},
    path::Path,
};

use anyhow::{Context, Result};
use camino::Utf8Path;
use cargo_metadata::{Metadata, MetadataCommand, Node, Package, PackageId};
use chrono::Utc;

use crate::{
    config::PolicyConfig,
    model::{
        CrateRef, DependencyEntry, DependencyRegistry, PolicySummary, RiskTier, ViolationEntry,
        ViolationKind, ViolationReport,
    },
};

pub struct BuildOptions<'a> {
    pub manifest_path: Option<&'a Path>,
    pub policy: &'a PolicyConfig,
    pub config_path: &'a Path,
    pub override_depth: Option<usize>,
}

pub struct BuildOutput {
    pub registry: DependencyRegistry,
    pub violations: ViolationReport,
}

pub fn build_registry(options: BuildOptions<'_>) -> Result<BuildOutput> {
    let metadata = load_metadata(options.manifest_path)?;
    let resolve = metadata
        .resolve
        .as_ref()
        .context("cargo metadata did not return a resolved dependency graph")?;

    let packages: HashMap<PackageId, &Package> = metadata
        .packages
        .iter()
        .map(|package| (package.id.clone(), package))
        .collect();

    let tier_map = options.policy.tier_map();
    let max_depth = options.policy.max_depth(options.override_depth);
    let forbidden_licenses = options.policy.forbidden_licenses().to_vec();

    let adjacency = build_adjacency(resolve.nodes.iter());
    let reverse = build_reverse(&adjacency);

    let root_candidates: Vec<PackageId> = if metadata.workspace_default_members.is_empty() {
        metadata.workspace_members.clone()
    } else {
        metadata.workspace_default_members.iter().cloned().collect()
    };

    let depth_map = compute_depths(&root_candidates, &adjacency);

    let mut entries: Vec<DependencyEntry> = depth_map
        .keys()
        .filter_map(|id| packages.get(id).map(|pkg| (id.clone(), *pkg)))
        .map(|(id, package)| {
            let tier = tier_map
                .get(&package.name.to_ascii_lowercase())
                .cloned()
                .unwrap_or(RiskTier::Unclassified);
            let license = package.license.clone().or_else(|| {
                package
                    .license_file
                    .as_ref()
                    .map(|path| format!("file:{}", path))
            });
            let dependencies = adjacency
                .get(&id)
                .into_iter()
                .flatten()
                .filter_map(|dep| {
                    packages
                        .get(dep)
                        .map(|pkg| CrateRef::new(pkg.name.clone(), pkg.version.to_string()))
                })
                .collect::<Vec<_>>();
            let dependents = reverse
                .get(&id)
                .into_iter()
                .flatten()
                .filter_map(|dep| {
                    packages
                        .get(dep)
                        .map(|pkg| CrateRef::new(pkg.name.clone(), pkg.version.to_string()))
                })
                .collect::<Vec<_>>();
            let mut entry = DependencyEntry {
                name: package.name.clone(),
                version: package.version.to_string(),
                tier,
                origin: crate_origin(
                    package,
                    &metadata.workspace_members,
                    metadata.workspace_root.as_path(),
                ),
                license,
                depth: *depth_map.get(&id).unwrap_or(&0),
                dependencies,
                dependents,
            };
            entry.dependencies.sort_by(|a, b| a.name.cmp(&b.name));
            entry
                .dependencies
                .dedup_by(|a, b| a.name == b.name && a.version == b.version);
            entry.dependents.sort_by(|a, b| a.name.cmp(&b.name));
            entry
                .dependents
                .dedup_by(|a, b| a.name == b.name && a.version == b.version);
            entry
        })
        .collect();

    entries.sort_by(|a, b| {
        a.tier
            .cmp(&b.tier)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.version.cmp(&b.version))
    });

    let mut root_packages = metadata
        .workspace_members
        .iter()
        .filter_map(|id| packages.get(id).map(|pkg| pkg.name.clone()))
        .collect::<Vec<_>>();
    root_packages.sort();

    let registry = DependencyRegistry {
        generated_at: Utc::now(),
        workspace_root: metadata.workspace_root.to_string(),
        root_packages,
        policy: PolicySummary {
            config_path: options.config_path.display().to_string(),
            max_depth,
            forbidden_licenses: forbidden_licenses.clone(),
        },
        entries,
    };

    let violations = detect_violations(&registry, max_depth, &forbidden_licenses);

    Ok(BuildOutput {
        registry,
        violations,
    })
}

fn load_metadata(manifest_path: Option<&Path>) -> Result<Metadata> {
    let mut cmd = MetadataCommand::new();
    if let Some(path) = manifest_path {
        cmd.manifest_path(path);
    }
    cmd.exec().context("failed to execute cargo metadata")
}

fn build_adjacency<'a>(
    nodes: impl Iterator<Item = &'a Node>,
) -> HashMap<PackageId, Vec<PackageId>> {
    let mut map: HashMap<PackageId, Vec<PackageId>> = HashMap::new();
    for node in nodes {
        let entry = map.entry(node.id.clone()).or_default();
        for dep in &node.deps {
            entry.push(dep.pkg.clone());
        }
        entry.sort();
        entry.dedup();
    }
    map
}

fn build_reverse(
    adjacency: &HashMap<PackageId, Vec<PackageId>>,
) -> HashMap<PackageId, Vec<PackageId>> {
    let mut reverse: HashMap<PackageId, Vec<PackageId>> = HashMap::new();
    for (parent, children) in adjacency {
        for child in children {
            reverse
                .entry(child.clone())
                .or_default()
                .push(parent.clone());
        }
    }
    for deps in reverse.values_mut() {
        deps.sort();
        deps.dedup();
    }
    reverse
}

fn compute_depths(
    roots: &[PackageId],
    adjacency: &HashMap<PackageId, Vec<PackageId>>,
) -> HashMap<PackageId, usize> {
    let mut depths: HashMap<PackageId, usize> = HashMap::new();
    let mut queue = VecDeque::new();

    for root in roots {
        depths.insert(root.clone(), 0);
        queue.push_back((root.clone(), 0));
    }

    while let Some((id, depth)) = queue.pop_front() {
        if let Some(children) = adjacency.get(&id) {
            for child in children {
                let next_depth = depth + 1;
                let entry = depths.get(child).copied();
                if entry.map_or(true, |existing| next_depth < existing) {
                    depths.insert(child.clone(), next_depth);
                    queue.push_back((child.clone(), next_depth));
                }
            }
        }
    }

    depths
}

fn detect_violations(
    registry: &DependencyRegistry,
    max_depth: usize,
    forbidden_licenses: &[String],
) -> ViolationReport {
    let mut report = ViolationReport::default();
    for entry in &registry.entries {
        if entry.depth > max_depth {
            report.push(ViolationEntry {
                name: entry.name.clone(),
                version: entry.version.clone(),
                kind: ViolationKind::Depth,
                detail: format!(
                    "dependency depth {} exceeds policy limit {}",
                    entry.depth, max_depth
                ),
                depth: Some(entry.depth),
            });
        }

        if let Some(license) = &entry.license {
            if license_is_forbidden(license, forbidden_licenses) {
                report.push(ViolationEntry {
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                    kind: ViolationKind::License,
                    detail: format!("forbidden license detected: {}", license),
                    depth: Some(entry.depth),
                });
            }
        }

        if entry.tier == RiskTier::Forbidden {
            report.push(ViolationEntry {
                name: entry.name.clone(),
                version: entry.version.clone(),
                kind: ViolationKind::Tier,
                detail: "crate marked as forbidden by policy".to_string(),
                depth: Some(entry.depth),
            });
        }
    }
    report
}

fn license_is_forbidden(license: &str, forbidden: &[String]) -> bool {
    let license_upper = license.to_ascii_uppercase();
    forbidden
        .iter()
        .any(|needle| license_upper.contains(&needle.to_ascii_uppercase()))
}

fn crate_origin(
    package: &Package,
    workspace_members: &[PackageId],
    workspace_root: &Utf8Path,
) -> String {
    if workspace_members.contains(&package.id) {
        return "workspace".to_string();
    }
    match &package.source {
        Some(source) if source.repr.starts_with("registry+") => "crates.io".to_string(),
        Some(source) if source.repr.starts_with("git+") => source.repr.clone(),
        Some(source) => source.repr.clone(),
        None => {
            if package.manifest_path.starts_with(workspace_root) {
                "path".to_string()
            } else {
                "local".to_string()
            }
        }
    }
}
