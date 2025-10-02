use std::{collections::BTreeMap, fs, path::Path};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error at line {line}: {msg}")]
    Parse { line: usize, msg: String },
}

#[derive(Debug, Default, Clone)]
pub struct ConfigReader {
    values: BTreeMap<String, String>,
}

impl ConfigReader {
    pub fn load_file(path: &Path) -> Result<Self, ConfigError> {
        let raw = fs::read_to_string(path)?;
        Self::parse(&raw)
    }

    pub fn parse(input: &str) -> Result<Self, ConfigError> {
        let mut values = BTreeMap::new();
        for (idx, line) in input.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(ConfigError::Parse {
                    line: idx + 1,
                    msg: "missing '=' delimiter".to_string(),
                });
            };
            values.insert(
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            );
        }
        Ok(Self { values })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }
}
