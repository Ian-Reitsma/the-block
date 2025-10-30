use crate::budget::{
    BidShadingApplication as BudgetBidShadingApplication,
    BidShadingGuidance as BudgetBidShadingGuidance,
};
use crypto_suite::hashing::blake3;
use foundation_metrics::{gauge, histogram, increment_counter};
use foundation_serialization::{
    self,
    json::{self, Map as JsonMap, Number as JsonNumber, Value as JsonValue},
    Deserialize, Serialize,
};
use sled::{Config as SledConfig, Db as SledDb, Tree as SledTree};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, RwLock};
use zkp::selection::{self, SelectionProofPublicInputs, SelectionProofVerification};

mod attestation;
mod badge;
mod budget;
mod privacy;
mod uplift;

pub use attestation::{
    AttestationSatisfaction, SelectionAttestationConfig, SelectionAttestationManager,
    VerifierCommitteeConfig,
};
pub use badge::{ann, BadgeDecision, BadgeGuard, BadgeGuardConfig, BadgeSoftIntentContext};
pub use budget::{
    BidShadingApplication, BidShadingGuidance, BudgetBroker, BudgetBrokerAnalytics,
    BudgetBrokerConfig, BudgetBrokerPacingDelta, BudgetBrokerSnapshot, CampaignBudgetSnapshot,
    CohortBudgetSnapshot, CohortKeySnapshot,
};
pub use privacy::{
    PrivacyBudgetConfig, PrivacyBudgetDecision, PrivacyBudgetManager, PrivacyBudgetSnapshot,
};
pub use uplift::{
    UpliftEstimate, UpliftEstimator, UpliftEstimatorConfig, UpliftHoldoutAssignment, UpliftSnapshot,
};
#[cfg(test)]
mod test_support;

const TREE_CAMPAIGNS: &str = "campaigns";
const TREE_METADATA: &str = "metadata";
const KEY_DISTRIBUTION: &[u8] = b"distribution";
const KEY_BUDGET: &[u8] = b"budget";
const KEY_TOKEN_REMAINDERS: &[u8] = b"token_remainders";

pub const MICROS_PER_DOLLAR: u64 = 1_000_000;
const PPM_SCALE: u64 = 1_000_000;
const BYTES_PER_MIB: u64 = 1_048_576;

#[derive(Debug)]
pub enum PersistenceError {
    Storage(sled::Error),
    Serialization(foundation_serialization::Error),
    Invalid(String),
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PersistenceError::Storage(err) => write!(f, "storage error: {err}"),
            PersistenceError::Serialization(err) => {
                write!(f, "serialization error: {err}")
            }
            PersistenceError::Invalid(msg) => write!(f, "invalid data: {msg}"),
        }
    }
}

impl std::error::Error for PersistenceError {}

impl From<sled::Error> for PersistenceError {
    fn from(err: sled::Error) -> Self {
        PersistenceError::Storage(err)
    }
}

impl From<foundation_serialization::Error> for PersistenceError {
    fn from(err: foundation_serialization::Error) -> Self {
        PersistenceError::Serialization(err)
    }
}

fn invalid<T: Into<String>>(msg: T) -> PersistenceError {
    PersistenceError::Invalid(msg.into())
}

fn read_string(map: &JsonMap, key: &str) -> Result<String, PersistenceError> {
    match map.get(key) {
        Some(JsonValue::String(value)) => Ok(value.clone()),
        Some(_) => Err(invalid(format!("{key} must be a string"))),
        None => Err(invalid(format!("missing {key}"))),
    }
}

fn read_u64(map: &JsonMap, key: &str) -> Result<u64, PersistenceError> {
    match map.get(key) {
        Some(JsonValue::Number(num)) => num
            .as_u64()
            .ok_or_else(|| invalid(format!("{key} must be an unsigned integer"))),
        Some(_) => Err(invalid(format!("{key} must be an unsigned integer"))),
        None => Err(invalid(format!("missing {key}"))),
    }
}

fn read_u32(map: &JsonMap, key: &str) -> Result<u32, PersistenceError> {
    match map.get(key) {
        Some(JsonValue::Number(num)) => num
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| invalid(format!("{key} must be an unsigned 32-bit integer"))),
        Some(_) => Err(invalid(format!("{key} must be an unsigned integer"))),
        None => Err(invalid(format!("missing {key}"))),
    }
}

fn read_f64(map: &JsonMap, key: &str) -> Result<f64, PersistenceError> {
    match map.get(key) {
        Some(JsonValue::Number(num)) => Ok(num.as_f64()),
        Some(_) => Err(invalid(format!("{key} must be a floating point number"))),
        None => Err(invalid(format!("missing {key}"))),
    }
}

fn number_from_f64(value: f64) -> JsonNumber {
    JsonNumber::from_f64(value).unwrap_or_else(|| JsonNumber::from(0))
}

fn read_string_vec(map: &JsonMap, key: &str) -> Result<Vec<String>, PersistenceError> {
    match map.get(key) {
        None => Ok(Vec::new()),
        Some(JsonValue::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| invalid(format!("{key} entries must be strings")))
            })
            .collect(),
        Some(_) => Err(invalid(format!("{key} must be an array"))),
    }
}

fn read_string_map(map: &JsonMap, key: &str) -> Result<HashMap<String, String>, PersistenceError> {
    match map.get(key) {
        None => Ok(HashMap::new()),
        Some(JsonValue::Object(entries)) => entries
            .iter()
            .map(|(k, v)| {
                v.as_str()
                    .map(|s| (k.clone(), s.to_string()))
                    .ok_or_else(|| invalid(format!("{key} values must be strings")))
            })
            .collect(),
        Some(_) => Err(invalid(format!("{key} must be an object"))),
    }
}

fn default_liquidity_split_ppm() -> u32 {
    (PPM_SCALE / 2) as u32
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DistributionPolicy {
    pub viewer_percent: u64,
    pub host_percent: u64,
    pub hardware_percent: u64,
    pub verifier_percent: u64,
    pub liquidity_percent: u64,
    #[serde(default = "default_liquidity_split_ppm")]
    pub liquidity_split_ct_ppm: u32,
    #[serde(default)]
    pub dual_token_settlement_enabled: bool,
}

impl DistributionPolicy {
    pub fn new(viewer: u64, host: u64, hardware: u64, verifier: u64, liquidity: u64) -> Self {
        Self {
            viewer_percent: viewer,
            host_percent: host,
            hardware_percent: hardware,
            verifier_percent: verifier,
            liquidity_percent: liquidity,
            liquidity_split_ct_ppm: default_liquidity_split_ppm(),
            dual_token_settlement_enabled: false,
        }
    }

    pub fn with_liquidity_split(mut self, split_ct_ppm: u32) -> Self {
        self.liquidity_split_ct_ppm = split_ct_ppm.min(PPM_SCALE as u32);
        self
    }

    pub fn with_dual_token_settlement(mut self, enabled: bool) -> Self {
        self.dual_token_settlement_enabled = enabled;
        self
    }

    pub fn normalize(self) -> Self {
        let mut policy = self;
        policy.liquidity_split_ct_ppm = policy.liquidity_split_ct_ppm.min(PPM_SCALE as u32);
        policy
    }
}

impl Default for DistributionPolicy {
    fn default() -> Self {
        Self {
            viewer_percent: 40,
            host_percent: 30,
            hardware_percent: 15,
            verifier_percent: 10,
            liquidity_percent: 5,
            liquidity_split_ct_ppm: default_liquidity_split_ppm(),
            dual_token_settlement_enabled: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenOracle {
    pub ct_price_usd_micros: u64,
    pub it_price_usd_micros: u64,
    #[serde(default)]
    pub ct_twap_window_id: u64,
    #[serde(default)]
    pub it_twap_window_id: u64,
}

impl TokenOracle {
    pub fn new(ct_price_usd_micros: u64, it_price_usd_micros: u64) -> Self {
        Self {
            ct_price_usd_micros: ct_price_usd_micros.max(1),
            it_price_usd_micros: it_price_usd_micros.max(1),
            ct_twap_window_id: 0,
            it_twap_window_id: 0,
        }
    }

    pub fn with_twap_windows(mut self, ct_window: u64, it_window: u64) -> Self {
        self.ct_twap_window_id = ct_window;
        self.it_twap_window_id = it_window;
        self
    }
}

impl Default for TokenOracle {
    fn default() -> Self {
        Self {
            ct_price_usd_micros: MICROS_PER_DOLLAR,
            it_price_usd_micros: MICROS_PER_DOLLAR,
            ct_twap_window_id: 0,
            it_twap_window_id: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceFloorConfig {
    pub verifier_cost_usd_micros: u64,
    pub expected_impressions_per_proof: u32,
    pub min_impressions_per_proof: u32,
    pub committee_size: u32,
    pub host_fee_usd_micros: u64,
}

impl ResourceFloorConfig {
    fn normalized(mut self) -> Self {
        self.expected_impressions_per_proof = self.expected_impressions_per_proof.max(1);
        self.min_impressions_per_proof = self.min_impressions_per_proof.max(1);
        self.committee_size = self.committee_size.max(1);
        self
    }

    fn qualified_impressions(&self, population_hint: Option<u64>) -> u64 {
        let hint = population_hint
            .filter(|value| *value > 0)
            .unwrap_or(self.expected_impressions_per_proof as u64);
        let committee = self.committee_size.max(1) as u64;
        let per_proof = (hint + committee - 1) / committee;
        per_proof.max(self.min_impressions_per_proof as u64).max(1)
    }

    fn breakdown(
        &self,
        price_per_mib_usd_micros: u64,
        bytes: u64,
        _cohort: &CohortKey,
        population_hint: Option<u64>,
    ) -> ResourceFloorBreakdown {
        let bandwidth = usd_cost_for_bytes(price_per_mib_usd_micros, bytes);
        let qualified_impressions = self.qualified_impressions(population_hint);
        let verifier = if qualified_impressions == 0 {
            self.verifier_cost_usd_micros
        } else {
            self.verifier_cost_usd_micros
                .saturating_add(qualified_impressions - 1)
                / qualified_impressions
        };
        ResourceFloorBreakdown {
            bandwidth_usd_micros: bandwidth,
            verifier_usd_micros: verifier,
            host_usd_micros: self.host_fee_usd_micros,
            qualified_impressions_per_proof: qualified_impressions,
        }
    }
}

impl Default for ResourceFloorConfig {
    fn default() -> Self {
        Self {
            verifier_cost_usd_micros: 50_000,
            expected_impressions_per_proof: 1_000,
            min_impressions_per_proof: 50,
            committee_size: 8,
            host_fee_usd_micros: 10_000,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResourceFloorBreakdown {
    pub bandwidth_usd_micros: u64,
    pub verifier_usd_micros: u64,
    pub host_usd_micros: u64,
    pub qualified_impressions_per_proof: u64,
}

impl ResourceFloorBreakdown {
    pub fn total_usd_micros(&self) -> u64 {
        self.bandwidth_usd_micros
            .saturating_add(self.verifier_usd_micros)
            .saturating_add(self.host_usd_micros)
    }
}

#[derive(Clone, Debug)]
pub struct MarketplaceConfig {
    pub distribution: DistributionPolicy,
    pub default_price_per_mib_usd_micros: u64,
    pub target_utilization_ppm: u32,
    pub smoothing_ppm: u32,
    pub price_eta_p_ppm: i32,
    pub price_eta_i_ppm: i32,
    pub price_forgetting_ppm: u32,
    pub min_price_per_mib_usd_micros: u64,
    pub max_price_per_mib_usd_micros: u64,
    pub default_oracle: TokenOracle,
    pub resource_floor: ResourceFloorConfig,
    pub quality_alpha: f32,
    pub quality_beta: f32,
    pub quality_floor_ppm: u32,
    pub attestation: SelectionAttestationConfig,
    pub budget_broker: BudgetBrokerConfig,
    pub badge_guard: BadgeGuardConfig,
    pub privacy_budget: PrivacyBudgetConfig,
    pub uplift: UpliftEstimatorConfig,
}

impl MarketplaceConfig {
    pub fn normalized(self) -> Self {
        let mut normalized = self;
        normalized.default_price_per_mib_usd_micros =
            normalized.default_price_per_mib_usd_micros.clamp(
                normalized.min_price_per_mib_usd_micros,
                normalized.max_price_per_mib_usd_micros,
            );
        let eta_p_max = (PPM_SCALE as f64 * 0.25).round() as i32;
        normalized.price_eta_p_ppm = normalized.price_eta_p_ppm.clamp(-eta_p_max, eta_p_max);
        let abs_eta_p = normalized.price_eta_p_ppm.abs() as f64;
        let max_eta_i = (abs_eta_p * 0.05).round() as i32;
        normalized.price_eta_i_ppm = normalized.price_eta_i_ppm.clamp(-(max_eta_i), max_eta_i);
        normalized.price_forgetting_ppm =
            normalized.price_forgetting_ppm.clamp(0, PPM_SCALE as u32);
        normalized.quality_beta = normalized
            .quality_beta
            .max(normalized.quality_alpha + f32::EPSILON);
        normalized.quality_floor_ppm = normalized.quality_floor_ppm.clamp(1, PPM_SCALE as u32);
        normalized.resource_floor = normalized.resource_floor.normalized();
        normalized.attestation = normalized.attestation.normalized();
        normalized.budget_broker = normalized.budget_broker.normalized();
        normalized.badge_guard = normalized.badge_guard.normalized();
        normalized.privacy_budget = normalized.privacy_budget.normalized();
        normalized.uplift = normalized.uplift.normalized();
        normalized
    }

    fn composite_floor_breakdown(
        &self,
        price_per_mib_usd_micros: u64,
        bytes: u64,
        cohort: &CohortKey,
        population_hint: Option<u64>,
    ) -> ResourceFloorBreakdown {
        self.resource_floor
            .breakdown(price_per_mib_usd_micros, bytes, cohort, population_hint)
    }

    fn quality_multiplier(&self, response_ppm: u32, lift_ppm: u32) -> f64 {
        let floor = (self.quality_floor_ppm as f64 / PPM_SCALE as f64).max(f64::EPSILON);
        let response = (response_ppm as f64 / PPM_SCALE as f64).max(floor);
        let lift = (lift_ppm as f64 / PPM_SCALE as f64)
            .max(response)
            .max(floor);
        let phi = response.powf(self.quality_alpha.into());
        let psi = lift.powf(self.quality_beta.into());
        (phi * psi).max(1.0)
    }
}

impl Default for MarketplaceConfig {
    fn default() -> Self {
        Self {
            distribution: DistributionPolicy::default(),
            default_price_per_mib_usd_micros: MICROS_PER_DOLLAR,
            target_utilization_ppm: 900_000,
            smoothing_ppm: 200_000,
            price_eta_p_ppm: 150_000,
            price_eta_i_ppm: 5_000,
            price_forgetting_ppm: 950_000,
            min_price_per_mib_usd_micros: 10_000,
            max_price_per_mib_usd_micros: 1_000 * MICROS_PER_DOLLAR,
            default_oracle: TokenOracle::default(),
            resource_floor: ResourceFloorConfig::default(),
            quality_alpha: 0.5,
            quality_beta: 0.8,
            quality_floor_ppm: 10_000,
            attestation: SelectionAttestationConfig::default(),
            budget_broker: BudgetBrokerConfig::default(),
            badge_guard: BadgeGuardConfig::default(),
            privacy_budget: PrivacyBudgetConfig::default(),
            uplift: UpliftEstimatorConfig::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CohortKey {
    domain: String,
    provider: Option<String>,
    badges: Vec<String>,
}

impl CohortKey {
    fn new(domain: String, provider: Option<String>, mut badges: Vec<String>) -> Self {
        badges.sort();
        badges.dedup();
        Self {
            domain,
            provider,
            badges,
        }
    }
}

impl Hash for CohortKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.domain.hash(state);
        self.provider.hash(state);
        self.badges.hash(state);
    }
}

#[derive(Clone, Debug)]
struct CohortTelemetryId {
    cohort_hash: String,
    domain: String,
    provider: Option<String>,
    badge_hash: String,
}

impl CohortTelemetryId {
    fn from_key(key: &CohortKey) -> Self {
        let mut cohort_hasher = blake3::Hasher::new();
        cohort_hasher.update(key.domain.as_bytes());
        if let Some(provider) = &key.provider {
            cohort_hasher.update(provider.as_bytes());
        }
        for badge in &key.badges {
            cohort_hasher.update(badge.as_bytes());
        }
        let mut badge_hasher = blake3::Hasher::new();
        for badge in &key.badges {
            badge_hasher.update(badge.as_bytes());
        }
        Self {
            cohort_hash: cohort_hasher.finalize().to_hex().to_hex_string(),
            domain: key.domain.clone(),
            provider: key.provider.clone(),
            badge_hash: badge_hasher.finalize().to_hex().to_hex_string(),
        }
    }

    fn provider_label(&self) -> &str {
        self.provider.as_deref().unwrap_or("-")
    }
}

#[derive(Clone, Debug)]
struct CohortPricingState {
    price_per_mib_usd_micros: u64,
    target_utilization_ppm: u32,
    smoothing_ppm: u32,
    log_price_per_mib: f64,
    eta_p: f64,
    eta_i: f64,
    forgetting: f64,
    integral_error: f64,
    ema_supply_usd_micros: f64,
    ema_demand_usd_micros: f64,
    min_price_per_mib_usd_micros: u64,
    max_price_per_mib_usd_micros: u64,
    observed_utilization_ppm: u32,
    telemetry: CohortTelemetryId,
}

impl CohortPricingState {
    fn new(config: &MarketplaceConfig, telemetry: CohortTelemetryId) -> Self {
        Self {
            price_per_mib_usd_micros: config.default_price_per_mib_usd_micros,
            target_utilization_ppm: config.target_utilization_ppm,
            smoothing_ppm: config.smoothing_ppm,
            log_price_per_mib: (config.default_price_per_mib_usd_micros.max(1) as f64).ln(),
            eta_p: config.price_eta_p_ppm as f64 / PPM_SCALE as f64,
            eta_i: config.price_eta_i_ppm as f64 / PPM_SCALE as f64,
            forgetting: config.price_forgetting_ppm as f64 / PPM_SCALE as f64,
            integral_error: 0.0,
            ema_supply_usd_micros: 0.0,
            ema_demand_usd_micros: 0.0,
            min_price_per_mib_usd_micros: config.min_price_per_mib_usd_micros,
            max_price_per_mib_usd_micros: config.max_price_per_mib_usd_micros,
            observed_utilization_ppm: 0,
            telemetry,
        }
    }

    fn price_per_mib_usd_micros(&self) -> u64 {
        self.price_per_mib_usd_micros
    }

    fn observed_utilization_ppm(&self) -> u32 {
        self.observed_utilization_ppm
    }

    fn record(&mut self, demand_usd_micros: u64, supply_usd_micros: u64) {
        if supply_usd_micros == 0 {
            return;
        }
        let smoothing = (self.smoothing_ppm as f64 / PPM_SCALE as f64).clamp(0.0, 1.0);
        if self.ema_supply_usd_micros == 0.0 {
            self.ema_supply_usd_micros = supply_usd_micros as f64;
            self.ema_demand_usd_micros = demand_usd_micros as f64;
        } else {
            self.ema_supply_usd_micros = (1.0 - smoothing) * self.ema_supply_usd_micros
                + smoothing * supply_usd_micros as f64;
            self.ema_demand_usd_micros = (1.0 - smoothing) * self.ema_demand_usd_micros
                + smoothing * demand_usd_micros as f64;
        }
        if self.ema_supply_usd_micros <= 0.0 {
            return;
        }
        let utilization = (self.ema_demand_usd_micros / self.ema_supply_usd_micros)
            .min(1.0)
            .max(0.0);
        self.observed_utilization_ppm =
            ((utilization * PPM_SCALE as f64).round() as u64).min(PPM_SCALE) as u32;
        let target =
            (self.target_utilization_ppm as f64 / PPM_SCALE as f64).clamp(f64::EPSILON, 1.0);
        let normalized = (utilization / target).max(0.0);
        let error = normalized - 1.0;
        let forgetting = self.forgetting.clamp(0.0, 1.0);
        self.integral_error = self.integral_error * forgetting + error;
        self.integral_error = self.integral_error.clamp(-10.0, 10.0);
        let delta_log = self.eta_p * error + self.eta_i * self.integral_error;
        self.log_price_per_mib = (self.log_price_per_mib + delta_log).clamp(
            (self.min_price_per_mib_usd_micros.max(1) as f64).ln(),
            (self.max_price_per_mib_usd_micros.max(1) as f64).ln(),
        );
        let updated = self.log_price_per_mib.exp().round() as u64;
        self.price_per_mib_usd_micros = updated
            .clamp(
                self.min_price_per_mib_usd_micros,
                self.max_price_per_mib_usd_micros,
            )
            .max(1);
        self.log_price_per_mib = (self.price_per_mib_usd_micros.max(1) as f64).ln();

        gauge!(
            "ad_price_pi_error",
            error,
            "cohort" => self.telemetry.cohort_hash.as_str(),
            "domain" => self.telemetry.domain.as_str(),
            "provider" => self.telemetry.provider_label(),
            "badges" => self.telemetry.badge_hash.as_str(),
        );
        gauge!(
            "ad_price_pi_integral",
            self.integral_error,
            "cohort" => self.telemetry.cohort_hash.as_str()
        );
        gauge!(
            "ad_price_pi_forgetting",
            forgetting,
            "cohort" => self.telemetry.cohort_hash.as_str()
        );
        gauge!(
            "ad_price_pi_price_per_mib",
            self.price_per_mib_usd_micros as f64,
            "cohort" => self.telemetry.cohort_hash.as_str()
        );
        gauge!(
            "ad_price_pi_utilization",
            utilization,
            "cohort" => self.telemetry.cohort_hash.as_str()
        );
        histogram!(
            "ad_price_pi_delta_log",
            delta_log,
            "cohort" => self.telemetry.cohort_hash.as_str()
        );
        if self.price_per_mib_usd_micros == self.min_price_per_mib_usd_micros {
            increment_counter!(
                "ad_price_pi_saturation_total",
                "cohort" => self.telemetry.cohort_hash.as_str(),
                "bound" => "min"
            );
        } else if self.price_per_mib_usd_micros == self.max_price_per_mib_usd_micros {
            increment_counter!(
                "ad_price_pi_saturation_total",
                "cohort" => self.telemetry.cohort_hash.as_str(),
                "bound" => "max"
            );
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReservationKey {
    pub manifest: [u8; 32],
    pub path_hash: [u8; 32],
    pub discriminator: [u8; 32],
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CampaignTargeting {
    pub domains: Vec<String>,
    pub badges: Vec<String>,
}

fn default_margin_ppm() -> u32 {
    PPM_SCALE as u32
}

fn default_action_rate_ppm() -> u32 {
    0
}

fn default_lift_ppm() -> u32 {
    0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Creative {
    pub id: String,
    #[serde(default = "default_action_rate_ppm")]
    pub action_rate_ppm: u32,
    #[serde(default = "default_margin_ppm")]
    pub margin_ppm: u32,
    pub value_per_action_usd_micros: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cpi_usd_micros: Option<u64>,
    #[serde(default = "default_lift_ppm")]
    pub lift_ppm: u32,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub badges: Vec<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub domains: Vec<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub metadata: HashMap<String, String>,
}

impl Creative {
    pub fn willingness_to_pay_usd_micros(&self) -> u64 {
        let theta = self.margin_ppm.min(PPM_SCALE as u32) as u128;
        let action = self.action_rate_ppm.min(PPM_SCALE as u32) as u128;
        let value = self.value_per_action_usd_micros as u128;
        let mut willingness = value.saturating_mul(action).saturating_mul(theta)
            / (PPM_SCALE as u128)
            / (PPM_SCALE as u128);
        if let Some(max_cpi) = self.max_cpi_usd_micros {
            willingness = willingness.min(max_cpi as u128);
        }
        willingness as u64
    }

    fn effective_lift_ppm(&self) -> u32 {
        if self.lift_ppm == 0 {
            self.action_rate_ppm
        } else {
            self.lift_ppm
        }
    }

    fn quality_multiplier_with_lift(&self, config: &MarketplaceConfig, lift_ppm: u32) -> f64 {
        config.quality_multiplier(self.action_rate_ppm, lift_ppm)
    }

    pub fn quality_adjusted_bid(
        &self,
        config: &MarketplaceConfig,
        available_budget_usd_micros: u64,
    ) -> QualityBid {
        self.quality_adjusted_bid_with_lift(
            config,
            available_budget_usd_micros,
            Some(self.effective_lift_ppm()),
        )
    }

    pub fn quality_adjusted_bid_with_lift(
        &self,
        config: &MarketplaceConfig,
        available_budget_usd_micros: u64,
        lift_override_ppm: Option<u32>,
    ) -> QualityBid {
        let willingness = self.willingness_to_pay_usd_micros();
        let base = available_budget_usd_micros.min(willingness);
        if base == 0 {
            return QualityBid::zero();
        }
        let lift = lift_override_ppm.unwrap_or_else(|| self.effective_lift_ppm());
        let multiplier = self.quality_multiplier_with_lift(config, lift);
        let adjusted = (base as f64)
            .mul_add(multiplier, 0.0)
            .round()
            .min(u64::MAX as f64) as u64;
        QualityBid {
            base_bid_usd_micros: base,
            quality_adjusted_usd_micros: adjusted,
            quality_multiplier: multiplier,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct QualityBid {
    pub base_bid_usd_micros: u64,
    pub quality_adjusted_usd_micros: u64,
    pub quality_multiplier: f64,
}

impl QualityBid {
    fn zero() -> Self {
        Self {
            base_bid_usd_micros: 0,
            quality_adjusted_usd_micros: 0,
            quality_multiplier: 0.0,
        }
    }

    fn shade(
        self,
        guidance: BudgetBidShadingGuidance,
        available_budget_usd_micros: u64,
    ) -> (Self, BudgetBidShadingApplication) {
        let requested = guidance.kappa.clamp(0.0, 10.0);
        let multiplier = guidance.scaling_factor();
        if multiplier <= f64::EPSILON {
            return (
                Self {
                    base_bid_usd_micros: 0,
                    quality_adjusted_usd_micros: 0,
                    quality_multiplier: self.quality_multiplier,
                },
                BudgetBidShadingApplication {
                    requested_kappa: requested,
                    applied_multiplier: multiplier,
                    shadow_price: guidance.shadow_price,
                    dual_price: guidance.dual_price,
                },
            );
        }
        if (multiplier - 1.0).abs() < f64::EPSILON {
            return (
                self,
                BudgetBidShadingApplication {
                    requested_kappa: requested,
                    applied_multiplier: multiplier,
                    shadow_price: guidance.shadow_price,
                    dual_price: guidance.dual_price,
                },
            );
        }
        let budget = available_budget_usd_micros as f64;
        let base = (self.base_bid_usd_micros as f64 * multiplier)
            .round()
            .clamp(0.0, budget) as u64;
        let adjusted = (self.quality_adjusted_usd_micros as f64 * multiplier)
            .round()
            .clamp(0.0, budget) as u64;
        (
            QualityBid {
                base_bid_usd_micros: base,
                quality_adjusted_usd_micros: adjusted,
                quality_multiplier: self.quality_multiplier,
            },
            BudgetBidShadingApplication {
                requested_kappa: requested,
                applied_multiplier: multiplier,
                shadow_price: guidance.shadow_price,
                dual_price: guidance.dual_price,
            },
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Campaign {
    pub id: String,
    pub advertiser_account: String,
    pub budget_usd_micros: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub creatives: Vec<Creative>,
    #[serde(default)]
    pub targeting: CampaignTargeting,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub metadata: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignSummary {
    pub id: String,
    pub advertiser_account: String,
    pub remaining_budget_usd_micros: u64,
    pub creatives: Vec<String>,
    pub reserved_budget_usd_micros: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ImpressionContext {
    pub domain: String,
    pub provider: Option<String>,
    pub badges: Vec<String>,
    pub bytes: u64,
    pub attestations: Vec<SelectionAttestation>,
    pub population_estimate: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_intent: Option<BadgeSoftIntentContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_stake_snapshot: Option<verifier_selection::StakeSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_committee: Option<verifier_selection::SelectionReceipt>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "foundation_serialization::serde_bytes"
    )]
    pub verifier_transcript: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct MatchOutcome {
    pub campaign_id: String,
    pub creative_id: String,
    pub price_per_mib_usd_micros: u64,
    pub total_usd_micros: u64,
    pub resource_floor_usd_micros: u64,
    pub resource_floor_breakdown: ResourceFloorBreakdown,
    pub runner_up_quality_bid_usd_micros: u64,
    pub quality_adjusted_bid_usd_micros: u64,
    pub selection_receipt: SelectionReceipt,
    pub uplift: UpliftEstimate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettlementBreakdown {
    pub campaign_id: String,
    pub creative_id: String,
    pub bytes: u64,
    pub price_per_mib_usd_micros: u64,
    pub total_usd_micros: u64,
    pub demand_usd_micros: u64,
    pub resource_floor_usd_micros: u64,
    #[serde(default)]
    pub resource_floor_breakdown: ResourceFloorBreakdown,
    pub runner_up_quality_bid_usd_micros: u64,
    pub quality_adjusted_bid_usd_micros: u64,
    pub viewer_ct: u64,
    pub host_ct: u64,
    pub hardware_ct: u64,
    pub verifier_ct: u64,
    pub liquidity_ct: u64,
    pub miner_ct: u64,
    pub total_ct: u64,
    pub host_it: u64,
    pub hardware_it: u64,
    pub verifier_it: u64,
    pub liquidity_it: u64,
    pub miner_it: u64,
    pub unsettled_usd_micros: u64,
    pub ct_price_usd_micros: u64,
    pub it_price_usd_micros: u64,
    #[serde(default)]
    pub ct_remainders_usd_micros: CtRemainderBreakdown,
    #[serde(default)]
    pub it_remainders_usd_micros: ItRemainderBreakdown,
    #[serde(default)]
    pub ct_twap_window_id: u64,
    #[serde(default)]
    pub it_twap_window_id: u64,
    pub selection_receipt: SelectionReceipt,
    #[serde(default)]
    pub uplift: UpliftEstimate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionCohortTrace {
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub badges: Vec<String>,
    pub bytes: u64,
    pub price_per_mib_usd_micros: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionCandidateTrace {
    pub campaign_id: String,
    pub creative_id: String,
    pub base_bid_usd_micros: u64,
    pub quality_adjusted_bid_usd_micros: u64,
    pub available_budget_usd_micros: u64,
    pub action_rate_ppm: u32,
    pub lift_ppm: u32,
    pub quality_multiplier: f64,
    #[serde(default)]
    pub pacing_kappa: f64,
    #[serde(default)]
    pub requested_kappa: f64,
    #[serde(default)]
    pub shading_multiplier: f64,
    #[serde(default)]
    pub shadow_price: f64,
    #[serde(default)]
    pub dual_price: f64,
    #[serde(default)]
    pub predicted_lift_ppm: u32,
    #[serde(default)]
    pub baseline_action_rate_ppm: u32,
    #[serde(default)]
    pub predicted_propensity: f64,
    #[serde(default)]
    pub uplift_sample_size: u64,
    #[serde(default)]
    pub uplift_ece: f64,
}

impl Default for SelectionCandidateTrace {
    fn default() -> Self {
        Self {
            campaign_id: String::new(),
            creative_id: String::new(),
            base_bid_usd_micros: 0,
            quality_adjusted_bid_usd_micros: 0,
            available_budget_usd_micros: 0,
            action_rate_ppm: 0,
            lift_ppm: 0,
            quality_multiplier: 1.0,
            pacing_kappa: 0.0,
            requested_kappa: 0.0,
            shading_multiplier: 0.0,
            shadow_price: 0.0,
            dual_price: 0.0,
            predicted_lift_ppm: 0,
            baseline_action_rate_ppm: 0,
            predicted_propensity: 0.0,
            uplift_sample_size: 0,
            uplift_ece: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SelectionAttestation {
    Snark {
        #[serde(with = "foundation_serialization::serde_bytes")]
        proof: Vec<u8>,
        circuit_id: String,
    },
    Tee {
        #[serde(with = "foundation_serialization::serde_bytes")]
        report: Vec<u8>,
        #[serde(with = "foundation_serialization::serde_bytes")]
        quote: Vec<u8>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SelectionProofMetadata {
    pub circuit_id: String,
    pub circuit_revision: u16,
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub proof_digest: Vec<u8>,
    #[serde(default, with = "foundation_serialization::serde_bytes")]
    pub proof_bytes_digest: Vec<u8>,
    #[serde(default, with = "foundation_serialization::serde_bytes")]
    pub verifying_key_digest: Vec<u8>,
    #[serde(default)]
    pub proof_length: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "serde_bytes_vec"
    )]
    pub witness_commitments: Vec<Vec<u8>>,
    pub public_inputs: SelectionProofPublicInputs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_committee: Option<verifier_selection::SelectionReceipt>,
}

impl SelectionProofMetadata {
    fn from_verification(circuit_id: String, verification: SelectionProofVerification) -> Self {
        let digest = match selection::expected_transcript_digest(
            &circuit_id,
            verification.revision,
            &verification.proof_bytes_digest,
            &verification.public_inputs,
        ) {
            Ok(expected) => {
                debug_assert_eq!(expected, verification.proof_digest);
                expected
            }
            Err(_) => verification.proof_digest,
        };
        Self {
            circuit_id: circuit_id.to_lowercase(),
            circuit_revision: verification.revision,
            proof_digest: digest.to_vec(),
            proof_bytes_digest: verification.proof_bytes_digest.to_vec(),
            verifying_key_digest: selection::selection_circuit_artifact(&circuit_id)
                .map(|artifact| artifact.verifying_key_digest.to_vec())
                .unwrap_or_default(),
            proof_length: verification.proof_len,
            protocol: verification.protocol,
            witness_commitments: verification
                .witness_commitments
                .iter()
                .map(|bytes| bytes.to_vec())
                .collect(),
            public_inputs: verification.public_inputs,
            verifier_committee: None,
        }
    }

    fn with_verifier_committee(
        mut self,
        committee: Option<verifier_selection::SelectionReceipt>,
    ) -> Self {
        self.verifier_committee = committee;
        self
    }

    fn proof_digest_array(&self) -> Option<[u8; 32]> {
        if self.proof_digest.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&self.proof_digest);
        Some(bytes)
    }

    fn proof_bytes_digest_array(&self) -> Option<[u8; 32]> {
        if self.proof_bytes_digest.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&self.proof_bytes_digest);
        Some(bytes)
    }

    fn verifying_key_digest_array(&self) -> Option<[u8; 32]> {
        if self.verifying_key_digest.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&self.verifying_key_digest);
        Some(bytes)
    }

    fn witness_commitment_arrays(&self) -> Option<Vec<[u8; 32]>> {
        if self.witness_commitments.is_empty() {
            return Some(Vec::new());
        }
        let mut out = Vec::with_capacity(self.witness_commitments.len());
        for commitment in &self.witness_commitments {
            if commitment.len() != 32 {
                return None;
            }
            let mut buf = [0u8; 32];
            buf.copy_from_slice(commitment);
            out.push(buf);
        }
        Some(out)
    }
}

mod serde_bytes_vec {
    use serde::{ser::SerializeSeq, Deserialize, Deserializer, Serializer};

    #[allow(dead_code)]
    pub fn serialize<S>(values: &[Vec<u8>], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(values.len()))?;
        for item in values {
            seq.serialize_element(&ByteSlice(item))?;
        }
        seq.end()
    }

    #[allow(dead_code)]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(transparent)]
        struct ByteVec(#[serde(with = "foundation_serialization::serde_bytes")] Vec<u8>);

        let items: Vec<ByteVec> = Vec::<ByteVec>::deserialize(deserializer)?;
        Ok(items.into_iter().map(|ByteVec(bytes)| bytes).collect())
    }

    struct ByteSlice<'a>(&'a [u8]);

    impl serde::Serialize for ByteSlice<'_> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            foundation_serialization::serde_bytes::serialize(self.0, serializer)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionReceipt {
    pub cohort: SelectionCohortTrace,
    pub candidates: Vec<SelectionCandidateTrace>,
    pub winner_index: usize,
    pub resource_floor_usd_micros: u64,
    #[serde(default)]
    pub resource_floor_breakdown: ResourceFloorBreakdown,
    pub runner_up_quality_bid_usd_micros: u64,
    pub clearing_price_usd_micros: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation: Option<SelectionAttestation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_metadata: Option<SelectionProofMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_committee: Option<verifier_selection::SelectionReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_stake_snapshot: Option<verifier_selection::StakeSnapshot>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "foundation_serialization::serde_bytes"
    )]
    pub verifier_transcript: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge_soft_intent: Option<badge::ann::SoftIntentReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge_soft_intent_snapshot: Option<badge::ann::WalletAnnIndexSnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionAttestationKind {
    Missing,
    Snark,
    Tee,
}

impl SelectionAttestationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SelectionAttestationKind::Missing => "missing",
            SelectionAttestationKind::Snark => "snark",
            SelectionAttestationKind::Tee => "tee",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionReceiptInsights {
    pub winner_index: usize,
    pub winner_quality_bid_usd_micros: u64,
    pub runner_up_quality_bid_usd_micros: u64,
    pub clearing_price_usd_micros: u64,
    pub resource_floor_usd_micros: u64,
    pub attestation_kind: SelectionAttestationKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionReceiptError {
    NoCandidates,
    WinnerOutOfRange {
        declared: usize,
        total: usize,
    },
    WinnerBelowFloor {
        winner: u64,
        floor: u64,
    },
    BudgetBelowClearing {
        available: u64,
        clearing: u64,
    },
    ResourceFloorBreakdownMismatch {
        declared: u64,
        breakdown: u64,
    },
    RunnerUpMismatch {
        declared: u64,
        computed: u64,
    },
    ClearingPriceMismatch {
        declared: u64,
        expected: u64,
    },
    ClearingPriceAboveWinner {
        clearing: u64,
        winner: u64,
    },
    QualityOrderViolation {
        candidate: u64,
        winner: u64,
    },
    CommitmentSerialization,
    ProofMetadataMissing,
    ProofMetadataMismatch {
        field: &'static str,
    },
    InvalidAttestation {
        kind: SelectionAttestationKind,
        reason: &'static str,
    },
    BadgeSoftIntentMissingSnapshot,
    BadgeSoftIntentInvalid,
    VerifierCommitteeMissing,
    VerifierCommitteeInvalid {
        reason: String,
    },
}

impl std::fmt::Display for SelectionReceiptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectionReceiptError::NoCandidates => write!(f, "selection receipt has no candidates"),
            SelectionReceiptError::WinnerOutOfRange { declared, total } => write!(
                f,
                "winner index {declared} out of range for {total} candidates"
            ),
            SelectionReceiptError::WinnerBelowFloor { winner, floor } => {
                write!(f, "winner quality {winner} below resource floor {floor}")
            }
            SelectionReceiptError::BudgetBelowClearing {
                available,
                clearing,
            } => write!(
                f,
                "available budget {available} below clearing price {clearing}"
            ),
            SelectionReceiptError::ResourceFloorBreakdownMismatch {
                declared,
                breakdown,
            } => write!(
                f,
                "resource floor breakdown total {breakdown} does not cover declared floor {declared}"
            ),
            SelectionReceiptError::RunnerUpMismatch { declared, computed } => write!(
                f,
                "runner-up quality mismatch: declared {declared}, computed {computed}"
            ),
            SelectionReceiptError::ClearingPriceMismatch { declared, expected } => write!(
                f,
                "clearing price mismatch: declared {declared}, expected {expected}"
            ),
            SelectionReceiptError::ClearingPriceAboveWinner { clearing, winner } => write!(
                f,
                "clearing price {clearing} exceeds winner quality {winner}"
            ),
            SelectionReceiptError::QualityOrderViolation { candidate, winner } => write!(
                f,
                "non-winning candidate quality {candidate} exceeds winner quality {winner}"
            ),
            SelectionReceiptError::CommitmentSerialization => {
                write!(f, "unable to encode selection commitment")
            }
            SelectionReceiptError::ProofMetadataMissing => {
                write!(f, "missing selection proof metadata for SNARK attestation")
            }
            SelectionReceiptError::ProofMetadataMismatch { field } => {
                write!(f, "selection proof metadata mismatch for field '{field}'")
            }
            SelectionReceiptError::InvalidAttestation { kind, reason } => {
                write!(f, "invalid {:?} attestation: {reason}", kind)
            }
            SelectionReceiptError::BadgeSoftIntentMissingSnapshot => {
                write!(f, "badge soft intent snapshot missing")
            }
            SelectionReceiptError::BadgeSoftIntentInvalid => {
                write!(f, "badge soft intent proof invalid")
            }
            SelectionReceiptError::VerifierCommitteeMissing => {
                write!(f, "verifier committee receipt missing")
            }
            SelectionReceiptError::VerifierCommitteeInvalid { reason } => {
                write!(f, "verifier committee invalid: {reason}")
            }
        }
    }
}

impl std::error::Error for SelectionReceiptError {}

impl SelectionReceipt {
    pub(crate) fn commitment_bytes_raw(&self) -> Result<[u8; 32], foundation_serialization::Error> {
        let mut candidates_values = Vec::with_capacity(self.candidates.len());
        for candidate in &self.candidates {
            let mut candidate_map = JsonMap::new();
            candidate_map.insert(
                "campaign_id".into(),
                JsonValue::String(candidate.campaign_id.clone()),
            );
            candidate_map.insert(
                "creative_id".into(),
                JsonValue::String(candidate.creative_id.clone()),
            );
            candidate_map.insert(
                "base_bid_usd_micros".into(),
                JsonValue::from(candidate.base_bid_usd_micros),
            );
            candidate_map.insert(
                "quality_adjusted_bid_usd_micros".into(),
                JsonValue::from(candidate.quality_adjusted_bid_usd_micros),
            );
            candidate_map.insert(
                "available_budget_usd_micros".into(),
                JsonValue::from(candidate.available_budget_usd_micros),
            );
            candidate_map.insert(
                "action_rate_ppm".into(),
                JsonValue::from(candidate.action_rate_ppm),
            );
            candidate_map.insert("lift_ppm".into(), JsonValue::from(candidate.lift_ppm));
            candidate_map.insert(
                "quality_multiplier".into(),
                JsonValue::from(candidate.quality_multiplier),
            );
            candidate_map.insert(
                "pacing_kappa".into(),
                JsonValue::from(candidate.pacing_kappa),
            );
            candidate_map.insert(
                "requested_kappa".into(),
                JsonValue::from(candidate.requested_kappa),
            );
            candidate_map.insert(
                "shading_multiplier".into(),
                JsonValue::from(candidate.shading_multiplier),
            );
            candidate_map.insert(
                "shadow_price".into(),
                JsonValue::from(candidate.shadow_price),
            );
            candidate_map.insert("dual_price".into(), JsonValue::from(candidate.dual_price));
            candidate_map.insert(
                "predicted_lift_ppm".into(),
                JsonValue::from(candidate.predicted_lift_ppm),
            );
            candidate_map.insert(
                "baseline_action_rate_ppm".into(),
                JsonValue::from(candidate.baseline_action_rate_ppm),
            );
            candidate_map.insert(
                "predicted_propensity".into(),
                JsonValue::from(candidate.predicted_propensity),
            );
            candidate_map.insert(
                "uplift_sample_size".into(),
                JsonValue::from(candidate.uplift_sample_size),
            );
            candidate_map.insert("uplift_ece".into(), JsonValue::from(candidate.uplift_ece));
            candidates_values.push(JsonValue::Object(candidate_map));
        }
        let mut commitment_map = JsonMap::new();
        commitment_map.insert(
            "domain".into(),
            JsonValue::String(self.cohort.domain.clone()),
        );
        commitment_map.insert(
            "provider".into(),
            self.cohort
                .provider
                .as_ref()
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        );
        let badge_values = self
            .cohort
            .badges
            .iter()
            .cloned()
            .map(JsonValue::String)
            .collect();
        commitment_map.insert("badges".into(), JsonValue::Array(badge_values));
        commitment_map.insert("bytes".into(), JsonValue::from(self.cohort.bytes));
        commitment_map.insert(
            "price_per_mib_usd_micros".into(),
            JsonValue::from(self.cohort.price_per_mib_usd_micros),
        );
        commitment_map.insert(
            "winner_index".into(),
            JsonValue::from(self.winner_index as u64),
        );
        commitment_map.insert(
            "runner_up_quality_bid_usd_micros".into(),
            JsonValue::from(self.runner_up_quality_bid_usd_micros),
        );
        commitment_map.insert(
            "clearing_price_usd_micros".into(),
            JsonValue::from(self.clearing_price_usd_micros),
        );
        commitment_map.insert(
            "resource_floor_usd_micros".into(),
            JsonValue::from(self.resource_floor_usd_micros),
        );
        commitment_map.insert("candidates".into(), JsonValue::Array(candidates_values));
        let serialized = json::to_vec_value(&JsonValue::Object(commitment_map));
        Ok(*blake3::hash(&serialized).as_bytes())
    }

    pub fn commitment_digest(&self) -> Result<[u8; 32], SelectionReceiptError> {
        self.commitment_bytes_raw()
            .map_err(|_| SelectionReceiptError::CommitmentSerialization)
    }

    pub fn attestation_kind(&self) -> SelectionAttestationKind {
        match self.attestation {
            Some(SelectionAttestation::Snark { .. }) => SelectionAttestationKind::Snark,
            Some(SelectionAttestation::Tee { .. }) => SelectionAttestationKind::Tee,
            None => SelectionAttestationKind::Missing,
        }
    }

    pub fn validate(&self) -> Result<SelectionReceiptInsights, SelectionReceiptError> {
        if self.candidates.is_empty() {
            return Err(SelectionReceiptError::NoCandidates);
        }
        if self.winner_index >= self.candidates.len() {
            return Err(SelectionReceiptError::WinnerOutOfRange {
                declared: self.winner_index,
                total: self.candidates.len(),
            });
        }
        let breakdown_total = self.resource_floor_breakdown.total_usd_micros();
        if breakdown_total > 0 && breakdown_total < self.resource_floor_usd_micros {
            return Err(SelectionReceiptError::ResourceFloorBreakdownMismatch {
                declared: self.resource_floor_usd_micros,
                breakdown: breakdown_total,
            });
        }
        let winner = &self.candidates[self.winner_index];
        if winner.quality_adjusted_bid_usd_micros < self.resource_floor_usd_micros {
            return Err(SelectionReceiptError::WinnerBelowFloor {
                winner: winner.quality_adjusted_bid_usd_micros,
                floor: self.resource_floor_usd_micros,
            });
        }
        if winner.available_budget_usd_micros < self.clearing_price_usd_micros {
            return Err(SelectionReceiptError::BudgetBelowClearing {
                available: winner.available_budget_usd_micros,
                clearing: self.clearing_price_usd_micros,
            });
        }
        let mut runner_up = 0u64;
        for (idx, candidate) in self.candidates.iter().enumerate() {
            if idx == self.winner_index {
                continue;
            }
            if candidate.quality_adjusted_bid_usd_micros > winner.quality_adjusted_bid_usd_micros {
                return Err(SelectionReceiptError::QualityOrderViolation {
                    candidate: candidate.quality_adjusted_bid_usd_micros,
                    winner: winner.quality_adjusted_bid_usd_micros,
                });
            }
            runner_up = runner_up.max(candidate.quality_adjusted_bid_usd_micros);
        }
        if self.runner_up_quality_bid_usd_micros != runner_up {
            return Err(SelectionReceiptError::RunnerUpMismatch {
                declared: self.runner_up_quality_bid_usd_micros,
                computed: runner_up,
            });
        }
        let expected_clearing = self.resource_floor_usd_micros.max(runner_up);
        if self.clearing_price_usd_micros != expected_clearing {
            return Err(SelectionReceiptError::ClearingPriceMismatch {
                declared: self.clearing_price_usd_micros,
                expected: expected_clearing,
            });
        }
        if self.clearing_price_usd_micros > winner.quality_adjusted_bid_usd_micros {
            return Err(SelectionReceiptError::ClearingPriceAboveWinner {
                clearing: self.clearing_price_usd_micros,
                winner: winner.quality_adjusted_bid_usd_micros,
            });
        }
        if let Some(attestation) = &self.attestation {
            match attestation {
                SelectionAttestation::Snark { proof, circuit_id } => {
                    if proof.is_empty() {
                        return Err(SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Snark,
                            reason: "empty_proof",
                        });
                    }
                    if circuit_id.trim().is_empty() {
                        return Err(SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Snark,
                            reason: "empty_circuit",
                        });
                    }
                    let metadata = self
                        .proof_metadata
                        .as_ref()
                        .ok_or(SelectionReceiptError::ProofMetadataMissing)?;
                    if metadata.circuit_id != circuit_id.to_lowercase() {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "circuit_id",
                        });
                    }
                    if metadata.public_inputs.winner_index as usize != self.winner_index {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "winner_index",
                        });
                    }
                    if metadata.public_inputs.winner_quality_bid_usd_micros
                        != winner.quality_adjusted_bid_usd_micros
                    {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "winner_quality",
                        });
                    }
                    if metadata.public_inputs.runner_up_quality_bid_usd_micros != runner_up {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "runner_up_quality",
                        });
                    }
                    if metadata.public_inputs.resource_floor_usd_micros
                        != self.resource_floor_usd_micros
                    {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "resource_floor",
                        });
                    }
                    if metadata.public_inputs.clearing_price_usd_micros
                        != self.clearing_price_usd_micros
                    {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "clearing_price",
                        });
                    }
                    if metadata.public_inputs.candidate_count as usize != self.candidates.len() {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "candidate_count",
                        });
                    }
                    let proof_digest = metadata.proof_digest_array().ok_or(
                        SelectionReceiptError::ProofMetadataMismatch {
                            field: "proof_digest",
                        },
                    )?;
                    let proof_bytes_digest = metadata.proof_bytes_digest_array().ok_or(
                        SelectionReceiptError::ProofMetadataMismatch {
                            field: "proof_bytes_digest",
                        },
                    )?;
                    let computed_proof_digest = selection::extract_proof_body_digest(proof)
                        .map_err(|_| SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Snark,
                            reason: "proof_format",
                        })?;
                    if proof_bytes_digest != computed_proof_digest {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "proof_bytes_digest",
                        });
                    }
                    let verifying_key_digest = metadata.verifying_key_digest_array().ok_or(
                        SelectionReceiptError::ProofMetadataMismatch {
                            field: "verifying_key_digest",
                        },
                    )?;
                    let expected_verifying_key =
                        selection::selection_circuit_artifact(&metadata.circuit_id)
                            .map(|artifact| artifact.verifying_key_digest)
                            .ok_or(SelectionReceiptError::InvalidAttestation {
                                kind: SelectionAttestationKind::Snark,
                                reason: "artifact",
                            })?;
                    if verifying_key_digest != expected_verifying_key {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "verifying_key_digest",
                        });
                    }
                    let proof_commitment =
                        metadata.public_inputs.commitment_array().map_err(|_| {
                            SelectionReceiptError::ProofMetadataMismatch {
                                field: "commitment",
                            }
                        })?;
                    let expected_commitment = self.commitment_digest()?;
                    if proof_commitment != expected_commitment {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "commitment",
                        });
                    }
                    if metadata.proof_length == 0 {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "proof_length",
                        });
                    }
                    if metadata.witness_commitment_arrays().is_none() {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "witness_commitments",
                        });
                    }
                    let expected_digest = selection::expected_transcript_digest(
                        &metadata.circuit_id,
                        metadata.circuit_revision,
                        &proof_bytes_digest,
                        &metadata.public_inputs,
                    )
                    .map_err(|_| {
                        SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Snark,
                            reason: "metadata",
                        }
                    })?;
                    if proof_digest != expected_digest {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "proof_digest",
                        });
                    }
                    if metadata.verifier_committee.as_ref() != self.verifier_committee.as_ref() {
                        return Err(SelectionReceiptError::ProofMetadataMismatch {
                            field: "verifier_committee",
                        });
                    }
                }
                SelectionAttestation::Tee { report, quote } => {
                    if report.is_empty() {
                        return Err(SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Tee,
                            reason: "empty_report",
                        });
                    }
                    if quote.is_empty() {
                        return Err(SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Tee,
                            reason: "empty_quote",
                        });
                    }
                }
            }
        }
        if let Some(proof) = &self.badge_soft_intent {
            let snapshot = self
                .badge_soft_intent_snapshot
                .as_ref()
                .ok_or(SelectionReceiptError::BadgeSoftIntentMissingSnapshot)?;
            if !badge::ann::verify_receipt(snapshot, proof, &self.cohort.badges) {
                return Err(SelectionReceiptError::BadgeSoftIntentInvalid);
            }
        }
        Ok(SelectionReceiptInsights {
            winner_index: self.winner_index,
            winner_quality_bid_usd_micros: winner.quality_adjusted_bid_usd_micros,
            runner_up_quality_bid_usd_micros: runner_up,
            clearing_price_usd_micros: self.clearing_price_usd_micros,
            resource_floor_usd_micros: self.resource_floor_usd_micros,
            attestation_kind: self.attestation_kind(),
        })
    }
}

fn soft_intent_artifacts(
    badges: &[String],
    context: &Option<BadgeSoftIntentContext>,
) -> (
    Option<badge::ann::SoftIntentReceipt>,
    Option<badge::ann::WalletAnnIndexSnapshot>,
) {
    if let Some(ctx) = context {
        if let Some(snapshot) = ctx.wallet_index.as_ref() {
            if let Some(proof) = ctx.proof.as_ref() {
                if badge::ann::verify_receipt(snapshot, proof, badges) {
                    return (Some(proof.clone()), Some(snapshot.clone()));
                }
                return (None, Some(snapshot.clone()));
            }
            if let Some(proof) = badge::ann::build_proof(snapshot, badges) {
                return (Some(proof), Some(snapshot.clone()));
            }
            return (None, Some(snapshot.clone()));
        }
    }
    (None, None)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CohortPriceSnapshot {
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub badges: Vec<String>,
    pub price_per_mib_usd_micros: u64,
    pub target_utilization_ppm: u32,
    #[serde(default)]
    pub observed_utilization_ppm: u32,
}

pub trait Marketplace: Send + Sync {
    fn register_campaign(&self, campaign: Campaign) -> Result<(), MarketplaceError>;
    fn list_campaigns(&self) -> Vec<CampaignSummary>;
    fn reserve_impression(
        &self,
        key: ReservationKey,
        ctx: ImpressionContext,
    ) -> Option<MatchOutcome>;
    fn commit(&self, key: &ReservationKey) -> Option<SettlementBreakdown>;
    fn cancel(&self, key: &ReservationKey);
    fn distribution(&self) -> DistributionPolicy;
    fn update_distribution(&self, policy: DistributionPolicy);
    fn update_oracle(&self, oracle: TokenOracle);
    fn oracle(&self) -> TokenOracle;
    fn cohort_prices(&self) -> Vec<CohortPriceSnapshot>;
    fn budget_broker(&self) -> &RwLock<BudgetBroker>;

    fn budget_broker_config(&self) -> BudgetBrokerConfig {
        self.budget_broker().read().unwrap().config().clone()
    }

    fn budget_snapshot(&self) -> BudgetBrokerSnapshot {
        self.budget_broker().read().unwrap().snapshot()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CampaignState {
    campaign: Campaign,
    remaining_budget_usd_micros: u64,
    #[serde(default)]
    reserved_budget_usd_micros: u64,
}

struct ReservationState {
    campaign_id: String,
    creative_id: String,
    bytes: u64,
    price_per_mib_usd_micros: u64,
    total_usd_micros: u64,
    demand_usd_micros: u64,
    resource_floor_usd_micros: u64,
    resource_floor_breakdown: ResourceFloorBreakdown,
    runner_up_quality_bid_usd_micros: u64,
    quality_adjusted_bid_usd_micros: u64,
    cohort: CohortKey,
    selection_receipt: SelectionReceipt,
    uplift: UpliftEstimate,
}

pub struct InMemoryMarketplace {
    config: MarketplaceConfig,
    campaigns: RwLock<HashMap<String, CampaignState>>,
    reservations: RwLock<HashMap<ReservationKey, ReservationState>>,
    consumed_reservations: RwLock<HashSet<ReservationKey>>,
    distribution: RwLock<DistributionPolicy>,
    pricing: RwLock<HashMap<CohortKey, CohortPricingState>>,
    oracle: RwLock<TokenOracle>,
    attestation: SelectionAttestationManager,
    budget_broker: RwLock<BudgetBroker>,
    badge_guard: BadgeGuard,
    privacy_budget: RwLock<PrivacyBudgetManager>,
    uplift: RwLock<UpliftEstimator>,
    token_remainders: RwLock<TokenRemainderLedger>,
}

pub struct SledMarketplace {
    _db: SledDb,
    campaigns_tree: SledTree,
    metadata_tree: SledTree,
    config: MarketplaceConfig,
    campaigns: RwLock<HashMap<String, CampaignState>>,
    reservations: RwLock<HashMap<ReservationKey, ReservationState>>,
    consumed_reservations: RwLock<HashSet<ReservationKey>>,
    distribution: RwLock<DistributionPolicy>,
    pricing: RwLock<HashMap<CohortKey, CohortPricingState>>,
    oracle: RwLock<TokenOracle>,
    attestation: SelectionAttestationManager,
    budget_broker: RwLock<BudgetBroker>,
    badge_guard: BadgeGuard,
    privacy_budget: RwLock<PrivacyBudgetManager>,
    uplift: RwLock<UpliftEstimator>,
    token_remainders: RwLock<TokenRemainderLedger>,
}

#[derive(Debug)]
pub enum MarketplaceError {
    DuplicateCampaign,
    UnknownCampaign,
    PersistenceFailure(String),
}

impl From<PersistenceError> for MarketplaceError {
    fn from(err: PersistenceError) -> Self {
        MarketplaceError::PersistenceFailure(err.to_string())
    }
}
fn creative_to_value(creative: &Creative) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("id".into(), JsonValue::String(creative.id.clone()));
    map.insert(
        "action_rate_ppm".into(),
        JsonValue::Number(JsonNumber::from(creative.action_rate_ppm)),
    );
    map.insert(
        "margin_ppm".into(),
        JsonValue::Number(JsonNumber::from(creative.margin_ppm)),
    );
    map.insert(
        "lift_ppm".into(),
        JsonValue::Number(JsonNumber::from(creative.lift_ppm)),
    );
    map.insert(
        "value_per_action_usd_micros".into(),
        JsonValue::Number(JsonNumber::from(creative.value_per_action_usd_micros)),
    );
    if let Some(max_cpi) = creative.max_cpi_usd_micros {
        map.insert(
            "max_cpi_usd_micros".into(),
            JsonValue::Number(JsonNumber::from(max_cpi)),
        );
    }
    map.insert(
        "badges".into(),
        JsonValue::Array(
            creative
                .badges
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    map.insert(
        "domains".into(),
        JsonValue::Array(
            creative
                .domains
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    let mut metadata = JsonMap::new();
    for (key, value) in &creative.metadata {
        metadata.insert(key.clone(), JsonValue::String(value.clone()));
    }
    map.insert("metadata".into(), JsonValue::Object(metadata));
    JsonValue::Object(map)
}

fn creative_from_value(value: &JsonValue) -> Result<Creative, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("creative must be an object"))?;
    Ok(Creative {
        id: read_string(obj, "id")?,
        action_rate_ppm: obj
            .get("action_rate_ppm")
            .map(|_| read_u32(obj, "action_rate_ppm"))
            .transpose()?
            .unwrap_or_else(default_action_rate_ppm),
        margin_ppm: obj
            .get("margin_ppm")
            .map(|_| read_u32(obj, "margin_ppm"))
            .transpose()?
            .unwrap_or_else(default_margin_ppm),
        value_per_action_usd_micros: read_u64(obj, "value_per_action_usd_micros")?,
        max_cpi_usd_micros: obj
            .get("max_cpi_usd_micros")
            .map(|_| read_u64(obj, "max_cpi_usd_micros"))
            .transpose()?,
        lift_ppm: obj
            .get("lift_ppm")
            .map(|_| read_u32(obj, "lift_ppm"))
            .transpose()?
            .unwrap_or_else(default_lift_ppm),
        badges: read_string_vec(obj, "badges")?,
        domains: read_string_vec(obj, "domains")?,
        metadata: read_string_map(obj, "metadata")?,
    })
}

fn targeting_to_value(targeting: &CampaignTargeting) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "domains".into(),
        JsonValue::Array(
            targeting
                .domains
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    map.insert(
        "badges".into(),
        JsonValue::Array(
            targeting
                .badges
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    JsonValue::Object(map)
}

fn targeting_from_value(value: &JsonValue) -> Result<CampaignTargeting, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("targeting must be an object"))?;
    Ok(CampaignTargeting {
        domains: read_string_vec(obj, "domains")?,
        badges: read_string_vec(obj, "badges")?,
    })
}

fn campaign_to_value(campaign: &Campaign) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("id".into(), JsonValue::String(campaign.id.clone()));
    map.insert(
        "advertiser_account".into(),
        JsonValue::String(campaign.advertiser_account.clone()),
    );
    map.insert(
        "budget_usd_micros".into(),
        JsonValue::Number(JsonNumber::from(campaign.budget_usd_micros)),
    );
    map.insert(
        "creatives".into(),
        JsonValue::Array(campaign.creatives.iter().map(creative_to_value).collect()),
    );
    map.insert("targeting".into(), targeting_to_value(&campaign.targeting));
    let mut metadata = JsonMap::new();
    for (key, value) in &campaign.metadata {
        metadata.insert(key.clone(), JsonValue::String(value.clone()));
    }
    map.insert("metadata".into(), JsonValue::Object(metadata));
    JsonValue::Object(map)
}

pub fn campaign_from_value(value: &JsonValue) -> Result<Campaign, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("campaign must be an object"))?;
    let creatives_value = obj
        .get("creatives")
        .ok_or_else(|| invalid("campaign creatives missing"))?;
    let creatives = creatives_value
        .as_array()
        .ok_or_else(|| invalid("campaign creatives must be an array"))?
        .iter()
        .map(creative_from_value)
        .collect::<Result<Vec<_>, _>>()?;
    let targeting = match obj.get("targeting") {
        Some(value) => targeting_from_value(value)?,
        None => CampaignTargeting::default(),
    };
    Ok(Campaign {
        id: read_string(obj, "id")?,
        advertiser_account: read_string(obj, "advertiser_account")?,
        budget_usd_micros: read_u64(obj, "budget_usd_micros")?,
        creatives,
        targeting,
        metadata: read_string_map(obj, "metadata")?,
    })
}

fn campaign_state_to_value(state: &CampaignState) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("campaign".into(), campaign_to_value(&state.campaign));
    map.insert(
        "remaining_budget_usd_micros".into(),
        JsonValue::Number(JsonNumber::from(state.remaining_budget_usd_micros)),
    );
    map.insert(
        "reserved_budget_usd_micros".into(),
        JsonValue::Number(JsonNumber::from(state.reserved_budget_usd_micros)),
    );
    JsonValue::Object(map)
}

fn campaign_state_from_value(value: &JsonValue) -> Result<CampaignState, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("campaign state must be an object"))?;
    let campaign_value = obj
        .get("campaign")
        .ok_or_else(|| invalid("campaign state missing campaign"))?;
    let campaign = campaign_from_value(campaign_value)?;
    let remaining = read_u64(obj, "remaining_budget_usd_micros")?;
    let reserved = obj
        .get("reserved_budget_usd_micros")
        .map(|_| read_u64(obj, "reserved_budget_usd_micros"))
        .transpose()?
        .unwrap_or(0);
    Ok(CampaignState {
        campaign,
        remaining_budget_usd_micros: remaining,
        reserved_budget_usd_micros: reserved,
    })
}

fn serialize_campaign_state(state: &CampaignState) -> Result<Vec<u8>, PersistenceError> {
    Ok(json::to_vec_value(&campaign_state_to_value(state)))
}

fn deserialize_campaign_state(bytes: &[u8]) -> Result<CampaignState, PersistenceError> {
    let value = json::value_from_slice(bytes)?;
    campaign_state_from_value(&value)
}

fn serialize_distribution(policy: &DistributionPolicy) -> Result<Vec<u8>, PersistenceError> {
    Ok(json::to_vec_value(&distribution_policy_to_value(policy)))
}

fn deserialize_distribution(bytes: &[u8]) -> Result<DistributionPolicy, PersistenceError> {
    let value = json::value_from_slice(bytes)?;
    distribution_policy_from_value(&value)
}

fn serialize_budget_snapshot(snapshot: &BudgetBrokerSnapshot) -> Result<Vec<u8>, PersistenceError> {
    Ok(json::to_vec_value(&budget_snapshot_to_value(snapshot)))
}

fn deserialize_budget_snapshot(bytes: &[u8]) -> Result<BudgetBrokerSnapshot, PersistenceError> {
    let value = json::value_from_slice(bytes)?;
    budget_snapshot_from_value(&value)
}

fn serialize_token_remainders(ledger: &TokenRemainderLedger) -> Result<Vec<u8>, PersistenceError> {
    Ok(json::to_vec_value(&token_remainders_to_value(ledger)))
}

fn deserialize_token_remainders(bytes: &[u8]) -> Result<TokenRemainderLedger, PersistenceError> {
    let value = json::value_from_slice(bytes)?;
    token_remainders_from_value(&value)
}

fn token_remainders_to_value(ledger: &TokenRemainderLedger) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "ct_viewer_usd".into(),
        JsonValue::from(ledger.ct_viewer_usd),
    );
    map.insert("ct_host_usd".into(), JsonValue::from(ledger.ct_host_usd));
    map.insert(
        "ct_hardware_usd".into(),
        JsonValue::from(ledger.ct_hardware_usd),
    );
    map.insert(
        "ct_verifier_usd".into(),
        JsonValue::from(ledger.ct_verifier_usd),
    );
    map.insert(
        "ct_liquidity_usd".into(),
        JsonValue::from(ledger.ct_liquidity_usd),
    );
    map.insert("ct_miner_usd".into(), JsonValue::from(ledger.ct_miner_usd));
    map.insert("it_host_usd".into(), JsonValue::from(ledger.it_host_usd));
    map.insert(
        "it_hardware_usd".into(),
        JsonValue::from(ledger.it_hardware_usd),
    );
    map.insert(
        "it_verifier_usd".into(),
        JsonValue::from(ledger.it_verifier_usd),
    );
    map.insert(
        "it_liquidity_usd".into(),
        JsonValue::from(ledger.it_liquidity_usd),
    );
    map.insert("it_miner_usd".into(), JsonValue::from(ledger.it_miner_usd));
    JsonValue::Object(map)
}

fn token_remainders_from_value(
    value: &JsonValue,
) -> Result<TokenRemainderLedger, PersistenceError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("token remainders must be an object"))?;
    Ok(TokenRemainderLedger {
        ct_viewer_usd: read_u64(map, "ct_viewer_usd")?,
        ct_host_usd: read_u64(map, "ct_host_usd")?,
        ct_hardware_usd: read_u64(map, "ct_hardware_usd")?,
        ct_verifier_usd: read_u64(map, "ct_verifier_usd")?,
        ct_liquidity_usd: read_u64(map, "ct_liquidity_usd")?,
        ct_miner_usd: read_u64(map, "ct_miner_usd")?,
        it_host_usd: read_u64(map, "it_host_usd")?,
        it_hardware_usd: read_u64(map, "it_hardware_usd")?,
        it_verifier_usd: read_u64(map, "it_verifier_usd")?,
        it_liquidity_usd: read_u64(map, "it_liquidity_usd")?,
        it_miner_usd: read_u64(map, "it_miner_usd")?,
    })
}

fn budget_broker_config_to_value(config: &BudgetBrokerConfig) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "epoch_impressions".into(),
        JsonValue::from(config.epoch_impressions),
    );
    map.insert("step_size".into(), JsonValue::from(config.step_size));
    map.insert("dual_step".into(), JsonValue::from(config.dual_step));
    map.insert(
        "dual_forgetting".into(),
        JsonValue::from(config.dual_forgetting),
    );
    map.insert("max_kappa".into(), JsonValue::from(config.max_kappa));
    map.insert("min_kappa".into(), JsonValue::from(config.min_kappa));
    map.insert(
        "shadow_price_cap".into(),
        JsonValue::from(config.shadow_price_cap),
    );
    map.insert("smoothing".into(), JsonValue::from(config.smoothing));
    map.insert(
        "epochs_per_budget".into(),
        JsonValue::from(config.epochs_per_budget),
    );
    JsonValue::Object(map)
}

fn budget_broker_config_from_value(
    value: &JsonValue,
) -> Result<BudgetBrokerConfig, PersistenceError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("budget broker config must be an object"))?;
    let defaults = BudgetBrokerConfig::default();
    let config = BudgetBrokerConfig {
        epoch_impressions: read_u64(map, "epoch_impressions")?,
        step_size: read_f64(map, "step_size")?,
        dual_step: if map.contains_key("dual_step") {
            read_f64(map, "dual_step")?
        } else {
            defaults.dual_step
        },
        dual_forgetting: if map.contains_key("dual_forgetting") {
            read_f64(map, "dual_forgetting")?
        } else {
            defaults.dual_forgetting
        },
        max_kappa: read_f64(map, "max_kappa")?,
        min_kappa: if map.contains_key("min_kappa") {
            read_f64(map, "min_kappa")?
        } else {
            defaults.min_kappa
        },
        shadow_price_cap: if map.contains_key("shadow_price_cap") {
            read_f64(map, "shadow_price_cap")?
        } else {
            defaults.shadow_price_cap
        },
        smoothing: read_f64(map, "smoothing")?,
        epochs_per_budget: read_u64(map, "epochs_per_budget")?,
    };
    Ok(config.normalized())
}

fn cohort_key_snapshot_to_value(snapshot: &CohortKeySnapshot) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("domain".into(), JsonValue::String(snapshot.domain.clone()));
    map.insert(
        "provider".into(),
        snapshot
            .provider
            .as_ref()
            .map(|value| JsonValue::String(value.clone()))
            .unwrap_or(JsonValue::Null),
    );
    let badges = snapshot
        .badges
        .iter()
        .cloned()
        .map(JsonValue::String)
        .collect();
    map.insert("badges".into(), JsonValue::Array(badges));
    JsonValue::Object(map)
}

fn cohort_key_snapshot_from_value(
    value: &JsonValue,
) -> Result<CohortKeySnapshot, PersistenceError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("cohort key must be an object"))?;
    let domain = read_string(map, "domain")?;
    let provider = match map.get("provider") {
        Some(JsonValue::String(value)) => Some(value.clone()),
        Some(JsonValue::Null) | None => None,
        Some(_) => return Err(invalid("provider must be a string or null")),
    };
    let badges = match map.get("badges") {
        None => Vec::new(),
        Some(JsonValue::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| invalid("badge entries must be strings"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(invalid("badges must be an array")),
    };
    Ok(CohortKeySnapshot {
        domain,
        provider,
        badges,
    })
}

fn cohort_budget_snapshot_to_value(snapshot: &CohortBudgetSnapshot) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "cohort".into(),
        cohort_key_snapshot_to_value(&snapshot.cohort),
    );
    map.insert("kappa".into(), JsonValue::from(snapshot.kappa));
    map.insert(
        "smoothed_error".into(),
        JsonValue::from(snapshot.smoothed_error),
    );
    map.insert(
        "realized_spend".into(),
        JsonValue::from(snapshot.realized_spend),
    );
    JsonValue::Object(map)
}

fn cohort_budget_snapshot_from_value(
    value: &JsonValue,
) -> Result<CohortBudgetSnapshot, PersistenceError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("cohort budget must be an object"))?;
    let cohort_value = map.get("cohort").ok_or_else(|| invalid("missing cohort"))?;
    Ok(CohortBudgetSnapshot {
        cohort: cohort_key_snapshot_from_value(cohort_value)?,
        kappa: read_f64(map, "kappa")?,
        smoothed_error: read_f64(map, "smoothed_error")?,
        realized_spend: read_f64(map, "realized_spend")?,
    })
}

fn campaign_budget_snapshot_to_value(snapshot: &CampaignBudgetSnapshot) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "campaign_id".into(),
        JsonValue::String(snapshot.campaign_id.clone()),
    );
    map.insert(
        "total_budget".into(),
        JsonValue::from(snapshot.total_budget),
    );
    map.insert(
        "remaining_budget".into(),
        JsonValue::from(snapshot.remaining_budget),
    );
    map.insert(
        "epoch_target".into(),
        JsonValue::from(snapshot.epoch_target),
    );
    map.insert("epoch_spend".into(), JsonValue::from(snapshot.epoch_spend));
    map.insert(
        "epoch_impressions".into(),
        JsonValue::from(snapshot.epoch_impressions),
    );
    map.insert("dual_price".into(), JsonValue::from(snapshot.dual_price));
    let cohorts = snapshot
        .cohorts
        .iter()
        .map(cohort_budget_snapshot_to_value)
        .collect();
    map.insert("cohorts".into(), JsonValue::Array(cohorts));
    JsonValue::Object(map)
}

fn campaign_budget_snapshot_from_value(
    value: &JsonValue,
) -> Result<CampaignBudgetSnapshot, PersistenceError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("campaign budget must be an object"))?;
    let cohorts_value = map
        .get("cohorts")
        .ok_or_else(|| invalid("missing cohorts"))?;
    let cohorts = match cohorts_value {
        JsonValue::Array(entries) => entries
            .iter()
            .map(cohort_budget_snapshot_from_value)
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(invalid("cohorts must be an array")),
    };
    Ok(CampaignBudgetSnapshot {
        campaign_id: read_string(map, "campaign_id")?,
        total_budget: read_u64(map, "total_budget")?,
        remaining_budget: read_u64(map, "remaining_budget")?,
        epoch_target: read_f64(map, "epoch_target")?,
        epoch_spend: read_f64(map, "epoch_spend")?,
        epoch_impressions: read_u64(map, "epoch_impressions")?,
        dual_price: read_f64(map, "dual_price")?,
        cohorts,
    })
}

fn budget_snapshot_to_value(snapshot: &BudgetBrokerSnapshot) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "config".into(),
        budget_broker_config_to_value(&snapshot.config),
    );
    let campaigns = snapshot
        .campaigns
        .iter()
        .map(campaign_budget_snapshot_to_value)
        .collect();
    map.insert("campaigns".into(), JsonValue::Array(campaigns));
    let analytics = budget_snapshot_analytics(snapshot);
    map.insert(
        "generated_at_micros".into(),
        JsonValue::from(snapshot.generated_at_micros),
    );
    map.insert(
        "summary".into(),
        analytics_to_value(&analytics, &snapshot.config),
    );
    map.insert(
        "pacing".into(),
        budget_snapshot_pacing(snapshot, &analytics),
    );
    JsonValue::Object(map)
}

pub fn budget_snapshot_analytics(snapshot: &BudgetBrokerSnapshot) -> BudgetBrokerAnalytics {
    budget::compute_budget_analytics(snapshot)
}

pub fn budget_snapshot_pacing_delta(
    previous: &BudgetBrokerSnapshot,
    current: &BudgetBrokerSnapshot,
) -> BudgetBrokerPacingDelta {
    budget::budget_snapshot_pacing_delta(previous, current)
}

pub fn merge_budget_snapshots(
    base: &BudgetBrokerSnapshot,
    update: &BudgetBrokerSnapshot,
) -> BudgetBrokerSnapshot {
    budget::merge_budget_snapshots(base, update)
}

fn budget_snapshot_from_value(value: &JsonValue) -> Result<BudgetBrokerSnapshot, PersistenceError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("budget broker snapshot must be an object"))?;
    let config_value = map
        .get("config")
        .ok_or_else(|| invalid("missing budget broker config"))?;
    let campaigns_value = map
        .get("campaigns")
        .ok_or_else(|| invalid("missing campaigns"))?;
    let campaigns = match campaigns_value {
        JsonValue::Array(entries) => entries
            .iter()
            .map(campaign_budget_snapshot_from_value)
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(invalid("campaigns must be an array")),
    };
    let generated_at = map
        .get("generated_at_micros")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    Ok(BudgetBrokerSnapshot {
        generated_at_micros: generated_at,
        config: budget_broker_config_from_value(config_value)?,
        campaigns,
    })
}

fn analytics_to_value(analytics: &BudgetBrokerAnalytics, config: &BudgetBrokerConfig) -> JsonValue {
    let mut summary = JsonMap::new();
    summary.insert(
        "campaign_count".into(),
        JsonValue::from(analytics.campaign_count),
    );
    summary.insert(
        "cohort_count".into(),
        JsonValue::from(analytics.cohort_count),
    );
    summary.insert("mean_kappa".into(), JsonValue::from(analytics.mean_kappa));
    summary.insert("min_kappa".into(), JsonValue::from(analytics.min_kappa));
    summary.insert("max_kappa".into(), JsonValue::from(analytics.max_kappa));
    summary.insert(
        "mean_smoothed_error".into(),
        JsonValue::from(analytics.mean_smoothed_error),
    );
    summary.insert(
        "max_abs_smoothed_error".into(),
        JsonValue::from(analytics.max_abs_smoothed_error),
    );
    summary.insert(
        "realized_spend_total".into(),
        JsonValue::from(analytics.realized_spend_total),
    );
    summary.insert(
        "epoch_target_total".into(),
        JsonValue::from(analytics.epoch_target_total),
    );
    summary.insert(
        "epoch_spend_total".into(),
        JsonValue::from(analytics.epoch_spend_total),
    );
    summary.insert(
        "dual_price_max".into(),
        JsonValue::from(analytics.dual_price_max),
    );
    summary.insert("config_step_size".into(), JsonValue::from(config.step_size));
    summary.insert("config_max_kappa".into(), JsonValue::from(config.max_kappa));
    summary.insert("config_smoothing".into(), JsonValue::from(config.smoothing));
    JsonValue::Object(summary)
}

fn budget_snapshot_pacing(
    snapshot: &BudgetBrokerSnapshot,
    analytics: &BudgetBrokerAnalytics,
) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "step_size".into(),
        JsonValue::Number(number_from_f64(snapshot.config.step_size)),
    );
    map.insert(
        "max_kappa_config".into(),
        JsonValue::Number(number_from_f64(snapshot.config.max_kappa)),
    );
    map.insert(
        "smoothing".into(),
        JsonValue::Number(number_from_f64(snapshot.config.smoothing)),
    );
    map.insert(
        "epochs_per_budget".into(),
        JsonValue::Number(JsonNumber::from(snapshot.config.epochs_per_budget)),
    );
    map.insert(
        "campaign_count".into(),
        JsonValue::Number(JsonNumber::from(analytics.campaign_count)),
    );
    map.insert(
        "cohort_count".into(),
        JsonValue::Number(JsonNumber::from(analytics.cohort_count)),
    );
    map.insert(
        "mean_kappa".into(),
        JsonValue::Number(number_from_f64(analytics.mean_kappa)),
    );
    map.insert(
        "max_kappa_observed".into(),
        JsonValue::Number(number_from_f64(analytics.max_kappa)),
    );
    map.insert(
        "mean_smoothed_error".into(),
        JsonValue::Number(number_from_f64(analytics.mean_smoothed_error)),
    );
    map.insert(
        "max_abs_smoothed_error".into(),
        JsonValue::Number(number_from_f64(analytics.max_abs_smoothed_error)),
    );
    map.insert(
        "dual_price_max".into(),
        JsonValue::Number(number_from_f64(analytics.dual_price_max)),
    );
    JsonValue::Object(map)
}

fn distribution_policy_to_value(policy: &DistributionPolicy) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "viewer_percent".into(),
        JsonValue::Number(JsonNumber::from(policy.viewer_percent)),
    );
    map.insert(
        "host_percent".into(),
        JsonValue::Number(JsonNumber::from(policy.host_percent)),
    );
    map.insert(
        "hardware_percent".into(),
        JsonValue::Number(JsonNumber::from(policy.hardware_percent)),
    );
    map.insert(
        "verifier_percent".into(),
        JsonValue::Number(JsonNumber::from(policy.verifier_percent)),
    );
    map.insert(
        "liquidity_percent".into(),
        JsonValue::Number(JsonNumber::from(policy.liquidity_percent)),
    );
    map.insert(
        "liquidity_split_ct_ppm".into(),
        JsonValue::Number(JsonNumber::from(policy.liquidity_split_ct_ppm)),
    );
    map.insert(
        "dual_token_settlement_enabled".into(),
        JsonValue::Bool(policy.dual_token_settlement_enabled),
    );
    JsonValue::Object(map)
}

fn distribution_policy_from_value(
    value: &JsonValue,
) -> Result<DistributionPolicy, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("distribution policy must be an object"))?;
    let mut policy = DistributionPolicy::new(
        read_u64(obj, "viewer_percent")?,
        read_u64(obj, "host_percent")?,
        read_u64(obj, "hardware_percent")?,
        read_u64(obj, "verifier_percent")?,
        read_u64(obj, "liquidity_percent")?,
    );
    if let Some(_) = obj.get("liquidity_split_ct_ppm") {
        policy = policy.with_liquidity_split(read_u32(obj, "liquidity_split_ct_ppm")?);
    }
    if let Some(value) = obj.get("dual_token_settlement_enabled") {
        policy = policy.with_dual_token_settlement(value.as_bool().unwrap_or(false));
    }
    Ok(policy.normalize())
}
impl InMemoryMarketplace {
    pub fn new(config: MarketplaceConfig) -> Self {
        let normalized = config.normalized();
        Self {
            config: normalized.clone(),
            campaigns: RwLock::new(HashMap::new()),
            reservations: RwLock::new(HashMap::new()),
            consumed_reservations: RwLock::new(HashSet::new()),
            distribution: RwLock::new(normalized.distribution.normalize()),
            pricing: RwLock::new(HashMap::new()),
            oracle: RwLock::new(normalized.default_oracle),
            attestation: SelectionAttestationManager::new(normalized.attestation.clone()),
            budget_broker: RwLock::new(BudgetBroker::new(normalized.budget_broker.clone())),
            badge_guard: BadgeGuard::new(normalized.badge_guard.clone()),
            privacy_budget: RwLock::new(PrivacyBudgetManager::new(
                normalized.privacy_budget.clone(),
            )),
            uplift: RwLock::new(UpliftEstimator::new(normalized.uplift.clone())),
            token_remainders: RwLock::new(TokenRemainderLedger::default()),
        }
    }

    fn matches_targeting(&self, targeting: &CampaignTargeting, ctx: &ImpressionContext) -> bool {
        if !targeting.domains.is_empty() && !targeting.domains.iter().any(|d| d == &ctx.domain) {
            return false;
        }
        if !targeting.badges.is_empty() {
            let required = match self
                .badge_guard
                .evaluate(&targeting.badges, ctx.soft_intent.as_ref())
            {
                BadgeDecision::Blocked => return false,
                BadgeDecision::Allowed { required, .. } => {
                    if required.len() < targeting.badges.len() {
                        increment_counter!(
                            "ad_badge_relax_total",
                            "from" => targeting.badges.len() as i64,
                            "to" => required.len() as i64
                        );
                    }
                    required
                }
            };
            let ctx_badges: HashSet<&str> = ctx.badges.iter().map(String::as_str).collect();
            if required
                .iter()
                .any(|badge| !ctx_badges.contains(badge.as_str()))
            {
                return false;
            }
        }
        true
    }

    fn matches_creative(&self, creative: &Creative, ctx: &ImpressionContext) -> bool {
        if !creative.domains.is_empty() && !creative.domains.iter().any(|d| d == &ctx.domain) {
            return false;
        }
        if !creative.badges.is_empty() {
            let required = match self
                .badge_guard
                .evaluate(&creative.badges, ctx.soft_intent.as_ref())
            {
                BadgeDecision::Blocked => return false,
                BadgeDecision::Allowed { required, .. } => required,
            };
            let ctx_badges: HashSet<&str> = ctx.badges.iter().map(String::as_str).collect();
            if required
                .iter()
                .any(|badge| !ctx_badges.contains(badge.as_str()))
            {
                return false;
            }
        }
        true
    }

    fn persist_token_remainders(&self) -> Result<(), PersistenceError> {
        Ok(())
    }

    fn cohort_key(ctx: &ImpressionContext) -> CohortKey {
        CohortKey::new(ctx.domain.clone(), ctx.provider.clone(), ctx.badges.clone())
    }

    fn get_price_and_state<'a>(
        pricing: &'a mut HashMap<CohortKey, CohortPricingState>,
        key: &CohortKey,
        config: &MarketplaceConfig,
    ) -> &'a mut CohortPricingState {
        pricing.entry(key.clone()).or_insert_with(|| {
            let telemetry = CohortTelemetryId::from_key(key);
            CohortPricingState::new(config, telemetry)
        })
    }
}

impl Marketplace for InMemoryMarketplace {
    fn register_campaign(&self, campaign: Campaign) -> Result<(), MarketplaceError> {
        let mut guard = self.campaigns.write().unwrap();
        if guard.contains_key(&campaign.id) {
            return Err(MarketplaceError::DuplicateCampaign);
        }
        let campaign_id = campaign.id.clone();
        let budget_usd_micros = campaign.budget_usd_micros;
        let state = CampaignState {
            remaining_budget_usd_micros: campaign.budget_usd_micros,
            reserved_budget_usd_micros: 0,
            campaign,
        };
        guard.insert(campaign_id.clone(), state);
        let mut broker = self.budget_broker.write().unwrap();
        broker.ensure_registered(&campaign_id, budget_usd_micros);
        Ok(())
    }

    fn list_campaigns(&self) -> Vec<CampaignSummary> {
        let guard = self.campaigns.read().unwrap();
        guard
            .values()
            .map(|state| CampaignSummary {
                id: state.campaign.id.clone(),
                advertiser_account: state.campaign.advertiser_account.clone(),
                remaining_budget_usd_micros: state.remaining_budget_usd_micros,
                creatives: state
                    .campaign
                    .creatives
                    .iter()
                    .map(|c| c.id.clone())
                    .collect(),
                reserved_budget_usd_micros: state.reserved_budget_usd_micros,
            })
            .collect()
    }

    fn budget_broker(&self) -> &RwLock<BudgetBroker> {
        &self.budget_broker
    }

    fn reserve_impression(
        &self,
        key: ReservationKey,
        ctx: ImpressionContext,
    ) -> Option<MatchOutcome> {
        if self.consumed_reservations.read().unwrap().contains(&key) {
            eprintln!("reservation key {:?} already consumed", key);
            return None;
        }
        self.badge_guard
            .record(&ctx.badges, ctx.population_estimate);
        {
            let mut budgets = self.privacy_budget.write().unwrap();
            match budgets.authorize(&ctx.badges, ctx.population_estimate) {
                PrivacyBudgetDecision::Allowed => {}
                PrivacyBudgetDecision::Cooling { .. } | PrivacyBudgetDecision::Denied { .. } => {
                    return None;
                }
            }
        }
        let cohort = InMemoryMarketplace::cohort_key(&ctx);
        let price_per_mib = {
            let mut pricing = self.pricing.write().unwrap();
            InMemoryMarketplace::get_price_and_state(&mut pricing, &cohort, &self.config)
                .price_per_mib_usd_micros()
        };
        let floor_breakdown = self.config.composite_floor_breakdown(
            price_per_mib,
            ctx.bytes,
            &cohort,
            ctx.population_estimate,
        );
        let resource_floor = floor_breakdown.total_usd_micros();
        if resource_floor == 0 {
            eprintln!(
                "resource floor zero domain {} bytes {} price {}",
                ctx.domain, ctx.bytes, price_per_mib
            );
            return None;
        }
        record_resource_floor_metrics(&ctx, &floor_breakdown, resource_floor);

        struct Candidate {
            trace: SelectionCandidateTrace,
            uplift: UpliftEstimate,
        }

        let campaigns = self.campaigns.read().unwrap();
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut best_index: Option<usize> = None;
        for state in campaigns.values() {
            if !self.matches_targeting(&state.campaign.targeting, &ctx) {
                continue;
            }
            let available_budget = state.remaining_budget_usd_micros;
            if available_budget < resource_floor {
                continue;
            }
            for creative in &state.campaign.creatives {
                if !self.matches_creative(creative, &ctx) {
                    continue;
                }
                let uplift_estimate = {
                    let estimator = self.uplift.read().unwrap();
                    estimator.estimate(&state.campaign.id, &creative.id)
                };
                let quality = creative.quality_adjusted_bid_with_lift(
                    &self.config,
                    available_budget,
                    Some(uplift_estimate.lift_ppm),
                );
                let guidance = {
                    let mut broker = self.budget_broker.write().unwrap();
                    broker.guidance_for(&state.campaign.id, &cohort)
                };
                let (quality, shading) = quality.shade(guidance, available_budget);
                if quality.base_bid_usd_micros < resource_floor
                    || quality.quality_adjusted_usd_micros < resource_floor
                {
                    continue;
                }
                let trace = SelectionCandidateTrace {
                    campaign_id: state.campaign.id.clone(),
                    creative_id: creative.id.clone(),
                    base_bid_usd_micros: quality.base_bid_usd_micros,
                    quality_adjusted_bid_usd_micros: quality.quality_adjusted_usd_micros,
                    available_budget_usd_micros: available_budget,
                    action_rate_ppm: creative.action_rate_ppm,
                    lift_ppm: creative.effective_lift_ppm(),
                    quality_multiplier: quality.quality_multiplier,
                    pacing_kappa: shading.applied_multiplier,
                    requested_kappa: shading.requested_kappa,
                    shading_multiplier: shading.applied_multiplier,
                    shadow_price: shading.shadow_price,
                    dual_price: shading.dual_price,
                    predicted_lift_ppm: uplift_estimate.lift_ppm,
                    baseline_action_rate_ppm: uplift_estimate.baseline_action_rate_ppm,
                    predicted_propensity: uplift_estimate.propensity,
                    uplift_sample_size: uplift_estimate.sample_size,
                    uplift_ece: uplift_estimate.ece,
                };
                let idx = candidates.len();
                if let Some(best) = best_index {
                    let best_trace = &candidates[best].trace;
                    if trace.quality_adjusted_bid_usd_micros
                        > best_trace.quality_adjusted_bid_usd_micros
                        || (trace.quality_adjusted_bid_usd_micros
                            == best_trace.quality_adjusted_bid_usd_micros
                            && trace.available_budget_usd_micros
                                > best_trace.available_budget_usd_micros)
                    {
                        best_index = Some(idx);
                    }
                } else {
                    best_index = Some(idx);
                }
                candidates.push(Candidate {
                    trace,
                    uplift: uplift_estimate,
                });
            }
        }
        drop(campaigns);

        let Some(winner_index) = best_index else {
            eprintln!("no candidates found for domain {}", ctx.domain);
            return None;
        };
        let runner_up_quality = candidates
            .iter()
            .enumerate()
            .filter(|(idx, _)| *idx != winner_index)
            .map(|(_, candidate)| candidate.trace.quality_adjusted_bid_usd_micros)
            .max()
            .unwrap_or(0);
        let receipt_candidates: Vec<SelectionCandidateTrace> = candidates
            .iter()
            .map(|candidate| candidate.trace.clone())
            .collect();
        let winner_trace = receipt_candidates[winner_index].clone();
        let winner_uplift = candidates[winner_index].uplift.clone();
        let clearing_price = resource_floor
            .max(runner_up_quality)
            .min(winner_trace.quality_adjusted_bid_usd_micros);
        if clearing_price == 0 {
            return None;
        }
        let (soft_intent_receipt, soft_intent_snapshot) =
            soft_intent_artifacts(&ctx.badges, &ctx.soft_intent);
        let mut receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: ctx.domain.clone(),
                provider: ctx.provider.clone(),
                badges: ctx.badges.clone(),
                bytes: ctx.bytes,
                price_per_mib_usd_micros: price_per_mib,
            },
            candidates: receipt_candidates,
            winner_index,
            resource_floor_usd_micros: resource_floor,
            resource_floor_breakdown: floor_breakdown.clone(),
            runner_up_quality_bid_usd_micros: runner_up_quality,
            clearing_price_usd_micros: clearing_price,
            attestation: None,
            proof_metadata: None,
            verifier_committee: ctx.verifier_committee.clone(),
            verifier_stake_snapshot: ctx.verifier_stake_snapshot.clone(),
            verifier_transcript: ctx.verifier_transcript.clone(),
            badge_soft_intent: soft_intent_receipt,
            badge_soft_intent_snapshot: soft_intent_snapshot,
        };
        let (attestation, satisfaction, metadata) = self
            .attestation
            .attach_attestation(&receipt, &ctx.attestations);
        if matches!(satisfaction, AttestationSatisfaction::Missing)
            && self.attestation.config().require_attestation
        {
            return None;
        }
        receipt.attestation = attestation;
        receipt.proof_metadata = metadata;
        if let Err(err) = self.attestation.validate_receipt(&receipt) {
            eprintln!("selection attestation validation failed: {err}");
            return None;
        }

        let mut reservations = self.reservations.write().unwrap();
        if reservations.contains_key(&key) {
            eprintln!("duplicate reservation key {key:?}");
            return None;
        }
        let mut campaigns = self.campaigns.write().unwrap();
        let Some(state) = campaigns.get_mut(&winner_trace.campaign_id) else {
            return None;
        };
        if state.remaining_budget_usd_micros < clearing_price {
            return None;
        }
        state.remaining_budget_usd_micros -= clearing_price;
        state.reserved_budget_usd_micros = state
            .reserved_budget_usd_micros
            .saturating_add(clearing_price);
        reservations.insert(
            key,
            ReservationState {
                campaign_id: winner_trace.campaign_id.clone(),
                creative_id: winner_trace.creative_id.clone(),
                bytes: ctx.bytes,
                price_per_mib_usd_micros: price_per_mib,
                total_usd_micros: clearing_price,
                demand_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
                resource_floor_usd_micros: resource_floor,
                resource_floor_breakdown: floor_breakdown.clone(),
                runner_up_quality_bid_usd_micros: runner_up_quality,
                quality_adjusted_bid_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
                cohort,
                selection_receipt: receipt.clone(),
                uplift: winner_uplift.clone(),
            },
        );
        Some(MatchOutcome {
            campaign_id: winner_trace.campaign_id,
            creative_id: winner_trace.creative_id,
            price_per_mib_usd_micros: price_per_mib,
            total_usd_micros: clearing_price,
            resource_floor_usd_micros: resource_floor,
            resource_floor_breakdown: floor_breakdown,
            runner_up_quality_bid_usd_micros: runner_up_quality,
            quality_adjusted_bid_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
            selection_receipt: receipt,
            uplift: winner_uplift,
        })
    }

    fn commit(&self, key: &ReservationKey) -> Option<SettlementBreakdown> {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)?
        };
        let mut campaigns = self.campaigns.write().unwrap();
        let state = campaigns.get_mut(&reservation.campaign_id)?;
        if state.reserved_budget_usd_micros < reservation.total_usd_micros {
            return None;
        }
        state.reserved_budget_usd_micros = state
            .reserved_budget_usd_micros
            .saturating_sub(reservation.total_usd_micros);
        {
            let mut consumed = self.consumed_reservations.write().unwrap();
            consumed.insert(*key);
        }
        drop(campaigns);
        {
            let mut pricing = self.pricing.write().unwrap();
            if let Some(state) = pricing.get_mut(&reservation.cohort) {
                state.record(reservation.demand_usd_micros, reservation.total_usd_micros);
            }
        }
        let policy = *self.distribution.read().unwrap();
        let oracle = *self.oracle.read().unwrap();
        let parts = allocate_usd(reservation.total_usd_micros, policy);
        let tokens = {
            let mut ledger = self.token_remainders.write().unwrap();
            convert_parts_to_tokens(parts, oracle, &mut ledger)
        };
        {
            let mut broker = self.budget_broker.write().unwrap();
            broker.record_reservation(
                &reservation.campaign_id,
                &reservation.cohort,
                reservation.total_usd_micros,
            );
        }
        if let Err(err) = self.persist_token_remainders() {
            eprintln!("failed to persist token remainders: {err}");
        }
        Some(SettlementBreakdown {
            campaign_id: reservation.campaign_id,
            creative_id: reservation.creative_id,
            bytes: reservation.bytes,
            price_per_mib_usd_micros: reservation.price_per_mib_usd_micros,
            total_usd_micros: reservation.total_usd_micros,
            demand_usd_micros: reservation.demand_usd_micros,
            resource_floor_usd_micros: reservation.resource_floor_usd_micros,
            resource_floor_breakdown: reservation.resource_floor_breakdown.clone(),
            runner_up_quality_bid_usd_micros: reservation.runner_up_quality_bid_usd_micros,
            quality_adjusted_bid_usd_micros: reservation.quality_adjusted_bid_usd_micros,
            viewer_ct: tokens.viewer_ct,
            host_ct: tokens.host_ct,
            hardware_ct: tokens.hardware_ct,
            verifier_ct: tokens.verifier_ct,
            host_it: tokens.host_it,
            hardware_it: tokens.hardware_it,
            verifier_it: tokens.verifier_it,
            liquidity_ct: tokens.liquidity_ct,
            liquidity_it: tokens.liquidity_it,
            miner_ct: tokens.miner_ct,
            miner_it: tokens.miner_it,
            total_ct: tokens.total_ct,
            unsettled_usd_micros: tokens.unsettled_usd_micros,
            ct_price_usd_micros: oracle.ct_price_usd_micros,
            it_price_usd_micros: oracle.it_price_usd_micros,
            ct_remainders_usd_micros: tokens.remainders.ct.clone(),
            it_remainders_usd_micros: tokens.remainders.it.clone(),
            ct_twap_window_id: oracle.ct_twap_window_id,
            it_twap_window_id: oracle.it_twap_window_id,
            selection_receipt: reservation.selection_receipt,
            uplift: reservation.uplift,
        })
    }

    fn cancel(&self, key: &ReservationKey) {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)
        };
        if let Some(res) = reservation {
            let mut campaigns = self.campaigns.write().unwrap();
            if let Some(state) = campaigns.get_mut(&res.campaign_id) {
                state.remaining_budget_usd_micros = state
                    .remaining_budget_usd_micros
                    .saturating_add(res.total_usd_micros);
                state.reserved_budget_usd_micros = state
                    .reserved_budget_usd_micros
                    .saturating_sub(res.total_usd_micros);
            }
        }
    }

    fn distribution(&self) -> DistributionPolicy {
        *self.distribution.read().unwrap()
    }

    fn update_distribution(&self, policy: DistributionPolicy) {
        let mut guard = self.distribution.write().unwrap();
        *guard = policy.normalize();
    }

    fn update_oracle(&self, oracle: TokenOracle) {
        let mut guard = self.oracle.write().unwrap();
        *guard = oracle;
    }

    fn oracle(&self) -> TokenOracle {
        *self.oracle.read().unwrap()
    }

    fn cohort_prices(&self) -> Vec<CohortPriceSnapshot> {
        let pricing = self.pricing.read().unwrap();
        pricing
            .iter()
            .map(|(key, state)| CohortPriceSnapshot {
                domain: key.domain.clone(),
                provider: key.provider.clone(),
                badges: key.badges.clone(),
                price_per_mib_usd_micros: state.price_per_mib_usd_micros(),
                target_utilization_ppm: state.target_utilization_ppm,
                observed_utilization_ppm: state.observed_utilization_ppm(),
            })
            .collect()
    }
}
impl SledMarketplace {
    pub fn open<P: AsRef<Path>>(
        path: P,
        config: MarketplaceConfig,
    ) -> Result<Self, PersistenceError> {
        let normalized = config.normalized();
        let db = SledConfig::new().path(path).open()?;
        let campaigns_tree = db.open_tree(TREE_CAMPAIGNS)?;
        let metadata_tree = db.open_tree(TREE_METADATA)?;
        let distribution = match metadata_tree.get(KEY_DISTRIBUTION)? {
            Some(bytes) => {
                let mut policy = deserialize_distribution(&bytes)?;
                policy = policy.normalize();
                policy
            }
            None => {
                let normalized_policy = normalized.distribution.normalize();
                let payload = serialize_distribution(&normalized_policy)?;
                metadata_tree.insert(KEY_DISTRIBUTION, payload)?;
                metadata_tree.flush()?;
                normalized_policy
            }
        };
        let mut campaigns = HashMap::new();
        for entry in campaigns_tree.iter() {
            let (_key, value) = entry?;
            let state = deserialize_campaign_state(&value)?;
            campaigns.insert(state.campaign.id.clone(), state);
        }
        let broker_config = normalized.budget_broker.clone();
        let mut broker_needs_snapshot = false;
        let mut broker = if let Some(bytes) = metadata_tree.get(KEY_BUDGET)? {
            let snapshot = deserialize_budget_snapshot(&bytes)?;
            BudgetBroker::restore(broker_config.clone(), &snapshot)
        } else {
            broker_needs_snapshot = true;
            BudgetBroker::new(broker_config.clone())
        };
        for state in campaigns.values() {
            broker.ensure_registered(&state.campaign.id, state.campaign.budget_usd_micros);
        }
        if broker_needs_snapshot {
            let snapshot = broker.snapshot();
            let bytes = serialize_budget_snapshot(&snapshot)?;
            metadata_tree.insert(KEY_BUDGET, bytes)?;
            metadata_tree.flush()?;
        }
        let badge_guard = BadgeGuard::new(normalized.badge_guard.clone());
        let attestation = SelectionAttestationManager::new(normalized.attestation.clone());
        let token_remainders = if let Some(bytes) = metadata_tree.get(KEY_TOKEN_REMAINDERS)? {
            deserialize_token_remainders(&bytes)?
        } else {
            let ledger = TokenRemainderLedger::default();
            let bytes = serialize_token_remainders(&ledger)?;
            metadata_tree.insert(KEY_TOKEN_REMAINDERS, bytes)?;
            metadata_tree.flush()?;
            ledger
        };

        Ok(Self {
            _db: db,
            campaigns_tree,
            metadata_tree,
            config: normalized.clone(),
            campaigns: RwLock::new(campaigns),
            reservations: RwLock::new(HashMap::new()),
            consumed_reservations: RwLock::new(HashSet::new()),
            distribution: RwLock::new(distribution),
            pricing: RwLock::new(HashMap::new()),
            oracle: RwLock::new(normalized.default_oracle),
            attestation,
            budget_broker: RwLock::new(broker),
            badge_guard,
            privacy_budget: RwLock::new(PrivacyBudgetManager::new(
                normalized.privacy_budget.clone(),
            )),
            uplift: RwLock::new(UpliftEstimator::new(normalized.uplift.clone())),
            token_remainders: RwLock::new(token_remainders),
        })
    }

    fn persist_campaign(&self, state: &CampaignState) -> Result<(), PersistenceError> {
        let bytes = serialize_campaign_state(state)?;
        self.campaigns_tree
            .insert(state.campaign.id.as_bytes(), bytes)?;
        self.campaigns_tree.flush()?;
        Ok(())
    }

    fn persist_distribution(&self, policy: &DistributionPolicy) -> Result<(), PersistenceError> {
        let bytes = serialize_distribution(policy)?;
        self.metadata_tree.insert(KEY_DISTRIBUTION, bytes)?;
        self.metadata_tree.flush()?;
        Ok(())
    }

    fn persist_budget_broker(&self) -> Result<(), PersistenceError> {
        let snapshot = {
            let guard = self.budget_broker.read().unwrap();
            guard.snapshot()
        };
        let bytes = serialize_budget_snapshot(&snapshot)?;
        self.metadata_tree.insert(KEY_BUDGET, bytes)?;
        self.metadata_tree.flush()?;
        Ok(())
    }

    fn persist_token_remainders(&self) -> Result<(), PersistenceError> {
        let ledger = {
            let guard = self.token_remainders.read().unwrap();
            guard.clone()
        };
        let bytes = serialize_token_remainders(&ledger)?;
        self.metadata_tree.insert(KEY_TOKEN_REMAINDERS, bytes)?;
        self.metadata_tree.flush()?;
        Ok(())
    }

    fn matches_targeting(&self, targeting: &CampaignTargeting, ctx: &ImpressionContext) -> bool {
        if !targeting.domains.is_empty() && !targeting.domains.iter().any(|d| d == &ctx.domain) {
            return false;
        }
        if !targeting.badges.is_empty() {
            let required = match self
                .badge_guard
                .evaluate(&targeting.badges, ctx.soft_intent.as_ref())
            {
                BadgeDecision::Blocked => return false,
                BadgeDecision::Allowed { required, .. } => {
                    if required.len() < targeting.badges.len() {
                        increment_counter!(
                            "ad_badge_relax_total",
                            "from" => targeting.badges.len() as i64,
                            "to" => required.len() as i64
                        );
                    }
                    required
                }
            };
            let ctx_badges: HashSet<&str> = ctx.badges.iter().map(String::as_str).collect();
            if required
                .iter()
                .any(|badge| !ctx_badges.contains(badge.as_str()))
            {
                return false;
            }
        }
        true
    }

    fn matches_creative(&self, creative: &Creative, ctx: &ImpressionContext) -> bool {
        if !creative.domains.is_empty() && !creative.domains.iter().any(|d| d == &ctx.domain) {
            return false;
        }
        if !creative.badges.is_empty() {
            let required = match self
                .badge_guard
                .evaluate(&creative.badges, ctx.soft_intent.as_ref())
            {
                BadgeDecision::Blocked => return false,
                BadgeDecision::Allowed { required, .. } => required,
            };
            let ctx_badges: HashSet<&str> = ctx.badges.iter().map(String::as_str).collect();
            if required
                .iter()
                .any(|badge| !ctx_badges.contains(badge.as_str()))
            {
                return false;
            }
        }
        true
    }
}

impl Marketplace for SledMarketplace {
    fn register_campaign(&self, campaign: Campaign) -> Result<(), MarketplaceError> {
        let mut guard = self.campaigns.write().unwrap();
        if guard.contains_key(&campaign.id) {
            return Err(MarketplaceError::DuplicateCampaign);
        }
        let state = CampaignState {
            remaining_budget_usd_micros: campaign.budget_usd_micros,
            reserved_budget_usd_micros: 0,
            campaign,
        };
        guard.insert(state.campaign.id.clone(), state.clone());
        {
            let mut broker = self.budget_broker.write().unwrap();
            broker.ensure_registered(&state.campaign.id, state.campaign.budget_usd_micros);
        }
        drop(guard);
        if let Err(err) = self.persist_campaign(&state) {
            let mut guard = self.campaigns.write().unwrap();
            guard.remove(&state.campaign.id);
            self.budget_broker
                .write()
                .unwrap()
                .remove_campaign(&state.campaign.id);
            return Err(err.into());
        }
        if let Err(err) = self.persist_budget_broker() {
            let mut guard = self.campaigns.write().unwrap();
            guard.remove(&state.campaign.id);
            self.budget_broker
                .write()
                .unwrap()
                .remove_campaign(&state.campaign.id);
            let _ = self.campaigns_tree.remove(state.campaign.id.as_bytes());
            let _ = self.campaigns_tree.flush();
            return Err(err.into());
        }
        Ok(())
    }

    fn list_campaigns(&self) -> Vec<CampaignSummary> {
        let guard = self.campaigns.read().unwrap();
        guard
            .values()
            .map(|state| CampaignSummary {
                id: state.campaign.id.clone(),
                advertiser_account: state.campaign.advertiser_account.clone(),
                remaining_budget_usd_micros: state.remaining_budget_usd_micros,
                creatives: state
                    .campaign
                    .creatives
                    .iter()
                    .map(|c| c.id.clone())
                    .collect(),
                reserved_budget_usd_micros: state.reserved_budget_usd_micros,
            })
            .collect()
    }

    fn reserve_impression(
        &self,
        key: ReservationKey,
        ctx: ImpressionContext,
    ) -> Option<MatchOutcome> {
        if self.consumed_reservations.read().unwrap().contains(&key) {
            return None;
        }
        self.badge_guard
            .record(&ctx.badges, ctx.population_estimate);
        {
            let mut budgets = self.privacy_budget.write().unwrap();
            match budgets.authorize(&ctx.badges, ctx.population_estimate) {
                PrivacyBudgetDecision::Allowed => {}
                PrivacyBudgetDecision::Cooling { .. } | PrivacyBudgetDecision::Denied { .. } => {
                    return None;
                }
            }
        }
        let cohort = InMemoryMarketplace::cohort_key(&ctx);
        let price_per_mib = {
            let mut pricing = self.pricing.write().unwrap();
            InMemoryMarketplace::get_price_and_state(&mut pricing, &cohort, &self.config)
                .price_per_mib_usd_micros()
        };
        let floor_breakdown = self.config.composite_floor_breakdown(
            price_per_mib,
            ctx.bytes,
            &cohort,
            ctx.population_estimate,
        );
        let resource_floor = floor_breakdown.total_usd_micros();
        if resource_floor == 0 {
            return None;
        }
        record_resource_floor_metrics(&ctx, &floor_breakdown, resource_floor);

        struct Candidate {
            trace: SelectionCandidateTrace,
            uplift: UpliftEstimate,
        }

        let campaigns = self.campaigns.read().unwrap();
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut best_index: Option<usize> = None;
        for state in campaigns.values() {
            if !self.matches_targeting(&state.campaign.targeting, &ctx) {
                continue;
            }
            let available_budget = state.remaining_budget_usd_micros;
            if available_budget < resource_floor {
                continue;
            }
            for creative in &state.campaign.creatives {
                if !self.matches_creative(creative, &ctx) {
                    continue;
                }
                let uplift_estimate = {
                    let estimator = self.uplift.read().unwrap();
                    estimator.estimate(&state.campaign.id, &creative.id)
                };
                let quality = creative.quality_adjusted_bid_with_lift(
                    &self.config,
                    available_budget,
                    Some(uplift_estimate.lift_ppm),
                );
                let guidance = {
                    let mut broker = self.budget_broker.write().unwrap();
                    broker.guidance_for(&state.campaign.id, &cohort)
                };
                let (quality, shading) = quality.shade(guidance, available_budget);
                if quality.base_bid_usd_micros < resource_floor
                    || quality.quality_adjusted_usd_micros < resource_floor
                {
                    continue;
                }
                let trace = SelectionCandidateTrace {
                    campaign_id: state.campaign.id.clone(),
                    creative_id: creative.id.clone(),
                    base_bid_usd_micros: quality.base_bid_usd_micros,
                    quality_adjusted_bid_usd_micros: quality.quality_adjusted_usd_micros,
                    available_budget_usd_micros: available_budget,
                    action_rate_ppm: creative.action_rate_ppm,
                    lift_ppm: creative.effective_lift_ppm(),
                    quality_multiplier: quality.quality_multiplier,
                    pacing_kappa: shading.applied_multiplier,
                    requested_kappa: shading.requested_kappa,
                    shading_multiplier: shading.applied_multiplier,
                    shadow_price: shading.shadow_price,
                    dual_price: shading.dual_price,
                    predicted_lift_ppm: uplift_estimate.lift_ppm,
                    baseline_action_rate_ppm: uplift_estimate.baseline_action_rate_ppm,
                    predicted_propensity: uplift_estimate.propensity,
                    uplift_sample_size: uplift_estimate.sample_size,
                    uplift_ece: uplift_estimate.ece,
                };
                let idx = candidates.len();
                if let Some(best) = best_index {
                    let best_trace = &candidates[best].trace;
                    if trace.quality_adjusted_bid_usd_micros
                        > best_trace.quality_adjusted_bid_usd_micros
                        || (trace.quality_adjusted_bid_usd_micros
                            == best_trace.quality_adjusted_bid_usd_micros
                            && trace.available_budget_usd_micros
                                > best_trace.available_budget_usd_micros)
                    {
                        best_index = Some(idx);
                    }
                } else {
                    best_index = Some(idx);
                }
                candidates.push(Candidate {
                    trace,
                    uplift: uplift_estimate,
                });
            }
        }
        drop(campaigns);

        let Some(winner_index) = best_index else {
            return None;
        };
        let runner_up_quality = candidates
            .iter()
            .enumerate()
            .filter(|(idx, _)| *idx != winner_index)
            .map(|(_, candidate)| candidate.trace.quality_adjusted_bid_usd_micros)
            .max()
            .unwrap_or(0);
        let receipt_candidates: Vec<SelectionCandidateTrace> = candidates
            .iter()
            .map(|candidate| candidate.trace.clone())
            .collect();
        let winner_trace = receipt_candidates[winner_index].clone();
        let winner_uplift = candidates[winner_index].uplift.clone();
        let clearing_price = resource_floor
            .max(runner_up_quality)
            .min(winner_trace.quality_adjusted_bid_usd_micros)
            .max(resource_floor);
        if clearing_price == 0 {
            return None;
        }
        let (soft_intent_receipt, soft_intent_snapshot) =
            soft_intent_artifacts(&ctx.badges, &ctx.soft_intent);
        let mut receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: ctx.domain.clone(),
                provider: ctx.provider.clone(),
                badges: ctx.badges.clone(),
                bytes: ctx.bytes,
                price_per_mib_usd_micros: price_per_mib,
            },
            candidates: receipt_candidates,
            winner_index,
            resource_floor_usd_micros: resource_floor,
            resource_floor_breakdown: floor_breakdown.clone(),
            runner_up_quality_bid_usd_micros: runner_up_quality,
            clearing_price_usd_micros: clearing_price,
            attestation: None,
            proof_metadata: None,
            verifier_committee: ctx.verifier_committee.clone(),
            verifier_stake_snapshot: ctx.verifier_stake_snapshot.clone(),
            verifier_transcript: ctx.verifier_transcript.clone(),
            badge_soft_intent: soft_intent_receipt,
            badge_soft_intent_snapshot: soft_intent_snapshot,
        };
        let (attestation, satisfaction, metadata) = self
            .attestation
            .attach_attestation(&receipt, &ctx.attestations);
        if matches!(satisfaction, AttestationSatisfaction::Missing) {
            return None;
        }
        receipt.attestation = attestation;
        receipt.proof_metadata = metadata;
        if self.attestation.validate_receipt(&receipt).is_err() {
            return None;
        }

        let mut reservations = self.reservations.write().unwrap();
        if reservations.contains_key(&key) {
            return None;
        }
        let mut campaigns = self.campaigns.write().unwrap();
        let Some(state) = campaigns.get_mut(&winner_trace.campaign_id) else {
            return None;
        };
        if state.remaining_budget_usd_micros < clearing_price {
            return None;
        }
        let prev_state = state.clone();
        state.remaining_budget_usd_micros -= clearing_price;
        state.reserved_budget_usd_micros = state
            .reserved_budget_usd_micros
            .saturating_add(clearing_price);
        if let Err(err) = self.persist_campaign(state) {
            *state = prev_state;
            eprintln!(
                "failed to persist campaign {} after reservation: {err}",
                state.campaign.id
            );
            return None;
        }
        reservations.insert(
            key,
            ReservationState {
                campaign_id: winner_trace.campaign_id.clone(),
                creative_id: winner_trace.creative_id.clone(),
                bytes: ctx.bytes,
                price_per_mib_usd_micros: price_per_mib,
                total_usd_micros: clearing_price,
                demand_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
                resource_floor_usd_micros: resource_floor,
                resource_floor_breakdown: floor_breakdown.clone(),
                runner_up_quality_bid_usd_micros: runner_up_quality,
                quality_adjusted_bid_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
                cohort,
                selection_receipt: receipt.clone(),
                uplift: winner_uplift.clone(),
            },
        );
        Some(MatchOutcome {
            campaign_id: winner_trace.campaign_id,
            creative_id: winner_trace.creative_id,
            price_per_mib_usd_micros: price_per_mib,
            total_usd_micros: clearing_price,
            resource_floor_usd_micros: resource_floor,
            resource_floor_breakdown: floor_breakdown,
            runner_up_quality_bid_usd_micros: runner_up_quality,
            quality_adjusted_bid_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
            selection_receipt: receipt,
            uplift: winner_uplift,
        })
    }

    fn commit(&self, key: &ReservationKey) -> Option<SettlementBreakdown> {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)?
        };
        let mut campaigns = self.campaigns.write().unwrap();
        let state = campaigns.get_mut(&reservation.campaign_id)?;
        if state.reserved_budget_usd_micros < reservation.total_usd_micros {
            return None;
        }
        state.reserved_budget_usd_micros = state
            .reserved_budget_usd_micros
            .saturating_sub(reservation.total_usd_micros);
        {
            let mut consumed = self.consumed_reservations.write().unwrap();
            consumed.insert(*key);
        }
        let snapshot = state.clone();
        drop(campaigns);
        if let Err(err) = self.persist_campaign(&snapshot) {
            let mut campaigns = self.campaigns.write().unwrap();
            if let Some(state) = campaigns.get_mut(&snapshot.campaign.id) {
                state.reserved_budget_usd_micros = state
                    .reserved_budget_usd_micros
                    .saturating_add(reservation.total_usd_micros);
            }
            eprintln!(
                "failed to persist campaign {} after commit: {err}",
                snapshot.campaign.id
            );
            return None;
        }
        {
            let mut pricing = self.pricing.write().unwrap();
            if let Some(state) = pricing.get_mut(&reservation.cohort) {
                state.record(reservation.demand_usd_micros, reservation.total_usd_micros);
            }
        }
        let policy = *self.distribution.read().unwrap();
        let oracle = *self.oracle.read().unwrap();
        let parts = allocate_usd(reservation.total_usd_micros, policy);
        let tokens = {
            let mut ledger = self.token_remainders.write().unwrap();
            convert_parts_to_tokens(parts, oracle, &mut ledger)
        };
        {
            let mut broker = self.budget_broker.write().unwrap();
            broker.record_reservation(
                &reservation.campaign_id,
                &reservation.cohort,
                reservation.total_usd_micros,
            );
        }
        if let Err(err) = self.persist_token_remainders() {
            eprintln!("failed to persist token remainders: {err}");
        }
        Some(SettlementBreakdown {
            campaign_id: reservation.campaign_id,
            creative_id: reservation.creative_id,
            bytes: reservation.bytes,
            price_per_mib_usd_micros: reservation.price_per_mib_usd_micros,
            total_usd_micros: reservation.total_usd_micros,
            demand_usd_micros: reservation.demand_usd_micros,
            resource_floor_usd_micros: reservation.resource_floor_usd_micros,
            resource_floor_breakdown: reservation.resource_floor_breakdown.clone(),
            runner_up_quality_bid_usd_micros: reservation.runner_up_quality_bid_usd_micros,
            quality_adjusted_bid_usd_micros: reservation.quality_adjusted_bid_usd_micros,
            viewer_ct: tokens.viewer_ct,
            host_ct: tokens.host_ct,
            hardware_ct: tokens.hardware_ct,
            verifier_ct: tokens.verifier_ct,
            host_it: tokens.host_it,
            hardware_it: tokens.hardware_it,
            verifier_it: tokens.verifier_it,
            liquidity_ct: tokens.liquidity_ct,
            liquidity_it: tokens.liquidity_it,
            miner_ct: tokens.miner_ct,
            miner_it: tokens.miner_it,
            total_ct: tokens.total_ct,
            unsettled_usd_micros: tokens.unsettled_usd_micros,
            ct_price_usd_micros: oracle.ct_price_usd_micros,
            it_price_usd_micros: oracle.it_price_usd_micros,
            ct_remainders_usd_micros: tokens.remainders.ct.clone(),
            it_remainders_usd_micros: tokens.remainders.it.clone(),
            ct_twap_window_id: oracle.ct_twap_window_id,
            it_twap_window_id: oracle.it_twap_window_id,
            selection_receipt: reservation.selection_receipt,
            uplift: reservation.uplift,
        })
    }

    fn cancel(&self, key: &ReservationKey) {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)
        };
        if let Some(res) = reservation {
            let mut campaigns = self.campaigns.write().unwrap();
            if let Some(state) = campaigns.get_mut(&res.campaign_id) {
                state.remaining_budget_usd_micros = state
                    .remaining_budget_usd_micros
                    .saturating_add(res.total_usd_micros);
                state.reserved_budget_usd_micros = state
                    .reserved_budget_usd_micros
                    .saturating_sub(res.total_usd_micros);
                let snapshot = state.clone();
                drop(campaigns);
                if let Err(err) = self.persist_campaign(&snapshot) {
                    eprintln!("failed to persist campaign after cancel: {err}");
                }
                return;
            }
        }
    }

    fn distribution(&self) -> DistributionPolicy {
        *self.distribution.read().unwrap()
    }

    fn update_distribution(&self, policy: DistributionPolicy) {
        if let Err(err) = self.persist_distribution(&policy.normalize()) {
            eprintln!("failed to persist ad_market distribution: {err}");
            return;
        }
        let mut guard = self.distribution.write().unwrap();
        *guard = policy.normalize();
    }

    fn update_oracle(&self, oracle: TokenOracle) {
        let mut guard = self.oracle.write().unwrap();
        *guard = oracle;
    }

    fn oracle(&self) -> TokenOracle {
        *self.oracle.read().unwrap()
    }

    fn cohort_prices(&self) -> Vec<CohortPriceSnapshot> {
        let pricing = self.pricing.read().unwrap();
        pricing
            .iter()
            .map(|(key, state)| CohortPriceSnapshot {
                domain: key.domain.clone(),
                provider: key.provider.clone(),
                badges: key.badges.clone(),
                price_per_mib_usd_micros: state.price_per_mib_usd_micros(),
                target_utilization_ppm: state.target_utilization_ppm,
                observed_utilization_ppm: state.observed_utilization_ppm(),
            })
            .collect()
    }

    fn budget_broker(&self) -> &RwLock<BudgetBroker> {
        &self.budget_broker
    }
}
struct RoleUsdParts {
    viewer: u64,
    host: u64,
    hardware: u64,
    verifier: u64,
    liquidity_ct: u64,
    liquidity_it: u64,
    remainder: u64,
    dual_token_enabled: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CtRemainderBreakdown {
    viewer_usd_micros: u64,
    host_usd_micros: u64,
    hardware_usd_micros: u64,
    verifier_usd_micros: u64,
    liquidity_usd_micros: u64,
    miner_usd_micros: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItRemainderBreakdown {
    host_usd_micros: u64,
    hardware_usd_micros: u64,
    verifier_usd_micros: u64,
    liquidity_usd_micros: u64,
    miner_usd_micros: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenRemaindersSnapshot {
    ct: CtRemainderBreakdown,
    it: ItRemainderBreakdown,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct TokenRemainderLedger {
    ct_viewer_usd: u64,
    ct_host_usd: u64,
    ct_hardware_usd: u64,
    ct_verifier_usd: u64,
    ct_liquidity_usd: u64,
    ct_miner_usd: u64,
    it_host_usd: u64,
    it_hardware_usd: u64,
    it_verifier_usd: u64,
    it_liquidity_usd: u64,
    it_miner_usd: u64,
}

impl TokenRemainderLedger {
    fn convert(&mut self, parts: RoleUsdParts, oracle: TokenOracle) -> TokenizedPayouts {
        let (viewer_ct, _) = convert_role(
            parts.viewer,
            &mut self.ct_viewer_usd,
            oracle.ct_price_usd_micros,
        );
        let (host_ct, _) = convert_role(
            parts.host,
            &mut self.ct_host_usd,
            oracle.ct_price_usd_micros,
        );
        let (hardware_ct, _) = convert_role(
            parts.hardware,
            &mut self.ct_hardware_usd,
            oracle.ct_price_usd_micros,
        );
        let (verifier_ct, _) = convert_role(
            parts.verifier,
            &mut self.ct_verifier_usd,
            oracle.ct_price_usd_micros,
        );
        let (liquidity_ct, _) = convert_role(
            parts.liquidity_ct,
            &mut self.ct_liquidity_usd,
            oracle.ct_price_usd_micros,
        );
        let (miner_ct, _) = convert_role(
            parts.remainder,
            &mut self.ct_miner_usd,
            oracle.ct_price_usd_micros,
        );

        let total_ct = viewer_ct
            .saturating_add(host_ct)
            .saturating_add(hardware_ct)
            .saturating_add(verifier_ct)
            .saturating_add(liquidity_ct)
            .saturating_add(miner_ct);

        let (host_it, hardware_it, verifier_it, liquidity_it, miner_it) = if parts
            .dual_token_enabled
        {
            let (host_it, _) = convert_role(
                parts.host,
                &mut self.it_host_usd,
                oracle.it_price_usd_micros,
            );
            let (hardware_it, _) = convert_role(
                parts.hardware,
                &mut self.it_hardware_usd,
                oracle.it_price_usd_micros,
            );
            let (verifier_it, _) = convert_role(
                parts.verifier,
                &mut self.it_verifier_usd,
                oracle.it_price_usd_micros,
            );
            let (liquidity_it, _) = convert_role(
                parts.liquidity_it,
                &mut self.it_liquidity_usd,
                oracle.it_price_usd_micros,
            );
            let (miner_it, _) = convert_role(0, &mut self.it_miner_usd, oracle.it_price_usd_micros);
            (host_it, hardware_it, verifier_it, liquidity_it, miner_it)
        } else {
            (0, 0, 0, 0, 0)
        };

        let snapshot = self.snapshot();
        let unsettled_usd_micros = self.total_remainder_usd();

        TokenizedPayouts {
            viewer_ct,
            host_ct,
            hardware_ct,
            verifier_ct,
            liquidity_ct,
            miner_ct,
            total_ct,
            host_it,
            hardware_it,
            verifier_it,
            liquidity_it,
            miner_it,
            unsettled_usd_micros,
            remainders: snapshot,
        }
    }

    fn snapshot(&self) -> TokenRemaindersSnapshot {
        TokenRemaindersSnapshot {
            ct: CtRemainderBreakdown {
                viewer_usd_micros: self.ct_viewer_usd,
                host_usd_micros: self.ct_host_usd,
                hardware_usd_micros: self.ct_hardware_usd,
                verifier_usd_micros: self.ct_verifier_usd,
                liquidity_usd_micros: self.ct_liquidity_usd,
                miner_usd_micros: self.ct_miner_usd,
            },
            it: ItRemainderBreakdown {
                host_usd_micros: self.it_host_usd,
                hardware_usd_micros: self.it_hardware_usd,
                verifier_usd_micros: self.it_verifier_usd,
                liquidity_usd_micros: self.it_liquidity_usd,
                miner_usd_micros: self.it_miner_usd,
            },
        }
    }

    fn total_remainder_usd(&self) -> u64 {
        self.ct_viewer_usd
            .saturating_add(self.ct_host_usd)
            .saturating_add(self.ct_hardware_usd)
            .saturating_add(self.ct_verifier_usd)
            .saturating_add(self.ct_liquidity_usd)
            .saturating_add(self.ct_miner_usd)
            .saturating_add(self.it_host_usd)
            .saturating_add(self.it_hardware_usd)
            .saturating_add(self.it_verifier_usd)
            .saturating_add(self.it_liquidity_usd)
            .saturating_add(self.it_miner_usd)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenizedPayouts {
    viewer_ct: u64,
    host_ct: u64,
    hardware_ct: u64,
    verifier_ct: u64,
    liquidity_ct: u64,
    miner_ct: u64,
    total_ct: u64,
    host_it: u64,
    hardware_it: u64,
    verifier_it: u64,
    liquidity_it: u64,
    miner_it: u64,
    unsettled_usd_micros: u64,
    remainders: TokenRemaindersSnapshot,
}

fn allocate_usd(total_usd_micros: u64, distribution: DistributionPolicy) -> RoleUsdParts {
    if total_usd_micros == 0 {
        return RoleUsdParts {
            viewer: 0,
            host: 0,
            hardware: 0,
            verifier: 0,
            liquidity_ct: 0,
            liquidity_it: 0,
            remainder: 0,
            dual_token_enabled: distribution.dual_token_settlement_enabled,
        };
    }
    let weights = [
        (0usize, distribution.viewer_percent),
        (1, distribution.host_percent),
        (2, distribution.hardware_percent),
        (3, distribution.verifier_percent),
        (4, distribution.liquidity_percent),
    ];
    let allocations = distribute_scalar(total_usd_micros, &weights);
    let liquidity_total = allocations.get(4).copied().unwrap_or(0);
    let (liquidity_ct, liquidity_it) = split_liquidity_usd(liquidity_total, distribution);
    let distributed_sum = allocations.iter().copied().sum::<u64>();
    RoleUsdParts {
        viewer: allocations.get(0).copied().unwrap_or(0),
        host: allocations.get(1).copied().unwrap_or(0),
        hardware: allocations.get(2).copied().unwrap_or(0),
        verifier: allocations.get(3).copied().unwrap_or(0),
        liquidity_ct,
        liquidity_it,
        remainder: total_usd_micros.saturating_sub(distributed_sum),
        dual_token_enabled: distribution.dual_token_settlement_enabled,
    }
}

fn split_liquidity_usd(total_liquidity_usd: u64, policy: DistributionPolicy) -> (u64, u64) {
    if total_liquidity_usd == 0 {
        return (0, 0);
    }
    if !policy.dual_token_settlement_enabled {
        return (total_liquidity_usd, 0);
    }
    let ct_usd = (u128::from(total_liquidity_usd) * u128::from(policy.liquidity_split_ct_ppm))
        / u128::from(PPM_SCALE);
    let ct_usd = ct_usd as u64;
    let it_usd = total_liquidity_usd.saturating_sub(ct_usd);
    (ct_usd, it_usd)
}

fn convert_parts_to_tokens(
    parts: RoleUsdParts,
    oracle: TokenOracle,
    remainders: &mut TokenRemainderLedger,
) -> TokenizedPayouts {
    remainders.convert(parts, oracle)
}

fn usd_to_tokens(amount_usd_micros: u64, price_usd_micros: u64) -> (u64, u64) {
    if price_usd_micros == 0 {
        return (0, amount_usd_micros);
    }
    let tokens = amount_usd_micros / price_usd_micros;
    let remainder = amount_usd_micros.saturating_sub(tokens.saturating_mul(price_usd_micros));
    (tokens, remainder)
}

fn convert_role(amount: u64, remainder_store: &mut u64, price_usd_micros: u64) -> (u64, u64) {
    let total = amount.saturating_add(*remainder_store);
    let (tokens, remainder) = usd_to_tokens(total, price_usd_micros);
    *remainder_store = remainder;
    (tokens, remainder)
}

fn record_resource_floor_metrics(
    context: &ImpressionContext,
    breakdown: &ResourceFloorBreakdown,
    total_usd_micros: u64,
) {
    let provider_label = context.provider.as_deref().unwrap_or("-");
    gauge!(
        "ad_resource_floor_component_usd",
        total_usd_micros as f64,
        "component" => "total",
        "domain" => context.domain.as_str(),
        "provider" => provider_label
    );
    gauge!(
        "ad_resource_floor_component_usd",
        breakdown.bandwidth_usd_micros as f64,
        "component" => "bandwidth",
        "domain" => context.domain.as_str(),
        "provider" => provider_label
    );
    gauge!(
        "ad_resource_floor_component_usd",
        breakdown.verifier_usd_micros as f64,
        "component" => "verifier",
        "domain" => context.domain.as_str(),
        "provider" => provider_label
    );
    gauge!(
        "ad_resource_floor_component_usd",
        breakdown.host_usd_micros as f64,
        "component" => "host",
        "domain" => context.domain.as_str(),
        "provider" => provider_label
    );
    gauge!(
        "ad_resource_floor_impressions_per_proof",
        breakdown.qualified_impressions_per_proof as f64,
        "domain" => context.domain.as_str(),
        "provider" => provider_label
    );
}

fn usd_cost_for_bytes(price_per_mib_usd_micros: u64, bytes: u64) -> u64 {
    if price_per_mib_usd_micros == 0 || bytes == 0 {
        return 0;
    }
    let numerator = price_per_mib_usd_micros.saturating_mul(bytes);
    (numerator + BYTES_PER_MIB - 1) / BYTES_PER_MIB
}

fn distribute_scalar(total: u64, weights: &[(usize, u64)]) -> Vec<u64> {
    if total == 0 || weights.is_empty() {
        return vec![0; weights.len()];
    }
    let sum: u128 = weights.iter().map(|(_, w)| u128::from(*w)).sum();
    if sum == 0 {
        return vec![0; weights.len()];
    }
    let mut allocations = vec![0u64; weights.len()];
    let mut distributed = 0u64;
    let mut remainders: Vec<(usize, usize, u64)> = Vec::with_capacity(weights.len());
    for (idx, (order, weight)) in weights.iter().enumerate() {
        if *weight == 0 {
            remainders.push((idx, *order, 0));
            continue;
        }
        let numerator = u128::from(total) * u128::from(*weight);
        let base = (numerator / sum) as u64;
        let remainder = (numerator % sum) as u64;
        allocations[idx] = base;
        distributed = distributed.saturating_add(base);
        remainders.push((idx, *order, remainder));
    }
    let mut remainder_tokens = total.saturating_sub(distributed);
    if remainder_tokens > 0 {
        remainders.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.0.cmp(&b.0))
        });
        for (idx, _, _) in &remainders {
            if remainder_tokens == 0 {
                break;
            }
            allocations[*idx] = allocations[*idx].saturating_add(1);
            remainder_tokens -= 1;
        }
        if remainder_tokens > 0 && !allocations.is_empty() {
            allocations[0] = allocations[0].saturating_add(remainder_tokens);
        }
    }
    allocations
}

pub type MarketplaceHandle = Arc<dyn Marketplace>;
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    use sys::tempfile::TempDir;

    const SNARK_CIRCUIT_ID: &str = "selection_argmax_v1";

    fn encode_bytes(bytes: &[u8]) -> String {
        let mut encoded = String::from("[");
        for (idx, byte) in bytes.iter().enumerate() {
            if idx > 0 {
                encoded.push(',');
            }
            use std::fmt::Write;
            write!(&mut encoded, "{}", byte).expect("write byte");
        }
        encoded.push(']');
        encoded
    }

    fn build_attested_receipt() -> (SelectionReceipt, SelectionProofMetadata) {
        let mut receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: "example.test".into(),
                provider: Some("wallet".into()),
                badges: vec!["badge-a".into(), "badge-b".into()],
                bytes: BYTES_PER_MIB,
                price_per_mib_usd_micros: 120,
            },
            candidates: vec![
                SelectionCandidateTrace {
                    campaign_id: "cmp-snark".into(),
                    creative_id: "creative-1".into(),
                    base_bid_usd_micros: 170,
                    quality_adjusted_bid_usd_micros: 180,
                    available_budget_usd_micros: 5_000,
                    action_rate_ppm: 0,
                    lift_ppm: 0,
                    quality_multiplier: 1.0,
                    pacing_kappa: 1.0,
                    requested_kappa: 1.0,
                    shading_multiplier: 1.0,
                    shadow_price: 0.0,
                    dual_price: 0.0,
                    ..SelectionCandidateTrace::default()
                },
                SelectionCandidateTrace {
                    campaign_id: "cmp-runner".into(),
                    creative_id: "creative-2".into(),
                    base_bid_usd_micros: 140,
                    quality_adjusted_bid_usd_micros: 140,
                    available_budget_usd_micros: 5_000,
                    action_rate_ppm: 0,
                    lift_ppm: 0,
                    quality_multiplier: 1.0,
                    pacing_kappa: 1.0,
                    requested_kappa: 1.0,
                    shading_multiplier: 1.0,
                    shadow_price: 0.0,
                    dual_price: 0.0,
                    ..SelectionCandidateTrace::default()
                },
            ],
            winner_index: 0,
            resource_floor_usd_micros: 100,
            resource_floor_breakdown: ResourceFloorBreakdown {
                bandwidth_usd_micros: 80,
                verifier_usd_micros: 15,
                host_usd_micros: 10,
                qualified_impressions_per_proof: 320,
            },
            runner_up_quality_bid_usd_micros: 140,
            clearing_price_usd_micros: 140,
            attestation: None,
            proof_metadata: None,
            verifier_committee: None,
            verifier_stake_snapshot: None,
            verifier_transcript: Vec::new(),
            badge_soft_intent: None,
            badge_soft_intent_snapshot: None,
        };
        let commitment = receipt
            .commitment_bytes_raw()
            .expect("commitment bytes available");
        let inputs = SelectionProofPublicInputs {
            commitment: commitment.to_vec(),
            winner_index: receipt.winner_index as u16,
            winner_quality_bid_usd_micros: receipt.candidates[0].quality_adjusted_bid_usd_micros,
            runner_up_quality_bid_usd_micros: receipt.runner_up_quality_bid_usd_micros,
            resource_floor_usd_micros: receipt.resource_floor_usd_micros,
            clearing_price_usd_micros: receipt.clearing_price_usd_micros,
            candidate_count: receipt.candidates.len() as u16,
        };
        let proof_bytes = vec![0xBC; 128];
        let proof_bytes_digest = selection::proof_bytes_digest(&proof_bytes);
        let transcript = selection::expected_transcript_digest(
            SNARK_CIRCUIT_ID,
            1,
            &proof_bytes_digest,
            &inputs,
        )
        .expect("transcript digest");
        let commitments_json = format!(
            "[{},{}]",
            encode_bytes(&[0x44; 32]),
            encode_bytes(&[0x77; 32])
        );
        let public_inputs_json = format!(
            "{{\"commitment\":{},\"winner_index\":{},\"winner_quality_bid_usd_micros\":{},\"runner_up_quality_bid_usd_micros\":{},\"resource_floor_usd_micros\":{},\"clearing_price_usd_micros\":{},\"candidate_count\":{}}}",
            encode_bytes(&inputs.commitment),
            inputs.winner_index,
            inputs.winner_quality_bid_usd_micros,
            inputs.runner_up_quality_bid_usd_micros,
            inputs.resource_floor_usd_micros,
            inputs.clearing_price_usd_micros,
            inputs.candidate_count,
        );
        let proof_payload = format!(
            "{{\"version\":1,\"circuit_revision\":1,\"public_inputs\":{},\"proof\":{{\"protocol\":\"groth16\",\"transcript_digest\":{},\"bytes\":{},\"witness_commitments\":{}}}}}",
            public_inputs_json,
            encode_bytes(&transcript),
            encode_bytes(&proof_bytes),
            commitments_json,
        )
        .into_bytes();
        let verification = selection::verify_selection_proof(
            SNARK_CIRCUIT_ID,
            &proof_payload,
            &receipt.commitment_digest().expect("commitment digest"),
        )
        .expect("proof verifies");
        let metadata =
            SelectionProofMetadata::from_verification(SNARK_CIRCUIT_ID.to_string(), verification);
        receipt.attestation = Some(SelectionAttestation::Snark {
            proof: proof_payload,
            circuit_id: SNARK_CIRCUIT_ID.to_string(),
        });
        receipt.proof_metadata = Some(metadata.clone());
        (receipt, metadata)
    }

    fn sample_campaign(id: &str, budget_usd_micros: u64) -> Campaign {
        Campaign {
            id: id.to_string(),
            advertiser_account: "adv".to_string(),
            budget_usd_micros,
            creatives: vec![Creative {
                id: format!("creative-{id}"),
                action_rate_ppm: 500_000,
                margin_ppm: 700_000,
                value_per_action_usd_micros: 5 * MICROS_PER_DOLLAR,
                max_cpi_usd_micros: None,
                lift_ppm: 550_000,
                badges: Vec::new(),
                domains: vec!["example.test".to_string()],
                metadata: HashMap::new(),
            }],
            targeting: CampaignTargeting {
                domains: vec!["example.test".to_string()],
                badges: Vec::new(),
            },
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn in_memory_reserve_and_commit() {
        let mut config = MarketplaceConfig::default();
        config.distribution = config.distribution.with_dual_token_settlement(true);
        let market = InMemoryMarketplace::new(config);
        market
            .register_campaign(sample_campaign("cmp", 5 * MICROS_PER_DOLLAR))
            .expect("campaign registered");
        let key = ReservationKey {
            manifest: [1u8; 32],
            path_hash: [2u8; 32],
            discriminator: [3u8; 32],
        };
        let ctx = ImpressionContext {
            domain: "example.test".to_string(),
            provider: Some("provider".to_string()),
            badges: Vec::new(),
            bytes: BYTES_PER_MIB,
            attestations: Vec::new(),
            population_estimate: Some(1_000),
            ..ImpressionContext::default()
        };
        market.update_oracle(TokenOracle::new(50_000, 25_000));
        let outcome = market
            .reserve_impression(key, ctx.clone())
            .expect("reservation succeeded");
        assert!(outcome.total_usd_micros > 0);
        assert!(outcome.resource_floor_usd_micros > 0);
        assert_eq!(
            outcome.total_usd_micros,
            outcome.selection_receipt.clearing_price_usd_micros
        );
        assert!(
            outcome.selection_receipt.clearing_price_usd_micros
                >= outcome.resource_floor_usd_micros
        );
        let settlement = market.commit(&key).expect("commit succeeds");
        assert_eq!(settlement.bytes, BYTES_PER_MIB);
        assert_eq!(settlement.total_usd_micros, outcome.total_usd_micros);
        assert!(settlement.viewer_ct > 0);
        assert!(settlement.host_ct > 0);
        assert!(settlement.hardware_ct > 0);
        assert!(settlement.verifier_ct > 0);
        let policy = market.distribution();
        let parts = allocate_usd(settlement.total_usd_micros, policy);
        let expected_liquidity_ct_usd = parts.liquidity_ct;
        let expected_liquidity_it_usd = parts.liquidity_it;
        let (expected_liquidity_ct, _) =
            usd_to_tokens(expected_liquidity_ct_usd, settlement.ct_price_usd_micros);
        let (expected_liquidity_it, _) =
            usd_to_tokens(expected_liquidity_it_usd, settlement.it_price_usd_micros);
        assert_eq!(settlement.liquidity_ct, expected_liquidity_ct);
        assert_eq!(settlement.liquidity_it, expected_liquidity_it);
        assert!(settlement.liquidity_it > 0);
        assert_eq!(
            settlement.total_ct,
            settlement
                .viewer_ct
                .saturating_add(settlement.host_ct)
                .saturating_add(settlement.hardware_ct)
                .saturating_add(settlement.verifier_ct)
                .saturating_add(settlement.liquidity_ct)
                .saturating_add(settlement.miner_ct)
        );
        assert!(settlement.host_it > 0);
        assert!(settlement.hardware_it > 0);
        assert!(settlement.verifier_it > 0);
        assert!(settlement.ct_price_usd_micros > 0);
        assert!(settlement.it_price_usd_micros > 0);
        assert!(settlement.unsettled_usd_micros < settlement.ct_price_usd_micros);
        assert!(
            settlement
                .total_ct
                .saturating_mul(settlement.ct_price_usd_micros)
                <= settlement.total_usd_micros
        );
        assert!(settlement.unsettled_usd_micros < settlement.total_usd_micros);
        let summary = market.list_campaigns();
        assert_eq!(
            summary[0].remaining_budget_usd_micros,
            5 * MICROS_PER_DOLLAR - settlement.total_usd_micros
        );
        assert_eq!(summary[0].reserved_budget_usd_micros, 0);
    }

    #[test]
    fn liquidity_split_controls_dual_token_liquidity() {
        fn settle_with_split(split_ct_ppm: u32, seed: u8) -> SettlementBreakdown {
            let mut config = MarketplaceConfig::default();
            config.distribution = config
                .distribution
                .with_dual_token_settlement(true)
                .with_liquidity_split(split_ct_ppm);
            config.default_oracle = TokenOracle::new(10_000, 20_000);
            config.default_price_per_mib_usd_micros = 800_000;
            let market = InMemoryMarketplace::new(config);
            market
                .register_campaign(sample_campaign("cmp", 20 * MICROS_PER_DOLLAR))
                .expect("campaign registered");
            let key = ReservationKey {
                manifest: [seed; 32],
                path_hash: [seed.wrapping_add(1); 32],
                discriminator: [seed.wrapping_add(2); 32],
            };
            let ctx = ImpressionContext {
                domain: "example.test".to_string(),
                provider: Some("provider".to_string()),
                badges: Vec::new(),
                bytes: BYTES_PER_MIB,
                attestations: Vec::new(),
                population_estimate: Some(1_000),
                ..ImpressionContext::default()
            };
            market
                .reserve_impression(key, ctx)
                .expect("reservation succeeded");
            market.commit(&key).expect("settlement produced")
        }

        let all_it = settle_with_split(0, 7);
        assert_eq!(all_it.liquidity_ct, 0);
        assert!(all_it.liquidity_it > 0);

        let all_ct = settle_with_split(PPM_SCALE as u32, 13);
        assert!(all_ct.liquidity_ct > 0);
        assert_eq!(all_ct.liquidity_it, 0);

        let half_split = settle_with_split((PPM_SCALE / 2) as u32, 23);
        assert!(half_split.liquidity_ct > 0);
        assert!(half_split.liquidity_it > 0);
        let liquidity_value_ct = half_split
            .liquidity_ct
            .saturating_mul(half_split.ct_price_usd_micros);
        let liquidity_value_it = half_split
            .liquidity_it
            .saturating_mul(half_split.it_price_usd_micros);
        let total_liquidity_value = liquidity_value_ct + liquidity_value_it;
        assert!(total_liquidity_value > 0);
        assert!(
            (liquidity_value_ct as i128 - liquidity_value_it as i128).abs()
                <= total_liquidity_value as i128
        );
    }

    #[test]
    fn dual_token_flag_prevents_it_conversions_when_disabled() {
        let policy =
            DistributionPolicy::new(40, 30, 15, 10, 5).with_liquidity_split((PPM_SCALE / 2) as u32);
        let oracle = TokenOracle::new(100_000, 200_000);

        let disabled = policy.with_dual_token_settlement(false);
        let disabled_parts = allocate_usd(10_000_000, disabled);
        assert_eq!(disabled_parts.liquidity_it, 0);
        let mut disabled_ledger = TokenRemainderLedger::default();
        let disabled_tokens = convert_parts_to_tokens(disabled_parts, oracle, &mut disabled_ledger);
        assert_eq!(disabled_tokens.host_it, 0);
        assert_eq!(disabled_tokens.hardware_it, 0);
        assert_eq!(disabled_tokens.verifier_it, 0);
        assert_eq!(disabled_tokens.liquidity_it, 0);
        assert_eq!(disabled_tokens.miner_it, 0);

        let enabled = policy.with_dual_token_settlement(true);
        let enabled_parts = allocate_usd(10_000_000, enabled);
        assert!(enabled_parts.liquidity_it > 0);
        let mut enabled_ledger = TokenRemainderLedger::default();
        let enabled_tokens = convert_parts_to_tokens(enabled_parts, oracle, &mut enabled_ledger);
        assert!(enabled_tokens.host_it > 0);
        assert!(enabled_tokens.hardware_it > 0);
        assert!(enabled_tokens.verifier_it > 0);
        assert!(enabled_tokens.liquidity_it > 0);
        // Miner payouts may remain CT-denominated depending on remainder rounding, so we
        // only assert on the roles that must mint IT when dual-token settlement is enabled.
    }

    #[test]
    fn liquidity_split_rounding_does_not_double_count() {
        let mut policy = DistributionPolicy::new(0, 0, 0, 0, 100);
        policy = policy
            .with_dual_token_settlement(true)
            .with_liquidity_split((PPM_SCALE / 3) as u32);
        let total_usd = 1_000_003u64;
        let parts = allocate_usd(total_usd, policy);
        assert_eq!(parts.viewer, 0);
        assert_eq!(parts.host, 0);
        assert_eq!(parts.hardware, 0);
        assert_eq!(parts.verifier, 0);
        assert!(parts.liquidity_ct > 0);
        assert!(parts.liquidity_it > 0);

        let liquidity_ct_usd = parts.liquidity_ct;
        let liquidity_it_usd = parts.liquidity_it;
        let oracle = TokenOracle::new(73, 41);
        let mut ledger = TokenRemainderLedger::default();
        let tokens = convert_parts_to_tokens(parts, oracle, &mut ledger);

        assert_eq!(tokens.viewer_ct, 0);
        assert_eq!(tokens.host_ct, 0);
        assert_eq!(tokens.hardware_ct, 0);
        assert_eq!(tokens.verifier_ct, 0);
        assert_eq!(tokens.host_it, 0);
        assert_eq!(tokens.hardware_it, 0);
        assert_eq!(tokens.verifier_it, 0);

        let (expected_liquidity_ct, expected_ct_rem) =
            usd_to_tokens(liquidity_ct_usd, oracle.ct_price_usd_micros);
        let (expected_liquidity_it, expected_it_rem) =
            usd_to_tokens(liquidity_it_usd, oracle.it_price_usd_micros);

        assert_eq!(tokens.liquidity_ct, expected_liquidity_ct);
        assert_eq!(tokens.liquidity_it, expected_liquidity_it);

        let accounted_ct_usd = tokens
            .liquidity_ct
            .saturating_mul(oracle.ct_price_usd_micros)
            .saturating_add(expected_ct_rem);
        assert_eq!(accounted_ct_usd, liquidity_ct_usd);

        let accounted_it_usd = tokens
            .liquidity_it
            .saturating_mul(oracle.it_price_usd_micros)
            .saturating_add(expected_it_rem);
        assert_eq!(accounted_it_usd, liquidity_it_usd);

        let ct_value = tokens.total_ct.saturating_mul(oracle.ct_price_usd_micros);
        let it_value = tokens
            .liquidity_it
            .saturating_mul(oracle.it_price_usd_micros)
            .saturating_add(tokens.miner_it.saturating_mul(oracle.it_price_usd_micros));
        let settled_value = ct_value.saturating_add(it_value);
        assert!(settled_value <= total_usd);
        assert!(total_usd - settled_value <= oracle.it_price_usd_micros);
    }

    #[test]
    fn cancel_releases_pending_budget() {
        let market = InMemoryMarketplace::new(MarketplaceConfig::default());
        market
            .register_campaign(sample_campaign("cmp", 2 * MICROS_PER_DOLLAR))
            .expect("campaign registered");
        let ctx = ImpressionContext {
            domain: "example.test".to_string(),
            provider: None,
            badges: Vec::new(),
            bytes: BYTES_PER_MIB,
            attestations: Vec::new(),
            population_estimate: Some(1_000),
            ..ImpressionContext::default()
        };
        let key = ReservationKey {
            manifest: [9u8; 32],
            path_hash: [8u8; 32],
            discriminator: [7u8; 32],
        };
        let outcome = market.reserve_impression(key, ctx).expect("reserved");
        let summary_reserved = market.list_campaigns();
        assert_eq!(
            summary_reserved[0].remaining_budget_usd_micros,
            2 * MICROS_PER_DOLLAR - outcome.total_usd_micros
        );
        assert_eq!(
            summary_reserved[0].reserved_budget_usd_micros,
            outcome.total_usd_micros
        );
        market.cancel(&key);
        let summary = market.list_campaigns();
        assert_eq!(
            summary[0].remaining_budget_usd_micros,
            2 * MICROS_PER_DOLLAR
        );
        assert_eq!(summary[0].reserved_budget_usd_micros, 0);
    }

    #[test]
    fn sled_persists_campaigns() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("ad_market.db");
        let market = SledMarketplace::open(path.clone(), MarketplaceConfig::default())
            .expect("market opened");
        market
            .register_campaign(sample_campaign("cmp", MICROS_PER_DOLLAR))
            .expect("registered");
        drop(market);
        let reopened = SledMarketplace::open(path, MarketplaceConfig::default()).expect("reopen");
        let campaigns = reopened.list_campaigns();
        assert_eq!(campaigns.len(), 1);
        assert_eq!(campaigns[0].remaining_budget_usd_micros, MICROS_PER_DOLLAR);
    }

    #[test]
    fn adaptive_pricing_increases_under_excess_demand() {
        let market = InMemoryMarketplace::new(MarketplaceConfig {
            default_price_per_mib_usd_micros: 50_000,
            smoothing_ppm: 500_000,
            price_eta_p_ppm: 220_000,
            price_eta_i_ppm: 9_000,
            price_forgetting_ppm: 930_000,
            ..MarketplaceConfig::default()
        });
        market
            .register_campaign(sample_campaign("cmp", 20 * MICROS_PER_DOLLAR))
            .expect("registered");
        let ctx = ImpressionContext {
            domain: "example.test".to_string(),
            provider: None,
            badges: Vec::new(),
            bytes: BYTES_PER_MIB,
            attestations: Vec::new(),
            population_estimate: Some(1_000),
            ..ImpressionContext::default()
        };
        let mut last_price = 0;
        for idx in 0..10 {
            let key = ReservationKey {
                manifest: [idx; 32],
                path_hash: [idx.wrapping_add(1); 32],
                discriminator: [idx.wrapping_add(2); 32],
            };
            let outcome = market
                .reserve_impression(key, ctx.clone())
                .expect("reserved");
            last_price = outcome.price_per_mib_usd_micros;
            market.commit(&key).expect("commit");
        }
        let snapshots = market.cohort_prices();
        assert_eq!(snapshots.len(), 1);
        assert!(snapshots[0].price_per_mib_usd_micros >= last_price);
    }

    #[test]
    fn reservations_are_atomic() {
        let market = Arc::new(InMemoryMarketplace::new(MarketplaceConfig::default()));
        market
            .register_campaign(sample_campaign("cmp", 5 * MICROS_PER_DOLLAR))
            .expect("registered");
        let barrier = Arc::new(Barrier::new(4));
        let successes = Arc::new(AtomicUsize::new(0));
        let ctx = ImpressionContext {
            domain: "example.test".to_string(),
            provider: Some("provider".to_string()),
            badges: Vec::new(),
            bytes: BYTES_PER_MIB,
            attestations: Vec::new(),
            population_estimate: Some(1_000),
            ..ImpressionContext::default()
        };
        let handles: Vec<_> = (0u8..4u8)
            .map(|idx| {
                let market = Arc::clone(&market);
                let barrier = Arc::clone(&barrier);
                let successes = Arc::clone(&successes);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let key = ReservationKey {
                        manifest: [idx; 32],
                        path_hash: [idx.wrapping_add(1); 32],
                        discriminator: [idx.wrapping_add(2); 32],
                    };
                    barrier.wait();
                    if market.reserve_impression(key, ctx.clone()).is_some() {
                        successes.fetch_add(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("thread join");
        }
        assert!(successes.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn selection_receipt_validation_passes_for_consistent_receipt() {
        let receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: "example.com".into(),
                provider: Some("edge".into()),
                badges: vec!["a".into()],
                bytes: 1_024,
                price_per_mib_usd_micros: 120,
            },
            candidates: vec![
                SelectionCandidateTrace {
                    campaign_id: "cmp-1".into(),
                    creative_id: "creative-1".into(),
                    base_bid_usd_micros: 170,
                    quality_adjusted_bid_usd_micros: 180,
                    available_budget_usd_micros: 5_000,
                    action_rate_ppm: 0,
                    lift_ppm: 0,
                    quality_multiplier: 1.0,
                    pacing_kappa: 1.0,
                    requested_kappa: 1.0,
                    shading_multiplier: 1.0,
                    shadow_price: 0.0,
                    dual_price: 0.0,
                    ..SelectionCandidateTrace::default()
                },
                SelectionCandidateTrace {
                    campaign_id: "cmp-2".into(),
                    creative_id: "creative-2".into(),
                    base_bid_usd_micros: 110,
                    quality_adjusted_bid_usd_micros: 110,
                    available_budget_usd_micros: 5_000,
                    action_rate_ppm: 0,
                    lift_ppm: 0,
                    quality_multiplier: 1.0,
                    pacing_kappa: 1.0,
                    requested_kappa: 1.0,
                    shading_multiplier: 1.0,
                    shadow_price: 0.0,
                    dual_price: 0.0,
                    ..SelectionCandidateTrace::default()
                },
            ],
            winner_index: 0,
            resource_floor_usd_micros: 100,
            resource_floor_breakdown: ResourceFloorBreakdown {
                bandwidth_usd_micros: 75,
                verifier_usd_micros: 15,
                host_usd_micros: 12,
                qualified_impressions_per_proof: 400,
            },
            runner_up_quality_bid_usd_micros: 110,
            clearing_price_usd_micros: 110,
            attestation: None,
            proof_metadata: None,
            verifier_committee: None,
            verifier_stake_snapshot: None,
            verifier_transcript: Vec::new(),
            badge_soft_intent: None,
            badge_soft_intent_snapshot: None,
        };
        let insights = receipt.validate().expect("receipt valid");
        assert_eq!(insights.winner_index, 0);
        assert_eq!(insights.runner_up_quality_bid_usd_micros, 110);
        assert_eq!(insights.clearing_price_usd_micros, 110);
        assert_eq!(insights.attestation_kind, SelectionAttestationKind::Missing);
    }

    #[test]
    fn selection_receipt_validation_rejects_runner_up_mismatch() {
        let receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: "example.com".into(),
                provider: None,
                badges: Vec::new(),
                bytes: 512,
                price_per_mib_usd_micros: 90,
            },
            candidates: vec![
                SelectionCandidateTrace {
                    campaign_id: "cmp-1".into(),
                    creative_id: "creative-1".into(),
                    base_bid_usd_micros: 150,
                    quality_adjusted_bid_usd_micros: 160,
                    available_budget_usd_micros: 2_000,
                    action_rate_ppm: 0,
                    lift_ppm: 0,
                    quality_multiplier: 1.0,
                    pacing_kappa: 1.0,
                    requested_kappa: 1.0,
                    shading_multiplier: 1.0,
                    shadow_price: 0.0,
                    dual_price: 0.0,
                    ..SelectionCandidateTrace::default()
                },
                SelectionCandidateTrace {
                    campaign_id: "cmp-2".into(),
                    creative_id: "creative-2".into(),
                    base_bid_usd_micros: 120,
                    quality_adjusted_bid_usd_micros: 120,
                    available_budget_usd_micros: 2_000,
                    action_rate_ppm: 0,
                    lift_ppm: 0,
                    quality_multiplier: 1.0,
                    pacing_kappa: 1.0,
                    requested_kappa: 1.0,
                    shading_multiplier: 1.0,
                    shadow_price: 0.0,
                    dual_price: 0.0,
                    ..SelectionCandidateTrace::default()
                },
            ],
            winner_index: 0,
            resource_floor_usd_micros: 90,
            resource_floor_breakdown: ResourceFloorBreakdown {
                bandwidth_usd_micros: 60,
                verifier_usd_micros: 20,
                host_usd_micros: 15,
                qualified_impressions_per_proof: 250,
            },
            runner_up_quality_bid_usd_micros: 90,
            clearing_price_usd_micros: 120,
            attestation: None,
            proof_metadata: None,
            verifier_committee: None,
            verifier_stake_snapshot: None,
            verifier_transcript: Vec::new(),
            badge_soft_intent: None,
            badge_soft_intent_snapshot: None,
        };
        let err = receipt.validate().expect_err("receipt invalid");
        assert!(matches!(
            err,
            SelectionReceiptError::RunnerUpMismatch {
                declared: 90,
                computed: 120
            }
        ));
    }

    #[test]
    fn selection_receipt_validation_rejects_breakdown_underflow() {
        let receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: "example.com".into(),
                provider: Some("edge".into()),
                badges: vec!["badge".into()],
                bytes: 256,
                price_per_mib_usd_micros: 80,
            },
            candidates: vec![SelectionCandidateTrace {
                campaign_id: "cmp".into(),
                creative_id: "creative".into(),
                base_bid_usd_micros: 90,
                quality_adjusted_bid_usd_micros: 120,
                available_budget_usd_micros: 1_000,
                action_rate_ppm: 0,
                lift_ppm: 0,
                quality_multiplier: 1.0,
                pacing_kappa: 1.0,
                ..SelectionCandidateTrace::default()
            }],
            winner_index: 0,
            resource_floor_usd_micros: 100,
            resource_floor_breakdown: ResourceFloorBreakdown {
                bandwidth_usd_micros: 30,
                verifier_usd_micros: 10,
                host_usd_micros: 5,
                qualified_impressions_per_proof: 1,
            },
            runner_up_quality_bid_usd_micros: 0,
            clearing_price_usd_micros: 100,
            attestation: None,
            proof_metadata: None,
            verifier_committee: None,
            verifier_stake_snapshot: None,
            verifier_transcript: Vec::new(),
            badge_soft_intent: None,
            badge_soft_intent_snapshot: None,
        };
        let err = receipt.validate().expect_err("breakdown mismatch");
        assert!(matches!(
            err,
            SelectionReceiptError::ResourceFloorBreakdownMismatch { .. }
        ));
    }

    #[test]
    fn selection_receipt_validation_accepts_snark_attestation() {
        let (receipt, _) = build_attested_receipt();
        let insights = receipt.validate().expect("attested receipt valid");
        assert_eq!(insights.attestation_kind, SelectionAttestationKind::Snark);
        assert_eq!(insights.winner_index, 0);
    }

    #[test]
    fn selection_receipt_validation_rejects_proof_bytes_digest_mismatch() {
        let (mut receipt, mut metadata) = build_attested_receipt();
        metadata.proof_bytes_digest[0] ^= 0x01;
        receipt.proof_metadata = Some(metadata);
        let err = receipt.validate().expect_err("digest mismatch detected");
        assert!(matches!(
            err,
            SelectionReceiptError::ProofMetadataMismatch { field }
            if field == "proof_bytes_digest"
        ));
    }

    #[test]
    fn selection_receipt_validation_rejects_verifying_key_mismatch() {
        let (mut receipt, mut metadata) = build_attested_receipt();
        metadata.verifying_key_digest[0] ^= 0x01;
        receipt.proof_metadata = Some(metadata);
        let err = receipt
            .validate()
            .expect_err("verifying key mismatch detected");
        assert!(matches!(
            err,
            SelectionReceiptError::ProofMetadataMismatch { field }
            if field == "verifying_key_digest"
        ));
    }
}
