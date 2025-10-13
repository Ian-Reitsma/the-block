#![forbid(unsafe_code)]

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::env;
use std::path::Path;
use std::process::Command;

mod json;

/// Error returned when the dependency guard fails.
#[derive(Debug)]
pub struct GuardError {
    kind: GuardErrorKind,
}

#[derive(Debug)]
enum GuardErrorKind {
    MetadataFailure { status: Option<i32>, stderr: String },
    ThirdParty { offenders: BTreeSet<String> },
}

impl GuardError {
    fn metadata_failure(status: Option<i32>, stderr: String) -> Self {
        Self {
            kind: GuardErrorKind::MetadataFailure { status, stderr },
        }
    }

    fn third_party(offenders: BTreeSet<String>) -> Self {
        Self {
            kind: GuardErrorKind::ThirdParty { offenders },
        }
    }
}

impl std::fmt::Display for GuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            GuardErrorKind::MetadataFailure { status, stderr } => {
                write!(
                    f,
                    "dependency guard failed: cargo metadata exited with status {:?}: {}",
                    status, stderr
                )
            }
            GuardErrorKind::ThirdParty { offenders } => {
                let mut offenders_vec: Vec<_> = offenders.iter().cloned().collect();
                offenders_vec.sort();
                let mut rendered = offenders_vec
                    .iter()
                    .take(10)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                if offenders_vec.len() > 10 {
                    rendered.push_str(" ...");
                }
                write!(
                    f,
                    "third-party crates detected while FIRST_PARTY_ONLY=1: {rendered}"
                )
            }
        }
    }
}

impl std::error::Error for GuardError {}

/// Run the dependency guard against the crate that owns the current build script.
///
/// Build scripts can call this helper and panic when it returns an error.
pub fn enforce_current_crate() -> Result<(), GuardError> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| String::from("."));
    enforce_manifest(Path::new(&manifest_dir))
}

/// Run the dependency guard against the manifest at the provided path.
///
/// The guard executes `cargo metadata` to discover dependencies and fails when registry or git
/// sources are detected while `FIRST_PARTY_ONLY` is enabled.
pub fn enforce_manifest(manifest_dir: &Path) -> Result<(), GuardError> {
    if !should_enforce() {
        return Ok(());
    }

    let manifest_path = manifest_dir.join("Cargo.toml");
    let metadata = run_cargo_metadata(&manifest_path)?;
    let offenders = detect_third_party(&metadata);
    if offenders.is_empty() {
        Ok(())
    } else {
        Err(GuardError::third_party(offenders))
    }
}

fn should_enforce() -> bool {
    match env::var("FIRST_PARTY_ONLY") {
        Ok(value) => value != "0",
        Err(_) => true,
    }
}

fn run_cargo_metadata(manifest_path: &Path) -> Result<String, GuardError> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));
    let output = Command::new(cargo)
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(manifest_path)
        .output()
        .expect("failed to execute cargo metadata");

    if !output.status.success() {
        return Err(GuardError::metadata_failure(
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn detect_third_party(metadata: &str) -> BTreeSet<String> {
    detect_third_party_precise(metadata).unwrap_or_else(|| detect_third_party_naive(metadata))
}

fn detect_third_party_precise(metadata: &str) -> Option<BTreeSet<String>> {
    use json::Value;

    let root_value = json::value_from_slice(metadata.as_bytes()).ok()?;
    let mut root_map = match root_value {
        Value::Object(map) => map,
        _ => return None,
    };

    let packages_value = root_map.remove("packages")?;
    let resolve_value = root_map.remove("resolve")?;

    let packages = match packages_value {
        Value::Array(items) => items,
        _ => return None,
    };

    let mut package_sources: HashMap<String, (String, Option<String>)> = HashMap::new();
    for pkg in packages {
        let mut pkg_map = match pkg {
            Value::Object(map) => map,
            _ => continue,
        };
        let id = match pkg_map.remove("id") {
            Some(Value::String(id)) => id,
            _ => continue,
        };
        let name = match pkg_map.remove("name") {
            Some(Value::String(name)) => name,
            _ => continue,
        };
        let source = match pkg_map.remove("source") {
            Some(Value::String(src)) => Some(src),
            _ => None,
        };
        package_sources.insert(id, (name, source));
    }

    let mut resolve_map = match resolve_value {
        Value::Object(map) => map,
        _ => return None,
    };
    let root_id = match resolve_map.remove("root") {
        Some(Value::String(root)) => root,
        _ => return None,
    };
    let nodes = match resolve_map.remove("nodes") {
        Some(Value::Array(nodes)) => nodes,
        _ => return None,
    };

    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
    for node in nodes {
        let mut node_map = match node {
            Value::Object(map) => map,
            _ => continue,
        };
        let id = match node_map.remove("id") {
            Some(Value::String(id)) => id,
            _ => continue,
        };
        let deps_value = node_map.remove("dependencies");
        let deps_iter = match deps_value {
            Some(Value::Array(values)) => values.into_iter(),
            _ => Vec::new().into_iter(),
        };
        let mut deps = Vec::new();
        for dep in deps_iter {
            if let Value::String(dep_id) = dep {
                deps.push(dep_id);
            }
        }
        adjacency.insert(id, deps);
    }

    let mut reachable = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(root_id.clone());
    while let Some(pkg) = queue.pop_front() {
        if !reachable.insert(pkg.clone()) {
            continue;
        }
        if let Some(deps) = adjacency.get(&pkg) {
            for dep in deps {
                queue.push_back(dep.clone());
            }
        }
    }

    let mut offenders = BTreeSet::new();
    for pkg_id in reachable {
        if pkg_id == root_id {
            continue;
        }
        if let Some((name, Some(source))) = package_sources.get(&pkg_id) {
            if source.starts_with("registry+") || source.starts_with("git+") {
                offenders.insert(name.clone());
            }
        }
    }

    Some(offenders)
}

fn detect_third_party_naive(metadata: &str) -> BTreeSet<String> {
    let mut offenders = BTreeSet::new();
    let mut search_start = 0usize;
    const SOURCE_MARKER: &str = "\"source\":";
    const NAME_MARKER: &str = "\"name\":";

    while let Some(relative_idx) = metadata[search_start..].find(SOURCE_MARKER) {
        let source_idx = search_start + relative_idx + SOURCE_MARKER.len();
        let tail = &metadata[source_idx..];
        if !(tail.starts_with("\"registry+") || tail.starts_with("\"git+")) {
            search_start = source_idx;
            continue;
        }

        if let Some(prefix) = metadata[..source_idx - SOURCE_MARKER.len()].rfind(NAME_MARKER) {
            let mut name_start = prefix + NAME_MARKER.len();
            if metadata[name_start..].starts_with('"') {
                name_start += 1;
            }
            if let Some(rest) = metadata[name_start..].find('"') {
                let name = &metadata[name_start..name_start + rest];
                if !name.trim().is_empty() {
                    offenders.insert(name.trim().to_string());
                }
            }
        }

        search_start = source_idx + 1;
    }

    offenders
}

/// Utility that can be used by build scripts to display a descriptive panic message.
pub fn panic_on_failure(result: Result<(), GuardError>) {
    if let Err(err) = result {
        panic!("{err}");
    }
}

/// Helper that emits the standard `FIRST_PARTY_ONLY` change detection for build scripts.
pub fn rerun_if_env_changed() {
    println!("cargo:rerun-if-env-changed=FIRST_PARTY_ONLY");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_env_variable() {
        env::set_var("FIRST_PARTY_ONLY", "0");
        enforce_manifest(Path::new(".")).expect("FIRST_PARTY_ONLY=0 disables guard");
        env::remove_var("FIRST_PARTY_ONLY");
    }

    #[test]
    fn detect_registry_sources() {
        let metadata = r#"{"packages":[{"name":"a","source":"registry+https://example"}]}"#;
        let offenders = detect_third_party(metadata);
        assert!(offenders.contains("a"));
    }
}
