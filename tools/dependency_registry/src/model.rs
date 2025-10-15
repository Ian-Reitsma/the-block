use std::{cmp::Ordering, fmt};

use concurrency::OrderedMap;
use diagnostics::anyhow::{bail, Context, Result};
use foundation_serialization::json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CrateRef {
    pub name: String,
    pub version: String,
}

impl CrateRef {
    pub fn new(name: String, version: String) -> Self {
        Self { name, version }
    }

    pub fn to_json_value(&self) -> JsonValue {
        let mut map = JsonMap::new();
        map.insert("name".to_string(), JsonValue::String(self.name.clone()));
        map.insert(
            "version".to_string(),
            JsonValue::String(self.version.clone()),
        );
        JsonValue::Object(map)
    }

    pub(crate) fn from_json_value(value: JsonValue, context: &str) -> Result<Self> {
        let mut map = expect_object(value, context)?;
        let name = take_string(&mut map, "name", context)?;
        let version = take_string(&mut map, "version", context)?;
        Ok(Self { name, version })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyEntry {
    pub name: String,
    pub version: String,
    pub tier: RiskTier,
    pub origin: String,
    pub license: Option<String>,
    pub depth: usize,
    pub dependencies: Vec<CrateRef>,
    pub dependents: Vec<CrateRef>,
}

impl DependencyEntry {
    pub fn to_json_value(&self) -> JsonValue {
        let mut map = JsonMap::new();
        map.insert("name".to_string(), JsonValue::String(self.name.clone()));
        map.insert(
            "version".to_string(),
            JsonValue::String(self.version.clone()),
        );
        map.insert(
            "tier".to_string(),
            JsonValue::String(self.tier.as_str().to_string()),
        );
        map.insert("origin".to_string(), JsonValue::String(self.origin.clone()));
        map.insert(
            "license".to_string(),
            self.license
                .as_ref()
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        );
        map.insert(
            "depth".to_string(),
            JsonValue::Number(JsonNumber::from(self.depth as u64)),
        );
        map.insert(
            "dependencies".to_string(),
            JsonValue::Array(
                self.dependencies
                    .iter()
                    .map(CrateRef::to_json_value)
                    .collect(),
            ),
        );
        map.insert(
            "dependents".to_string(),
            JsonValue::Array(
                self.dependents
                    .iter()
                    .map(CrateRef::to_json_value)
                    .collect(),
            ),
        );
        JsonValue::Object(map)
    }

    pub(crate) fn from_json_value(value: JsonValue) -> Result<Self> {
        let mut map = expect_object(value, "dependency entry")?;
        let name = take_string(&mut map, "name", "dependency.name")?;
        let version = take_string(&mut map, "version", "dependency.version")?;
        let tier_label = take_string(&mut map, "tier", "dependency.tier")?;
        let tier = match RiskTier::from_str(&tier_label) {
            Some(tier) => tier,
            None => bail!("invalid dependency tier '{tier_label}'"),
        };
        let origin = take_string(&mut map, "origin", "dependency.origin")?;
        let license = take_optional_string(&mut map, "license")?;
        let depth = take_usize(&mut map, "depth", "dependency.depth")?;

        let dependencies = take_object_array(&mut map, "dependencies", "dependency.dependencies")?
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let context = format!("dependency.dependencies[{index}]");
                CrateRef::from_json_value(value, &context)
            })
            .collect::<Result<Vec<_>>>()?;

        let dependents = take_object_array(&mut map, "dependents", "dependency.dependents")?
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let context = format!("dependency.dependents[{index}]");
                CrateRef::from_json_value(value, &context)
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            name,
            version,
            tier,
            origin,
            license,
            depth,
            dependencies,
            dependents,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicySummary {
    pub config_path: String,
    pub max_depth: usize,
    pub forbidden_licenses: Vec<String>,
}

impl PolicySummary {
    pub fn to_json_value(&self) -> JsonValue {
        let mut map = JsonMap::new();
        map.insert(
            "config_path".to_string(),
            JsonValue::String(self.config_path.clone()),
        );
        map.insert(
            "max_depth".to_string(),
            JsonValue::Number(JsonNumber::from(self.max_depth as u64)),
        );
        map.insert(
            "forbidden_licenses".to_string(),
            JsonValue::Array(
                self.forbidden_licenses
                    .iter()
                    .map(|s| JsonValue::String(s.clone()))
                    .collect(),
            ),
        );
        JsonValue::Object(map)
    }

    pub(crate) fn from_json_value(value: JsonValue) -> Result<Self> {
        let mut map = expect_object(value, "policy summary")?;
        let config_path = take_string(&mut map, "config_path", "policy.config_path")?;
        let max_depth = take_usize(&mut map, "max_depth", "policy.max_depth")?;
        let forbidden_licenses = take_string_array_optional(
            &mut map,
            "forbidden_licenses",
            "policy.forbidden_licenses",
        )?;
        Ok(Self {
            config_path,
            max_depth,
            forbidden_licenses,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyRegistry {
    pub generated_at: String,
    pub workspace_root: String,
    pub root_packages: Vec<String>,
    pub policy: PolicySummary,
    pub entries: Vec<DependencyEntry>,
}

impl DependencyRegistry {
    pub fn comparison_key(&self) -> ComparisonRegistry {
        ComparisonRegistry {
            root_packages: self.root_packages.clone(),
            policy: self.policy.clone(),
            entries: self.entries.clone(),
        }
    }

    pub fn to_json_value(&self) -> JsonValue {
        let mut map = JsonMap::new();
        map.insert(
            "generated_at".to_string(),
            JsonValue::String(self.generated_at.clone()),
        );
        map.insert(
            "workspace_root".to_string(),
            JsonValue::String(self.workspace_root.clone()),
        );
        map.insert(
            "root_packages".to_string(),
            JsonValue::Array(
                self.root_packages
                    .iter()
                    .map(|pkg| JsonValue::String(pkg.clone()))
                    .collect(),
            ),
        );
        map.insert("policy".to_string(), self.policy.to_json_value());
        map.insert(
            "entries".to_string(),
            JsonValue::Array(
                self.entries
                    .iter()
                    .map(DependencyEntry::to_json_value)
                    .collect(),
            ),
        );
        JsonValue::Object(map)
    }

    pub fn from_json_value(value: JsonValue) -> Result<Self> {
        let mut map = expect_object(value, "dependency registry")?;
        let generated_at = take_string(&mut map, "generated_at", "registry.generated_at")?;
        let workspace_root = take_string(&mut map, "workspace_root", "registry.workspace_root")?;
        let root_packages =
            take_string_array_optional(&mut map, "root_packages", "registry.root_packages")?;
        let policy_value = take_value(&mut map, "policy", "registry.policy")?;
        let policy = PolicySummary::from_json_value(policy_value)?;
        let entries = take_object_array(&mut map, "entries", "registry.entries")?
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                DependencyEntry::from_json_value(value)
                    .with_context(|| format!("invalid registry entry at index {index}"))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            generated_at,
            workspace_root,
            root_packages,
            policy,
            entries,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonRegistry {
    pub root_packages: Vec<String>,
    pub policy: PolicySummary,
    pub entries: Vec<DependencyEntry>,
}

impl ComparisonRegistry {
    pub fn to_json_value(&self) -> JsonValue {
        let mut map = JsonMap::new();
        map.insert(
            "root_packages".to_string(),
            JsonValue::Array(
                self.root_packages
                    .iter()
                    .map(|pkg| JsonValue::String(pkg.clone()))
                    .collect(),
            ),
        );
        map.insert("policy".to_string(), self.policy.to_json_value());
        map.insert(
            "entries".to_string(),
            JsonValue::Array(
                self.entries
                    .iter()
                    .map(DependencyEntry::to_json_value)
                    .collect(),
            ),
        );
        JsonValue::Object(map)
    }

    pub fn from_json_value(value: JsonValue) -> Result<Self> {
        let mut map = expect_object(value, "comparison registry")?;
        let root_packages =
            take_string_array_optional(&mut map, "root_packages", "comparison.root_packages")?;
        let policy_value = take_value(&mut map, "policy", "comparison.policy")?;
        let policy = PolicySummary::from_json_value(policy_value)?;
        let entries = take_object_array(&mut map, "entries", "comparison.entries")?
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                DependencyEntry::from_json_value(value)
                    .with_context(|| format!("invalid comparison entry at index {index}"))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            root_packages,
            policy,
            entries,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViolationEntry {
    pub name: String,
    pub version: String,
    pub kind: ViolationKind,
    pub detail: String,
    pub depth: Option<usize>,
}

impl ViolationEntry {
    pub fn to_json_value(&self) -> JsonValue {
        let mut map = JsonMap::new();
        map.insert("name".to_string(), JsonValue::String(self.name.clone()));
        map.insert(
            "version".to_string(),
            JsonValue::String(self.version.clone()),
        );
        map.insert(
            "kind".to_string(),
            JsonValue::String(self.kind.as_str().to_string()),
        );
        map.insert("detail".to_string(), JsonValue::String(self.detail.clone()));
        map.insert(
            "depth".to_string(),
            self.depth
                .map(|value| JsonValue::Number(JsonNumber::from(value as u64)))
                .unwrap_or(JsonValue::Null),
        );
        JsonValue::Object(map)
    }

    pub(crate) fn from_json_value(value: JsonValue) -> Result<Self> {
        let mut map = expect_object(value, "violation entry")?;
        let name = take_string(&mut map, "name", "violation.name")?;
        let version = take_string(&mut map, "version", "violation.version")?;
        let kind_label = take_string(&mut map, "kind", "violation.kind")?;
        let kind = match ViolationKind::from_str(&kind_label) {
            Some(kind) => kind,
            None => bail!("invalid violation kind '{kind_label}'"),
        };
        let detail = take_string(&mut map, "detail", "violation.detail")?;
        let depth = take_optional_usize(&mut map, "depth", "violation.depth")?;
        Ok(Self {
            name,
            version,
            kind,
            detail,
            depth,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViolationKind {
    Depth,
    License,
    Tier,
    DirectLibp2p,
}

impl fmt::Display for ViolationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl ViolationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ViolationKind::Depth => "depth",
            ViolationKind::License => "license",
            ViolationKind::Tier => "tier",
            ViolationKind::DirectLibp2p => "direct_libp2p",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "depth" => Some(ViolationKind::Depth),
            "license" => Some(ViolationKind::License),
            "tier" => Some(ViolationKind::Tier),
            "direct_libp2p" => Some(ViolationKind::DirectLibp2p),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViolationReport {
    pub entries: Vec<ViolationEntry>,
}

impl ViolationReport {
    pub fn push(&mut self, entry: ViolationEntry) {
        self.entries.push(entry);
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn to_json_value(&self) -> JsonValue {
        JsonValue::Array(
            self.entries
                .iter()
                .map(ViolationEntry::to_json_value)
                .collect(),
        )
    }

    pub fn from_json_value(value: JsonValue) -> Result<Self> {
        let entries = parse_array(value, "violation report")?
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                ViolationEntry::from_json_value(entry)
                    .with_context(|| format!("invalid violation entry at index {index}"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { entries })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RiskTier {
    Strategic,
    Replaceable,
    Forbidden,
    Unclassified,
}

impl RiskTier {
    pub fn sort_key(&self) -> u8 {
        match self {
            RiskTier::Strategic => 0,
            RiskTier::Replaceable => 1,
            RiskTier::Forbidden => 2,
            RiskTier::Unclassified => 3,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RiskTier::Strategic => "strategic",
            RiskTier::Replaceable => "replaceable",
            RiskTier::Forbidden => "forbidden",
            RiskTier::Unclassified => "unclassified",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "strategic" => Some(RiskTier::Strategic),
            "replaceable" => Some(RiskTier::Replaceable),
            "forbidden" => Some(RiskTier::Forbidden),
            "unclassified" => Some(RiskTier::Unclassified),
            _ => None,
        }
    }
}

impl PartialOrd for RiskTier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RiskTier {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sort_key().cmp(&other.sort_key())
    }
}

impl fmt::Display for RiskTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

pub type DependencyMap = OrderedMap<String, DependencyEntry>;

fn expect_object(value: JsonValue, context: &str) -> Result<JsonMap> {
    match value {
        JsonValue::Object(map) => Ok(map),
        other => bail!(
            "{context} must be an object, found {}",
            describe_json(&other)
        ),
    }
}

fn take_string(map: &mut JsonMap, key: &str, context: &str) -> Result<String> {
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

fn take_usize(map: &mut JsonMap, key: &str, context: &str) -> Result<usize> {
    match map.remove(key) {
        Some(JsonValue::Number(number)) => {
            let Some(raw) = number.as_u64() else {
                bail!("{context} must be a non-negative integer");
            };
            if raw > usize::MAX as u64 {
                bail!("{context} is too large: {raw}");
            }
            Ok(raw as usize)
        }
        Some(other) => bail!(
            "{context} must be a number, found {}",
            describe_json(&other)
        ),
        None => bail!("{context} is missing"),
    }
}

fn take_optional_usize(map: &mut JsonMap, key: &str, context: &str) -> Result<Option<usize>> {
    match map.remove(key) {
        Some(JsonValue::Number(number)) => {
            let Some(raw) = number.as_u64() else {
                bail!("{context} must be a non-negative integer");
            };
            if raw > usize::MAX as u64 {
                bail!("{context} is too large: {raw}");
            }
            Ok(Some(raw as usize))
        }
        Some(JsonValue::Null) | None => Ok(None),
        Some(other) => bail!(
            "{context} must be a number or null, found {}",
            describe_json(&other)
        ),
    }
}

fn take_string_array_optional(map: &mut JsonMap, key: &str, context: &str) -> Result<Vec<String>> {
    match map.remove(key) {
        Some(JsonValue::Array(values)) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| match value {
                JsonValue::String(s) => Ok(s),
                other => bail!(
                    "{context}[{index}] must be a string, found {}",
                    describe_json(&other)
                ),
            })
            .collect(),
        Some(JsonValue::Null) | None => Ok(Vec::new()),
        Some(other) => bail!(
            "{context} must be an array of strings, found {}",
            describe_json(&other)
        ),
    }
}

fn take_object_array(map: &mut JsonMap, key: &str, context: &str) -> Result<Vec<JsonValue>> {
    match map.remove(key) {
        Some(JsonValue::Array(values)) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| match value {
                JsonValue::Object(inner) => Ok(JsonValue::Object(inner)),
                other => bail!(
                    "{context}[{index}] must be an object, found {}",
                    describe_json(&other)
                ),
            })
            .collect(),
        Some(JsonValue::Null) | None => Ok(Vec::new()),
        Some(other) => bail!(
            "{context} must be an array of objects, found {}",
            describe_json(&other)
        ),
    }
}

fn take_value(map: &mut JsonMap, key: &str, context: &str) -> Result<JsonValue> {
    if let Some(value) = map.remove(key) {
        Ok(value)
    } else {
        bail!("{context} is missing")
    }
}

fn parse_array(value: JsonValue, context: &str) -> Result<Vec<JsonValue>> {
    match value {
        JsonValue::Array(values) => Ok(values),
        JsonValue::Null => Ok(Vec::new()),
        other => bail!(
            "{context} must be an array, found {}",
            describe_json(&other)
        ),
    }
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

    #[test]
    fn registry_json_roundtrip() {
        let entry = DependencyEntry {
            name: "dep_a".to_string(),
            version: "1.2.3".to_string(),
            tier: RiskTier::Replaceable,
            origin: "registry".to_string(),
            license: Some("MIT".to_string()),
            depth: 2,
            dependencies: vec![CrateRef::new("dep_b".to_string(), "0.1.0".to_string())],
            dependents: vec![CrateRef::new("root".to_string(), "0.0.1".to_string())],
        };

        let registry = DependencyRegistry {
            generated_at: "2025-10-14T12:00:00Z".to_string(),
            workspace_root: "/workspace".to_string(),
            root_packages: vec!["root".to_string()],
            policy: PolicySummary {
                config_path: "policy.toml".to_string(),
                max_depth: 3,
                forbidden_licenses: vec!["AGPL-3.0".to_string()],
            },
            entries: vec![entry.clone()],
        };

        let value = registry.to_json_value();
        let decoded = DependencyRegistry::from_json_value(value).expect("decode registry");
        assert_eq!(registry, decoded);

        let report = ViolationReport {
            entries: vec![ViolationEntry {
                name: "dep_a".to_string(),
                version: "1.2.3".to_string(),
                kind: ViolationKind::License,
                detail: "forbidden license".to_string(),
                depth: Some(2),
            }],
        };

        let report_value = report.to_json_value();
        let decoded_report =
            ViolationReport::from_json_value(report_value).expect("decode violation report");
        assert_eq!(report, decoded_report);
    }
}
