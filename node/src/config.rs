#[cfg(feature = "telemetry")]
use crate::telemetry::CONFIG_RELOAD_TOTAL;
use anyhow::Result;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use signal_hook::consts::signal::SIGHUP;
use signal_hook::iterator::Signals;
use std::fs;
use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::RwLock;
use std::thread;

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
    #[serde(default = "default_max_peer_metrics")]
    pub max_peer_metrics: usize,
    #[serde(default = "default_true")]
    pub peer_metrics_export: bool,
    #[serde(default = "default_peer_metrics_path")]
    pub peer_metrics_path: String,
    #[serde(default = "default_peer_metrics_retention")]
    pub peer_metrics_retention: u64,
    #[serde(default)]
    pub peer_metrics_compress: bool,
    #[serde(default = "default_peer_metrics_sample_rate")]
    pub peer_metrics_sample_rate: u32,
    #[serde(default = "default_metrics_export_dir")]
    pub metrics_export_dir: String,
    #[serde(default = "default_peer_metrics_export_quota_bytes")]
    pub peer_metrics_export_quota_bytes: u64,
    #[serde(default)]
    pub metrics_aggregator: Option<AggregatorConfig>,
    #[serde(default = "default_true")]
    pub track_peer_drop_reasons: bool,
    #[serde(default = "default_true")]
    pub track_handshake_failures: bool,
    #[serde(default = "default_peer_reputation_decay")]
    pub peer_reputation_decay: f64,
    #[serde(default = "default_p2p_max_per_sec")]
    pub p2p_max_per_sec: u32,
    #[serde(default = "default_p2p_max_bytes_per_sec")]
    pub p2p_max_bytes_per_sec: u64,
    #[serde(default = "default_provider_reputation_decay")]
    pub provider_reputation_decay: f64,
    #[serde(default = "default_provider_reputation_retention")]
    pub provider_reputation_retention: u64,
    #[serde(default = "default_true")]
    pub reputation_gossip: bool,
    #[serde(default = "default_true")]
    pub scheduler_metrics: bool,
    #[serde(default = "default_false")]
    pub gateway_dns_allow_external: bool,
    #[serde(default)]
    pub lighthouse: LighthouseConfig,
    #[serde(default)]
    pub quic: Option<QuicConfig>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            snapshot_interval: crate::DEFAULT_SNAPSHOT_INTERVAL,
            price_board_path: "state/price_board.v2.bin".to_string(),
            price_board_window: 100,
            price_board_save_interval: 30,
            rpc: RpcConfig::default(),
            compute_market: ComputeMarketConfig::default(),
            telemetry_summary_interval: 0,
            max_peer_metrics: default_max_peer_metrics(),
            peer_metrics_export: default_true(),
            peer_metrics_path: default_peer_metrics_path(),
            peer_metrics_retention: default_peer_metrics_retention(),
            peer_metrics_compress: false,
            peer_metrics_sample_rate: default_peer_metrics_sample_rate(),
            metrics_export_dir: default_metrics_export_dir(),
            peer_metrics_export_quota_bytes: default_peer_metrics_export_quota_bytes(),
            metrics_aggregator: None,
            track_peer_drop_reasons: default_true(),
            track_handshake_failures: default_true(),
            peer_reputation_decay: default_peer_reputation_decay(),
            p2p_max_per_sec: default_p2p_max_per_sec(),
            p2p_max_bytes_per_sec: default_p2p_max_bytes_per_sec(),
            provider_reputation_decay: default_provider_reputation_decay(),
            provider_reputation_retention: default_provider_reputation_retention(),
            reputation_gossip: default_true(),
            scheduler_metrics: default_true(),
            gateway_dns_allow_external: default_false(),
            lighthouse: LighthouseConfig::default(),
            quic: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct AggregatorConfig {
    pub url: String,
    pub auth_token: String,
}

static CONFIG_DIR: Lazy<RwLock<String>> = Lazy::new(|| RwLock::new(String::new()));
static CURRENT_CONFIG: Lazy<RwLock<NodeConfig>> = Lazy::new(|| RwLock::new(NodeConfig::default()));

fn default_max_peer_metrics() -> usize {
    1024
}

fn default_true() -> bool {
    true
}

fn default_peer_reputation_decay() -> f64 {
    0.01
}

fn default_p2p_max_per_sec() -> u32 {
    100
}

fn default_p2p_max_bytes_per_sec() -> u64 {
    65536
}

fn default_provider_reputation_decay() -> f64 {
    0.05
}

fn default_provider_reputation_retention() -> u64 {
    7 * 24 * 60 * 60
}

fn default_peer_metrics_path() -> String {
    "state/peer_metrics.json".into()
}

fn default_peer_metrics_retention() -> u64 {
    7 * 24 * 60 * 60
}

fn default_metrics_export_dir() -> String {
    "state".into()
}

fn default_peer_metrics_export_quota_bytes() -> u64 {
    10 * 1024 * 1024
}

fn default_peer_metrics_sample_rate() -> u32 {
    1
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ComputeMarketConfig {
    pub settle_mode: crate::compute_market::settlement::SettleMode,
    #[serde(default = "default_false")]
    pub enable_preempt: bool,
    #[serde(default = "default_preempt_min_delta")]
    pub preempt_min_delta: i64,
    #[serde(default = "default_low_priority_cap_pct")]
    pub low_priority_cap_pct: u8,
    #[serde(default = "default_reputation_multiplier_min")]
    pub reputation_multiplier_min: f64,
    #[serde(default = "default_reputation_multiplier_max")]
    pub reputation_multiplier_max: f64,
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
            enable_preempt: default_false(),
            preempt_min_delta: default_preempt_min_delta(),
            low_priority_cap_pct: default_low_priority_cap_pct(),
            reputation_multiplier_min: default_reputation_multiplier_min(),
            reputation_multiplier_max: default_reputation_multiplier_max(),
        }
    }
}

fn default_false() -> bool {
    false
}

fn default_preempt_min_delta() -> i64 {
    10
}

fn default_low_priority_cap_pct() -> u8 {
    50
}

fn default_reputation_multiplier_min() -> f64 {
    0.5
}

fn default_reputation_multiplier_max() -> f64 {
    1.0
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

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct QuicConfig {
    pub port: u16,
    pub cert_path: String,
    pub key_path: String,
    #[serde(default = "default_cert_ttl_days")]
    pub cert_ttl_days: u64,
}

fn default_cert_ttl_days() -> u64 {
    30
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

fn load_file(dir: &str) -> Result<NodeConfig> {
    let path = format!("{}/config.toml", dir);
    let data = fs::read_to_string(&path)?;
    Ok(toml::from_str(&data)?)
}

fn apply(cfg: &NodeConfig) {
    crate::net::set_peer_reputation_decay(cfg.peer_reputation_decay);
    crate::net::set_p2p_max_per_sec(cfg.p2p_max_per_sec);
    crate::net::set_p2p_max_bytes_per_sec(cfg.p2p_max_bytes_per_sec);
    crate::compute_market::scheduler::set_provider_reputation_decay(cfg.provider_reputation_decay);
    crate::compute_market::scheduler::set_provider_reputation_retention(
        cfg.provider_reputation_retention,
    );
    crate::net::set_track_handshake_fail(cfg.track_handshake_failures);
    crate::net::set_peer_metrics_sample_rate(cfg.peer_metrics_sample_rate as u64);
    crate::net::set_metrics_aggregator(
        cfg.metrics_aggregator.as_ref().map(|c| c.url.clone()),
        cfg.metrics_aggregator
            .as_ref()
            .map(|c| c.auth_token.clone()),
    );
}

pub fn reload() -> bool {
    let dir = CONFIG_DIR.read().unwrap().clone();
    if dir.is_empty() {
        return false;
    }
    match load_file(&dir) {
        Ok(cfg) => {
            apply(&cfg);
            *CURRENT_CONFIG.write().unwrap() = cfg;
            #[cfg(feature = "telemetry")]
            CONFIG_RELOAD_TOTAL.with_label_values(&["ok"]).inc();
            true
        }
        Err(e) => {
            #[cfg(feature = "telemetry")]
            {
                log::warn!("config_reload_failed: {e}");
                CONFIG_RELOAD_TOTAL.with_label_values(&["err"]).inc();
            }
            #[cfg(not(feature = "telemetry"))]
            eprintln!("config_reload_failed: {e}");
            false
        }
    }
}

pub fn watch(dir: &str) {
    {
        *CONFIG_DIR.write().unwrap() = dir.to_string();
    }
    let cfg_dir = dir.to_string();
    thread::spawn(move || {
        let (tx, rx) = channel();
        let mut watcher: RecommendedWatcher = notify::recommended_watcher(tx).expect("watcher");
        let path = Path::new(&cfg_dir).join("config.toml");
        watcher
            .watch(&path, RecursiveMode::NonRecursive)
            .expect("watch config");
        for res in rx {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Modify(_)) {
                    let _ = reload();
                }
            }
        }
    });

    thread::spawn(|| {
        let mut signals = Signals::new([SIGHUP]).expect("signals");
        for _ in signals.forever() {
            let _ = reload();
        }
    });
}

pub fn current() -> NodeConfig {
    CURRENT_CONFIG.read().unwrap().clone()
}

pub fn set_current(cfg: NodeConfig) {
    *CURRENT_CONFIG.write().unwrap() = cfg;
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InflationConfig {
    pub beta_storage_sub_ct: f64,
    pub gamma_read_sub_ct: f64,
    pub kappa_cpu_sub_ct: f64,
    pub lambda_bytes_out_sub_ct: f64,
    pub risk_lambda: f64,
    pub entropy_phi: f64,
    pub vdf_kappa: u64,
    pub haar_eta: f64,
    pub util_var_threshold: f64,
    pub fib_window_base_secs: f64,
    pub heuristic_mu: f64,
}

impl Default for InflationConfig {
    fn default() -> Self {
        Self {
            beta_storage_sub_ct: 0.05,
            gamma_read_sub_ct: 0.02,
            kappa_cpu_sub_ct: 0.01,
            lambda_bytes_out_sub_ct: 0.005,
            risk_lambda: 4.0 * std::f64::consts::LN_2,
            entropy_phi: 2.0,
            vdf_kappa: 1u64 << 28,
            haar_eta: 1.5,
            util_var_threshold: 0.1,
            fib_window_base_secs: 4.0,
            heuristic_mu: 0.5,
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
            gateway: GatewayCaps {
                req_rate_per_ip: 20,
            },
            func: FuncCaps {
                gas_limit_default: 100_000,
            },
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
