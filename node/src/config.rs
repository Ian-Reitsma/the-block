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
use std::sync::{mpsc::channel, Arc, RwLock};
use std::thread;
use std::time::Duration;

#[cfg(feature = "quic")]
use transport::{Config as TransportConfig, ProviderKind};

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
    #[serde(default = "default_peer_metrics_db")]
    pub peer_metrics_db: String,
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
    #[serde(default = "default_false")]
    pub gateway_dns_disable_verify: bool,
    #[serde(default = "default_gateway_blocklist")]
    pub gateway_blocklist: String,
    #[serde(default)]
    pub lighthouse: LighthouseConfig,
    #[serde(default)]
    pub quic: Option<QuicConfig>,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    #[serde(default)]
    pub jurisdiction: Option<String>,
    #[serde(default = "default_proof_rebate_rate")]
    pub proof_rebate_rate: u64,
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
            peer_metrics_db: default_peer_metrics_db(),
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
            gateway_dns_disable_verify: default_false(),
            gateway_blocklist: default_gateway_blocklist(),
            lighthouse: LighthouseConfig::default(),
            quic: None,
            telemetry: TelemetryConfig::default(),
            jurisdiction: None,
            proof_rebate_rate: default_proof_rebate_rate(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_sample_rate")]
    pub sample_rate: f64,
    #[serde(default = "default_compaction_secs")]
    pub compaction_secs: u64,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            sample_rate: default_sample_rate(),
            compaction_secs: default_compaction_secs(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct AggregatorConfig {
    pub url: String,
    pub auth_token: String,
    #[serde(default)]
    pub srv_record: Option<String>,
    #[serde(default = "default_retention_secs")]
    pub retention_secs: u64,
}

fn default_retention_secs() -> u64 {
    7 * 24 * 60 * 60
}

static CONFIG_DIR: Lazy<RwLock<String>> = Lazy::new(|| RwLock::new(String::new()));
static CURRENT_CONFIG: Lazy<RwLock<NodeConfig>> = Lazy::new(|| RwLock::new(NodeConfig::default()));
#[derive(Clone)]
pub struct RateLimitConfig {
    pub p2p_max_per_sec: u32,
    pub p2p_max_bytes_per_sec: u64,
}
#[derive(Clone)]
pub struct ReputationConfig {
    pub peer_reputation_decay: f64,
    pub provider_reputation_decay: f64,
    pub provider_reputation_retention: u64,
}
static RATE_LIMIT_CFG: Lazy<Arc<RwLock<RateLimitConfig>>> = Lazy::new(|| {
    let cfg = NodeConfig::default();
    Arc::new(RwLock::new(RateLimitConfig {
        p2p_max_per_sec: cfg.p2p_max_per_sec,
        p2p_max_bytes_per_sec: cfg.p2p_max_bytes_per_sec,
    }))
});
static REPUTATION_CFG: Lazy<Arc<RwLock<ReputationConfig>>> = Lazy::new(|| {
    let cfg = NodeConfig::default();
    Arc::new(RwLock::new(ReputationConfig {
        peer_reputation_decay: cfg.peer_reputation_decay,
        provider_reputation_decay: cfg.provider_reputation_decay,
        provider_reputation_retention: cfg.provider_reputation_retention,
    }))
});

pub fn rate_limit_cfg() -> Arc<RwLock<RateLimitConfig>> {
    Arc::clone(&RATE_LIMIT_CFG)
}

pub fn reputation_cfg() -> Arc<RwLock<ReputationConfig>> {
    Arc::clone(&REPUTATION_CFG)
}

fn default_max_peer_metrics() -> usize {
    1024
}

fn default_true() -> bool {
    true
}

fn default_peer_reputation_decay() -> f64 {
    0.01
}

fn default_gateway_blocklist() -> String {
    "gateway-blocklist.txt".into()
}

fn default_proof_rebate_rate() -> u64 {
    1
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

fn default_peer_metrics_db() -> String {
    std::env::var("PEER_METRICS_DB").unwrap_or_else(|_| "state/peer_metrics.db".into())
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

fn default_sample_rate() -> f64 {
    1.0
}

fn default_compaction_secs() -> u64 {
    60
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
    #[serde(default)]
    pub relay_only: bool,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct QuicConfig {
    pub port: u16,
    pub cert_path: String,
    pub key_path: String,
    #[serde(default = "default_cert_ttl_days")]
    pub cert_ttl_days: u64,
    #[serde(default)]
    pub transport: QuicTransportConfig,
}

fn default_cert_ttl_days() -> u64 {
    30
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct QuicTransportConfig {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub certificate_cache: Option<String>,
    #[serde(default)]
    pub retry_attempts: Option<u32>,
    #[serde(default)]
    pub retry_backoff_ms: Option<u64>,
    #[serde(default)]
    pub handshake_timeout_ms: Option<u64>,
    #[serde(default)]
    pub rotation_history: Option<usize>,
    #[serde(default)]
    pub rotation_max_age_secs: Option<u64>,
}

impl QuicTransportConfig {
    #[cfg(feature = "quic")]
    pub fn to_transport_config(&self) -> TransportConfig {
        let mut cfg = TransportConfig::default();
        if let Some(provider) = self.provider.as_ref() {
            let parsed = match provider.to_ascii_lowercase().as_str() {
                "quinn" => Some(ProviderKind::Quinn),
                "s2n" | "s2n-quic" => Some(ProviderKind::S2nQuic),
                _ => None,
            };
            if let Some(kind) = parsed {
                cfg.provider = kind;
            }
        }
        if let Some(path) = self.certificate_cache.as_ref() {
            cfg.certificate_cache = Some(Path::new(path).to_path_buf());
        }
        if let Some(attempts) = self.retry_attempts {
            cfg.retry.attempts = attempts as usize;
        }
        if let Some(ms) = self.retry_backoff_ms {
            cfg.retry.backoff = Duration::from_millis(ms as u64);
        }
        if let Some(ms) = self.handshake_timeout_ms {
            cfg.handshake_timeout = Duration::from_millis(ms);
        }
        cfg
    }

    #[cfg(feature = "quic")]
    pub fn apply_overrides(&mut self, other: &QuicTransportConfig) {
        if let Some(provider) = other.provider.as_ref() {
            self.provider = Some(provider.clone());
        }
        if let Some(cache) = other.certificate_cache.as_ref() {
            self.certificate_cache = Some(cache.clone());
        }
        if let Some(attempts) = other.retry_attempts {
            self.retry_attempts = Some(attempts);
        }
        if let Some(backoff) = other.retry_backoff_ms {
            self.retry_backoff_ms = Some(backoff);
        }
        if let Some(timeout) = other.handshake_timeout_ms {
            self.handshake_timeout_ms = Some(timeout);
        }
        if let Some(history) = other.rotation_history {
            self.rotation_history = Some(history);
        }
        if let Some(age) = other.rotation_max_age_secs {
            self.rotation_max_age_secs = Some(age);
        }
    }
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
            relay_only: false,
        }
    }
}

impl NodeConfig {
    pub fn load(dir: &str) -> Self {
        let path = format!("{}/default.toml", dir);
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
        let path = format!("{}/default.toml", dir);
        let data =
            toml::to_string(self).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(path, data)
    }
}

fn load_file(dir: &str) -> Result<NodeConfig> {
    let path = format!("{}/default.toml", dir);
    let data = fs::read_to_string(&path)?;
    let mut cfg: NodeConfig = toml::from_str(&data)?;
    #[cfg(feature = "quic")]
    {
        let quic_path = format!("{}/quic.toml", dir);
        if let Ok(quic_raw) = fs::read_to_string(&quic_path) {
            if let Ok(overrides) = toml::from_str::<QuicTransportConfig>(&quic_raw) {
                let quic_cfg = cfg.quic.get_or_insert_with(QuicConfig::default);
                quic_cfg.transport.apply_overrides(&overrides);
            }
        }
    }
    Ok(cfg)
}

fn apply(cfg: &NodeConfig) {
    {
        let mut rl = RATE_LIMIT_CFG.write().unwrap();
        rl.p2p_max_per_sec = cfg.p2p_max_per_sec;
        rl.p2p_max_bytes_per_sec = cfg.p2p_max_bytes_per_sec;
    }
    {
        let mut rep = REPUTATION_CFG.write().unwrap();
        rep.peer_reputation_decay = cfg.peer_reputation_decay;
        rep.provider_reputation_decay = cfg.provider_reputation_decay;
        rep.provider_reputation_retention = cfg.provider_reputation_retention;
    }
    crate::net::set_peer_reputation_decay(cfg.peer_reputation_decay);
    crate::net::set_p2p_max_per_sec(cfg.p2p_max_per_sec);
    crate::net::set_p2p_max_bytes_per_sec(cfg.p2p_max_bytes_per_sec);
    crate::compute_market::scheduler::set_provider_reputation_decay(cfg.provider_reputation_decay);
    crate::compute_market::scheduler::set_provider_reputation_retention(
        cfg.provider_reputation_retention,
    );
    crate::net::set_track_handshake_fail(cfg.track_handshake_failures);
    crate::net::set_peer_metrics_sample_rate(cfg.peer_metrics_sample_rate as u64);
    crate::net::set_peer_metrics_export(cfg.peer_metrics_export);
    crate::net::peer_metrics_store::init(&cfg.peer_metrics_db);
    crate::net::load_peer_metrics();
    crate::net::set_metrics_aggregator(cfg.metrics_aggregator.clone());
    crate::gateway::dns::set_allow_external(cfg.gateway_dns_allow_external);
    crate::gateway::dns::set_disable_verify(cfg.gateway_dns_disable_verify);
    #[cfg(feature = "gateway")]
    {
        crate::web::gateway::load_blocklist(&cfg.gateway_blocklist);
        crate::web::gateway::install_blocklist_reload();
    }
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::set_sample_rate(cfg.telemetry.sample_rate);
        crate::telemetry::set_compaction_interval(cfg.telemetry.compaction_secs);
    }
    #[cfg(feature = "quic")]
    {
        let quic_cfg = cfg.quic.as_ref();
        let transport_cfg = quic_cfg
            .map(|quic| quic.transport.to_transport_config())
            .unwrap_or_else(TransportConfig::default);
        if let Err(err) = crate::net::configure_transport(&transport_cfg) {
            #[cfg(feature = "telemetry")]
            tracing::warn!(reason = %err, "transport_configure_failed");
            #[cfg(not(feature = "telemetry"))]
            eprintln!("transport_configure_failed: {err}");
        }
        let (history, max_age) = quic_cfg
            .map(|quic| {
                (
                    quic.transport.rotation_history,
                    quic.transport.rotation_max_age_secs,
                )
            })
            .unwrap_or((None, None));
        crate::net::configure_peer_cert_policy(history, max_age);
    }
}

pub fn reload() -> bool {
    let dir = CONFIG_DIR.read().unwrap().clone();
    if dir.is_empty() {
        return false;
    }
    let path = Path::new(&dir).join("default.toml");
    if !path.exists() {
        return false;
    }
    match load_file(&dir) {
        Ok(cfg) => {
            apply(&cfg);
            *CURRENT_CONFIG.write().unwrap() = cfg;
            #[cfg(feature = "telemetry")]
            {
                CONFIG_RELOAD_TOTAL.with_label_values(&["ok"]).inc();
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                crate::telemetry::CONFIG_RELOAD_LAST_TS.set(ts);
            }
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
    crate::gossip::config::watch(dir);
    let cfg_dir = dir.to_string();
    thread::spawn(move || {
        let (tx, rx) = channel();
        let mut watcher: RecommendedWatcher = notify::recommended_watcher(tx).expect("watcher");
        let path = Path::new(&cfg_dir);
        if watcher.watch(path, RecursiveMode::NonRecursive).is_err() {
            return;
        }
        for res in rx {
            if let Ok(event) = res {
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                ) {
                    let mut reload_node = false;
                    let mut reload_gossip = false;
                    for path in &event.paths {
                        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                            match name {
                                "default.toml" => reload_node = true,
                                "gossip.toml" => reload_gossip = true,
                                _ => {}
                            }
                        }
                    }
                    if reload_node {
                        let _ = reload();
                    }
                    if reload_gossip {
                        crate::gossip::config::reload();
                    }
                }
            }
        }
    });

    thread::spawn(|| {
        let mut signals = Signals::new([SIGHUP]).expect("signals");
        for _ in signals.forever() {
            let _ = reload();
            crate::gossip::config::reload();
        }
    });
}

pub fn current() -> NodeConfig {
    CURRENT_CONFIG.read().unwrap().clone()
}

pub fn set_current(cfg: NodeConfig) {
    *CURRENT_CONFIG.write().unwrap() = cfg;
}

/// Return the directory backing the node configuration if available.
pub fn config_dir() -> Option<String> {
    let dir = CONFIG_DIR.read().unwrap().clone();
    if dir.is_empty() {
        None
    } else {
        Some(dir)
    }
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
