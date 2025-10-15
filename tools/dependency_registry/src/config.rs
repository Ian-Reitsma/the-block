use std::{collections::HashMap, fs, path::Path};

use diagnostics::anyhow::{bail, Context};
use foundation_serialization::toml::{self, Table as TomlTable, Value as TomlValue};

use crate::model::RiskTier;

#[derive(Debug, Clone, Default)]
pub struct PolicyConfig {
    pub tiers: TierConfig,
    pub licenses: LicenseConfig,
    pub settings: SettingsConfig,
}

impl PolicyConfig {
    pub fn load(path: &Path) -> diagnostics::Result<Self> {
        let raw = fs::read_to_string(path).with_context(|| {
            format!("unable to read policy configuration at {}", path.display())
        })?;
        let table = toml::parse_table(&raw).with_context(|| {
            format!("unable to parse policy configuration at {}", path.display())
        })?;
        let mut config = PolicyConfig::from_table(table)
            .with_context(|| format!("invalid policy configuration at {}", path.display()))?;
        config.normalise();
        Ok(config)
    }

    fn from_table(mut table: TomlTable) -> diagnostics::Result<Self> {
        let tiers = match table.remove("tiers") {
            Some(value) => TierConfig::from_value(value).context("invalid [tiers] section")?,
            None => TierConfig::default(),
        };

        let licenses = match table.remove("licenses") {
            Some(value) => {
                LicenseConfig::from_value(value).context("invalid [licenses] section")?
            }
            None => LicenseConfig::default(),
        };

        let settings = match table.remove("settings") {
            Some(value) => {
                SettingsConfig::from_value(value).context("invalid [settings] section")?
            }
            None => SettingsConfig::default(),
        };

        Ok(Self {
            tiers,
            licenses,
            settings,
        })
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

#[derive(Debug, Clone)]
pub struct SettingsConfig {
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

    fn from_value(value: TomlValue) -> diagnostics::Result<Self> {
        let mut table = expect_table(value, "settings")?;
        let max_depth = match table.remove("max_depth") {
            Some(value) => parse_usize(value, "settings.max_depth")?,
            None => Self::default_max_depth(),
        };
        Ok(Self { max_depth })
    }
}

#[derive(Debug, Clone, Default)]
pub struct TierConfig {
    pub strategic: Vec<String>,
    pub replaceable: Vec<String>,
    pub forbidden: Vec<String>,
}

impl TierConfig {
    fn normalise(&mut self) {
        self.strategic = normalise_list(&self.strategic);
        self.replaceable = normalise_list(&self.replaceable);
        self.forbidden = normalise_list(&self.forbidden);
    }

    fn from_value(value: TomlValue) -> diagnostics::Result<Self> {
        let mut table = expect_table(value, "tiers")?;
        let strategic = match table.remove("strategic") {
            Some(value) => parse_string_array(value, "tiers.strategic")?,
            None => Vec::new(),
        };
        let replaceable = match table.remove("replaceable") {
            Some(value) => parse_string_array(value, "tiers.replaceable")?,
            None => Vec::new(),
        };
        let forbidden = match table.remove("forbidden") {
            Some(value) => parse_string_array(value, "tiers.forbidden")?,
            None => Vec::new(),
        };
        Ok(Self {
            strategic,
            replaceable,
            forbidden,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct LicenseConfig {
    pub forbidden: Vec<String>,
}

impl LicenseConfig {
    fn normalise(&mut self) {
        self.forbidden = normalise_list_case_insensitive(&self.forbidden);
    }

    fn from_value(value: TomlValue) -> diagnostics::Result<Self> {
        let mut table = expect_table(value, "licenses")?;
        let forbidden = match table.remove("forbidden") {
            Some(value) => parse_string_array(value, "licenses.forbidden")?,
            None => Vec::new(),
        };
        Ok(Self { forbidden })
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

fn expect_table(value: TomlValue, context: &str) -> diagnostics::Result<TomlTable> {
    match value {
        TomlValue::Object(map) => Ok(map),
        other => bail!(
            "{context} must be a table, found {}",
            describe_value(&other)
        ),
    }
}

fn parse_string_array(value: TomlValue, context: &str) -> diagnostics::Result<Vec<String>> {
    match value {
        TomlValue::Array(items) => {
            let mut result = Vec::with_capacity(items.len());
            for (index, item) in items.into_iter().enumerate() {
                match item {
                    TomlValue::String(s) => result.push(s),
                    other => bail!(
                        "{context}[{index}] must be a string, found {}",
                        describe_value(&other)
                    ),
                }
            }
            Ok(result)
        }
        other => bail!(
            "{context} must be an array of strings, found {}",
            describe_value(&other)
        ),
    }
}

fn parse_usize(value: TomlValue, context: &str) -> diagnostics::Result<usize> {
    match value {
        TomlValue::Number(number) => {
            let Some(raw) = number.as_u64() else {
                bail!("{context} must be a non-negative integer");
            };
            if raw > usize::MAX as u64 {
                bail!("{context} is too large: {raw}");
            }
            Ok(raw as usize)
        }
        other => bail!(
            "{context} must be an integer, found {}",
            describe_value(&other)
        ),
    }
}

fn describe_value(value: &TomlValue) -> &'static str {
    match value {
        TomlValue::Null => "null",
        TomlValue::Bool(_) => "boolean",
        TomlValue::Number(_) => "number",
        TomlValue::String(_) => "string",
        TomlValue::Array(_) => "array",
        TomlValue::Object(_) => "table",
    }
}
