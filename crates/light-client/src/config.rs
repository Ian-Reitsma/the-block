use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::device::DeviceFallback;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LightClientConfig {
    #[serde(default)]
    pub ignore_charging_requirement: bool,
    #[serde(default)]
    pub wifi_only_override: Option<bool>,
    #[serde(default)]
    pub min_battery_override: Option<f32>,
    #[serde(default)]
    pub fallback_override: Option<DeviceFallback>,
    #[serde(default = "default_max_snapshot_bytes")]
    pub max_snapshot_bytes: u64,
}

const fn default_max_snapshot_bytes() -> u64 {
    16 * 1024 * 1024
}

pub fn config_path() -> Option<PathBuf> {
    sys::paths::home_dir().map(|mut dir| {
        dir.push(".the_block");
        dir.push("light_client.toml");
        dir
    })
}

pub fn state_cache_path() -> Option<PathBuf> {
    sys::paths::home_dir().map(|mut dir| {
        dir.push(".the_block");
        dir.push("light_state.cache");
        dir
    })
}

pub fn load_user_config() -> io::Result<LightClientConfig> {
    let path = match config_path() {
        Some(p) => p,
        None => return Ok(LightClientConfig::default()),
    };
    match fs::read_to_string(&path) {
        Ok(contents) => {
            toml::from_str(&contents).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(LightClientConfig::default()),
        Err(err) => Err(err),
    }
}

pub fn save_user_config(config: &LightClientConfig) -> io::Result<()> {
    let path = match config_path() {
        Some(p) => p,
        None => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "home directory unavailable",
            ))
        }
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let rendered = toml::to_string_pretty(config)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(path, rendered)
}

impl LightClientConfig {
    pub fn snapshot_limit_bytes(&self) -> usize {
        let max = if self.max_snapshot_bytes == 0 {
            default_max_snapshot_bytes()
        } else {
            self.max_snapshot_bytes
        };
        max.min(u64::from(u32::MAX)) as usize
    }
}
