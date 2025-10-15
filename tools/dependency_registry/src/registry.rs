use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    process::Command,
};

use diagnostics::anyhow::{anyhow, bail, Context, Result};
use foundation_serialization::json::{self, Map as JsonMap, Value as JsonValue};
use foundation_time::UtcDateTime;

use crate::{
    config::PolicyConfig,
    model::{
        CrateRef, DependencyEntry, DependencyRegistry, PolicySummary, RiskTier, ViolationEntry,
        ViolationKind, ViolationReport,
    },
};

const LIBP2P_PREFIX: &str = "libp2p";
const DIRECT_LIBP2P_TARGETS: &[&str] = &["node", "cli", "explorer"];

type PackageId = String;

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

    let generated_at = UtcDateTime::now()
        .format_iso8601()
        .context("format generated_at timestamp")?;
    let registry = DependencyRegistry {
        generated_at,
        workspace_root: metadata.workspace_root.to_string_lossy().into_owned(),
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
    let mut cmd = Command::new("cargo");
    cmd.arg("metadata").arg("--format-version=1");
    if let Some(path) = manifest_path {
        cmd.arg("--manifest-path").arg(path);
    }

    let output = cmd
        .output()
        .context("failed to execute cargo metadata command")?;

    if !output.status.success() {
        let status = output.status;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        let detail = if message.is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            message.to_string()
        };
        return Err(anyhow!("cargo metadata failed with {status}: {detail}"));
    }

    parse_metadata(&output.stdout)
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
    let entry_lookup: HashMap<(String, String), &DependencyEntry> = registry
        .entries
        .iter()
        .map(|entry| ((entry.name.clone(), entry.version.clone()), entry))
        .collect();

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

        if entry.name.starts_with(LIBP2P_PREFIX) {
            for dependent in &entry.dependents {
                if DIRECT_LIBP2P_TARGETS
                    .iter()
                    .any(|target| dependent.name.eq_ignore_ascii_case(target))
                {
                    let depth = entry_lookup
                        .get(&(dependent.name.clone(), dependent.version.clone()))
                        .map(|dep| dep.depth);
                    report.push(ViolationEntry {
                        name: dependent.name.clone(),
                        version: dependent.version.clone(),
                        kind: ViolationKind::DirectLibp2p,
                        detail: format!(
                            "crate `{}` depends directly on `{}`; use crates/p2p_overlay instead",
                            dependent.name, entry.name
                        ),
                        depth,
                    });
                }
            }
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
    workspace_root: &Path,
) -> String {
    if workspace_members.contains(&package.id) {
        return "workspace".to_string();
    }
    match package.source.as_deref() {
        Some(source) if source.starts_with("registry+") => "crates.io".to_string(),
        Some(source) if source.starts_with("git+") => source.to_string(),
        Some(source) => source.to_string(),
        None => {
            if package.manifest_path.starts_with(workspace_root) {
                "path".to_string()
            } else {
                "local".to_string()
            }
        }
    }
}

fn parse_metadata(bytes: &[u8]) -> Result<Metadata> {
    let value = json::value_from_slice(bytes).context("parse cargo metadata output")?;
    Metadata::from_json_value(value).context("parse cargo metadata output")
}

#[derive(Debug)]
struct Metadata {
    packages: Vec<Package>,
    resolve: Option<Resolve>,
    workspace_members: Vec<PackageId>,
    workspace_default_members: Vec<PackageId>,
    workspace_root: PathBuf,
}

impl Metadata {
    fn from_json_value(value: JsonValue) -> Result<Self> {
        let mut map = expect_object(value, "metadata root")?;
        let packages = take_array_field_required(&mut map, "packages", "metadata.packages")?
            .into_iter()
            .map(Package::from_json_value)
            .collect::<Result<Vec<_>>>()?;

        let resolve = match map.remove("resolve") {
            Some(JsonValue::Null) | None => None,
            Some(value) => Some(Resolve::from_json_value(value)?),
        };

        let workspace_members = take_string_array_optional(
            &mut map,
            "workspace_members",
            "metadata.workspace_members",
        )?;
        let workspace_default_members = take_string_array_optional(
            &mut map,
            "workspace_default_members",
            "metadata.workspace_default_members",
        )?;
        let workspace_root =
            take_string_field(&mut map, "workspace_root", "metadata.workspace_root")?;

        Ok(Self {
            packages,
            resolve,
            workspace_members,
            workspace_default_members,
            workspace_root: PathBuf::from(workspace_root),
        })
    }
}

#[derive(Debug)]
struct Resolve {
    nodes: Vec<Node>,
}

impl Resolve {
    fn from_json_value(value: JsonValue) -> Result<Self> {
        let mut map = expect_object(value, "metadata.resolve")?;
        let nodes = take_array_field_optional(&mut map, "nodes", "metadata.resolve.nodes")?
            .into_iter()
            .map(Node::from_json_value)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { nodes })
    }
}

#[derive(Debug)]
struct Node {
    id: PackageId,
    deps: Vec<NodeDep>,
}

impl Node {
    fn from_json_value(value: JsonValue) -> Result<Self> {
        let mut map = expect_object(value, "metadata.resolve.nodes[]")?;
        let id = take_string_field(&mut map, "id", "metadata.resolve.nodes[].id")?;
        let deps = take_array_field_optional(&mut map, "deps", "metadata.resolve.nodes[].deps")?
            .into_iter()
            .map(NodeDep::from_json_value)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { id, deps })
    }
}

#[derive(Debug)]
struct NodeDep {
    pkg: PackageId,
}

impl NodeDep {
    fn from_json_value(value: JsonValue) -> Result<Self> {
        let mut map = expect_object(value, "metadata.resolve.nodes[].deps[]")?;
        let pkg = take_string_field(&mut map, "pkg", "metadata.resolve.nodes[].deps[].pkg")?;
        Ok(Self { pkg })
    }
}

#[derive(Debug)]
struct Package {
    id: PackageId,
    name: String,
    version: String,
    license: Option<String>,
    license_file: Option<String>,
    manifest_path: PathBuf,
    source: Option<String>,
}

impl Package {
    fn from_json_value(value: JsonValue) -> Result<Self> {
        let mut map = expect_object(value, "metadata.packages[]")?;
        let id = take_string_field(&mut map, "id", "metadata.packages[].id")?;
        let name = take_string_field(&mut map, "name", "metadata.packages[].name")?;
        let version = take_string_field(&mut map, "version", "metadata.packages[].version")?;
        let manifest_path = take_string_field(
            &mut map,
            "manifest_path",
            "metadata.packages[].manifest_path",
        )?;
        let source = take_optional_string(&mut map, "source")?;
        let license = take_optional_string(&mut map, "license")?;
        let license_file = take_optional_string(&mut map, "license_file")?;
        Ok(Self {
            id,
            name,
            version,
            license,
            license_file,
            manifest_path: PathBuf::from(manifest_path),
            source,
        })
    }
}

fn expect_object(value: JsonValue, context: &str) -> Result<JsonMap> {
    match value {
        JsonValue::Object(map) => Ok(map),
        other => bail!(
            "{context} must be an object, found {}",
            describe_json(&other)
        ),
    }
}

fn take_array_field_required(
    map: &mut JsonMap,
    key: &str,
    context: &str,
) -> Result<Vec<JsonValue>> {
    match map.remove(key) {
        Some(value) => parse_array_value(value, context),
        None => bail!("{context} is missing"),
    }
}

fn take_array_field_optional(
    map: &mut JsonMap,
    key: &str,
    context: &str,
) -> Result<Vec<JsonValue>> {
    match map.remove(key) {
        Some(value) => parse_array_value(value, context),
        None => Ok(Vec::new()),
    }
}

fn parse_array_value(value: JsonValue, context: &str) -> Result<Vec<JsonValue>> {
    match value {
        JsonValue::Array(items) => Ok(items),
        JsonValue::Null => Ok(Vec::new()),
        other => bail!(
            "{context} must be an array, found {}",
            describe_json(&other)
        ),
    }
}

fn take_string_field(map: &mut JsonMap, key: &str, context: &str) -> Result<String> {
    match map.remove(key) {
        Some(JsonValue::String(value)) => Ok(value),
        Some(other) => bail!(
            "{context} must be a string, found {}",
            describe_json(&other)
        ),
        None => bail!("{context} is missing"),
    }
}

fn take_optional_string(map: &mut JsonMap, key: &str) -> Result<Option<String>> {
    match map.remove(key) {
        Some(JsonValue::String(value)) => Ok(Some(value)),
        Some(JsonValue::Null) | None => Ok(None),
        Some(other) => bail!(
            "field '{key}' must be a string or null, found {}",
            describe_json(&other)
        ),
    }
}

fn take_string_array_optional(map: &mut JsonMap, key: &str, context: &str) -> Result<Vec<String>> {
    let values = take_array_field_optional(map, key, context)?;
    let mut result = Vec::with_capacity(values.len());
    for (index, value) in values.into_iter().enumerate() {
        match value {
            JsonValue::String(s) => result.push(s),
            other => bail!(
                "{context}[{index}] must be a string, found {}",
                describe_json(&other)
            ),
        }
    }
    Ok(result)
}

fn describe_json(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check;
    use crate::model::{ComparisonRegistry, CrateRef, DependencyEntry, PolicySummary, RiskTier};
    use foundation_serialization::json::{self, Map as JsonMap, Value as JsonValue};

    #[test]
    fn parses_metadata_from_json() {
        let json = r#"{
            "packages": [
                {
                    "name": "root",
                    "version": "0.1.0",
                    "id": "root 0.1.0 (path+file:///workspace/root)",
                    "manifest_path": "/workspace/Cargo.toml"
                },
                {
                    "name": "dep",
                    "version": "0.2.0",
                    "id": "dep 0.2.0 (registry+https://github.com/rust-lang/crates.io-index)",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/registry/dep/Cargo.toml"
                },
                {
                    "name": "path-dep",
                    "version": "0.3.0",
                    "id": "path-dep 0.3.0 (path+file:///workspace/path-dep)",
                    "manifest_path": "/workspace/path-dep/Cargo.toml"
                }
            ],
            "resolve": {
                "nodes": [
                    {
                        "id": "root 0.1.0 (path+file:///workspace/root)",
                        "deps": [
                            { "pkg": "dep 0.2.0 (registry+https://github.com/rust-lang/crates.io-index)" },
                            { "pkg": "path-dep 0.3.0 (path+file:///workspace/path-dep)" }
                        ]
                    }
                ]
            },
            "workspace_members": [
                "root 0.1.0 (path+file:///workspace/root)"
            ],
            "workspace_default_members": [
                "root 0.1.0 (path+file:///workspace/root)"
            ],
            "workspace_root": "/workspace"
        }"#;

        let metadata = parse_metadata(json.as_bytes()).expect("metadata should parse");
        assert_eq!(metadata.packages.len(), 3);
        let resolve = metadata.resolve.expect("resolve graph");
        assert_eq!(resolve.nodes.len(), 1);
        assert_eq!(resolve.nodes[0].deps.len(), 2);

        let root_pkg = metadata
            .packages
            .iter()
            .find(|pkg| pkg.name == "root")
            .expect("root package");
        assert_eq!(
            crate_origin(
                root_pkg,
                &metadata.workspace_members,
                metadata.workspace_root.as_path(),
            ),
            "workspace"
        );

        let registry_pkg = metadata
            .packages
            .iter()
            .find(|pkg| pkg.name == "dep")
            .expect("registry package");
        assert_eq!(
            crate_origin(
                registry_pkg,
                &metadata.workspace_members,
                metadata.workspace_root.as_path(),
            ),
            "crates.io"
        );

        let path_pkg = metadata
            .packages
            .iter()
            .find(|pkg| pkg.name == "path-dep")
            .expect("path package");
        assert_eq!(
            crate_origin(
                path_pkg,
                &metadata.workspace_members,
                metadata.workspace_root.as_path(),
            ),
            "path"
        );
    }

    #[test]
    fn parses_complex_metadata_graph() {
        let json = include_str!("../tests/fixtures/complex_metadata.json");
        let metadata = parse_metadata(json.as_bytes()).expect("metadata should parse");

        let resolve = metadata.resolve.expect("resolve graph present");
        assert_eq!(resolve.nodes.len(), 6);

        let adjacency = build_adjacency(resolve.nodes.iter());
        let root_id = "root 0.1.0 (path+file:///workspace/root)".to_string();
        let shared_id = "shared 0.2.0 (path+file:///workspace/shared)".to_string();
        let target_id =
            "target_dep 0.4.0 (git+https://example.com/target_dep?rev=12345)".to_string();
        let duplicate_id = "duplicate_dep 0.5.0 (path+file:///workspace/duplicate_dep)".to_string();

        let root_edges = adjacency.get(&root_id).expect("root edges present");
        assert_eq!(
            root_edges.len(),
            4,
            "duplicate dependencies should be deduped"
        );
        assert!(root_edges.contains(&target_id));

        let reverse = build_reverse(&adjacency);
        let target_dependents = reverse.get(&target_id).expect("target dependents present");
        assert!(target_dependents.contains(&root_id));
        assert!(target_dependents.contains(&shared_id));

        let roots = if metadata.workspace_default_members.is_empty() {
            metadata.workspace_members.clone()
        } else {
            metadata.workspace_default_members.clone()
        };
        let depth_map = compute_depths(&roots, &adjacency);
        assert_eq!(depth_map.get(&target_id), Some(&1usize));
        assert_eq!(depth_map.get(&duplicate_id), Some(&1usize));

        let optional_id =
            "optional_dep 0.3.0 (registry+https://github.com/rust-lang/crates.io-index)";
        let optional_pkg = metadata
            .packages
            .iter()
            .find(|pkg| pkg.id == optional_id)
            .expect("optional package present");
        assert_eq!(
            crate_origin(
                optional_pkg,
                &metadata.workspace_members,
                metadata.workspace_root.as_path(),
            ),
            "crates.io"
        );
    }

    #[test]
    fn selects_workspace_members_when_defaults_absent() {
        let json = include_str!("../tests/fixtures/platform_metadata.json");
        let metadata = parse_metadata(json.as_bytes()).expect("metadata should parse");
        assert!(metadata.workspace_default_members.is_empty());

        let resolve = metadata.resolve.expect("resolve graph present");
        let adjacency = build_adjacency(resolve.nodes.iter());
        let roots = metadata.workspace_members.clone();
        let depth_map = compute_depths(&roots, &adjacency);

        let linux_id = "linux_only 0.1.0 (path+file:///workspace/linux_only)".to_string();
        let windows_id = "windows_only 0.1.0 (path+file:///workspace/windows_only)".to_string();
        let shared_id = "shared 0.1.0 (path+file:///workspace/shared)".to_string();
        let transitive_id =
            "transitive 0.1.0 (registry+https://github.com/rust-lang/crates.io-index)".to_string();

        assert_eq!(depth_map.get(&linux_id), Some(&0usize));
        assert_eq!(depth_map.get(&windows_id), Some(&0usize));
        assert_eq!(depth_map.get(&shared_id), Some(&0usize));
        assert_eq!(depth_map.get(&transitive_id), Some(&1usize));
    }

    fn package_id(index: usize) -> String {
        format!("crate{index} 0.1.0 (path+file:///workspace/crate{index})")
    }

    fn large_metadata_json(count: usize) -> JsonValue {
        let mut packages = Vec::with_capacity(count);
        let mut nodes = Vec::with_capacity(count);
        let mut members = Vec::with_capacity(count);
        let mut defaults = Vec::new();

        for index in 0..count {
            let id = package_id(index);
            let mut package = JsonMap::new();
            package.insert("id".to_string(), JsonValue::String(id.clone()));
            package.insert(
                "name".to_string(),
                JsonValue::String(format!("crate{index}")),
            );
            package.insert(
                "version".to_string(),
                JsonValue::String("0.1.0".to_string()),
            );
            package.insert(
                "manifest_path".to_string(),
                JsonValue::String(format!("/workspace/crate{index}/Cargo.toml")),
            );
            if index % 3 == 0 {
                package.insert("license".to_string(), JsonValue::String("MIT".to_string()));
            } else {
                package.insert("license".to_string(), JsonValue::Null);
            }
            package.insert("license_file".to_string(), JsonValue::Null);
            package.insert("source".to_string(), JsonValue::Null);
            packages.push(JsonValue::Object(package));

            members.push(JsonValue::String(id.clone()));
            if index < 2 {
                defaults.push(JsonValue::String(id.clone()));
            }

            let mut deps = Vec::new();
            if index + 1 < count {
                let mut dep = JsonMap::new();
                dep.insert("pkg".to_string(), JsonValue::String(package_id(index + 1)));
                deps.push(JsonValue::Object(dep));
            }
            if index >= 5 {
                let mut dep = JsonMap::new();
                dep.insert("pkg".to_string(), JsonValue::String(package_id(index - 5)));
                deps.push(JsonValue::Object(dep));
            }

            let mut node = JsonMap::new();
            node.insert("id".to_string(), JsonValue::String(id));
            node.insert("deps".to_string(), JsonValue::Array(deps));
            nodes.push(JsonValue::Object(node));
        }

        let mut resolve = JsonMap::new();
        resolve.insert("nodes".to_string(), JsonValue::Array(nodes));

        let mut root = JsonMap::new();
        root.insert("packages".to_string(), JsonValue::Array(packages));
        root.insert("resolve".to_string(), JsonValue::Object(resolve));
        root.insert("workspace_members".to_string(), JsonValue::Array(members));
        root.insert(
            "workspace_default_members".to_string(),
            JsonValue::Array(defaults),
        );
        root.insert(
            "workspace_root".to_string(),
            JsonValue::String("/workspace".to_string()),
        );
        JsonValue::Object(root)
    }

    fn baseline_entry(index: usize) -> DependencyEntry {
        DependencyEntry {
            name: format!("crate_{index}"),
            version: "0.1.0".to_string(),
            tier: if index % 7 == 0 {
                RiskTier::Strategic
            } else {
                RiskTier::Unclassified
            },
            origin: if index % 2 == 0 {
                "workspace".to_string()
            } else {
                "crates.io".to_string()
            },
            license: Some("MIT".to_string()),
            depth: index % 16,
            dependencies: Vec::new(),
            dependents: Vec::new(),
        }
    }

    #[test]
    fn parse_metadata_handles_large_workspace() {
        let json_value = large_metadata_json(256);
        let bytes = json::to_vec_value(&json_value);
        let metadata = parse_metadata(&bytes).expect("metadata should parse");
        assert_eq!(metadata.packages.len(), 256);
        assert_eq!(metadata.workspace_default_members.len(), 2);

        let resolve = metadata
            .resolve
            .as_ref()
            .expect("resolve graph present for large workspace");
        let adjacency = build_adjacency(resolve.nodes.iter());
        let roots = if metadata.workspace_default_members.is_empty() {
            metadata.workspace_members.clone()
        } else {
            metadata.workspace_default_members.clone()
        };
        let depth_map = compute_depths(&roots, &adjacency);

        let deepest_id = package_id(255);
        assert_eq!(depth_map.get(&deepest_id).copied(), Some(254));
    }

    #[test]
    fn drift_analysis_handles_large_registries() {
        let mut baseline_entries: Vec<DependencyEntry> = (0..512).map(baseline_entry).collect();
        baseline_entries.sort_by(|a, b| a.name.cmp(&b.name));

        let baseline = ComparisonRegistry {
            root_packages: vec!["root-a".to_string(), "root-b".to_string()],
            policy: PolicySummary {
                config_path: "config/dependency_policies.toml".to_string(),
                max_depth: 10,
                forbidden_licenses: vec!["AGPL-3.0".to_string()],
            },
            entries: baseline_entries.clone(),
        };

        let mut updated_entries = baseline_entries;
        updated_entries.retain(|entry| entry.name != "crate_100" && entry.name != "crate_240");

        if let Some(entry) = updated_entries
            .iter_mut()
            .find(|entry| entry.name == "crate_10")
        {
            entry.tier = RiskTier::Forbidden;
            entry.origin = "crates.io".to_string();
            entry.license = Some("Apache-2.0".to_string());
            entry.dependencies = vec![CrateRef::new("crate_200".to_string(), "0.1.0".to_string())];
        }

        if let Some(entry) = updated_entries
            .iter_mut()
            .find(|entry| entry.name == "crate_11")
        {
            entry.depth = 9;
        }

        updated_entries.push(DependencyEntry {
            name: "crate_new_a".to_string(),
            version: "1.0.0".to_string(),
            tier: RiskTier::Strategic,
            origin: "git+https://example.com/crate_new_a".to_string(),
            license: Some("BSD-3-Clause".to_string()),
            depth: 3,
            dependencies: vec![CrateRef::new("crate_8".to_string(), "0.1.0".to_string())],
            dependents: Vec::new(),
        });
        updated_entries.push(DependencyEntry {
            name: "crate_new_b".to_string(),
            version: "0.2.0".to_string(),
            tier: RiskTier::Replaceable,
            origin: "workspace".to_string(),
            license: None,
            depth: 1,
            dependencies: Vec::new(),
            dependents: Vec::new(),
        });
        updated_entries.push(DependencyEntry {
            name: "crate_new_c".to_string(),
            version: "2.5.1".to_string(),
            tier: RiskTier::Unclassified,
            origin: "crates.io".to_string(),
            license: Some("MIT".to_string()),
            depth: 4,
            dependencies: vec![CrateRef::new("crate_20".to_string(), "0.1.0".to_string())],
            dependents: Vec::new(),
        });

        updated_entries.sort_by(|a, b| a.name.cmp(&b.name));

        let mut new_policy = baseline.policy.clone();
        new_policy.max_depth = 12;
        new_policy.config_path = "config/dependency_policies.next.toml".to_string();

        let mut new_roots = baseline.root_packages.clone();
        new_roots.push("root-c".to_string());
        new_roots.sort();

        let updated = ComparisonRegistry {
            root_packages: new_roots,
            policy: new_policy,
            entries: updated_entries,
        };

        let summary = check::compute(&baseline, &updated).expect("drift should be detected");
        let counts = summary.counts();
        assert_eq!(counts.additions, 3);
        assert_eq!(counts.removals, 2);
        assert_eq!(counts.field_changes, 5);
        assert_eq!(counts.policy_changes, 2);
        assert_eq!(counts.root_additions, 1);
        assert_eq!(counts.root_removals, 0);
    }
}
