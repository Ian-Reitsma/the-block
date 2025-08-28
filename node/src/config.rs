use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub snapshot_interval: u64,
    pub price_board_path: String,
    pub price_board_window: usize,
    pub price_board_save_interval: u64,
    #[serde(default)]
    pub rpc: RpcConfig,
    #[serde(default)]
    pub compute_market: ComputeMarketConfig,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            snapshot_interval: crate::DEFAULT_SNAPSHOT_INTERVAL,
            price_board_path: "state/price_board.v1.bin".to_string(),
            price_board_window: 100,
            price_board_save_interval: 30,
            rpc: RpcConfig::default(),
            compute_market: ComputeMarketConfig::default(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ComputeMarketConfig {
    pub settle_mode: crate::compute_market::settlement::SettleMode,
    pub min_fee_micros: u64,
}

impl Default for ComputeMarketConfig {
    fn default() -> Self {
        Self {
            settle_mode: crate::compute_market::settlement::SettleMode::DryRun,
            min_fee_micros: 100,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    pub allowed_hosts: Vec<String>,
    pub cors_allow_origins: Vec<String>,
    pub max_body_bytes: usize,
    pub request_timeout_ms: u64,
    pub enable_debug: bool,
    pub admin_token_file: Option<String>,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            allowed_hosts: vec!["127.0.0.1".into(), "localhost".into()],
            cors_allow_origins: vec![],
            max_body_bytes: 1_048_576,
            request_timeout_ms: 5_000,
            enable_debug: false,
            admin_token_file: Some("secrets/admin.token".into()),
        }
    }
}

impl NodeConfig {
    pub fn load(dir: &str) -> Self {
        let path = format!("{}/config.toml", dir);
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_else(|| {
                let cfg = Self::default();
                let _ = cfg.save(dir);
                cfg
            })
    }

    pub fn save(&self, dir: &str) -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        let path = format!("{}/config.toml", dir);
        let data =
            toml::to_string(self).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(path, data)
    }
}
