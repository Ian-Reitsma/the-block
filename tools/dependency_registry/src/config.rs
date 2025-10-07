use std::{collections::HashMap, fs, path::Path};

use diagnostics::anyhow::Context;
use serde::Deserialize;

use crate::model::RiskTier;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PolicyConfig {
    #[serde(default)]
    pub tiers: TierConfig,
    #[serde(default)]
    pub licenses: LicenseConfig,
    #[serde(default)]
    pub settings: SettingsConfig,
}

impl PolicyConfig {
    pub fn load(path: &Path) -> diagnostics::Result<Self> {
        let raw = fs::read_to_string(path).with_context(|| {
            format!("unable to read policy configuration at {}", path.display())
        })?;
        let mut config: PolicyConfig = toml::from_str(&raw).with_context(|| {
            format!("unable to parse policy configuration at {}", path.display())
        })?;
        config.normalise();
        Ok(config)
    }

    fn normalise(&mut self) {
        self.tiers.normalise();
        self.licenses.normalise();
        if self.settings.max_depth == 0 {
            self.settings.max_depth = SettingsConfig::default_max_depth();
        }
    }

    pub fn tier_map(&self) -> HashMap<String, RiskTier> {
        let mut map = HashMap::new();
        for name in &self.tiers.strategic {
            map.insert(name.clone(), RiskTier::Strategic);
        }
        for name in &self.tiers.replaceable {
            map.insert(name.clone(), RiskTier::Replaceable);
        }
        for name in &self.tiers.forbidden {
            map.insert(name.clone(), RiskTier::Forbidden);
        }
        map
    }

    pub fn tier_for(&self, name: &str) -> RiskTier {
        let key = name.to_ascii_lowercase();
        self.tier_map()
            .get(&key)
            .cloned()
            .unwrap_or(RiskTier::Unclassified)
    }

    pub fn forbidden_licenses(&self) -> &[String] {
        &self.licenses.forbidden
    }

    pub fn max_depth(&self, override_depth: Option<usize>) -> usize {
        override_depth.unwrap_or(self.settings.max_depth)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SettingsConfig {
    #[serde(default = "SettingsConfig::default_max_depth")]
    pub max_depth: usize,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            max_depth: Self::default_max_depth(),
        }
    }
}

impl SettingsConfig {
    pub fn default_max_depth() -> usize {
        3
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TierConfig {
    #[serde(default)]
    pub strategic: Vec<String>,
    #[serde(default)]
    pub replaceable: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
}

impl TierConfig {
    fn normalise(&mut self) {
        self.strategic = normalise_list(&self.strategic);
        self.replaceable = normalise_list(&self.replaceable);
        self.forbidden = normalise_list(&self.forbidden);
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LicenseConfig {
    #[serde(default)]
    pub forbidden: Vec<String>,
}

impl LicenseConfig {
    fn normalise(&mut self) {
        self.forbidden = normalise_list_case_insensitive(&self.forbidden);
    }
}

fn normalise_list(values: &[String]) -> Vec<String> {
    let mut deduped: Vec<String> = values.iter().map(|s| s.to_ascii_lowercase()).collect();
    deduped.sort();
    deduped.dedup();
    deduped
}

fn normalise_list_case_insensitive(values: &[String]) -> Vec<String> {
    let mut deduped: Vec<String> = values.iter().map(|s| s.to_ascii_uppercase()).collect();
    deduped.sort();
    deduped.dedup();
    deduped
}
