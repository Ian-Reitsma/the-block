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
    pub telemetry_summary_interval: u64,
    #[serde(default)]
    pub lighthouse: LighthouseConfig,
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
            telemetry_summary_interval: 0,
            lighthouse: LighthouseConfig::default(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ComputeMarketConfig {
    pub settle_mode: crate::compute_market::settlement::SettleMode,
    pub min_fee_micros: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LighthouseConfig {
    pub low_density_multiplier_max: u64,
}

impl Default for LighthouseConfig {
    fn default() -> Self {
        Self {
            low_density_multiplier_max: 1_000_000,
        }
    }
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
    #[serde(default)]
    pub dispute_window_epochs: u64,
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
            dispute_window_epochs: 0,
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

#[derive(Clone, Serialize, Deserialize)]
pub struct InflationConfig {
    pub beta_storage_sub_ct: f64,
    pub gamma_read_sub_ct: f64,
    pub kappa_cpu_sub_ct: f64,
    pub lambda_bytes_out_sub_ct: f64,
}

impl Default for InflationConfig {
    fn default() -> Self {
        Self {
            beta_storage_sub_ct: 0.05,
            gamma_read_sub_ct: 0.02,
            kappa_cpu_sub_ct: 0.01,
            lambda_bytes_out_sub_ct: 0.005,
        }
    }
}

pub fn load_inflation(dir: &str) -> InflationConfig {
    let path = format!("{}/inflation.toml", dir);
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StorageCaps {
    pub l2_cap_bytes_per_epoch: u64,
    pub bytes_per_sender_epoch_cap: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GatewayCaps {
    pub req_rate_per_ip: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FuncCaps {
    pub gas_limit_default: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CapsConfig {
    pub storage: StorageCaps,
    pub gateway: GatewayCaps,
    pub func: FuncCaps,
}

impl Default for CapsConfig {
    fn default() -> Self {
        Self {
            storage: StorageCaps {
                l2_cap_bytes_per_epoch: 33_554_432,
                bytes_per_sender_epoch_cap: 16_777_216,
            },
            gateway: GatewayCaps { req_rate_per_ip: 20 },
            func: FuncCaps { gas_limit_default: 100_000 },
        }
    }
}

pub fn load_caps(dir: &str) -> CapsConfig {
    let path = format!("{}/caps.toml", dir);
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}
