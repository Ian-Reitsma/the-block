use std::{cmp::Ordering, fmt};

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CrateRef {
    pub name: String,
    pub version: String,
}

impl CrateRef {
    pub fn new(name: String, version: String) -> Self {
        Self { name, version }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicySummary {
    pub config_path: String,
    pub max_depth: usize,
    pub forbidden_licenses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyRegistry {
    pub generated_at: DateTime<Utc>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComparisonRegistry {
    pub root_packages: Vec<String>,
    pub policy: PolicySummary,
    pub entries: Vec<DependencyEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ViolationEntry {
    pub name: String,
    pub version: String,
    pub kind: ViolationKind,
    pub detail: String,
    pub depth: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ViolationKind {
    Depth,
    License,
    Tier,
    DirectLibp2p,
}

impl fmt::Display for ViolationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            ViolationKind::Depth => "depth",
            ViolationKind::License => "license",
            ViolationKind::Tier => "tier",
            ViolationKind::DirectLibp2p => "direct_libp2p",
        };
        write!(f, "{}", label)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
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
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
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
        let label = match self {
            RiskTier::Strategic => "strategic",
            RiskTier::Replaceable => "replaceable",
            RiskTier::Forbidden => "forbidden",
            RiskTier::Unclassified => "unclassified",
        };
        write!(f, "{}", label)
    }
}

pub type DependencyMap = IndexMap<String, DependencyEntry>;
