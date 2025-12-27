use super::ParamKey;
use crate::ad_readiness::AdReadinessConfig;
use crate::energy::{self, GovernanceEnergyParams};
use crate::scheduler::{self, ServiceClass};
use crate::Blockchain;
use ad_market::{DistributionPolicy, MarketplaceHandle};
use bridge_types::BridgeIncentiveParameters;
#[cfg(feature = "telemetry")]
use diagnostics::tracing::info;
use foundation_math::linalg::{Matrix, Vector};
use foundation_serialization::{binary, json};
use foundation_serialization::{Deserialize, Serialize};
use governance_spec::{
    decode_runtime_backend_policy, decode_storage_engine_policy, decode_transport_provider_policy,
    validate_runtime_backend_policy, validate_storage_engine_policy,
    validate_transport_provider_policy, DEFAULT_RUNTIME_BACKEND_POLICY,
    DEFAULT_STORAGE_ENGINE_POLICY, DEFAULT_TRANSPORT_PROVIDER_POLICY, RUNTIME_BACKEND_OPTIONS,
    STORAGE_ENGINE_OPTIONS, TRANSPORT_PROVIDER_OPTIONS,
};
use std::time::Duration;
use std::{fs, fs::OpenOptions, io::Write, path::Path};

const fn mask_all(len: usize) -> i64 {
    ((1u64 << len) - 1) as i64
}

const RUNTIME_BACKEND_MASK_ALL: i64 = mask_all(RUNTIME_BACKEND_OPTIONS.len());
const TRANSPORT_PROVIDER_MASK_ALL: i64 = mask_all(TRANSPORT_PROVIDER_OPTIONS.len());
const STORAGE_ENGINE_MASK_ALL: i64 = mask_all(STORAGE_ENGINE_OPTIONS.len());

pub struct Runtime<'a> {
    pub bc: &'a mut Blockchain,
    current_params: Option<Params>,
    market: Option<MarketplaceHandle>,
    ad_readiness: Option<crate::ad_readiness::AdReadinessHandle>,
}

impl<'a> Runtime<'a> {
    pub fn new(bc: &'a mut Blockchain) -> Self {
        Self {
            bc,
            current_params: None,
            market: None,
            ad_readiness: None,
        }
    }

    pub fn with_market(bc: &'a mut Blockchain, market: MarketplaceHandle) -> Self {
        Self {
            bc,
            current_params: None,
            market: Some(market),
            ad_readiness: None,
        }
    }

    pub fn set_market(&mut self, market: MarketplaceHandle) {
        self.market = Some(market);
    }

    pub fn set_ad_readiness(&mut self, readiness: crate::ad_readiness::AdReadinessHandle) {
        self.ad_readiness = Some(readiness);
        self.sync_ad_readiness();
    }

    fn sync_energy_params(&self) {
        let snapshot = GovernanceEnergyParams {
            min_stake: self.bc.params.energy_min_stake.max(0) as u64,
            oracle_timeout_blocks: self.bc.params.energy_oracle_timeout_blocks.max(1) as u64,
            slashing_rate_bps: self.bc.params.energy_slashing_rate_bps.clamp(0, 10_000) as u16,
        };
        energy::set_governance_params(snapshot);
    }

    pub fn set_current_params(&mut self, params: &Params) {
        self.current_params = Some(params.clone());
    }

    pub fn clear_current_params(&mut self) {
        self.current_params = None;
    }

    pub fn params_snapshot(&self) -> Option<&Params> {
        self.current_params.as_ref()
    }

    fn sync_read_subsidy_distribution(&self) {
        let market = match &self.market {
            Some(handle) => handle,
            None => return,
        };
        let clamp_percent = |value: i64| value.clamp(0, 100) as u64;
        let policy = DistributionPolicy::new(
            clamp_percent(self.bc.params.read_subsidy_viewer_percent),
            clamp_percent(self.bc.params.read_subsidy_host_percent),
            clamp_percent(self.bc.params.read_subsidy_hardware_percent),
            clamp_percent(self.bc.params.read_subsidy_verifier_percent),
            clamp_percent(self.bc.params.read_subsidy_liquidity_percent),
        );
        market.update_distribution(policy.normalize());
    }

    fn sync_ad_readiness(&self) {
        let readiness = match &self.ad_readiness {
            Some(handle) => handle,
            None => return,
        };
        let cfg = AdReadinessConfig {
            window_secs: self.bc.params.ad_readiness_window_secs.max(1) as u64,
            min_unique_viewers: self.bc.params.ad_readiness_min_unique_viewers.max(0) as u64,
            min_host_count: self.bc.params.ad_readiness_min_host_count.max(0) as u64,
            min_provider_count: self.bc.params.ad_readiness_min_provider_count.max(0) as u64,
            use_percentile_thresholds: self.bc.params.ad_use_percentile_thresholds > 0,
            viewer_percentile: self.bc.params.ad_viewer_percentile.clamp(0, 100) as u8,
            host_percentile: self.bc.params.ad_host_percentile.clamp(0, 100) as u8,
            provider_percentile: self.bc.params.ad_provider_percentile.clamp(0, 100) as u8,
            ema_smoothing_ppm: self.bc.params.ad_ema_smoothing_ppm.clamp(0, 1_000_000) as u32,
            floor_unique_viewers: self.bc.params.ad_floor_unique_viewers.max(0) as u64,
            floor_host_count: self.bc.params.ad_floor_host_count.max(0) as u64,
            floor_provider_count: self.bc.params.ad_floor_provider_count.max(0) as u64,
            cap_unique_viewers: self.bc.params.ad_cap_unique_viewers.max(0) as u64,
            cap_host_count: self.bc.params.ad_cap_host_count.max(0) as u64,
            cap_provider_count: self.bc.params.ad_cap_provider_count.max(0) as u64,
            percentile_buckets: self.bc.params.ad_percentile_buckets.clamp(4, 360) as u16,
        };
        readiness.update_config(cfg);
    }

    pub fn set_launch_operational(&mut self, enabled: bool) {
        self.bc.params.launch_operational_flag = if enabled { 1 } else { 0 };
    }

    pub fn set_dns_rehearsal(&mut self, enabled: bool) {
        self.bc.params.dns_rehearsal_enabled = if enabled { 1 } else { 0 };
        crate::gateway::dns::set_rehearsal(enabled);
    }

    pub fn set_launch_economics(&mut self, enabled: bool) {
        self.bc.params.launch_economics_autopilot = if enabled { 1 } else { 0 };
    }

    pub fn set_consumer_p90_comfort(&mut self, v: u64) {
        self.bc.set_consumer_p90_comfort(v);
    }
    pub fn set_fee_floor_policy(&mut self, window: u64, percentile: u64) {
        self.bc.params.fee_floor_window = window as i64;
        self.bc.params.fee_floor_percentile = percentile as i64;
        self.bc
            .set_fee_floor_policy(window as usize, percentile as u32);
    }
    pub fn set_min_capacity(&mut self, v: u64) {
        crate::compute_market::admission::set_min_capacity(v);
    }
    pub fn set_snapshot_interval(&mut self, d: Duration) {
        self.bc.snapshot.set_interval(d.as_secs());
    }
    pub fn set_fair_share_cap(&mut self, v: f64) {
        crate::compute_market::admission::set_fair_share_cap(v);
    }
    pub fn set_burst_refill_rate(&mut self, v: f64) {
        crate::compute_market::admission::set_burst_refill_rate(v);
    }
    pub fn set_rent_rate(&mut self, v: i64) {
        self.bc.params.rent_rate_per_byte = v;
    }
    pub fn set_badge_expiry(&mut self, v: u64) {
        crate::service_badge::set_badge_ttl_secs(v);
    }
    pub fn set_badge_issue_uptime(&mut self, v: u64) {
        crate::service_badge::set_badge_issue_uptime(v);
    }
    pub fn set_badge_revoke_uptime(&mut self, v: u64) {
        crate::service_badge::set_badge_revoke_uptime(v);
    }
    pub fn set_jurisdiction_region(&mut self, v: i64) {
        let region = match v {
            1 => "US",
            2 => "EU",
            _ => "UNSPEC",
        };
        let language = default_language_for_region(region);
        self.bc.config.jurisdiction = Some(region.to_string());
        self.bc.save_config();
        let _ = crate::le_portal::record_action(
            &self.bc.path,
            "governance",
            &format!("set_jurisdiction_{region}"),
            region,
            language,
        );
    }
    pub fn set_ai_diagnostics_enabled(&mut self, v: bool) {
        self.bc.params.ai_diagnostics_enabled = v as i64;
    }
    pub fn set_dual_token_settlement_enabled(&mut self, v: bool) {
        self.bc.params.dual_token_settlement_enabled = if v { 1 } else { 0 };
        self.sync_read_subsidy_distribution();
    }
    pub fn set_scheduler_weight(&mut self, class: ServiceClass, weight: u64) {
        scheduler::set_weight(class, weight as u32);
    }

    pub fn set_runtime_backend_policy(&mut self, allowed: &[String]) {
        crate::config::set_runtime_backend_policy(allowed);
    }

    pub fn set_transport_provider_policy(&mut self, allowed: &[String]) {
        crate::config::set_transport_provider_policy(allowed);
    }

    pub fn set_storage_engine_policy(&mut self, allowed: &[String]) {
        crate::config::set_storage_engine_policy(allowed);
    }

    pub fn set_energy_min_stake(&mut self, value: u64) {
        self.bc.params.energy_min_stake = value as i64;
        self.sync_energy_params();
    }

    pub fn set_energy_oracle_timeout_blocks(&mut self, value: u64) {
        self.bc.params.energy_oracle_timeout_blocks = value as i64;
        self.sync_energy_params();
    }

    pub fn set_energy_slashing_rate_bps(&mut self, value: u64) {
        self.bc.params.energy_slashing_rate_bps = value.min(10_000) as i64;
        self.sync_energy_params();
    }

    pub fn set_bridge_incentives(
        &mut self,
        min_bond: u64,
        duty_reward: u64,
        failure_slash: u64,
        challenge_slash: u64,
        duty_window_secs: u64,
    ) {
        let params = BridgeIncentiveParameters {
            min_bond,
            duty_reward,
            failure_slash,
            challenge_slash,
            duty_window_secs,
        };
        crate::bridge::set_global_incentives(params.clone());
        self.bc.params.bridge_min_bond = params.min_bond as i64;
        self.bc.params.bridge_duty_reward = params.duty_reward as i64;
        self.bc.params.bridge_failure_slash = params.failure_slash as i64;
        self.bc.params.bridge_challenge_slash = params.challenge_slash as i64;
        self.bc.params.bridge_duty_window_secs = params.duty_window_secs as i64;
    }
}

fn default_language_for_region(region: &str) -> &'static str {
    match region {
        "US" => "en-US",
        "EU" => "en-GB",
        "UNSPEC" => "en",
        _ => "en",
    }
}

pub struct ParamSpec {
    pub key: ParamKey,
    pub default: i64,
    pub min: i64,
    pub max: i64,
    pub unit: &'static str,
    pub timelock_epochs: u64,
    pub apply: fn(i64, &mut Params) -> Result<(), ()>,
    pub apply_runtime: fn(i64, &mut Runtime) -> Result<(), ()>,
}

const DEFAULT_TIMELOCK_EPOCHS: u64 = 2;
const KILL_SWITCH_TIMELOCK_EPOCHS: u64 = 10800; // ≈12h at 4s epochs

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct Params {
    pub snapshot_interval_secs: i64,
    pub consumer_fee_comfort_p90_microunits: i64,
    pub fee_floor_window: i64,
    pub fee_floor_percentile: i64,
    pub industrial_admission_min_capacity: i64,
    pub fairshare_global_max_ppm: i64,
    pub burst_refill_rate_per_s_ppm: i64,
    pub beta_storage_sub: i64,
    pub gamma_read_sub: i64,
    pub kappa_cpu_sub: i64,
    pub lambda_bytes_out_sub: i64,
    #[serde(default = "default_read_subsidy_viewer_percent")]
    pub read_subsidy_viewer_percent: i64,
    #[serde(default = "default_read_subsidy_host_percent")]
    pub read_subsidy_host_percent: i64,
    #[serde(default = "default_read_subsidy_hardware_percent")]
    pub read_subsidy_hardware_percent: i64,
    #[serde(default = "default_read_subsidy_verifier_percent")]
    pub read_subsidy_verifier_percent: i64,
    #[serde(default = "default_read_subsidy_liquidity_percent")]
    pub read_subsidy_liquidity_percent: i64,
    #[serde(default = "default_dual_token_settlement_enabled")]
    pub dual_token_settlement_enabled: i64,
    #[serde(default)]
    pub launch_operational_flag: i64,
    #[serde(default = "default_dns_rehearsal_enabled")]
    pub dns_rehearsal_enabled: i64,
    #[serde(default = "default_launch_economics_autopilot")]
    pub launch_economics_autopilot: i64,
    #[serde(default = "default_ad_readiness_window_secs")]
    pub ad_readiness_window_secs: i64,
    #[serde(default = "default_ad_readiness_min_unique_viewers")]
    pub ad_readiness_min_unique_viewers: i64,
    #[serde(default = "default_ad_readiness_min_host_count")]
    pub ad_readiness_min_host_count: i64,
    #[serde(default = "default_ad_readiness_min_provider_count")]
    pub ad_readiness_min_provider_count: i64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub treasury_percent: i64,
    #[serde(default = "default_proof_rebate_limit")]
    pub proof_rebate_limit: i64,
    pub rent_rate_per_byte: i64,
    pub kill_switch_subsidy_reduction: i64,
    pub miner_reward_logistic_target: i64,
    pub logistic_slope_milli: i64,
    pub miner_hysteresis: i64,
    pub risk_lambda: i64,
    pub entropy_phi: i64,
    pub haar_eta: i64,
    pub util_var_threshold: i64,
    pub fib_window_base_secs: i64,
    pub heuristic_mu_milli: i64,
    pub industrial_multiplier: i64,
    pub badge_expiry_secs: i64,
    pub badge_issue_uptime_percent: i64,
    pub badge_revoke_uptime_percent: i64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub ad_rehearsal_enabled: i64,
    #[serde(default = "default_ad_rehearsal_stability_windows")]
    pub ad_rehearsal_stability_windows: i64,
    #[serde(default = "default_presence_min_crowd_size")]
    pub presence_min_crowd_size: i64,
    #[serde(default = "default_presence_ttl_secs")]
    pub presence_ttl_secs: i64,
    #[serde(default = "default_presence_radius_meters")]
    pub presence_radius_meters: i64,
    #[serde(default = "default_presence_proof_cache_size")]
    pub presence_proof_cache_size: i64,
    #[serde(default = "default_presence_min_confidence_bps")]
    pub presence_min_confidence_bps: i64,
    // Dynamic ad-readiness thresholding (governance-controlled)
    #[serde(default)]
    pub ad_use_percentile_thresholds: i64,
    #[serde(default = "default_viewer_percentile")]
    pub ad_viewer_percentile: i64,
    #[serde(default = "default_host_percentile")]
    pub ad_host_percentile: i64,
    #[serde(default = "default_provider_percentile")]
    pub ad_provider_percentile: i64,
    #[serde(default = "default_ema_smoothing_ppm")]
    pub ad_ema_smoothing_ppm: i64,
    #[serde(default)]
    pub ad_floor_unique_viewers: i64,
    #[serde(default)]
    pub ad_floor_host_count: i64,
    #[serde(default)]
    pub ad_floor_provider_count: i64,
    #[serde(default)]
    pub ad_cap_unique_viewers: i64,
    #[serde(default)]
    pub ad_cap_host_count: i64,
    #[serde(default)]
    pub ad_cap_provider_count: i64,
    #[serde(default = "default_percentile_buckets")]
    pub ad_percentile_buckets: i64,
    #[serde(default = "default_energy_min_stake")]
    pub energy_min_stake: i64,
    #[serde(default = "default_energy_oracle_timeout_blocks")]
    pub energy_oracle_timeout_blocks: i64,
    #[serde(default = "default_energy_slashing_rate_bps")]
    pub energy_slashing_rate_bps: i64,
    pub jurisdiction_region: i64,
    pub ai_diagnostics_enabled: i64,
    pub kalman_r_short: i64,
    pub kalman_r_med: i64,
    pub kalman_r_long: i64,
    pub scheduler_weight_gossip: i64,
    pub scheduler_weight_compute: i64,
    pub scheduler_weight_storage: i64,
    #[serde(default = "default_runtime_backend_policy")]
    pub runtime_backend_policy: i64,
    #[serde(default = "default_transport_provider_policy")]
    pub transport_provider_policy: i64,
    #[serde(default = "default_storage_engine_policy")]
    pub storage_engine_policy: i64,
    pub bridge_min_bond: i64,
    pub bridge_duty_reward: i64,
    pub bridge_failure_slash: i64,
    pub bridge_challenge_slash: i64,
    pub bridge_duty_window_secs: i64,
    // ===== Economic Control Laws =====
    // Layer 1: Inflation Controller
    #[serde(default = "default_inflation_target_bps")]
    pub inflation_target_bps: i64,
    #[serde(default = "default_inflation_controller_gain")]
    pub inflation_controller_gain: i64,
    #[serde(default = "default_min_annual_issuance_block")]
    pub min_annual_issuance_block: i64,
    #[serde(default = "default_max_annual_issuance_block")]
    pub max_annual_issuance_block: i64,
    // Layer 2: Subsidy Allocator
    #[serde(default = "default_storage_util_target_bps")]
    pub storage_util_target_bps: i64,
    #[serde(default = "default_storage_margin_target_bps")]
    pub storage_margin_target_bps: i64,
    #[serde(default = "default_compute_util_target_bps")]
    pub compute_util_target_bps: i64,
    #[serde(default = "default_compute_margin_target_bps")]
    pub compute_margin_target_bps: i64,
    #[serde(default = "default_energy_util_target_bps")]
    pub energy_util_target_bps: i64,
    #[serde(default = "default_energy_margin_target_bps")]
    pub energy_margin_target_bps: i64,
    #[serde(default = "default_ad_util_target_bps")]
    pub ad_util_target_bps: i64,
    #[serde(default = "default_ad_margin_target_bps")]
    pub ad_margin_target_bps: i64,
    #[serde(default = "default_subsidy_allocator_alpha")]
    pub subsidy_allocator_alpha: i64,
    #[serde(default = "default_subsidy_allocator_beta")]
    pub subsidy_allocator_beta: i64,
    #[serde(default = "default_subsidy_allocator_temperature")]
    pub subsidy_allocator_temperature: i64,
    #[serde(default = "default_subsidy_allocator_drift_rate")]
    pub subsidy_allocator_drift_rate: i64,
    // Layer 3: Market Multipliers - Storage
    #[serde(default = "default_storage_util_responsiveness")]
    pub storage_util_responsiveness: i64,
    #[serde(default = "default_storage_cost_responsiveness")]
    pub storage_cost_responsiveness: i64,
    #[serde(default = "default_storage_multiplier_floor")]
    pub storage_multiplier_floor: i64,
    #[serde(default = "default_storage_multiplier_ceiling")]
    pub storage_multiplier_ceiling: i64,
    // Layer 3: Market Multipliers - Compute
    #[serde(default = "default_compute_util_responsiveness")]
    pub compute_util_responsiveness: i64,
    #[serde(default = "default_compute_cost_responsiveness")]
    pub compute_cost_responsiveness: i64,
    #[serde(default = "default_compute_multiplier_floor")]
    pub compute_multiplier_floor: i64,
    #[serde(default = "default_compute_multiplier_ceiling")]
    pub compute_multiplier_ceiling: i64,
    // Layer 3: Market Multipliers - Energy
    #[serde(default = "default_energy_util_responsiveness")]
    pub energy_util_responsiveness: i64,
    #[serde(default = "default_energy_cost_responsiveness")]
    pub energy_cost_responsiveness: i64,
    #[serde(default = "default_energy_multiplier_floor")]
    pub energy_multiplier_floor: i64,
    #[serde(default = "default_energy_multiplier_ceiling")]
    pub energy_multiplier_ceiling: i64,
    // Layer 3: Market Multipliers - Ad
    #[serde(default = "default_ad_util_responsiveness")]
    pub ad_util_responsiveness: i64,
    #[serde(default = "default_ad_cost_responsiveness")]
    pub ad_cost_responsiveness: i64,
    #[serde(default = "default_ad_multiplier_floor")]
    pub ad_multiplier_floor: i64,
    #[serde(default = "default_ad_multiplier_ceiling")]
    pub ad_multiplier_ceiling: i64,
    // Layer 4: Ad Market Drift
    #[serde(default = "default_ad_platform_take_target_bps")]
    pub ad_platform_take_target_bps: i64,
    #[serde(default = "default_ad_user_share_target_bps")]
    pub ad_user_share_target_bps: i64,
    #[serde(default = "default_ad_drift_rate")]
    pub ad_drift_rate: i64,
    // Layer 4: Tariff Controller
    #[serde(default = "default_tariff_public_revenue_target_bps")]
    pub tariff_public_revenue_target_bps: i64,
    #[serde(default = "default_tariff_drift_rate")]
    pub tariff_drift_rate: i64,
    #[serde(default = "default_tariff_min_bps")]
    pub tariff_min_bps: i64,
    #[serde(default = "default_tariff_max_bps")]
    pub tariff_max_bps: i64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            snapshot_interval_secs: 30,
            consumer_fee_comfort_p90_microunits: 2_500,
            fee_floor_window: 256,
            fee_floor_percentile: 75,
            industrial_admission_min_capacity: 10,
            fairshare_global_max_ppm: 250_000,
            burst_refill_rate_per_s_ppm: ((30.0 / 60.0) * 1_000_000.0) as i64,
            beta_storage_sub: 50,
            gamma_read_sub: 20,
            kappa_cpu_sub: 10,
            lambda_bytes_out_sub: 5,
            read_subsidy_viewer_percent: default_read_subsidy_viewer_percent(),
            read_subsidy_host_percent: default_read_subsidy_host_percent(),
            read_subsidy_hardware_percent: default_read_subsidy_hardware_percent(),
            read_subsidy_verifier_percent: default_read_subsidy_verifier_percent(),
            read_subsidy_liquidity_percent: default_read_subsidy_liquidity_percent(),
            dual_token_settlement_enabled: default_dual_token_settlement_enabled(),
            launch_operational_flag: 0,
            dns_rehearsal_enabled: default_dns_rehearsal_enabled(),
            launch_economics_autopilot: default_launch_economics_autopilot(),
            ad_readiness_window_secs: default_ad_readiness_window_secs(),
            ad_readiness_min_unique_viewers: default_ad_readiness_min_unique_viewers(),
            ad_readiness_min_host_count: default_ad_readiness_min_host_count(),
            ad_readiness_min_provider_count: default_ad_readiness_min_provider_count(),
            treasury_percent: 0,
            proof_rebate_limit: default_proof_rebate_limit(),
            rent_rate_per_byte: 0,
            kill_switch_subsidy_reduction: 0,
            miner_reward_logistic_target: 100,
            logistic_slope_milli: (99f64.ln() / (0.1 * 100.0) * 1000.0) as i64,
            miner_hysteresis: 10,
            risk_lambda: (4.0 * std::f64::consts::LN_2 * 1000.0) as i64,
            entropy_phi: 2000,
            haar_eta: 1500,
            util_var_threshold: 100,
            fib_window_base_secs: 4,
            heuristic_mu_milli: 500,
            industrial_multiplier: 100,
            badge_expiry_secs: 30 * 24 * 60 * 60,
            badge_issue_uptime_percent: 99,
            badge_revoke_uptime_percent: 95,
            ad_rehearsal_enabled: 0,
            ad_rehearsal_stability_windows: 6,
            presence_min_crowd_size: 5,
            presence_ttl_secs: 86400,          // 24 hours
            presence_radius_meters: 500,       // 500m default aggregation radius
            presence_proof_cache_size: 10000,  // Max cached PresenceReceipt entries
            presence_min_confidence_bps: 8000, // 80% minimum confidence
            ad_use_percentile_thresholds: 0,
            ad_viewer_percentile: default_viewer_percentile(),
            ad_host_percentile: default_host_percentile(),
            ad_provider_percentile: default_provider_percentile(),
            ad_ema_smoothing_ppm: default_ema_smoothing_ppm(),
            ad_floor_unique_viewers: 0,
            ad_floor_host_count: 0,
            ad_floor_provider_count: 0,
            ad_cap_unique_viewers: 0,
            ad_cap_host_count: 0,
            ad_cap_provider_count: 0,
            ad_percentile_buckets: default_percentile_buckets(),
            energy_min_stake: default_energy_min_stake(),
            energy_oracle_timeout_blocks: default_energy_oracle_timeout_blocks(),
            energy_slashing_rate_bps: default_energy_slashing_rate_bps(),
            jurisdiction_region: 0,
            ai_diagnostics_enabled: 0,
            kalman_r_short: 1,
            kalman_r_med: 3,
            kalman_r_long: 8,
            scheduler_weight_gossip: 3,
            scheduler_weight_compute: 2,
            scheduler_weight_storage: 1,
            runtime_backend_policy: default_runtime_backend_policy(),
            transport_provider_policy: default_transport_provider_policy(),
            storage_engine_policy: default_storage_engine_policy(),
            bridge_min_bond: BridgeIncentiveParameters::DEFAULT_MIN_BOND as i64,
            bridge_duty_reward: BridgeIncentiveParameters::DEFAULT_DUTY_REWARD as i64,
            bridge_failure_slash: BridgeIncentiveParameters::DEFAULT_FAILURE_SLASH as i64,
            bridge_challenge_slash: BridgeIncentiveParameters::DEFAULT_CHALLENGE_SLASH as i64,
            bridge_duty_window_secs: BridgeIncentiveParameters::DEFAULT_DUTY_WINDOW_SECS as i64,
            // Economic Control Laws
            inflation_target_bps: default_inflation_target_bps(),
            inflation_controller_gain: default_inflation_controller_gain(),
            min_annual_issuance_block: default_min_annual_issuance_block(),
            max_annual_issuance_block: default_max_annual_issuance_block(),
            storage_util_target_bps: default_storage_util_target_bps(),
            storage_margin_target_bps: default_storage_margin_target_bps(),
            compute_util_target_bps: default_compute_util_target_bps(),
            compute_margin_target_bps: default_compute_margin_target_bps(),
            energy_util_target_bps: default_energy_util_target_bps(),
            energy_margin_target_bps: default_energy_margin_target_bps(),
            ad_util_target_bps: default_ad_util_target_bps(),
            ad_margin_target_bps: default_ad_margin_target_bps(),
            subsidy_allocator_alpha: default_subsidy_allocator_alpha(),
            subsidy_allocator_beta: default_subsidy_allocator_beta(),
            subsidy_allocator_temperature: default_subsidy_allocator_temperature(),
            subsidy_allocator_drift_rate: default_subsidy_allocator_drift_rate(),
            storage_util_responsiveness: default_storage_util_responsiveness(),
            storage_cost_responsiveness: default_storage_cost_responsiveness(),
            storage_multiplier_floor: default_storage_multiplier_floor(),
            storage_multiplier_ceiling: default_storage_multiplier_ceiling(),
            compute_util_responsiveness: default_compute_util_responsiveness(),
            compute_cost_responsiveness: default_compute_cost_responsiveness(),
            compute_multiplier_floor: default_compute_multiplier_floor(),
            compute_multiplier_ceiling: default_compute_multiplier_ceiling(),
            energy_util_responsiveness: default_energy_util_responsiveness(),
            energy_cost_responsiveness: default_energy_cost_responsiveness(),
            energy_multiplier_floor: default_energy_multiplier_floor(),
            energy_multiplier_ceiling: default_energy_multiplier_ceiling(),
            ad_util_responsiveness: default_ad_util_responsiveness(),
            ad_cost_responsiveness: default_ad_cost_responsiveness(),
            ad_multiplier_floor: default_ad_multiplier_floor(),
            ad_multiplier_ceiling: default_ad_multiplier_ceiling(),
            ad_platform_take_target_bps: default_ad_platform_take_target_bps(),
            ad_user_share_target_bps: default_ad_user_share_target_bps(),
            ad_drift_rate: default_ad_drift_rate(),
            tariff_public_revenue_target_bps: default_tariff_public_revenue_target_bps(),
            tariff_drift_rate: default_tariff_drift_rate(),
            tariff_min_bps: default_tariff_min_bps(),
            tariff_max_bps: default_tariff_max_bps(),
        }
    }
}

impl Params {
    pub fn to_value(&self) -> sled::Result<foundation_serialization::json::Value> {
        use foundation_serialization::json::Value;
        let mut map = foundation_serialization::json::Map::new();
        map.insert(
            "snapshot_interval_secs".into(),
            Value::Number(self.snapshot_interval_secs.into()),
        );
        map.insert(
            "consumer_fee_comfort_p90_microunits".into(),
            Value::Number(self.consumer_fee_comfort_p90_microunits.into()),
        );
        map.insert(
            "fee_floor_window".into(),
            Value::Number(self.fee_floor_window.into()),
        );
        map.insert(
            "fee_floor_percentile".into(),
            Value::Number(self.fee_floor_percentile.into()),
        );
        map.insert(
            "industrial_admission_min_capacity".into(),
            Value::Number(self.industrial_admission_min_capacity.into()),
        );
        map.insert(
            "fairshare_global_max_ppm".into(),
            Value::Number(self.fairshare_global_max_ppm.into()),
        );
        map.insert(
            "burst_refill_rate_per_s_ppm".into(),
            Value::Number(self.burst_refill_rate_per_s_ppm.into()),
        );
        map.insert(
            "beta_storage_sub".into(),
            Value::Number(self.beta_storage_sub.into()),
        );
        map.insert(
            "gamma_read_sub".into(),
            Value::Number(self.gamma_read_sub.into()),
        );
        map.insert(
            "kappa_cpu_sub".into(),
            Value::Number(self.kappa_cpu_sub.into()),
        );
        map.insert(
            "lambda_bytes_out_sub".into(),
            Value::Number(self.lambda_bytes_out_sub.into()),
        );
        map.insert(
            "read_subsidy_viewer_percent".into(),
            Value::Number(self.read_subsidy_viewer_percent.into()),
        );
        map.insert(
            "read_subsidy_host_percent".into(),
            Value::Number(self.read_subsidy_host_percent.into()),
        );
        map.insert(
            "read_subsidy_hardware_percent".into(),
            Value::Number(self.read_subsidy_hardware_percent.into()),
        );
        map.insert(
            "read_subsidy_verifier_percent".into(),
            Value::Number(self.read_subsidy_verifier_percent.into()),
        );
        map.insert(
            "read_subsidy_liquidity_percent".into(),
            Value::Number(self.read_subsidy_liquidity_percent.into()),
        );
        map.insert(
            "dual_token_settlement_enabled".into(),
            Value::Number(self.dual_token_settlement_enabled.into()),
        );
        map.insert(
            "launch_operational_flag".into(),
            Value::Number(self.launch_operational_flag.into()),
        );
        map.insert(
            "dns_rehearsal_enabled".into(),
            Value::Number(self.dns_rehearsal_enabled.into()),
        );
        map.insert(
            "ad_readiness_window_secs".into(),
            Value::Number(self.ad_readiness_window_secs.into()),
        );
        map.insert(
            "ad_readiness_min_unique_viewers".into(),
            Value::Number(self.ad_readiness_min_unique_viewers.into()),
        );
        map.insert(
            "ad_readiness_min_host_count".into(),
            Value::Number(self.ad_readiness_min_host_count.into()),
        );
        map.insert(
            "ad_readiness_min_provider_count".into(),
            Value::Number(self.ad_readiness_min_provider_count.into()),
        );
        map.insert(
            "treasury_percent".into(),
            Value::Number(self.treasury_percent.into()),
        );
        map.insert(
            "proof_rebate_limit".into(),
            Value::Number(self.proof_rebate_limit.into()),
        );
        map.insert(
            "rent_rate_per_byte".into(),
            Value::Number(self.rent_rate_per_byte.into()),
        );
        map.insert(
            "kill_switch_subsidy_reduction".into(),
            Value::Number(self.kill_switch_subsidy_reduction.into()),
        );
        map.insert(
            "miner_reward_logistic_target".into(),
            Value::Number(self.miner_reward_logistic_target.into()),
        );
        map.insert(
            "logistic_slope_milli".into(),
            Value::Number(self.logistic_slope_milli.into()),
        );
        map.insert(
            "miner_hysteresis".into(),
            Value::Number(self.miner_hysteresis.into()),
        );
        map.insert("risk_lambda".into(), Value::Number(self.risk_lambda.into()));
        map.insert("entropy_phi".into(), Value::Number(self.entropy_phi.into()));
        map.insert("haar_eta".into(), Value::Number(self.haar_eta.into()));
        map.insert(
            "util_var_threshold".into(),
            Value::Number(self.util_var_threshold.into()),
        );
        map.insert(
            "fib_window_base_secs".into(),
            Value::Number(self.fib_window_base_secs.into()),
        );
        map.insert(
            "heuristic_mu_milli".into(),
            Value::Number(self.heuristic_mu_milli.into()),
        );
        map.insert(
            "industrial_multiplier".into(),
            Value::Number(self.industrial_multiplier.into()),
        );
        map.insert(
            "badge_expiry_secs".into(),
            Value::Number(self.badge_expiry_secs.into()),
        );
        map.insert(
            "badge_issue_uptime_percent".into(),
            Value::Number(self.badge_issue_uptime_percent.into()),
        );
        map.insert(
            "badge_revoke_uptime_percent".into(),
            Value::Number(self.badge_revoke_uptime_percent.into()),
        );
        map.insert(
            "ad_rehearsal_enabled".into(),
            Value::Number(self.ad_rehearsal_enabled.into()),
        );
        map.insert(
            "ad_rehearsal_stability_windows".into(),
            Value::Number(self.ad_rehearsal_stability_windows.into()),
        );
        map.insert(
            "presence_min_crowd_size".into(),
            Value::Number(self.presence_min_crowd_size.into()),
        );
        map.insert(
            "presence_ttl_secs".into(),
            Value::Number(self.presence_ttl_secs.into()),
        );
        map.insert(
            "presence_radius_meters".into(),
            Value::Number(self.presence_radius_meters.into()),
        );
        map.insert(
            "presence_proof_cache_size".into(),
            Value::Number(self.presence_proof_cache_size.into()),
        );
        map.insert(
            "presence_min_confidence_bps".into(),
            Value::Number(self.presence_min_confidence_bps.into()),
        );
        map.insert(
            "ad_use_percentile_thresholds".into(),
            Value::Number(self.ad_use_percentile_thresholds.into()),
        );
        map.insert(
            "ad_viewer_percentile".into(),
            Value::Number(self.ad_viewer_percentile.into()),
        );
        map.insert(
            "ad_host_percentile".into(),
            Value::Number(self.ad_host_percentile.into()),
        );
        map.insert(
            "ad_provider_percentile".into(),
            Value::Number(self.ad_provider_percentile.into()),
        );
        map.insert(
            "ad_ema_smoothing_ppm".into(),
            Value::Number(self.ad_ema_smoothing_ppm.into()),
        );
        map.insert(
            "ad_floor_unique_viewers".into(),
            Value::Number(self.ad_floor_unique_viewers.into()),
        );
        map.insert(
            "ad_floor_host_count".into(),
            Value::Number(self.ad_floor_host_count.into()),
        );
        map.insert(
            "ad_floor_provider_count".into(),
            Value::Number(self.ad_floor_provider_count.into()),
        );
        map.insert(
            "ad_cap_unique_viewers".into(),
            Value::Number(self.ad_cap_unique_viewers.into()),
        );
        map.insert(
            "ad_cap_host_count".into(),
            Value::Number(self.ad_cap_host_count.into()),
        );
        map.insert(
            "ad_cap_provider_count".into(),
            Value::Number(self.ad_cap_provider_count.into()),
        );
        map.insert(
            "ad_percentile_buckets".into(),
            Value::Number(self.ad_percentile_buckets.into()),
        );
        map.insert(
            "jurisdiction_region".into(),
            Value::Number(self.jurisdiction_region.into()),
        );
        map.insert(
            "ai_diagnostics_enabled".into(),
            Value::Number(self.ai_diagnostics_enabled.into()),
        );
        map.insert(
            "kalman_r_short".into(),
            Value::Number(self.kalman_r_short.into()),
        );
        map.insert(
            "kalman_r_med".into(),
            Value::Number(self.kalman_r_med.into()),
        );
        map.insert(
            "kalman_r_long".into(),
            Value::Number(self.kalman_r_long.into()),
        );
        map.insert(
            "scheduler_weight_gossip".into(),
            Value::Number(self.scheduler_weight_gossip.into()),
        );
        map.insert(
            "scheduler_weight_compute".into(),
            Value::Number(self.scheduler_weight_compute.into()),
        );
        map.insert(
            "scheduler_weight_storage".into(),
            Value::Number(self.scheduler_weight_storage.into()),
        );
        map.insert(
            "runtime_backend_policy".into(),
            Value::Number(self.runtime_backend_policy.into()),
        );
        map.insert(
            "transport_provider_policy".into(),
            Value::Number(self.transport_provider_policy.into()),
        );
        map.insert(
            "storage_engine_policy".into(),
            Value::Number(self.storage_engine_policy.into()),
        );
        map.insert(
            "bridge_min_bond".into(),
            Value::Number(self.bridge_min_bond.into()),
        );
        map.insert(
            "bridge_duty_reward".into(),
            Value::Number(self.bridge_duty_reward.into()),
        );
        map.insert(
            "bridge_failure_slash".into(),
            Value::Number(self.bridge_failure_slash.into()),
        );
        map.insert(
            "bridge_challenge_slash".into(),
            Value::Number(self.bridge_challenge_slash.into()),
        );
        map.insert(
            "bridge_duty_window_secs".into(),
            Value::Number(self.bridge_duty_window_secs.into()),
        );
        map.insert(
            "energy_min_stake".into(),
            Value::Number(self.energy_min_stake.into()),
        );
        map.insert(
            "energy_oracle_timeout_blocks".into(),
            Value::Number(self.energy_oracle_timeout_blocks.into()),
        );
        map.insert(
            "energy_slashing_rate_bps".into(),
            Value::Number(self.energy_slashing_rate_bps.into()),
        );
        Ok(Value::Object(map))
    }

    pub fn deserialize(value: &foundation_serialization::json::Value) -> sled::Result<Self> {
        use foundation_serialization::json::Value;
        let obj = value
            .as_object()
            .ok_or_else(|| sled::Error::Unsupported("params JSON: expected object".into()))?;
        let take_i64 = |key: &str| -> sled::Result<i64> {
            obj.get(key).and_then(Value::as_i64).ok_or_else(|| {
                sled::Error::Unsupported(format!("params JSON: missing {key}").into())
            })
        };
        let params = Self {
            snapshot_interval_secs: take_i64("snapshot_interval_secs")?,
            consumer_fee_comfort_p90_microunits: take_i64("consumer_fee_comfort_p90_microunits")?,
            fee_floor_window: take_i64("fee_floor_window")?,
            fee_floor_percentile: take_i64("fee_floor_percentile")?,
            industrial_admission_min_capacity: take_i64("industrial_admission_min_capacity")?,
            fairshare_global_max_ppm: take_i64("fairshare_global_max_ppm")?,
            burst_refill_rate_per_s_ppm: take_i64("burst_refill_rate_per_s_ppm")?,
            beta_storage_sub: take_i64("beta_storage_sub")?,
            gamma_read_sub: take_i64("gamma_read_sub")?,
            kappa_cpu_sub: take_i64("kappa_cpu_sub")?,
            lambda_bytes_out_sub: take_i64("lambda_bytes_out_sub")?,
            read_subsidy_viewer_percent: obj
                .get("read_subsidy_viewer_percent")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_read_subsidy_viewer_percent),
            read_subsidy_host_percent: obj
                .get("read_subsidy_host_percent")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_read_subsidy_host_percent),
            read_subsidy_hardware_percent: obj
                .get("read_subsidy_hardware_percent")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_read_subsidy_hardware_percent),
            read_subsidy_verifier_percent: obj
                .get("read_subsidy_verifier_percent")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_read_subsidy_verifier_percent),
            read_subsidy_liquidity_percent: obj
                .get("read_subsidy_liquidity_percent")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_read_subsidy_liquidity_percent),
            dual_token_settlement_enabled: obj
                .get("dual_token_settlement_enabled")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_dual_token_settlement_enabled),
            launch_operational_flag: obj
                .get("launch_operational_flag")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            dns_rehearsal_enabled: obj
                .get("dns_rehearsal_enabled")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_dns_rehearsal_enabled),
            launch_economics_autopilot: obj
                .get("launch_economics_autopilot")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_launch_economics_autopilot),
            ad_readiness_window_secs: obj
                .get("ad_readiness_window_secs")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_readiness_window_secs),
            ad_readiness_min_unique_viewers: obj
                .get("ad_readiness_min_unique_viewers")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_readiness_min_unique_viewers),
            ad_readiness_min_host_count: obj
                .get("ad_readiness_min_host_count")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_readiness_min_host_count),
            ad_readiness_min_provider_count: obj
                .get("ad_readiness_min_provider_count")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_readiness_min_provider_count),
            treasury_percent: take_i64("treasury_percent")?,
            proof_rebate_limit: take_i64("proof_rebate_limit")?,
            rent_rate_per_byte: take_i64("rent_rate_per_byte")?,
            kill_switch_subsidy_reduction: take_i64("kill_switch_subsidy_reduction")?,
            miner_reward_logistic_target: take_i64("miner_reward_logistic_target")?,
            logistic_slope_milli: take_i64("logistic_slope_milli")?,
            miner_hysteresis: take_i64("miner_hysteresis")?,
            risk_lambda: take_i64("risk_lambda")?,
            entropy_phi: take_i64("entropy_phi")?,
            haar_eta: take_i64("haar_eta")?,
            util_var_threshold: take_i64("util_var_threshold")?,
            fib_window_base_secs: take_i64("fib_window_base_secs")?,
            heuristic_mu_milli: take_i64("heuristic_mu_milli")?,
            industrial_multiplier: take_i64("industrial_multiplier")?,
            badge_expiry_secs: take_i64("badge_expiry_secs")?,
            badge_issue_uptime_percent: take_i64("badge_issue_uptime_percent")?,
            badge_revoke_uptime_percent: take_i64("badge_revoke_uptime_percent")?,
            ad_rehearsal_enabled: obj
                .get("ad_rehearsal_enabled")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            ad_rehearsal_stability_windows: obj
                .get("ad_rehearsal_stability_windows")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_rehearsal_stability_windows),
            presence_min_crowd_size: obj
                .get("presence_min_crowd_size")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_presence_min_crowd_size),
            presence_ttl_secs: obj
                .get("presence_ttl_secs")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_presence_ttl_secs),
            presence_radius_meters: obj
                .get("presence_radius_meters")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_presence_radius_meters),
            presence_proof_cache_size: obj
                .get("presence_proof_cache_size")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_presence_proof_cache_size),
            presence_min_confidence_bps: obj
                .get("presence_min_confidence_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_presence_min_confidence_bps),
            ad_use_percentile_thresholds: obj
                .get("ad_use_percentile_thresholds")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            ad_viewer_percentile: obj
                .get("ad_viewer_percentile")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_viewer_percentile),
            ad_host_percentile: obj
                .get("ad_host_percentile")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_host_percentile),
            ad_provider_percentile: obj
                .get("ad_provider_percentile")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_provider_percentile),
            ad_ema_smoothing_ppm: obj
                .get("ad_ema_smoothing_ppm")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ema_smoothing_ppm),
            ad_floor_unique_viewers: obj
                .get("ad_floor_unique_viewers")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            ad_floor_host_count: obj
                .get("ad_floor_host_count")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            ad_floor_provider_count: obj
                .get("ad_floor_provider_count")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            ad_cap_unique_viewers: obj
                .get("ad_cap_unique_viewers")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            ad_cap_host_count: obj
                .get("ad_cap_host_count")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            ad_cap_provider_count: obj
                .get("ad_cap_provider_count")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            ad_percentile_buckets: obj
                .get("ad_percentile_buckets")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_percentile_buckets),
            energy_min_stake: take_i64("energy_min_stake")?,
            energy_oracle_timeout_blocks: take_i64("energy_oracle_timeout_blocks")?,
            energy_slashing_rate_bps: take_i64("energy_slashing_rate_bps")?,
            jurisdiction_region: take_i64("jurisdiction_region")?,
            ai_diagnostics_enabled: take_i64("ai_diagnostics_enabled")?,
            kalman_r_short: take_i64("kalman_r_short")?,
            kalman_r_med: take_i64("kalman_r_med")?,
            kalman_r_long: take_i64("kalman_r_long")?,
            scheduler_weight_gossip: take_i64("scheduler_weight_gossip")?,
            scheduler_weight_compute: take_i64("scheduler_weight_compute")?,
            scheduler_weight_storage: take_i64("scheduler_weight_storage")?,
            runtime_backend_policy: take_i64("runtime_backend_policy")?,
            transport_provider_policy: take_i64("transport_provider_policy")?,
            storage_engine_policy: take_i64("storage_engine_policy")?,
            bridge_min_bond: take_i64("bridge_min_bond")?,
            bridge_duty_reward: take_i64("bridge_duty_reward")?,
            bridge_failure_slash: take_i64("bridge_failure_slash")?,
            bridge_challenge_slash: take_i64("bridge_challenge_slash")?,
            bridge_duty_window_secs: take_i64("bridge_duty_window_secs")?,
            // Economic Control Laws - use defaults if not present for backward compatibility
            inflation_target_bps: obj
                .get("inflation_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_inflation_target_bps),
            inflation_controller_gain: obj
                .get("inflation_controller_gain")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_inflation_controller_gain),
            min_annual_issuance_block: obj
                .get("min_annual_issuance_block")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_min_annual_issuance_block),
            max_annual_issuance_block: obj
                .get("max_annual_issuance_block")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_max_annual_issuance_block),
            storage_util_target_bps: obj
                .get("storage_util_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_storage_util_target_bps),
            storage_margin_target_bps: obj
                .get("storage_margin_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_storage_margin_target_bps),
            compute_util_target_bps: obj
                .get("compute_util_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_compute_util_target_bps),
            compute_margin_target_bps: obj
                .get("compute_margin_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_compute_margin_target_bps),
            energy_util_target_bps: obj
                .get("energy_util_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_energy_util_target_bps),
            energy_margin_target_bps: obj
                .get("energy_margin_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_energy_margin_target_bps),
            ad_util_target_bps: obj
                .get("ad_util_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_util_target_bps),
            ad_margin_target_bps: obj
                .get("ad_margin_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_margin_target_bps),
            subsidy_allocator_alpha: obj
                .get("subsidy_allocator_alpha")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_subsidy_allocator_alpha),
            subsidy_allocator_beta: obj
                .get("subsidy_allocator_beta")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_subsidy_allocator_beta),
            subsidy_allocator_temperature: obj
                .get("subsidy_allocator_temperature")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_subsidy_allocator_temperature),
            subsidy_allocator_drift_rate: obj
                .get("subsidy_allocator_drift_rate")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_subsidy_allocator_drift_rate),
            storage_util_responsiveness: obj
                .get("storage_util_responsiveness")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_storage_util_responsiveness),
            storage_cost_responsiveness: obj
                .get("storage_cost_responsiveness")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_storage_cost_responsiveness),
            storage_multiplier_floor: obj
                .get("storage_multiplier_floor")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_storage_multiplier_floor),
            storage_multiplier_ceiling: obj
                .get("storage_multiplier_ceiling")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_storage_multiplier_ceiling),
            compute_util_responsiveness: obj
                .get("compute_util_responsiveness")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_compute_util_responsiveness),
            compute_cost_responsiveness: obj
                .get("compute_cost_responsiveness")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_compute_cost_responsiveness),
            compute_multiplier_floor: obj
                .get("compute_multiplier_floor")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_compute_multiplier_floor),
            compute_multiplier_ceiling: obj
                .get("compute_multiplier_ceiling")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_compute_multiplier_ceiling),
            energy_util_responsiveness: obj
                .get("energy_util_responsiveness")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_energy_util_responsiveness),
            energy_cost_responsiveness: obj
                .get("energy_cost_responsiveness")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_energy_cost_responsiveness),
            energy_multiplier_floor: obj
                .get("energy_multiplier_floor")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_energy_multiplier_floor),
            energy_multiplier_ceiling: obj
                .get("energy_multiplier_ceiling")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_energy_multiplier_ceiling),
            ad_util_responsiveness: obj
                .get("ad_util_responsiveness")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_util_responsiveness),
            ad_cost_responsiveness: obj
                .get("ad_cost_responsiveness")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_cost_responsiveness),
            ad_multiplier_floor: obj
                .get("ad_multiplier_floor")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_multiplier_floor),
            ad_multiplier_ceiling: obj
                .get("ad_multiplier_ceiling")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_multiplier_ceiling),
            ad_platform_take_target_bps: obj
                .get("ad_platform_take_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_platform_take_target_bps),
            ad_user_share_target_bps: obj
                .get("ad_user_share_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_user_share_target_bps),
            ad_drift_rate: obj
                .get("ad_drift_rate")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_ad_drift_rate),
            tariff_public_revenue_target_bps: obj
                .get("tariff_public_revenue_target_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_tariff_public_revenue_target_bps),
            tariff_drift_rate: obj
                .get("tariff_drift_rate")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_tariff_drift_rate),
            tariff_min_bps: obj
                .get("tariff_min_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_tariff_min_bps),
            tariff_max_bps: obj
                .get("tariff_max_bps")
                .and_then(Value::as_i64)
                .unwrap_or_else(default_tariff_max_bps),
        };
        Ok(params)
    }
}

const fn default_proof_rebate_limit() -> i64 {
    1
}

const fn default_read_subsidy_viewer_percent() -> i64 {
    40
}

const fn default_read_subsidy_host_percent() -> i64 {
    30
}

const fn default_read_subsidy_hardware_percent() -> i64 {
    15
}

const fn default_read_subsidy_verifier_percent() -> i64 {
    10
}

const fn default_read_subsidy_liquidity_percent() -> i64 {
    5
}

const fn default_dual_token_settlement_enabled() -> i64 {
    0
}

const fn default_dns_rehearsal_enabled() -> i64 {
    1
}

const fn default_launch_economics_autopilot() -> i64 {
    0
}

const fn default_ad_readiness_window_secs() -> i64 {
    6 * 60 * 60
}

const fn default_ad_readiness_min_unique_viewers() -> i64 {
    250
}

const fn default_ad_readiness_min_host_count() -> i64 {
    25
}

const fn default_ad_readiness_min_provider_count() -> i64 {
    10
}

const fn default_ad_rehearsal_stability_windows() -> i64 {
    6
}

const fn default_presence_min_crowd_size() -> i64 {
    5
}

const fn default_presence_ttl_secs() -> i64 {
    86400 // 24 hours
}

const fn default_presence_radius_meters() -> i64 {
    500 // 500m default aggregation radius
}

const fn default_presence_proof_cache_size() -> i64 {
    10000 // Max cached PresenceReceipt entries per node
}

const fn default_presence_min_confidence_bps() -> i64 {
    8000 // 80% minimum confidence for presence targeting
}

const fn default_viewer_percentile() -> i64 {
    90
}
const fn default_host_percentile() -> i64 {
    75
}
const fn default_provider_percentile() -> i64 {
    50
}
const fn default_ema_smoothing_ppm() -> i64 {
    200_000
}
const fn default_percentile_buckets() -> i64 {
    12
}

const fn default_energy_min_stake() -> i64 {
    1_000
}

const fn default_energy_oracle_timeout_blocks() -> i64 {
    720
}

const fn default_energy_slashing_rate_bps() -> i64 {
    0
}

const fn default_runtime_backend_policy() -> i64 {
    DEFAULT_RUNTIME_BACKEND_POLICY
}

const fn default_transport_provider_policy() -> i64 {
    DEFAULT_TRANSPORT_PROVIDER_POLICY
}

const fn default_storage_engine_policy() -> i64 {
    DEFAULT_STORAGE_ENGINE_POLICY
}

// ===== Economic Control Law Defaults =====
const fn default_inflation_target_bps() -> i64 {
    500 // 5%
}
const fn default_inflation_controller_gain() -> i64 {
    100 // 0.10 in millis
}
const fn default_min_annual_issuance_block() -> i64 {
    50_000_000
}
const fn default_max_annual_issuance_block() -> i64 {
    300_000_000
}
const fn default_storage_util_target_bps() -> i64 {
    4000 // 40%
}
const fn default_storage_margin_target_bps() -> i64 {
    5000 // 50%
}
const fn default_compute_util_target_bps() -> i64 {
    6000 // 60%
}
const fn default_compute_margin_target_bps() -> i64 {
    5000 // 50%
}
const fn default_energy_util_target_bps() -> i64 {
    5000 // 50%
}
const fn default_energy_margin_target_bps() -> i64 {
    2500 // 25%
}
const fn default_ad_util_target_bps() -> i64 {
    5000 // 50%
}
const fn default_ad_margin_target_bps() -> i64 {
    3000 // 30%
}
const fn default_subsidy_allocator_alpha() -> i64 {
    600 // 0.60 in millis
}
const fn default_subsidy_allocator_beta() -> i64 {
    400 // 0.40 in millis
}
const fn default_subsidy_allocator_temperature() -> i64 {
    1000 // 1.0 in millis
}
const fn default_subsidy_allocator_drift_rate() -> i64 {
    50 // 0.05 in millis
}
const fn default_storage_util_responsiveness() -> i64 {
    200 // 0.20 in millis
}
const fn default_storage_cost_responsiveness() -> i64 {
    150 // 0.15 in millis
}
const fn default_storage_multiplier_floor() -> i64 {
    200 // 0.20 in millis
}
const fn default_storage_multiplier_ceiling() -> i64 {
    5000 // 5.0 in millis
}
const fn default_compute_util_responsiveness() -> i64 {
    300 // 0.30 in millis
}
const fn default_compute_cost_responsiveness() -> i64 {
    200 // 0.20 in millis
}
const fn default_compute_multiplier_floor() -> i64 {
    200 // 0.20 in millis
}
const fn default_compute_multiplier_ceiling() -> i64 {
    8000 // 8.0 in millis
}
const fn default_energy_util_responsiveness() -> i64 {
    250 // 0.25 in millis
}
const fn default_energy_cost_responsiveness() -> i64 {
    300 // 0.30 in millis
}
const fn default_energy_multiplier_floor() -> i64 {
    100 // 0.10 in millis
}
const fn default_energy_multiplier_ceiling() -> i64 {
    10000 // 10.0 in millis
}
const fn default_ad_util_responsiveness() -> i64 {
    150 // 0.15 in millis
}
const fn default_ad_cost_responsiveness() -> i64 {
    100 // 0.10 in millis
}
const fn default_ad_multiplier_floor() -> i64 {
    500 // 0.50 in millis
}
const fn default_ad_multiplier_ceiling() -> i64 {
    3000 // 3.0 in millis
}
const fn default_ad_platform_take_target_bps() -> i64 {
    2800 // 28%
}
const fn default_ad_user_share_target_bps() -> i64 {
    2200 // 22%
}
const fn default_ad_drift_rate() -> i64 {
    10 // 0.01 in millis
}
const fn default_tariff_public_revenue_target_bps() -> i64 {
    1000 // 10%
}
const fn default_tariff_drift_rate() -> i64 {
    50 // 0.05 in millis
}
const fn default_tariff_min_bps() -> i64 {
    0 // 0%
}
const fn default_tariff_max_bps() -> i64 {
    200 // 2%
}

fn apply_snapshot_interval(v: i64, p: &mut Params) -> Result<(), ()> {
    p.snapshot_interval_secs = v;
    Ok(())
}
fn apply_consumer_fee_p90(v: i64, p: &mut Params) -> Result<(), ()> {
    p.consumer_fee_comfort_p90_microunits = v;
    Ok(())
}
fn apply_fee_floor_window(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 1 {
        return Err(());
    }
    p.fee_floor_window = v;
    Ok(())
}
fn apply_fee_floor_percentile(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 || v > 100 {
        return Err(());
    }
    p.fee_floor_percentile = v;
    Ok(())
}
fn apply_industrial_capacity(v: i64, p: &mut Params) -> Result<(), ()> {
    p.industrial_admission_min_capacity = v;
    Ok(())
}

fn apply_badge_expiry(v: i64, p: &mut Params) -> Result<(), ()> {
    p.badge_expiry_secs = v;
    Ok(())
}
fn apply_badge_issue_uptime(v: i64, p: &mut Params) -> Result<(), ()> {
    p.badge_issue_uptime_percent = v;
    Ok(())
}
fn apply_badge_revoke_uptime(v: i64, p: &mut Params) -> Result<(), ()> {
    p.badge_revoke_uptime_percent = v;
    Ok(())
}
fn apply_jurisdiction_region(v: i64, p: &mut Params) -> Result<(), ()> {
    p.jurisdiction_region = v;
    Ok(())
}
fn apply_ai_diagnostics_enabled(v: i64, p: &mut Params) -> Result<(), ()> {
    p.ai_diagnostics_enabled = v;
    Ok(())
}

fn apply_energy_min_stake(v: i64, p: &mut Params) -> Result<(), ()> {
    p.energy_min_stake = v;
    Ok(())
}

fn apply_energy_oracle_timeout_blocks(v: i64, p: &mut Params) -> Result<(), ()> {
    p.energy_oracle_timeout_blocks = v;
    Ok(())
}

fn apply_energy_slashing_rate_bps(v: i64, p: &mut Params) -> Result<(), ()> {
    p.energy_slashing_rate_bps = v;
    Ok(())
}

fn apply_kalman_r_short(v: i64, p: &mut Params) -> Result<(), ()> {
    p.kalman_r_short = v;
    Ok(())
}

fn apply_kalman_r_med(v: i64, p: &mut Params) -> Result<(), ()> {
    p.kalman_r_med = v;
    Ok(())
}

fn apply_kalman_r_long(v: i64, p: &mut Params) -> Result<(), ()> {
    p.kalman_r_long = v;
    Ok(())
}

fn apply_scheduler_weight_gossip(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.scheduler_weight_gossip = v;
    Ok(())
}

fn apply_scheduler_weight_compute(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.scheduler_weight_compute = v;
    Ok(())
}

fn apply_scheduler_weight_storage(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.scheduler_weight_storage = v;
    Ok(())
}
fn apply_fairshare_global_max(v: i64, p: &mut Params) -> Result<(), ()> {
    p.fairshare_global_max_ppm = v;
    Ok(())
}
fn apply_burst_refill_rate(v: i64, p: &mut Params) -> Result<(), ()> {
    p.burst_refill_rate_per_s_ppm = v;
    Ok(())
}

fn apply_beta_storage_sub(v: i64, p: &mut Params) -> Result<(), ()> {
    p.beta_storage_sub = v;
    Ok(())
}

fn apply_gamma_read_sub(v: i64, p: &mut Params) -> Result<(), ()> {
    p.gamma_read_sub = v;
    Ok(())
}

fn apply_read_subsidy_viewer_percent(v: i64, p: &mut Params) -> Result<(), ()> {
    p.read_subsidy_viewer_percent = v;
    Ok(())
}

fn apply_read_subsidy_host_percent(v: i64, p: &mut Params) -> Result<(), ()> {
    p.read_subsidy_host_percent = v;
    Ok(())
}

fn apply_read_subsidy_hardware_percent(v: i64, p: &mut Params) -> Result<(), ()> {
    p.read_subsidy_hardware_percent = v;
    Ok(())
}

fn apply_read_subsidy_verifier_percent(v: i64, p: &mut Params) -> Result<(), ()> {
    p.read_subsidy_verifier_percent = v;
    Ok(())
}

fn apply_read_subsidy_liquidity_percent(v: i64, p: &mut Params) -> Result<(), ()> {
    p.read_subsidy_liquidity_percent = v;
    Ok(())
}

fn apply_dual_token_settlement_enabled(v: i64, p: &mut Params) -> Result<(), ()> {
    p.dual_token_settlement_enabled = if v > 0 { 1 } else { 0 };
    Ok(())
}

fn apply_ad_readiness_window_secs(v: i64, p: &mut Params) -> Result<(), ()> {
    if v <= 0 {
        return Err(());
    }
    p.ad_readiness_window_secs = v;
    Ok(())
}

fn apply_ad_readiness_min_unique_viewers(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.ad_readiness_min_unique_viewers = v;
    Ok(())
}

fn apply_ad_readiness_min_host_count(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.ad_readiness_min_host_count = v;
    Ok(())
}

fn apply_ad_readiness_min_provider_count(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.ad_readiness_min_provider_count = v;
    Ok(())
}

fn apply_ad_rehearsal_enabled(v: i64, p: &mut Params) -> Result<(), ()> {
    p.ad_rehearsal_enabled = if v > 0 { 1 } else { 0 };
    Ok(())
}

fn apply_ad_rehearsal_stability_windows(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.ad_rehearsal_stability_windows = v;
    Ok(())
}

fn apply_kappa_cpu_sub(v: i64, p: &mut Params) -> Result<(), ()> {
    p.kappa_cpu_sub = v;
    Ok(())
}

fn apply_lambda_bytes_out_sub(v: i64, p: &mut Params) -> Result<(), ()> {
    p.lambda_bytes_out_sub = v;
    Ok(())
}

fn apply_treasury_percent(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 || v > 100 {
        return Err(());
    }
    p.treasury_percent = v;
    Ok(())
}

fn apply_proof_rebate_limit(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.proof_rebate_limit = v;
    Ok(())
}

fn apply_rent_rate(v: i64, p: &mut Params) -> Result<(), ()> {
    p.rent_rate_per_byte = v;
    Ok(())
}

fn apply_kill_switch(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 || v > 100 {
        return Err(());
    }
    p.kill_switch_subsidy_reduction = v;
    Ok(())
}

fn apply_miner_target(v: i64, p: &mut Params) -> Result<(), ()> {
    if v <= 0 {
        return Err(());
    }
    p.miner_reward_logistic_target = v;
    Ok(())
}

fn apply_logistic_slope(v: i64, p: &mut Params) -> Result<(), ()> {
    if v <= 0 {
        return Err(());
    }
    p.logistic_slope_milli = v;
    Ok(())
}

fn apply_miner_hysteresis(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.miner_hysteresis = v;
    Ok(())
}

fn apply_heuristic_mu(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.heuristic_mu_milli = v;
    #[cfg(feature = "telemetry")]
    crate::telemetry::HEURISTIC_MU_MILLI.set(v);
    Ok(())
}

fn apply_runtime_backend_policy(v: i64, p: &mut Params) -> Result<(), ()> {
    if !validate_runtime_backend_policy(v) {
        return Err(());
    }
    p.runtime_backend_policy = v;
    Ok(())
}

fn apply_transport_provider_policy(v: i64, p: &mut Params) -> Result<(), ()> {
    if !validate_transport_provider_policy(v) {
        return Err(());
    }
    p.transport_provider_policy = v;
    Ok(())
}

fn apply_storage_engine_policy(v: i64, p: &mut Params) -> Result<(), ()> {
    if !validate_storage_engine_policy(v) {
        return Err(());
    }
    p.storage_engine_policy = v;
    Ok(())
}

fn apply_bridge_min_bond(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.bridge_min_bond = v;
    Ok(())
}

fn apply_bridge_duty_reward(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.bridge_duty_reward = v;
    Ok(())
}

fn apply_bridge_failure_slash(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.bridge_failure_slash = v;
    Ok(())
}

fn apply_bridge_challenge_slash(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.bridge_challenge_slash = v;
    Ok(())
}

fn apply_bridge_duty_window(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 1 {
        return Err(());
    }
    p.bridge_duty_window_secs = v;
    Ok(())
}

fn apply_ad_use_percentile_thresholds(v: i64, p: &mut Params) -> Result<(), ()> {
    p.ad_use_percentile_thresholds = if v > 0 { 1 } else { 0 };
    Ok(())
}
fn apply_ad_viewer_percentile(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 || v > 100 {
        return Err(());
    }
    p.ad_viewer_percentile = v;
    Ok(())
}
fn apply_ad_host_percentile(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 || v > 100 {
        return Err(());
    }
    p.ad_host_percentile = v;
    Ok(())
}
fn apply_ad_provider_percentile(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 || v > 100 {
        return Err(());
    }
    p.ad_provider_percentile = v;
    Ok(())
}
fn apply_ad_ema_smoothing_ppm(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 || v > 1_000_000 {
        return Err(());
    }
    p.ad_ema_smoothing_ppm = v;
    Ok(())
}
fn apply_ad_floor_unique_viewers(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.ad_floor_unique_viewers = v;
    Ok(())
}
fn apply_ad_floor_host_count(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.ad_floor_host_count = v;
    Ok(())
}
fn apply_ad_floor_provider_count(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.ad_floor_provider_count = v;
    Ok(())
}
fn apply_ad_cap_unique_viewers(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.ad_cap_unique_viewers = v;
    Ok(())
}
fn apply_ad_cap_host_count(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.ad_cap_host_count = v;
    Ok(())
}
fn apply_ad_cap_provider_count(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.ad_cap_provider_count = v;
    Ok(())
}
fn apply_ad_percentile_buckets(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 4 || v > 360 {
        return Err(());
    }
    p.ad_percentile_buckets = v;
    Ok(())
}

fn push_bridge_incentives(
    rt: &mut Runtime,
    min_bond: Option<u64>,
    duty_reward: Option<u64>,
    failure_slash: Option<u64>,
    challenge_slash: Option<u64>,
    duty_window_secs: Option<u64>,
) -> Result<(), ()> {
    let defaults = BridgeIncentiveParameters::default();
    let snapshot = rt.params_snapshot();
    let bridge_min = min_bond.unwrap_or_else(|| {
        snapshot
            .map(|p| p.bridge_min_bond as u64)
            .unwrap_or(defaults.min_bond)
    });
    let bridge_reward = duty_reward.unwrap_or_else(|| {
        snapshot
            .map(|p| p.bridge_duty_reward as u64)
            .unwrap_or(defaults.duty_reward)
    });
    let bridge_failure = failure_slash.unwrap_or_else(|| {
        snapshot
            .map(|p| p.bridge_failure_slash as u64)
            .unwrap_or(defaults.failure_slash)
    });
    let bridge_challenge = challenge_slash.unwrap_or_else(|| {
        snapshot
            .map(|p| p.bridge_challenge_slash as u64)
            .unwrap_or(defaults.challenge_slash)
    });
    let bridge_window = duty_window_secs.unwrap_or_else(|| {
        snapshot
            .map(|p| p.bridge_duty_window_secs as u64)
            .unwrap_or(defaults.duty_window_secs)
    });
    rt.set_bridge_incentives(
        bridge_min,
        bridge_reward,
        bridge_failure,
        bridge_challenge,
        bridge_window,
    );
    Ok(())
}

pub fn registry() -> &'static [ParamSpec] {
    static REGS: [ParamSpec; 65] = [
        ParamSpec {
            key: ParamKey::SnapshotIntervalSecs,
            default: 30,
            min: 5,
            max: 600,
            unit: "seconds",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_snapshot_interval,
            apply_runtime: |v, rt| {
                rt.set_snapshot_interval(Duration::from_secs(v as u64));
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::ConsumerFeeComfortP90Microunits,
            default: 2_500,
            min: 500,
            max: 25_000,
            unit: "microunits",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_consumer_fee_p90,
            apply_runtime: |v, rt| {
                rt.set_consumer_p90_comfort(v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::FeeFloorWindow,
            default: 256,
            min: 1,
            max: 4_096,
            unit: "samples",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_fee_floor_window,
            apply_runtime: |v, rt| {
                let percentile = rt.bc.params.fee_floor_percentile as u64;
                rt.set_fee_floor_policy(v as u64, percentile);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::FeeFloorPercentile,
            default: 75,
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_fee_floor_percentile,
            apply_runtime: |v, rt| {
                let window = rt.bc.params.fee_floor_window as u64;
                rt.set_fee_floor_policy(window, v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::IndustrialAdmissionMinCapacity,
            default: 10,
            min: 1,
            max: 10_000,
            unit: "microshards/sec",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_industrial_capacity,
            apply_runtime: |v, rt| {
                rt.set_min_capacity(v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::FairshareGlobalMax,
            default: 250_000,
            min: 10_000,
            max: 1_000_000,
            unit: "ppm",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_fairshare_global_max,
            apply_runtime: |v, rt| {
                rt.set_fair_share_cap(v as f64 / 1_000_000.0);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::BurstRefillRatePerS,
            default: ((30.0 / 60.0) * 1_000_000.0) as i64,
            min: 0,
            max: 1_000_000,
            unit: "ppm",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_burst_refill_rate,
            apply_runtime: |v, rt| {
                rt.set_burst_refill_rate(v as f64 / 1_000_000.0);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::BetaStorageSub,
            default: 50,
            min: 0,
            max: 1_000_000,
            unit: "nCT per byte",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_beta_storage_sub,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::GammaReadSub,
            default: 20,
            min: 0,
            max: 1_000_000,
            unit: "nCT per byte",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_gamma_read_sub,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::ReadSubsidyViewerPercent,
            default: default_read_subsidy_viewer_percent(),
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_read_subsidy_viewer_percent,
            apply_runtime: |v, rt| {
                rt.bc.params.read_subsidy_viewer_percent = v;
                rt.sync_read_subsidy_distribution();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::ReadSubsidyHostPercent,
            default: default_read_subsidy_host_percent(),
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_read_subsidy_host_percent,
            apply_runtime: |v, rt| {
                rt.bc.params.read_subsidy_host_percent = v;
                rt.sync_read_subsidy_distribution();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::ReadSubsidyHardwarePercent,
            default: default_read_subsidy_hardware_percent(),
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_read_subsidy_hardware_percent,
            apply_runtime: |v, rt| {
                rt.bc.params.read_subsidy_hardware_percent = v;
                rt.sync_read_subsidy_distribution();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::ReadSubsidyVerifierPercent,
            default: default_read_subsidy_verifier_percent(),
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_read_subsidy_verifier_percent,
            apply_runtime: |v, rt| {
                rt.bc.params.read_subsidy_verifier_percent = v;
                rt.sync_read_subsidy_distribution();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::ReadSubsidyLiquidityPercent,
            default: default_read_subsidy_liquidity_percent(),
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_read_subsidy_liquidity_percent,
            apply_runtime: |v, rt| {
                rt.bc.params.read_subsidy_liquidity_percent = v;
                rt.sync_read_subsidy_distribution();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::DualTokenSettlementEnabled,
            default: default_dual_token_settlement_enabled(),
            min: 0,
            max: 1,
            unit: "bool",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_dual_token_settlement_enabled,
            apply_runtime: |v, rt| {
                rt.set_dual_token_settlement_enabled(v != 0);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdReadinessWindowSecs,
            default: default_ad_readiness_window_secs(),
            min: 60,
            max: (24 * 60 * 60) as i64,
            unit: "seconds",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_readiness_window_secs,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_readiness_window_secs = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdReadinessMinUniqueViewers,
            default: default_ad_readiness_min_unique_viewers(),
            min: 0,
            max: 1_000_000,
            unit: "unique viewers",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_readiness_min_unique_viewers,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_readiness_min_unique_viewers = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdReadinessMinHostCount,
            default: default_ad_readiness_min_host_count(),
            min: 0,
            max: 1_000_000,
            unit: "hosts",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_readiness_min_host_count,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_readiness_min_host_count = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdReadinessMinProviderCount,
            default: default_ad_readiness_min_provider_count(),
            min: 0,
            max: 1_000_000,
            unit: "providers",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_readiness_min_provider_count,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_readiness_min_provider_count = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdRehearsalEnabled,
            default: 0,
            min: 0,
            max: 1,
            unit: "bool",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_rehearsal_enabled,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_rehearsal_enabled = v;
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdRehearsalStabilityWindows,
            default: default_ad_rehearsal_stability_windows(),
            min: 0,
            max: 10_000,
            unit: "windows",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_rehearsal_stability_windows,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_rehearsal_stability_windows = v;
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::KappaCpuSub,
            default: 10,
            min: 0,
            max: 1_000_000,
            unit: "nCT per ms",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_kappa_cpu_sub,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::LambdaBytesOutSub,
            default: 5,
            min: 0,
            max: 1_000_000,
            unit: "nCT per byte",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_lambda_bytes_out_sub,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::TreasuryPercent,
            default: 0,
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_treasury_percent,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::ProofRebateLimit,
            default: default_proof_rebate_limit(),
            min: 0,
            max: 1_000_000,
            unit: "nCT per proof",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_proof_rebate_limit,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::RentRatePerByte,
            default: 0,
            min: 0,
            max: 1_000_000,
            unit: "nCT per byte",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_rent_rate,
            apply_runtime: |v, rt| {
                rt.set_rent_rate(v);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::KillSwitchSubsidyReduction,
            default: 0,
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: KILL_SWITCH_TIMELOCK_EPOCHS,
            apply: apply_kill_switch,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::MinerRewardLogisticTarget,
            default: 100,
            min: 1,
            max: 1_000_000,
            unit: "miners",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_miner_target,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::LogisticSlope,
            default: 460,
            min: 1,
            max: 1_000_000,
            unit: "slope_milli",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_logistic_slope,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::MinerHysteresis,
            default: 10,
            min: 0,
            max: 1_000_000,
            unit: "miners",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_miner_hysteresis,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::HeuristicMuMilli,
            default: 500,
            min: 0,
            max: 10_000,
            unit: "milli",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_heuristic_mu,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::BadgeExpirySecs,
            default: 30 * 24 * 60 * 60,
            min: 3_600,
            max: 365 * 24 * 60 * 60,
            unit: "seconds",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_badge_expiry,
            apply_runtime: |v, rt| {
                rt.set_badge_expiry(v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::BadgeIssueUptime,
            default: 99,
            min: 50,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_badge_issue_uptime,
            apply_runtime: |v, rt| {
                rt.set_badge_issue_uptime(v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::BadgeRevokeUptime,
            default: 95,
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_badge_revoke_uptime,
            apply_runtime: |v, rt| {
                rt.set_badge_revoke_uptime(v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::JurisdictionRegion,
            default: 0,
            min: 0,
            max: 10,
            unit: "code",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_jurisdiction_region,
            apply_runtime: |v, rt| {
                rt.set_jurisdiction_region(v);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AiDiagnosticsEnabled,
            default: 0,
            min: 0,
            max: 1,
            unit: "bool",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ai_diagnostics_enabled,
            apply_runtime: |v, rt| {
                rt.set_ai_diagnostics_enabled(v != 0);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::KalmanRShort,
            default: 1,
            min: 1,
            max: 1_000,
            unit: "weight",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_kalman_r_short,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::KalmanRMed,
            default: 3,
            min: 1,
            max: 1_000,
            unit: "weight",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_kalman_r_med,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::KalmanRLong,
            default: 8,
            min: 1,
            max: 1_000,
            unit: "weight",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_kalman_r_long,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::SchedulerWeightGossip,
            default: 3,
            min: 0,
            max: 16,
            unit: "tickets",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_scheduler_weight_gossip,
            apply_runtime: |v, rt| {
                rt.set_scheduler_weight(ServiceClass::Gossip, v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::SchedulerWeightCompute,
            default: 2,
            min: 0,
            max: 16,
            unit: "tickets",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_scheduler_weight_compute,
            apply_runtime: |v, rt| {
                rt.set_scheduler_weight(ServiceClass::Compute, v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::SchedulerWeightStorage,
            default: 1,
            min: 0,
            max: 16,
            unit: "tickets",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_scheduler_weight_storage,
            apply_runtime: |v, rt| {
                rt.set_scheduler_weight(ServiceClass::Storage, v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::RuntimeBackend,
            default: default_runtime_backend_policy(),
            min: 1,
            max: RUNTIME_BACKEND_MASK_ALL,
            unit: "bitmask",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_runtime_backend_policy,
            apply_runtime: |v, rt| {
                let allowed = decode_runtime_backend_policy(v);
                rt.set_runtime_backend_policy(&allowed);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::TransportProvider,
            default: default_transport_provider_policy(),
            min: 1,
            max: TRANSPORT_PROVIDER_MASK_ALL,
            unit: "bitmask",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_transport_provider_policy,
            apply_runtime: |v, rt| {
                let allowed = decode_transport_provider_policy(v);
                rt.set_transport_provider_policy(&allowed);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::StorageEnginePolicy,
            default: default_storage_engine_policy(),
            min: 1,
            max: STORAGE_ENGINE_MASK_ALL,
            unit: "bitmask",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_storage_engine_policy,
            apply_runtime: |v, rt| {
                let allowed = decode_storage_engine_policy(v);
                rt.set_storage_engine_policy(&allowed);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::BridgeMinBond,
            default: BridgeIncentiveParameters::DEFAULT_MIN_BOND as i64,
            min: 0,
            max: 1_000_000,
            unit: "tokens",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_bridge_min_bond,
            apply_runtime: |v, rt| {
                push_bridge_incentives(rt, Some(v as u64), None, None, None, None)
            },
        },
        ParamSpec {
            key: ParamKey::BridgeDutyReward,
            default: BridgeIncentiveParameters::DEFAULT_DUTY_REWARD as i64,
            min: 0,
            max: 1_000_000,
            unit: "tokens",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_bridge_duty_reward,
            apply_runtime: |v, rt| {
                push_bridge_incentives(rt, None, Some(v as u64), None, None, None)
            },
        },
        ParamSpec {
            key: ParamKey::BridgeFailureSlash,
            default: BridgeIncentiveParameters::DEFAULT_FAILURE_SLASH as i64,
            min: 0,
            max: 1_000_000,
            unit: "tokens",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_bridge_failure_slash,
            apply_runtime: |v, rt| {
                push_bridge_incentives(rt, None, None, Some(v as u64), None, None)
            },
        },
        ParamSpec {
            key: ParamKey::BridgeChallengeSlash,
            default: BridgeIncentiveParameters::DEFAULT_CHALLENGE_SLASH as i64,
            min: 0,
            max: 1_000_000,
            unit: "tokens",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_bridge_challenge_slash,
            apply_runtime: |v, rt| {
                push_bridge_incentives(rt, None, None, None, Some(v as u64), None)
            },
        },
        ParamSpec {
            key: ParamKey::BridgeDutyWindowSecs,
            default: BridgeIncentiveParameters::DEFAULT_DUTY_WINDOW_SECS as i64,
            min: 1,
            max: 86_400,
            unit: "seconds",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_bridge_duty_window,
            apply_runtime: |v, rt| {
                push_bridge_incentives(rt, None, None, None, None, Some(v as u64))
            },
        },
        // --- Dynamic readiness controls ---
        ParamSpec {
            key: ParamKey::AdUsePercentileThresholds,
            default: 0,
            min: 0,
            max: 1,
            unit: "bool",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_use_percentile_thresholds,
            apply_runtime: |v, rt| {
                // Migration: if toggling on, preserve current minima as floors
                let prev_on = rt
                    .params_snapshot()
                    .map(|p| p.ad_use_percentile_thresholds > 0)
                    .unwrap_or(false);
                let turning_on = v > 0 && !prev_on;
                if turning_on {
                    if let Some(handle) = &rt.ad_readiness {
                        let snap = handle.snapshot();
                        rt.bc.params.ad_floor_unique_viewers = snap.min_unique_viewers as i64;
                        rt.bc.params.ad_floor_host_count = snap.min_host_count as i64;
                        rt.bc.params.ad_floor_provider_count = snap.min_provider_count as i64;
                    }
                }
                rt.bc.params.ad_use_percentile_thresholds = if v > 0 { 1 } else { 0 };
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdViewerPercentile,
            default: default_viewer_percentile(),
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_viewer_percentile,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_viewer_percentile = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdHostPercentile,
            default: default_host_percentile(),
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_host_percentile,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_host_percentile = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdProviderPercentile,
            default: default_provider_percentile(),
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_provider_percentile,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_provider_percentile = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdEmaSmoothingPpm,
            default: default_ema_smoothing_ppm(),
            min: 0,
            max: 1_000_000,
            unit: "ppm",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_ema_smoothing_ppm,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_ema_smoothing_ppm = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdFloorUniqueViewers,
            default: 0,
            min: 0,
            max: 10_000_000,
            unit: "count",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_floor_unique_viewers,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_floor_unique_viewers = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdFloorHostCount,
            default: 0,
            min: 0,
            max: 10_000_000,
            unit: "count",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_floor_host_count,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_floor_host_count = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdFloorProviderCount,
            default: 0,
            min: 0,
            max: 10_000_000,
            unit: "count",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_floor_provider_count,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_floor_provider_count = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdCapUniqueViewers,
            default: 0,
            min: 0,
            max: 10_000_000,
            unit: "count",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_cap_unique_viewers,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_cap_unique_viewers = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdCapHostCount,
            default: 0,
            min: 0,
            max: 10_000_000,
            unit: "count",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_cap_host_count,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_cap_host_count = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdCapProviderCount,
            default: 0,
            min: 0,
            max: 10_000_000,
            unit: "count",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_cap_provider_count,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_cap_provider_count = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AdPercentileBuckets,
            default: default_percentile_buckets(),
            min: 4,
            max: 360,
            unit: "buckets",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ad_percentile_buckets,
            apply_runtime: |v, rt| {
                rt.bc.params.ad_percentile_buckets = v;
                rt.sync_ad_readiness();
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::EnergyMinStake,
            default: default_energy_min_stake(),
            min: 0,
            max: 1_000_000,
            unit: "ct",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_energy_min_stake,
            apply_runtime: |v, rt| {
                rt.set_energy_min_stake(v.max(0) as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::EnergyOracleTimeoutBlocks,
            default: default_energy_oracle_timeout_blocks(),
            min: 1,
            max: 10_000,
            unit: "blocks",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_energy_oracle_timeout_blocks,
            apply_runtime: |v, rt| {
                rt.set_energy_oracle_timeout_blocks(v.max(1) as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::EnergySlashingRateBps,
            default: default_energy_slashing_rate_bps(),
            min: 0,
            max: 10_000,
            unit: "bps",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_energy_slashing_rate_bps,
            apply_runtime: |v, rt| {
                rt.set_energy_slashing_rate_bps(v.max(0) as u64);
                Ok(())
            },
        },
    ];
    &REGS
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Utilization {
    pub bytes_stored: f64,
    pub bytes_read: f64,
    pub cpu_ms: f64,
    pub bytes_out: f64,
    pub epoch_secs: f64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct EncryptedUtilization(pub Vec<u8>);

#[allow(dead_code)]
impl EncryptedUtilization {
    pub fn decrypt(&self, key: &[u8]) -> Utilization {
        let mut buf = self.0.clone();
        for (b, k) in buf.iter_mut().zip(key.iter().cycle()) {
            *b ^= k;
        }
        binary::decode(&buf).unwrap_or_default()
    }
}

pub fn retune_multipliers(
    params: &mut Params,
    supply: f64,
    stats: &Utilization,
    current_epoch: u64,
    base_path: &Path,
    rolling_inflation: f64,
    rng_seed: Option<u64>,
) -> [i64; 4] {
    #[derive(Serialize, Deserialize)]
    struct KalmanState {
        x: [f64; 8],
        p: Vec<f64>,
    }
    #[derive(Serialize, Deserialize, Default)]
    struct UtilHistory {
        bytes_stored: Vec<f64>,
        bytes_read: Vec<f64>,
        cpu_ms: Vec<f64>,
        bytes_out: Vec<f64>,
    }

    let target = 0.02_f64;
    let hist_dir = base_path.join("governance/history");
    let _ = fs::create_dir_all(&hist_dir);
    let state_path = hist_dir.join("kalman_state.json");

    // Load previous Kalman filter state or initialise from current params.
    let mut state: KalmanState = if let Ok(bytes) = fs::read(&state_path) {
        json::from_slice(&bytes).unwrap_or(KalmanState {
            x: [
                params.beta_storage_sub as f64,
                params.gamma_read_sub as f64,
                params.kappa_cpu_sub as f64,
                params.lambda_bytes_out_sub as f64,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
            p: vec![0.0; 64],
        })
    } else {
        KalmanState {
            x: [
                params.beta_storage_sub as f64,
                params.gamma_read_sub as f64,
                params.kappa_cpu_sub as f64,
                params.lambda_bytes_out_sub as f64,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
            p: vec![0.0; 64],
        }
    };

    // Load utilization history and append current stats.
    const MAX_HIST: usize = 256;
    let hist_path = hist_dir.join("util_history.json");
    let mut hist: UtilHistory = if let Ok(bytes) = fs::read(&hist_path) {
        json::from_slice(&bytes).unwrap_or_default()
    } else {
        UtilHistory::default()
    };
    hist.bytes_stored.push(stats.bytes_stored);
    hist.bytes_read.push(stats.bytes_read);
    hist.cpu_ms.push(stats.cpu_ms);
    hist.bytes_out.push(stats.bytes_out);
    if hist.bytes_stored.len() > MAX_HIST {
        hist.bytes_stored.remove(0);
    }
    if hist.bytes_read.len() > MAX_HIST {
        hist.bytes_read.remove(0);
    }
    if hist.cpu_ms.len() > MAX_HIST {
        hist.cpu_ms.remove(0);
    }
    if hist.bytes_out.len() > MAX_HIST {
        hist.bytes_out.remove(0);
    }

    fn smooth(data: &[f64], current: f64, base: usize, eta: f64) -> f64 {
        const PHI: f64 = 1.618_033_988_749_894_8;
        let len = data.len();
        if len == 0 {
            return current;
        }
        let mut d = base.max(1);
        loop {
            let start = len.saturating_sub(d);
            let slice = &data[start..];
            // Hampel filter: compute median and MAD, reject outliers beyond 3*MAD
            let mut sorted = slice.to_vec();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let median = sorted[sorted.len() / 2];
            let mut devs: Vec<f64> = slice.iter().map(|v| (v - median).abs()).collect();
            devs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mad = devs[devs.len() / 2].max(1e-9);
            let thresh = 3.0 * mad;
            let filtered: Vec<f64> = slice
                .iter()
                .cloned()
                .filter(|v| (v - median).abs() <= thresh)
                .collect();
            let mean = filtered.iter().sum::<f64>() / filtered.len() as f64;
            let var = if filtered.len() > 1 {
                filtered.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / filtered.len() as f64
            } else {
                0.0
            };
            if var <= eta * eta * current * current || d >= len {
                return mean;
            }
            d = (PHI * d as f64).ceil() as usize;
            if d > len {
                d = len;
            }
        }
    }

    let base_epochs = (params.fib_window_base_secs as f64 / stats.epoch_secs).ceil() as usize;
    let eta = params.util_var_threshold as f64 / 1000.0;
    let smoothed = [
        smooth(&hist.bytes_stored, stats.bytes_stored, base_epochs, eta),
        smooth(&hist.bytes_read, stats.bytes_read, base_epochs, eta),
        smooth(&hist.cpu_ms, stats.cpu_ms, base_epochs, eta),
        smooth(&hist.bytes_out, stats.bytes_out, base_epochs, eta),
    ];

    let eta = params.haar_eta as f64 / 1000.0;
    let burst = crate::governance::variance::haar_burst_veto(&hist.bytes_stored, eta);

    let measurements = [
        if smoothed[0] <= 0.0 {
            state.x[0] * 2.0
        } else {
            (0.004 * target * supply / 365.0) / (smoothed[0] / stats.epoch_secs)
        },
        if smoothed[1] <= 0.0 {
            state.x[1] * 2.0
        } else {
            (0.0025 * target * supply / 365.0) / (smoothed[1] / stats.epoch_secs)
        },
        if smoothed[2] <= 0.0 {
            state.x[2] * 2.0
        } else {
            (0.0025 * target * supply / 365.0) / (smoothed[2] / stats.epoch_secs)
        },
        if smoothed[3] <= 0.0 {
            state.x[3] * 2.0
        } else {
            (0.0025 * target * supply / 365.0) / (smoothed[3] / stats.epoch_secs)
        },
    ];

    use crate::governance::kalman::KalmanLqg;
    let mut kf = KalmanLqg {
        x: Vector::<8>::from_array(state.x),
        p: Matrix::<8, 8>::from_row_major(&state.p),
    };
    if !burst {
        kf.step(
            &measurements,
            stats.epoch_secs,
            params.risk_lambda as f64 / 1000.0,
        );
    }
    state.x.copy_from_slice(kf.x.as_slice());
    state.p.copy_from_slice(kf.p.as_slice());
    let theta = kf.theta();
    let raw = [
        theta[0].round() as i64,
        theta[1].round() as i64,
        theta[2].round() as i64,
        theta[3].round() as i64,
    ];
    use rand::{rngs::StdRng, Rng};
    let b = supply * (1.0 / (1u64 << 20) as f64);
    let mut rng: StdRng = match rng_seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_rng(rand::thread_rng()).expect("rng seed"),
    };
    let noisy: [i64; 4] = raw.map(|v| {
        let u: f64 = rng.r#gen::<f64>() - 0.5;
        let noise = if u >= 0.0 {
            -b * (1.0_f64 - 2.0_f64 * u).ln()
        } else {
            b * (1.0_f64 + 2.0_f64 * u).ln()
        };
        (v as f64 + noise).round() as i64
    });
    params.beta_storage_sub = noisy[0];
    params.gamma_read_sub = noisy[1];
    params.kappa_cpu_sub = noisy[2];
    params.lambda_bytes_out_sub = noisy[3];

    let _ = json::to_vec(&state).map(|bytes| fs::write(&state_path, bytes));
    let _ = json::to_vec(&hist).map(|bytes| fs::write(&hist_path, bytes));
    let events_path = hist_dir.join("events.log");
    if rolling_inflation > 0.02 {
        #[cfg(feature = "telemetry")]
        {
            info!(
                "inflation_guard triggered (rolling_inflation={})",
                rolling_inflation
            );
            crate::telemetry::SUBSIDY_AUTO_REDUCED_TOTAL.inc();
        }
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)
        {
            let _ = writeln!(
                f,
                "{} inflation_guard {:.6}",
                current_epoch, rolling_inflation
            );
        }
        params.beta_storage_sub = (params.beta_storage_sub as f64 * 0.95).round() as i64;
        params.gamma_read_sub = (params.gamma_read_sub as f64 * 0.95).round() as i64;
        params.kappa_cpu_sub = (params.kappa_cpu_sub as f64 * 0.95).round() as i64;
        params.lambda_bytes_out_sub = (params.lambda_bytes_out_sub as f64 * 0.95).round() as i64;
    }
    if params.kill_switch_subsidy_reduction > 0 {
        #[cfg(feature = "telemetry")]
        {
            info!(
                "kill_switch_active reduction={}",
                params.kill_switch_subsidy_reduction
            );
            crate::telemetry::KILL_SWITCH_TRIGGER_TOTAL.inc();
        }
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)
        {
            let _ = writeln!(
                f,
                "{} kill_switch {}",
                current_epoch, params.kill_switch_subsidy_reduction
            );
        }
        let factor = 1.0 - (params.kill_switch_subsidy_reduction as f64 / 100.0);
        params.beta_storage_sub = (params.beta_storage_sub as f64 * factor).round() as i64;
        params.gamma_read_sub = (params.gamma_read_sub as f64 * factor).round() as i64;
        params.kappa_cpu_sub = (params.kappa_cpu_sub as f64 * factor).round() as i64;
        params.lambda_bytes_out_sub = (params.lambda_bytes_out_sub as f64 * factor).round() as i64;
    }
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&events_path)
    {
        let _ = writeln!(
            f,
            "{} retune {} {} {} {}",
            current_epoch,
            params.beta_storage_sub,
            params.gamma_read_sub,
            params.kappa_cpu_sub,
            params.lambda_bytes_out_sub,
        );
    }
    let snap_path = hist_dir.join(format!("inflation_{}.json", current_epoch));
    if let Ok(bytes) = json::to_vec(params) {
        let _ = fs::write(snap_path, bytes);
    }

    #[cfg(feature = "telemetry")]
    {
        use crate::telemetry::{SUBSIDY_MULTIPLIER, SUBSIDY_MULTIPLIER_RAW};
        SUBSIDY_MULTIPLIER
            .ensure_handle_for_label_values(&["storage"])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .set(params.beta_storage_sub);
        SUBSIDY_MULTIPLIER
            .ensure_handle_for_label_values(&["read"])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .set(params.gamma_read_sub);
        SUBSIDY_MULTIPLIER
            .ensure_handle_for_label_values(&["cpu"])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .set(params.kappa_cpu_sub);
        SUBSIDY_MULTIPLIER
            .ensure_handle_for_label_values(&["bytes_out"])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .set(params.lambda_bytes_out_sub);
        SUBSIDY_MULTIPLIER_RAW
            .ensure_handle_for_label_values(&["storage"])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .set(raw[0]);
        SUBSIDY_MULTIPLIER_RAW
            .ensure_handle_for_label_values(&["read"])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .set(raw[1]);
        SUBSIDY_MULTIPLIER_RAW
            .ensure_handle_for_label_values(&["cpu"])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .set(raw[2]);
        SUBSIDY_MULTIPLIER_RAW
            .ensure_handle_for_label_values(&["bytes_out"])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .set(raw[3]);
    }
    raw
}

#[allow(dead_code)]
pub fn retune_multipliers_encrypted(
    params: &mut Params,
    supply: f64,
    enc: &EncryptedUtilization,
    key: &[u8],
    current_epoch: u64,
    base_path: &Path,
    rolling_inflation: f64,
    rng_seed: Option<u64>,
) -> [i64; 4] {
    let stats = enc.decrypt(key);
    retune_multipliers(
        params,
        supply,
        &stats,
        current_epoch,
        base_path,
        rolling_inflation,
        rng_seed,
    )
}
