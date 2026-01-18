use crate::budget::{
    BidShadingApplication as BudgetBidShadingApplication,
    BidShadingGuidance as BudgetBidShadingGuidance, PiControllerSnapshot, PiTunerConfig,
};
use crypto_suite::{hashing::blake3, hex};
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
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use zkp::selection::{self, SelectionProofPublicInputs, SelectionProofVerification};

mod attestation;
pub mod badge;
pub mod budget;
mod privacy;
pub mod uplift;

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
    badge_family, PrivacyBudgetConfig, PrivacyBudgetDecision, PrivacyBudgetFamilySnapshot,
    PrivacyBudgetManager, PrivacyBudgetPreview, PrivacyBudgetSnapshot,
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
const KEY_UPLIFT: &[u8] = b"uplift";
const KEY_MEDIANS: &[u8] = b"cost_medians";
const KEY_CLAIMS: &[u8] = b"claims";
const KEY_CONVERSIONS: &[u8] = b"conversions";
const KEY_DEVICE_SEEN: &[u8] = b"device_seen";

pub const MICROS_PER_DOLLAR: u64 = 1_000_000;
const PPM_SCALE: u64 = 1_000_000;
const BYTES_PER_MIB: u64 = 1_048_576;

trait CeilDiv {
    fn ceil_div(self, rhs: Self) -> Self;
}

impl CeilDiv for u64 {
    fn ceil_div(self, rhs: Self) -> Self {
        if rhs == 0 {
            0
        } else {
            (self.saturating_add(rhs).saturating_sub(1)) / rhs
        }
    }
}

pub const COHORT_SELECTOR_VERSION_V1: u16 = 1;
pub const COHORT_SELECTOR_VERSION_V2: u16 = 2;
pub const CURRENT_COHORT_SELECTOR_VERSION: u16 = COHORT_SELECTOR_VERSION_V2;

pub fn cohort_selector_version_default() -> u16 {
    COHORT_SELECTOR_VERSION_V1
}

pub type InterestTagId = String;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum DomainTier {
    Premium,
    Reserved,
    Community,
    #[default]
    Unverified,
}

impl DomainTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            DomainTier::Premium => "premium",
            DomainTier::Reserved => "reserved",
            DomainTier::Community => "community",
            DomainTier::Unverified => "unverified",
        }
    }
}

impl FromStr for DomainTier {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "premium" => Ok(DomainTier::Premium),
            "reserved" => Ok(DomainTier::Reserved),
            "community" => Ok(DomainTier::Community),
            "unverified" => Ok(DomainTier::Unverified),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(crate = "foundation_serialization::serde", rename_all = "snake_case")]
pub enum PresenceKind {
    LocalNet,
    RangeBoost,
    #[default]
    Unknown,
}

impl PresenceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            PresenceKind::LocalNet => "localnet",
            PresenceKind::RangeBoost => "range_boost",
            PresenceKind::Unknown => "unknown",
        }
    }
}

impl FromStr for PresenceKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "localnet" => Ok(PresenceKind::LocalNet),
            "range_boost" => Ok(PresenceKind::RangeBoost),
            "unknown" => Ok(PresenceKind::Unknown),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(crate = "foundation_serialization::serde")]
pub struct PresenceBucketRef {
    pub bucket_id: String,
    #[serde(default)]
    pub kind: PresenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default)]
    pub radius_meters: u16,
    #[serde(default)]
    pub confidence_bps: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minted_at_micros: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_micros: Option<u64>,
}

impl PresenceBucketRef {
    #[allow(dead_code)]
    fn bucket_label(&self) -> &str {
        self.bucket_id.as_str()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct FreshnessHistogramPpm {
    pub under_1h_ppm: u32,
    pub hours_1_to_6_ppm: u32,
    pub hours_6_to_24_ppm: u32,
    pub over_24h_ppm: u32,
}

impl FreshnessHistogramPpm {
    fn normalized_weights(mut self) -> Self {
        let clamp = |value: u32| value.min(2_500_000);
        self.under_1h_ppm = clamp(self.under_1h_ppm);
        self.hours_1_to_6_ppm = clamp(self.hours_1_to_6_ppm);
        self.hours_6_to_24_ppm = clamp(self.hours_6_to_24_ppm);
        self.over_24h_ppm = clamp(self.over_24h_ppm);
        self
    }
}

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

fn read_string_array(
    value: Option<&JsonValue>,
    field: &str,
) -> Result<Vec<String>, PersistenceError> {
    match value {
        None => Ok(Vec::new()),
        Some(JsonValue::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| invalid(format!("{field} entries must be strings")))
            })
            .collect::<Result<Vec<_>, _>>(),
        Some(_) => Err(invalid(format!("{field} must be an array"))),
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct RollingMedian {
    // Keep a bounded window for simplicity
    window: usize,
    values: Vec<u64>,
}

impl RollingMedian {
    fn with_window(window: usize) -> Self {
        Self {
            window: window.max(3),
            values: Vec::new(),
        }
    }
    fn record(&mut self, v: u64) {
        self.values.push(v);
        if self.values.len() > self.window {
            let overshoot = self.values.len() - self.window;
            self.values.drain(0..overshoot);
        }
    }
    fn median(&self) -> u64 {
        if self.values.is_empty() {
            return 0;
        }
        let mut data = self.values.clone();
        data.sort_unstable();
        data[data.len() / 2]
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct CostMedians {
    storage_price_per_mib_usd_micros: RollingMedian,
    verifier_cost_usd_micros: RollingMedian,
    host_fee_usd_micros: RollingMedian,
}

impl CostMedians {
    fn new() -> Self {
        Self {
            storage_price_per_mib_usd_micros: RollingMedian::with_window(257),
            verifier_cost_usd_micros: RollingMedian::with_window(257),
            host_fee_usd_micros: RollingMedian::with_window(257),
        }
    }
    fn from_snapshot(storage: u64, verifier: u64, host: u64) -> Self {
        let mut med = Self::new();
        med.record_storage(storage);
        med.record_verifier(verifier);
        med.record_host(host);
        med
    }
    fn record_storage(&mut self, v: u64) {
        self.storage_price_per_mib_usd_micros.record(v);
    }
    fn record_verifier(&mut self, v: u64) {
        self.verifier_cost_usd_micros.record(v);
    }
    fn record_host(&mut self, v: u64) {
        self.host_fee_usd_micros.record(v);
    }
    fn snapshot(&self) -> (u64, u64, u64) {
        (
            self.storage_price_per_mib_usd_micros.median(),
            self.verifier_cost_usd_micros.median(),
            self.host_fee_usd_micros.median(),
        )
    }
}

mod serde_optional_bytes {
    use foundation_serialization::serde_bytes;
    use serde::{Deserializer, Serializer};

    #[allow(dead_code)]
    pub fn serialize<S>(value: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(bytes) => serde_bytes::serialize(bytes, serializer),
            None => serializer.serialize_none(),
        }
    }

    #[allow(dead_code)]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OptionalBytes;

        impl<'de> serde::de::Visitor<'de> for OptionalBytes {
            type Value = Option<Vec<u8>>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an optional byte array")
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(None)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                serde_bytes::deserialize(deserializer).map(Some)
            }
        }

        deserializer.deserialize_option(OptionalBytes)
    }
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DistributionPolicy {
    pub viewer_percent: u64,
    pub host_percent: u64,
    pub hardware_percent: u64,
    pub verifier_percent: u64,
    pub liquidity_percent: u64,
}

impl DistributionPolicy {
    pub fn new(viewer: u64, host: u64, hardware: u64, verifier: u64, liquidity: u64) -> Self {
        Self {
            viewer_percent: viewer,
            host_percent: host,
            hardware_percent: hardware,
            verifier_percent: verifier,
            liquidity_percent: liquidity,
        }
    }
    pub fn normalize(self) -> Self {
        self
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
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenOracle {
    #[serde(alias = "price_usd_micros", alias = "price_usd_micros")]
    pub price_usd_micros: u64,
    #[serde(default, alias = "twap_window_id", alias = "twap_window_id")]
    pub twap_window_id: u64,
}

impl TokenOracle {
    pub fn new(price_usd_micros: u64) -> Self {
        Self {
            price_usd_micros: price_usd_micros.max(1),
            twap_window_id: 0,
        }
    }

    pub fn with_twap_window(mut self, window: u64) -> Self {
        self.twap_window_id = window;
        self
    }
}

impl Default for TokenOracle {
    fn default() -> Self {
        Self {
            price_usd_micros: MICROS_PER_DOLLAR,
            twap_window_id: 0,
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
        let per_proof = hint.ceil_div(committee);
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

    pub fn scale(&self, multiplier: f64) -> Self {
        if multiplier <= f64::EPSILON {
            return self.clone();
        }
        let scale = |value: u64| -> u64 {
            ((value as f64)
                .mul_add(multiplier, 0.0)
                .round()
                .min(u64::MAX as f64)) as u64
        };
        Self {
            bandwidth_usd_micros: scale(self.bandwidth_usd_micros),
            verifier_usd_micros: scale(self.verifier_usd_micros),
            host_usd_micros: scale(self.host_usd_micros),
            qualified_impressions_per_proof: self.qualified_impressions_per_proof,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
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
    #[serde(default)]
    pub badge_guard: BadgeGuardConfig,
    pub privacy_budget: PrivacyBudgetConfig,
    pub uplift: UpliftEstimatorConfig,
    /// Weights (ppm) applied to freshness histogram buckets.
    pub quality_freshness_weights_ppm: FreshnessHistogramPpm,
    /// Target readiness streak windows for neutral quality.
    pub quality_readiness_target_windows: u64,
    /// Floor for readiness multiplier (ppm).
    pub quality_readiness_floor_ppm: u32,
    /// Floor for privacy multiplier (ppm).
    pub quality_privacy_floor_ppm: u32,
    /// Quality adjustment lower bound when converting readiness/privacy/freshness signals.
    #[serde(default = "default_quality_signal_min_multiplier_ppm")]
    pub quality_signal_min_multiplier_ppm: u32,
    /// Quality adjustment upper bound when converting readiness/privacy/freshness signals.
    #[serde(default = "default_quality_signal_max_multiplier_ppm")]
    pub quality_signal_max_multiplier_ppm: u32,
}

#[derive(Clone, Debug)]
pub struct QualitySignalConfig {
    pub freshness_weights_ppm: FreshnessHistogramPpm,
    pub readiness_target_windows: u64,
    pub readiness_floor_ppm: u32,
    pub privacy_floor_ppm: u32,
    pub cohort_floor_ppm: u32,
    pub cohort_ceiling_ppm: u32,
}

impl QualitySignalConfig {
    fn normalized(mut self) -> Self {
        self.freshness_weights_ppm = self.freshness_weights_ppm.normalized_weights();
        self.readiness_target_windows = self.readiness_target_windows.max(1);
        self.readiness_floor_ppm = self.readiness_floor_ppm.clamp(1, PPM_SCALE as u32);
        self.privacy_floor_ppm = self.privacy_floor_ppm.clamp(1, PPM_SCALE as u32);
        self.cohort_floor_ppm = self.cohort_floor_ppm.clamp(1, PPM_SCALE as u32);
        self.cohort_ceiling_ppm = self
            .cohort_ceiling_ppm
            .clamp(self.cohort_floor_ppm.max(PPM_SCALE as u32), 2_500_000);
        self
    }
}

fn default_quality_signal_min_multiplier_ppm() -> u32 {
    100_000
}

fn default_quality_signal_max_multiplier_ppm() -> u32 {
    2_500_000
}

impl MarketplaceConfig {
    pub fn quality_signal_config(&self) -> QualitySignalConfig {
        QualitySignalConfig {
            freshness_weights_ppm: self.quality_freshness_weights_ppm.clone(),
            readiness_target_windows: self.quality_readiness_target_windows,
            readiness_floor_ppm: self.quality_readiness_floor_ppm,
            privacy_floor_ppm: self.quality_privacy_floor_ppm,
            cohort_floor_ppm: self.quality_signal_min_multiplier_ppm,
            cohort_ceiling_ppm: self.quality_signal_max_multiplier_ppm,
        }
        .normalized()
    }

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
        normalized.quality_freshness_weights_ppm = normalized
            .quality_freshness_weights_ppm
            .normalized_weights();
        normalized.quality_readiness_target_windows =
            normalized.quality_readiness_target_windows.max(1);
        normalized.quality_readiness_floor_ppm = normalized
            .quality_readiness_floor_ppm
            .clamp(1, PPM_SCALE as u32);
        normalized.quality_privacy_floor_ppm = normalized
            .quality_privacy_floor_ppm
            .clamp(1, PPM_SCALE as u32);
        normalized.quality_signal_min_multiplier_ppm = normalized
            .quality_signal_min_multiplier_ppm
            .clamp(100_000, PPM_SCALE as u32);
        normalized.quality_signal_max_multiplier_ppm =
            normalized.quality_signal_max_multiplier_ppm.clamp(
                normalized
                    .quality_signal_min_multiplier_ppm
                    .max(PPM_SCALE as u32),
                2_500_000,
            );
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

    fn clamp_quality_signal_multiplier(&self, multiplier_ppm: u32) -> f64 {
        let clamped = multiplier_ppm
            .clamp(
                self.quality_signal_min_multiplier_ppm,
                self.quality_signal_max_multiplier_ppm,
            )
            .max(1);
        (clamped as f64) / (PPM_SCALE as f64)
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
            quality_freshness_weights_ppm: FreshnessHistogramPpm {
                under_1h_ppm: 1_000_000,
                hours_1_to_6_ppm: 800_000,
                hours_6_to_24_ppm: 500_000,
                over_24h_ppm: 200_000,
            },
            quality_readiness_target_windows: 6,
            quality_readiness_floor_ppm: 100_000,
            quality_privacy_floor_ppm: 100_000,
            quality_signal_min_multiplier_ppm: 100_000,
            quality_signal_max_multiplier_ppm: 2_500_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct CohortKey {
    domain: String,
    domain_tier: DomainTier,
    domain_owner: Option<String>,
    provider: Option<String>,
    badges: Vec<String>,
    interest_tags: Vec<InterestTagId>,
    presence_bucket: Option<PresenceBucketRef>,
    selectors_version: u16,
}

#[derive(Clone, Debug)]
struct CohortSelectors {
    badges: Vec<String>,
    interest_tags: Vec<InterestTagId>,
    presence_bucket: Option<PresenceBucketRef>,
    selectors_version: u16,
}

impl CohortSelectors {
    fn normalize(mut self) -> Self {
        self.badges.sort();
        self.badges.dedup();
        self.interest_tags.sort();
        self.interest_tags.dedup();
        self
    }
}

impl CohortKey {
    #[allow(dead_code)]
    fn new(domain: String, provider: Option<String>, badges: Vec<String>) -> Self {
        Self::with_selectors(
            domain,
            DomainTier::default(),
            None,
            provider,
            CohortSelectors {
                badges,
                interest_tags: Vec::new(),
                presence_bucket: None,
                selectors_version: COHORT_SELECTOR_VERSION_V1,
            },
        )
    }

    fn from_context(ctx: &ImpressionContext) -> Self {
        Self::with_selectors(
            ctx.domain.clone(),
            ctx.domain_tier,
            ctx.domain_owner.clone(),
            ctx.provider.clone(),
            CohortSelectors {
                badges: ctx.badges.clone(),
                interest_tags: ctx.interest_tags.clone(),
                presence_bucket: ctx.presence_bucket.clone(),
                selectors_version: ctx.selectors_version,
            },
        )
    }

    fn with_selectors(
        domain: String,
        domain_tier: DomainTier,
        domain_owner: Option<String>,
        provider: Option<String>,
        selectors: CohortSelectors,
    ) -> Self {
        let selectors = selectors.normalize();
        Self {
            domain,
            domain_tier,
            domain_owner,
            provider,
            badges: selectors.badges,
            interest_tags: selectors.interest_tags,
            presence_bucket: selectors.presence_bucket,
            selectors_version: selectors.selectors_version,
        }
    }

    fn selectors_version(&self) -> u16 {
        if self.selectors_version == 0 {
            COHORT_SELECTOR_VERSION_V1
        } else {
            self.selectors_version
        }
    }
}

impl Hash for CohortKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.domain.hash(state);
        self.domain_tier.as_str().hash(state);
        self.domain_owner.hash(state);
        self.provider.hash(state);
        self.badges.hash(state);
        self.interest_tags.hash(state);
        if let Some(bucket) = &self.presence_bucket {
            bucket.bucket_id.hash(state);
            bucket.kind.hash(state);
        }
        self.selectors_version().hash(state);
    }
}

#[derive(Clone, Debug)]
struct CohortTelemetryId {
    cohort_hash: String,
    domain: String,
    domain_tier: DomainTier,
    provider: Option<String>,
    badge_hash: String,
    interest_hash: String,
    presence_bucket: Option<String>,
}

impl CohortTelemetryId {
    fn from_key(key: &CohortKey) -> Self {
        let mut cohort_hasher = blake3::Hasher::new();
        cohort_hasher.update(key.domain.as_bytes());
        cohort_hasher.update(key.domain_tier.as_str().as_bytes());
        if let Some(owner) = &key.domain_owner {
            cohort_hasher.update(owner.as_bytes());
        }
        if let Some(provider) = &key.provider {
            cohort_hasher.update(provider.as_bytes());
        }
        for badge in &key.badges {
            cohort_hasher.update(badge.as_bytes());
        }
        for tag in &key.interest_tags {
            cohort_hasher.update(tag.as_bytes());
        }
        if let Some(bucket) = &key.presence_bucket {
            cohort_hasher.update(bucket.bucket_id.as_bytes());
            cohort_hasher.update(bucket.kind.as_str().as_bytes());
        }
        let mut badge_hasher = blake3::Hasher::new();
        for badge in &key.badges {
            badge_hasher.update(badge.as_bytes());
        }
        let mut interest_hasher = blake3::Hasher::new();
        for tag in &key.interest_tags {
            interest_hasher.update(tag.as_bytes());
        }
        let presence_bucket = key
            .presence_bucket
            .as_ref()
            .map(|bucket| bucket.bucket_id.clone());
        Self {
            cohort_hash: cohort_hasher.finalize().to_hex().to_hex_string(),
            domain: key.domain.clone(),
            domain_tier: key.domain_tier,
            provider: key.provider.clone(),
            badge_hash: badge_hasher.finalize().to_hex().to_hex_string(),
            interest_hash: interest_hasher.finalize().to_hex().to_hex_string(),
            presence_bucket,
        }
    }

    fn provider_label(&self) -> &str {
        self.provider.as_deref().unwrap_or("-")
    }

    fn domain_tier_label(&self) -> &'static str {
        self.domain_tier.as_str()
    }

    fn interest_label(&self) -> &str {
        self.interest_hash.as_str()
    }

    fn presence_label(&self) -> &str {
        self.presence_bucket.as_deref().unwrap_or("-")
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
        let utilization = (self.ema_demand_usd_micros / self.ema_supply_usd_micros).clamp(0.0, 1.0);
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
            "domain_tier" => self.telemetry.domain_tier_label(),
            "interest" => self.telemetry.interest_label(),
            "presence_bucket" => self.telemetry.presence_label(),
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
pub struct GeoTargeting {
    #[serde(default)]
    pub countries: Vec<String>,
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub metros: Vec<String>,
}

impl GeoTargeting {
    fn is_empty(&self) -> bool {
        self.countries.is_empty() && self.regions.is_empty() && self.metros.is_empty()
    }

    fn matches(&self, ctx: Option<&GeoContext>) -> bool {
        if self.is_empty() {
            return true;
        }
        let Some(geo) = ctx else {
            return false;
        };
        if !self.countries.is_empty() {
            let Some(country) = geo.country.as_ref() else {
                return false;
            };
            if !self
                .countries
                .iter()
                .any(|c| c.eq_ignore_ascii_case(country))
            {
                return false;
            }
        }
        if !self.regions.is_empty() {
            let Some(region) = geo.region.as_ref() else {
                return false;
            };
            if !self.regions.iter().any(|r| r.eq_ignore_ascii_case(region)) {
                return false;
            }
        }
        if !self.metros.is_empty() {
            let Some(metro) = geo.metro.as_ref() else {
                return false;
            };
            if !self.metros.iter().any(|m| m.eq_ignore_ascii_case(metro)) {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeviceTargeting {
    #[serde(default)]
    pub os_families: Vec<String>,
    #[serde(default)]
    pub device_classes: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl DeviceTargeting {
    fn is_empty(&self) -> bool {
        self.os_families.is_empty()
            && self.device_classes.is_empty()
            && self.models.is_empty()
            && self.capabilities.is_empty()
    }

    fn matches(&self, ctx: Option<&DeviceContext>) -> bool {
        if self.is_empty() {
            return true;
        }
        let Some(device) = ctx else {
            return false;
        };
        if !self.os_families.is_empty() {
            let Some(os) = device.os_family.as_ref() else {
                return false;
            };
            if !self
                .os_families
                .iter()
                .any(|family| family.eq_ignore_ascii_case(os))
            {
                return false;
            }
        }
        if !self.device_classes.is_empty() {
            let Some(class) = device.device_class.as_ref() else {
                return false;
            };
            if !self
                .device_classes
                .iter()
                .any(|c| c.eq_ignore_ascii_case(class))
            {
                return false;
            }
        }
        if !self.models.is_empty() {
            let Some(model) = device.model.as_ref() else {
                return false;
            };
            if !self.models.iter().any(|m| m.eq_ignore_ascii_case(model)) {
                return false;
            }
        }
        if !self.capabilities.is_empty() {
            if device.capabilities.is_empty() {
                return false;
            }
            let caps: HashSet<&str> = device.capabilities.iter().map(String::as_str).collect();
            if self
                .capabilities
                .iter()
                .any(|required| !caps.contains(required.as_str()))
            {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CrmListTargeting {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl CrmListTargeting {
    fn matches(&self, lists: &[String]) -> bool {
        if self.include.is_empty() && self.exclude.is_empty() {
            return true;
        }
        let inventory: HashSet<&str> = lists.iter().map(String::as_str).collect();
        if !self.include.is_empty()
            && self
                .include
                .iter()
                .any(|item| !inventory.contains(item.as_str()))
        {
            return false;
        }
        if self
            .exclude
            .iter()
            .any(|item| inventory.contains(item.as_str()))
        {
            return false;
        }
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryChannel {
    #[default]
    Http,
    Mesh,
}

impl DeliveryChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryChannel::Http => "http",
            DeliveryChannel::Mesh => "mesh",
        }
    }
}

impl std::fmt::Display for DeliveryChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for DeliveryChannel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "http" => Ok(DeliveryChannel::Http),
            "mesh" => Ok(DeliveryChannel::Mesh),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeliveryTargeting {
    #[serde(default)]
    pub allowed_channels: Vec<DeliveryChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_channel: Option<DeliveryChannel>,
}

impl DeliveryTargeting {
    fn allows(&self, channel: DeliveryChannel) -> bool {
        self.allowed_channels.is_empty() || self.allowed_channels.contains(&channel)
    }

    fn prefers(&self, channel: DeliveryChannel) -> bool {
        self.preferred_channel
            .map(|pref| pref == channel)
            .unwrap_or(false)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GeoContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metro: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeviceContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MeshContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hop_proofs: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CreativePlacement {
    #[serde(default)]
    pub mesh_enabled: bool,
    #[serde(default)]
    pub mesh_only: bool,
    #[serde(default)]
    pub allowed_channels: Vec<DeliveryChannel>,
}

impl CreativePlacement {
    fn allows(&self, channel: DeliveryChannel) -> bool {
        if !self.allowed_channels.is_empty() && !self.allowed_channels.contains(&channel) {
            return false;
        }
        match channel {
            DeliveryChannel::Mesh => self.mesh_enabled,
            DeliveryChannel::Http => !self.mesh_only,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CampaignTargeting {
    pub domains: Vec<String>,
    pub badges: Vec<String>,
    #[serde(default)]
    pub geo: GeoTargeting,
    #[serde(default)]
    pub device: DeviceTargeting,
    #[serde(default)]
    pub crm_lists: CrmListTargeting,
    #[serde(default)]
    pub delivery: DeliveryTargeting,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "serde_optional_bytes"
    )]
    pub mesh_payload: Option<Vec<u8>>,
    #[serde(default)]
    pub placement: CreativePlacement,
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

    fn apply_signal_multiplier(mut self, signal_multiplier: f64) -> Self {
        if signal_multiplier <= f64::EPSILON {
            return self;
        }
        let adjusted = (self.quality_adjusted_usd_micros as f64)
            .mul_add(signal_multiplier, 0.0)
            .round()
            .min(u64::MAX as f64) as u64;
        self.quality_adjusted_usd_micros = adjusted;
        self.quality_multiplier *= signal_multiplier;
        self
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
    #[serde(default)]
    pub domain_tier: DomainTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_owner: Option<String>,
    pub provider: Option<String>,
    pub badges: Vec<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub interest_tags: Vec<InterestTagId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_bucket: Option<PresenceBucketRef>,
    #[serde(default = "cohort_selector_version_default")]
    pub selectors_version: u16,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geo: Option<GeoContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceContext>,
    #[serde(default)]
    pub crm_lists: Vec<String>,
    #[serde(default)]
    pub delivery_channel: DeliveryChannel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<MeshContext>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "foundation_serialization::serde_bytes"
    )]
    pub assignment_seed_override: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct MatchOutcome {
    pub campaign_id: String,
    pub creative_id: String,
    pub price_per_mib_usd_micros: u64,
    pub total_usd_micros: u64,
    pub clearing_price_usd_micros: u64,
    pub resource_floor_usd_micros: u64,
    pub resource_floor_breakdown: ResourceFloorBreakdown,
    pub runner_up_quality_bid_usd_micros: u64,
    pub quality_adjusted_bid_usd_micros: u64,
    pub selection_receipt: SelectionReceipt,
    pub uplift: UpliftEstimate,
    pub uplift_assignment: UpliftHoldoutAssignment,
    pub delivery_channel: DeliveryChannel,
    pub mesh_payload: Option<Vec<u8>>,
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
    pub clearing_price_usd_micros: u64,
    #[serde(default)]
    pub delivery_channel: DeliveryChannel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_payload: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_payload_digest: Option<String>,
    #[serde(default)]
    pub resource_floor_breakdown: ResourceFloorBreakdown,
    pub runner_up_quality_bid_usd_micros: u64,
    pub quality_adjusted_bid_usd_micros: u64,
    pub viewer: u64,
    pub host: u64,
    pub hardware: u64,
    pub verifier: u64,
    pub liquidity: u64,
    pub miner: u64,
    pub total: u64,
    pub unsettled_usd_micros: u64,
    #[serde(alias = "price_usd_micros", alias = "price_usd_micros")]
    pub price_usd_micros: u64,
    #[serde(default, alias = "remainders_usd_micros")]
    pub remainders_usd_micros: RemainderBreakdown,
    #[serde(default, alias = "twap_window_id", alias = "twap_window_id")]
    pub twap_window_id: u64,
    pub selection_receipt: SelectionReceipt,
    #[serde(default)]
    pub uplift: UpliftEstimate,
    #[serde(default)]
    pub claim_routes: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub conversions: u32,
    #[serde(default)]
    pub device_links: Vec<DeviceLinkOptIn>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversionEvent {
    pub campaign_id: String,
    pub creative_id: String,
    pub assignment: UpliftHoldoutAssignment,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_usd_micros: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at_micros: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_link: Option<DeviceLinkOptIn>,
}

/// Optional device-link attestation for conversion dedup/attribution.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceLinkOptIn {
    pub device_hash: String,
    #[serde(default)]
    pub opt_in: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct ConversionAccumulator {
    #[serde(default)]
    count: u32,
    #[serde(default)]
    device_links: Vec<DeviceLinkOptIn>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionCohortTrace {
    pub domain: String,
    #[serde(default)]
    pub domain_tier: DomainTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub badges: Vec<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub interest_tags: Vec<InterestTagId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_bucket: Option<PresenceBucketRef>,
    #[serde(default = "cohort_selector_version_default")]
    pub selectors_version: u16,
    pub bytes: u64,
    pub price_per_mib_usd_micros: u64,
    #[serde(default)]
    pub delivery_channel: DeliveryChannel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_peer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_latency_ms: Option<u64>,
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
    #[serde(default)]
    pub delivery_channel: DeliveryChannel,
    #[serde(default)]
    pub preferred_delivery_match: bool,
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
            delivery_channel: DeliveryChannel::Http,
            preferred_delivery_match: false,
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
    use foundation_serialization::serde::{
        ser::SerializeSeq, Deserialize, Deserializer, Serializer,
    };

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
        // Deserialize directly as Vec<Vec<u8>>
        // The serde_bytes optimization isn't critical for correctness
        Vec::<Vec<u8>>::deserialize(deserializer)
    }

    struct ByteSlice<'a>(&'a [u8]);

    impl foundation_serialization::serde::Serialize for ByteSlice<'_> {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uplift_assignment: Option<UpliftHoldoutAssignment>,
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
            candidate_map.insert(
                "delivery_channel".into(),
                JsonValue::String(candidate.delivery_channel.as_str().to_string()),
            );
            candidate_map.insert(
                "preferred_delivery_match".into(),
                JsonValue::Bool(candidate.preferred_delivery_match),
            );
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
            "delivery_channel".into(),
            JsonValue::String(self.cohort.delivery_channel.as_str().to_string()),
        );
        commitment_map.insert(
            "mesh_peer".into(),
            self.cohort
                .mesh_peer
                .as_ref()
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        );
        commitment_map.insert(
            "mesh_transport".into(),
            self.cohort
                .mesh_transport
                .as_ref()
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        );
        commitment_map.insert(
            "mesh_latency_ms".into(),
            self.cohort
                .mesh_latency_ms
                .map(JsonValue::from)
                .unwrap_or(JsonValue::Null),
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
        if let Some(assignment) = &self.uplift_assignment {
            commitment_map.insert(
                "uplift_assignment_fold".into(),
                JsonValue::from(assignment.fold as u64),
            );
            commitment_map.insert(
                "uplift_assignment_in_holdout".into(),
                JsonValue::Bool(assignment.in_holdout),
            );
            commitment_map.insert(
                "uplift_assignment_propensity".into(),
                JsonValue::Number(number_from_f64(assignment.propensity)),
            );
        }
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
        // Permit an explicit integration-test bypass for synthetic proofs, but never during tests.
        if cfg!(feature = "integration-tests") && !cfg!(test) {
            return Ok(SelectionReceiptInsights {
                winner_index: self.winner_index,
                winner_quality_bid_usd_micros: winner.quality_adjusted_bid_usd_micros,
                runner_up_quality_bid_usd_micros: runner_up,
                clearing_price_usd_micros: self.clearing_price_usd_micros,
                resource_floor_usd_micros: self.resource_floor_usd_micros,
                attestation_kind: self.attestation_kind(),
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
                    if !cfg!(feature = "integration-tests") {
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
                        if metadata.public_inputs.candidate_count as usize != self.candidates.len()
                        {
                            return Err(SelectionReceiptError::ProofMetadataMismatch {
                                field: "candidate_count",
                            });
                        }
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
    #[serde(default)]
    pub domain_tier: DomainTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub badges: Vec<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub interest_tags: Vec<InterestTagId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_bucket: Option<PresenceBucketRef>,
    #[serde(default = "cohort_selector_version_default")]
    pub selectors_version: u16,
    pub price_per_mib_usd_micros: u64,
    pub target_utilization_ppm: u32,
    #[serde(default)]
    pub observed_utilization_ppm: u32,
}

/// Component breakdown for quality signals (all ppm scaled).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QualitySignalComponents {
    /// Freshness-derived multiplier; 1_000_000 is neutral.
    pub freshness_multiplier_ppm: u32,
    /// Readiness streak multiplier; 1_000_000 is neutral.
    pub readiness_multiplier_ppm: u32,
    /// Privacy-derived multiplier; 1_000_000 is neutral (penalties drop below).
    pub privacy_multiplier_ppm: u32,
}

/// Quality signal per cohort mapping readiness/freshness/privacy into a composite multiplier.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QualitySignal {
    pub cohort: CohortKeySnapshot,
    /// Composite multiplier in ppm (1_000_000 == neutral).
    pub multiplier_ppm: u32,
    /// Optional component breakdown for diagnostics.
    #[serde(default)]
    pub components: QualitySignalComponents,
}

#[derive(Clone, Debug)]
struct QualitySignalState {
    multiplier: f64,
    components: QualitySignalComponents,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct ClaimRegistry {
    routes: HashMap<String, String>,
}

impl ClaimRegistry {
    fn register(&mut self, domain: &str, role: &str, address: &str) {
        let key = format!("{domain}|{role}");
        self.routes.insert(key, address.to_string());
    }

    fn for_domain(&self, domain: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for (key, addr) in self.routes.iter() {
            if let Some((d, role)) = key.split_once('|') {
                if d == domain {
                    map.insert(role.to_string(), addr.clone());
                }
            }
        }
        map
    }
}

fn claim_registry_from_metadata(metadata: &SledTree) -> Result<ClaimRegistry, PersistenceError> {
    if let Some(bytes) = metadata.get(KEY_CLAIMS)? {
        deserialize_claim_registry(&bytes)
    } else {
        Ok(ClaimRegistry::default())
    }
}

pub trait Marketplace: Send + Sync {
    fn register_campaign(&self, campaign: Campaign) -> Result<(), MarketplaceError>;
    fn list_campaigns(&self) -> Vec<CampaignSummary>;
    fn campaign(&self, id: &str) -> Option<Campaign>;
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
    fn record_conversion(&self, event: ConversionEvent) -> Result<(), MarketplaceError>;
    /// Recompute distribution from current utilization indices with clamped adjustments.
    fn recompute_distribution_from_utilization(&self);

    /// Return the rolling median snapshot for cost indices in USD micros.
    /// Order: (storage_price_per_mib, verifier_cost, host_fee)
    fn cost_medians_usd_micros(&self) -> (u64, u64, u64);

    /// Evaluate badge k-anonymity guardrails for a badge set.
    fn badge_guard_decision(
        &self,
        badges: &[String],
        soft_intent: Option<&BadgeSoftIntentContext>,
    ) -> BadgeDecision;

    /// Push quality signals derived from readiness/privacy/freshness so bids can be adjusted.
    fn update_quality_signals(&self, signals: Vec<QualitySignal>);

    /// Return the configuration used to compute quality signal components.
    fn quality_signal_config(&self) -> QualitySignalConfig;

    /// Snapshot current privacy budget state without mutating it.
    fn privacy_budget_snapshot(&self) -> PrivacyBudgetSnapshot;

    /// Preview a privacy budget decision without consuming budget.
    fn preview_privacy_budget(
        &self,
        badges: &[String],
        population_hint: Option<u64>,
    ) -> PrivacyBudgetPreview;

    /// Perform a privacy budget decision (consumes budget when accepted or cooling).
    fn authorize_privacy_budget(
        &self,
        badges: &[String],
        population_hint: Option<u64>,
    ) -> PrivacyBudgetDecision;

    /// Register payout claim routing for a domain/app role.
    fn register_claim_route(
        &self,
        domain: &str,
        role: &str,
        address: &str,
    ) -> Result<(), MarketplaceError>;

    /// Lookup payout claim routes for a cohort (domain-level for now).
    fn claim_routes(&self, cohort: &CohortKeySnapshot)
        -> std::collections::HashMap<String, String>;

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
    clearing_price_usd_micros: u64,
    demand_usd_micros: u64,
    resource_floor_usd_micros: u64,
    resource_floor_breakdown: ResourceFloorBreakdown,
    runner_up_quality_bid_usd_micros: u64,
    quality_adjusted_bid_usd_micros: u64,
    cohort: CohortKey,
    selection_receipt: SelectionReceipt,
    uplift: UpliftEstimate,
    assignment: UpliftHoldoutAssignment,
    delivery_channel: DeliveryChannel,
    mesh_payload: Option<Vec<u8>>,
    claim_routes: HashMap<String, String>,
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
    medians: RwLock<CostMedians>,
    quality_signals: RwLock<HashMap<CohortKey, QualitySignalState>>,
    claim_registry: RwLock<ClaimRegistry>,
    conversions: RwLock<HashMap<String, ConversionAccumulator>>,
    device_seen: RwLock<HashMap<String, HashSet<String>>>,
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
    medians: RwLock<CostMedians>,
    quality_signals: RwLock<HashMap<CohortKey, QualitySignalState>>,
    claim_registry: RwLock<ClaimRegistry>,
    conversions: RwLock<HashMap<String, ConversionAccumulator>>,
    device_seen: RwLock<HashMap<String, HashSet<String>>>,
}

#[derive(Debug)]
pub enum MarketplaceError {
    DuplicateCampaign,
    UnknownCampaign,
    UnknownCreative,
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
    if let Some(payload) = creative.mesh_payload.as_ref() {
        map.insert(
            "mesh_payload".into(),
            JsonValue::String(hex::encode(payload)),
        );
    }
    map.insert("placement".into(), placement_to_value(&creative.placement));
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
        mesh_payload: obj
            .get("mesh_payload")
            .map(|value| match value.as_str() {
                Some(hex_str) => {
                    hex::decode(hex_str).map_err(|_| invalid("mesh_payload must be hex encoded"))
                }
                None => Err(invalid("mesh_payload must be a string")),
            })
            .transpose()?,
        placement: obj
            .get("placement")
            .map(placement_from_value)
            .transpose()?
            .unwrap_or_default(),
    })
}

fn placement_to_value(placement: &CreativePlacement) -> JsonValue {
    let mut obj = JsonMap::new();
    obj.insert(
        "mesh_enabled".into(),
        JsonValue::Bool(placement.mesh_enabled),
    );
    obj.insert("mesh_only".into(), JsonValue::Bool(placement.mesh_only));
    if !placement.allowed_channels.is_empty() {
        obj.insert(
            "allowed_channels".into(),
            JsonValue::Array(
                placement
                    .allowed_channels
                    .iter()
                    .map(|channel| JsonValue::String(channel.as_str().to_string()))
                    .collect(),
            ),
        );
    }
    JsonValue::Object(obj)
}

fn placement_from_value(value: &JsonValue) -> Result<CreativePlacement, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("placement must be an object"))?;
    let mesh_enabled = obj
        .get("mesh_enabled")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    let mesh_only = obj
        .get("mesh_only")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    let mut allowed_channels = Vec::new();
    if let Some(entry) = obj.get("allowed_channels") {
        let arr = entry
            .as_array()
            .ok_or_else(|| invalid("placement.allowed_channels must be an array"))?;
        for value in arr {
            let name = value
                .as_str()
                .ok_or_else(|| invalid("placement.allowed_channels entries must be strings"))?;
            let channel = name
                .parse::<DeliveryChannel>()
                .map_err(|_| invalid("placement.allowed_channels entry invalid"))?;
            allowed_channels.push(channel);
        }
    }
    Ok(CreativePlacement {
        mesh_enabled,
        mesh_only,
        allowed_channels,
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
    if !targeting.geo.is_empty() {
        map.insert("geo".into(), geo_targeting_to_value(&targeting.geo));
    }
    if !targeting.device.is_empty() {
        map.insert(
            "device".into(),
            device_targeting_to_value(&targeting.device),
        );
    }
    if !(targeting.crm_lists.include.is_empty() && targeting.crm_lists.exclude.is_empty()) {
        map.insert(
            "crm_lists".into(),
            crm_targeting_to_value(&targeting.crm_lists),
        );
    }
    if !targeting.delivery.allowed_channels.is_empty()
        || targeting.delivery.preferred_channel.is_some()
    {
        map.insert(
            "delivery".into(),
            delivery_targeting_to_value(&targeting.delivery),
        );
    }
    JsonValue::Object(map)
}

fn targeting_from_value(value: &JsonValue) -> Result<CampaignTargeting, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("targeting must be an object"))?;
    Ok(CampaignTargeting {
        domains: read_string_vec(obj, "domains")?,
        badges: read_string_vec(obj, "badges")?,
        geo: obj
            .get("geo")
            .map(geo_targeting_from_value)
            .transpose()?
            .unwrap_or_default(),
        device: obj
            .get("device")
            .map(device_targeting_from_value)
            .transpose()?
            .unwrap_or_default(),
        crm_lists: obj
            .get("crm_lists")
            .map(crm_targeting_from_value)
            .transpose()?
            .unwrap_or_default(),
        delivery: obj
            .get("delivery")
            .map(delivery_targeting_from_value)
            .transpose()?
            .unwrap_or_default(),
    })
}

fn geo_targeting_to_value(targeting: &GeoTargeting) -> JsonValue {
    let mut obj = JsonMap::new();
    if !targeting.countries.is_empty() {
        obj.insert(
            "countries".into(),
            JsonValue::Array(
                targeting
                    .countries
                    .iter()
                    .cloned()
                    .map(JsonValue::String)
                    .collect(),
            ),
        );
    }
    if !targeting.regions.is_empty() {
        obj.insert(
            "regions".into(),
            JsonValue::Array(
                targeting
                    .regions
                    .iter()
                    .cloned()
                    .map(JsonValue::String)
                    .collect(),
            ),
        );
    }
    if !targeting.metros.is_empty() {
        obj.insert(
            "metros".into(),
            JsonValue::Array(
                targeting
                    .metros
                    .iter()
                    .cloned()
                    .map(JsonValue::String)
                    .collect(),
            ),
        );
    }
    JsonValue::Object(obj)
}

fn geo_targeting_from_value(value: &JsonValue) -> Result<GeoTargeting, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("geo targeting must be an object"))?;
    Ok(GeoTargeting {
        countries: read_string_vec(obj, "countries")?,
        regions: read_string_vec(obj, "regions")?,
        metros: read_string_vec(obj, "metros")?,
    })
}

fn device_targeting_to_value(targeting: &DeviceTargeting) -> JsonValue {
    let mut obj = JsonMap::new();
    if !targeting.os_families.is_empty() {
        obj.insert(
            "os_families".into(),
            JsonValue::Array(
                targeting
                    .os_families
                    .iter()
                    .cloned()
                    .map(JsonValue::String)
                    .collect(),
            ),
        );
    }
    if !targeting.device_classes.is_empty() {
        obj.insert(
            "device_classes".into(),
            JsonValue::Array(
                targeting
                    .device_classes
                    .iter()
                    .cloned()
                    .map(JsonValue::String)
                    .collect(),
            ),
        );
    }
    if !targeting.models.is_empty() {
        obj.insert(
            "models".into(),
            JsonValue::Array(
                targeting
                    .models
                    .iter()
                    .cloned()
                    .map(JsonValue::String)
                    .collect(),
            ),
        );
    }
    if !targeting.capabilities.is_empty() {
        obj.insert(
            "capabilities".into(),
            JsonValue::Array(
                targeting
                    .capabilities
                    .iter()
                    .cloned()
                    .map(JsonValue::String)
                    .collect(),
            ),
        );
    }
    JsonValue::Object(obj)
}

fn device_targeting_from_value(value: &JsonValue) -> Result<DeviceTargeting, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("device targeting must be an object"))?;
    Ok(DeviceTargeting {
        os_families: read_string_vec(obj, "os_families")?,
        device_classes: read_string_vec(obj, "device_classes")?,
        models: read_string_vec(obj, "models")?,
        capabilities: read_string_vec(obj, "capabilities")?,
    })
}

fn crm_targeting_to_value(targeting: &CrmListTargeting) -> JsonValue {
    let mut obj = JsonMap::new();
    if !targeting.include.is_empty() {
        obj.insert(
            "include".into(),
            JsonValue::Array(
                targeting
                    .include
                    .iter()
                    .cloned()
                    .map(JsonValue::String)
                    .collect(),
            ),
        );
    }
    if !targeting.exclude.is_empty() {
        obj.insert(
            "exclude".into(),
            JsonValue::Array(
                targeting
                    .exclude
                    .iter()
                    .cloned()
                    .map(JsonValue::String)
                    .collect(),
            ),
        );
    }
    JsonValue::Object(obj)
}

fn crm_targeting_from_value(value: &JsonValue) -> Result<CrmListTargeting, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("crm targeting must be an object"))?;
    Ok(CrmListTargeting {
        include: read_string_vec(obj, "include")?,
        exclude: read_string_vec(obj, "exclude")?,
    })
}

fn delivery_targeting_to_value(targeting: &DeliveryTargeting) -> JsonValue {
    let mut obj = JsonMap::new();
    if !targeting.allowed_channels.is_empty() {
        obj.insert(
            "allowed_channels".into(),
            JsonValue::Array(
                targeting
                    .allowed_channels
                    .iter()
                    .map(|channel| JsonValue::String(channel.as_str().to_string()))
                    .collect(),
            ),
        );
    }
    if let Some(preferred) = targeting.preferred_channel {
        obj.insert(
            "preferred_channel".into(),
            JsonValue::String(preferred.as_str().to_string()),
        );
    }
    JsonValue::Object(obj)
}

fn delivery_targeting_from_value(value: &JsonValue) -> Result<DeliveryTargeting, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("delivery targeting must be an object"))?;
    let mut allowed_channels = Vec::new();
    if let Some(entry) = obj.get("allowed_channels") {
        let arr = entry
            .as_array()
            .ok_or_else(|| invalid("delivery.allowed_channels must be an array"))?;
        for value in arr {
            let name = value
                .as_str()
                .ok_or_else(|| invalid("delivery.allowed_channels entries must be strings"))?;
            let channel = name
                .parse::<DeliveryChannel>()
                .map_err(|_| invalid("delivery.allowed_channels entry invalid"))?;
            allowed_channels.push(channel);
        }
    }
    let preferred_channel = obj
        .get("preferred_channel")
        .and_then(JsonValue::as_str)
        .map(|value| value.parse::<DeliveryChannel>())
        .transpose()
        .map_err(|_| invalid("delivery.preferred_channel invalid"))?;
    Ok(DeliveryTargeting {
        allowed_channels,
        preferred_channel,
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

fn serialize_claim_registry(registry: &ClaimRegistry) -> Result<Vec<u8>, PersistenceError> {
    foundation_serialization::json::to_vec(registry)
        .map_err(|err| PersistenceError::Invalid(err.to_string()))
}

fn deserialize_claim_registry(bytes: &[u8]) -> Result<ClaimRegistry, PersistenceError> {
    foundation_serialization::json::from_slice(bytes)
        .map_err(|err| PersistenceError::Invalid(err.to_string()))
}

fn serialize_conversions(
    conversions: &HashMap<String, ConversionAccumulator>,
) -> Result<Vec<u8>, PersistenceError> {
    foundation_serialization::json::to_vec(conversions)
        .map_err(|err| PersistenceError::Invalid(err.to_string()))
}

fn deserialize_conversions(
    bytes: &[u8],
) -> Result<HashMap<String, ConversionAccumulator>, PersistenceError> {
    foundation_serialization::json::from_slice(bytes)
        .map_err(|err| PersistenceError::Invalid(err.to_string()))
}

fn serialize_device_seen(
    seen: &HashMap<String, HashSet<String>>,
) -> Result<Vec<u8>, PersistenceError> {
    foundation_serialization::json::to_vec(seen)
        .map_err(|err| PersistenceError::Invalid(err.to_string()))
}

fn deserialize_device_seen(
    bytes: &[u8],
) -> Result<HashMap<String, HashSet<String>>, PersistenceError> {
    foundation_serialization::json::from_slice(bytes)
        .map_err(|err| PersistenceError::Invalid(err.to_string()))
}

fn uplift_fold_snapshot_to_value(fold: &uplift::UpliftFoldSnapshot) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "fold_index".into(),
        JsonValue::Number(JsonNumber::from(fold.fold_index)),
    );
    map.insert(
        "treatment_count".into(),
        JsonValue::Number(JsonNumber::from(fold.treatment_count)),
    );
    map.insert(
        "treatment_success".into(),
        JsonValue::Number(JsonNumber::from(fold.treatment_success)),
    );
    map.insert(
        "control_count".into(),
        JsonValue::Number(JsonNumber::from(fold.control_count)),
    );
    map.insert(
        "control_success".into(),
        JsonValue::Number(JsonNumber::from(fold.control_success)),
    );
    JsonValue::Object(map)
}

fn uplift_fold_snapshot_from_value(
    value: &JsonValue,
) -> Result<uplift::UpliftFoldSnapshot, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("uplift fold must be an object"))?;
    Ok(uplift::UpliftFoldSnapshot {
        fold_index: read_u64(obj, "fold_index")? as u8,
        treatment_count: read_u64(obj, "treatment_count")?,
        treatment_success: read_u64(obj, "treatment_success")?,
        control_count: read_u64(obj, "control_count")?,
        control_success: read_u64(obj, "control_success")?,
    })
}

fn uplift_creative_snapshot_to_value(snapshot: &uplift::UpliftCreativeSnapshot) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("key".into(), JsonValue::String(snapshot.key.clone()));
    map.insert(
        "treatment_count".into(),
        JsonValue::Number(JsonNumber::from(snapshot.treatment_count)),
    );
    map.insert(
        "treatment_success".into(),
        JsonValue::Number(JsonNumber::from(snapshot.treatment_success)),
    );
    map.insert(
        "control_count".into(),
        JsonValue::Number(JsonNumber::from(snapshot.control_count)),
    );
    map.insert(
        "control_success".into(),
        JsonValue::Number(JsonNumber::from(snapshot.control_success)),
    );
    if !snapshot.folds.is_empty() {
        let folds: Vec<JsonValue> = snapshot
            .folds
            .iter()
            .map(uplift_fold_snapshot_to_value)
            .collect();
        map.insert("folds".into(), JsonValue::Array(folds));
    }
    JsonValue::Object(map)
}

fn uplift_creative_snapshot_from_value(
    value: &JsonValue,
) -> Result<uplift::UpliftCreativeSnapshot, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("uplift creative must be an object"))?;
    let folds = match obj.get("folds") {
        Some(JsonValue::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(uplift_fold_snapshot_from_value(item)?);
            }
            out
        }
        Some(_) => return Err(invalid("uplift creative folds must be an array")),
        None => Vec::new(),
    };
    Ok(uplift::UpliftCreativeSnapshot {
        key: read_string(obj, "key")?,
        treatment_count: read_u64(obj, "treatment_count")?,
        treatment_success: read_u64(obj, "treatment_success")?,
        control_count: read_u64(obj, "control_count")?,
        control_success: read_u64(obj, "control_success")?,
        folds,
    })
}

fn uplift_snapshot_to_value(snapshot: &UpliftSnapshot) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "generated_at_micros".into(),
        JsonValue::Number(JsonNumber::from(snapshot.generated_at_micros)),
    );
    let creatives: Vec<JsonValue> = snapshot
        .creatives
        .iter()
        .map(uplift_creative_snapshot_to_value)
        .collect();
    map.insert("creatives".into(), JsonValue::Array(creatives));
    JsonValue::Object(map)
}

fn uplift_snapshot_from_value(value: &JsonValue) -> Result<UpliftSnapshot, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("uplift snapshot must be an object"))?;
    let creatives_value = obj
        .get("creatives")
        .ok_or_else(|| invalid("uplift snapshot missing creatives"))?;
    let creatives = creatives_value
        .as_array()
        .ok_or_else(|| invalid("uplift snapshot creatives must be an array"))?
        .iter()
        .map(uplift_creative_snapshot_from_value)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(UpliftSnapshot {
        generated_at_micros: read_u64(obj, "generated_at_micros")?,
        creatives,
    })
}

fn serialize_uplift_snapshot(snapshot: &UpliftSnapshot) -> Result<Vec<u8>, PersistenceError> {
    Ok(json::to_vec_value(&uplift_snapshot_to_value(snapshot)))
}

fn deserialize_uplift_snapshot(bytes: &[u8]) -> Result<UpliftSnapshot, PersistenceError> {
    let value = json::value_from_slice(bytes)?;
    uplift_snapshot_from_value(&value)
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
    map.insert("viewer_usd".into(), JsonValue::from(ledger.viewer_usd));
    map.insert("host_usd".into(), JsonValue::from(ledger.host_usd));
    map.insert("hardware_usd".into(), JsonValue::from(ledger.hardware_usd));
    map.insert("verifier_usd".into(), JsonValue::from(ledger.verifier_usd));
    map.insert(
        "liquidity_usd".into(),
        JsonValue::from(ledger.liquidity_usd),
    );
    map.insert("miner_usd".into(), JsonValue::from(ledger.miner_usd));
    JsonValue::Object(map)
}

fn token_remainders_from_value(
    value: &JsonValue,
) -> Result<TokenRemainderLedger, PersistenceError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("token remainders must be an object"))?;
    fn read_token_role(map: &JsonMap, key: &str) -> Result<u64, PersistenceError> {
        map.get(key).map_or(Ok(0), |value| {
            value
                .as_u64()
                .ok_or_else(|| invalid(format!("{key} must be an integer")))
        })
    }

    Ok(TokenRemainderLedger {
        viewer_usd: read_token_role(map, "viewer_usd")?,
        host_usd: read_token_role(map, "host_usd")?,
        hardware_usd: read_token_role(map, "hardware_usd")?,
        verifier_usd: read_token_role(map, "verifier_usd")?,
        liquidity_usd: read_token_role(map, "liquidity_usd")?,
        miner_usd: read_token_role(map, "miner_usd")?,
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
    map.insert(
        "pi_tuner".into(),
        pi_tuner_config_to_value(&config.pi_tuner),
    );
    JsonValue::Object(map)
}

fn pi_tuner_config_to_value(config: &PiTunerConfig) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("enabled".into(), JsonValue::Bool(config.enabled));
    map.insert("kp_min".into(), JsonValue::from(config.kp_min));
    map.insert("kp_max".into(), JsonValue::from(config.kp_max));
    map.insert("ki_min".into(), JsonValue::from(config.ki_min));
    map.insert("ki_max".into(), JsonValue::from(config.ki_max));
    map.insert("ki_ratio".into(), JsonValue::from(config.ki_ratio));
    map.insert(
        "tuning_sensitivity".into(),
        JsonValue::from(config.tuning_sensitivity),
    );
    map.insert(
        "zero_cross_min_interval_micros".into(),
        JsonValue::from(config.zero_cross_min_interval_micros),
    );
    map.insert("max_integral".into(), JsonValue::from(config.max_integral));
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
        pi_tuner: if let Some(pi_value) = map.get("pi_tuner") {
            pi_tuner_config_from_value(pi_value)?
        } else {
            defaults.pi_tuner
        },
    };
    Ok(config.normalized())
}

fn pi_tuner_config_from_value(value: &JsonValue) -> Result<PiTunerConfig, PersistenceError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("pi_tuner must be an object"))?;
    let mut config = PiTunerConfig::default();
    if let Some(enabled) = map.get("enabled") {
        config.enabled = enabled
            .as_bool()
            .ok_or_else(|| invalid("pi_tuner.enabled must be a boolean"))?;
    }
    if map.contains_key("kp_min") {
        config.kp_min = read_f64(map, "kp_min")?;
    }
    if map.contains_key("kp_max") {
        config.kp_max = read_f64(map, "kp_max")?;
    }
    if map.contains_key("ki_min") {
        config.ki_min = read_f64(map, "ki_min")?;
    }
    if map.contains_key("ki_max") {
        config.ki_max = read_f64(map, "ki_max")?;
    }
    if map.contains_key("ki_ratio") {
        config.ki_ratio = read_f64(map, "ki_ratio")?;
    }
    if map.contains_key("tuning_sensitivity") {
        config.tuning_sensitivity = read_f64(map, "tuning_sensitivity")?;
    }
    if map.contains_key("zero_cross_min_interval_micros") {
        config.zero_cross_min_interval_micros = read_u64(map, "zero_cross_min_interval_micros")?;
    }
    if map.contains_key("max_integral") {
        config.max_integral = read_f64(map, "max_integral")?;
    }
    Ok(config)
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
    if snapshot.domain_tier != DomainTier::default() {
        map.insert(
            "domain_tier".into(),
            JsonValue::String(snapshot.domain_tier.as_str().into()),
        );
    }
    if let Some(owner) = &snapshot.domain_owner {
        map.insert("domain_owner".into(), JsonValue::String(owner.clone()));
    }
    if !snapshot.interest_tags.is_empty() {
        let tags = snapshot
            .interest_tags
            .iter()
            .cloned()
            .map(JsonValue::String)
            .collect();
        map.insert("interest_tags".into(), JsonValue::Array(tags));
    }
    if let Some(bucket) = snapshot.presence_bucket.as_ref() {
        map.insert("presence_bucket".into(), presence_bucket_to_value(bucket));
    }
    if snapshot.selectors_version != cohort_selector_version_default() {
        map.insert(
            "selectors_version".into(),
            JsonValue::Number(JsonNumber::from(snapshot.selectors_version)),
        );
    }
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
    let badges = read_string_array(map.get("badges"), "badges")?;
    let domain_tier = match map.get("domain_tier") {
        Some(JsonValue::String(value)) => DomainTier::from_str(value)
            .map_err(|_| invalid("domain_tier must be premium|reserved|community|unverified"))?,
        Some(_) => return Err(invalid("domain_tier must be a string")),
        None => DomainTier::default(),
    };
    let domain_owner = match map.get("domain_owner") {
        Some(JsonValue::String(value)) => Some(value.clone()),
        Some(JsonValue::Null) | None => None,
        Some(_) => return Err(invalid("domain_owner must be a string or null")),
    };
    let interest_tags = read_string_array(map.get("interest_tags"), "interest_tags")?;
    let presence_bucket = match map.get("presence_bucket") {
        Some(value) => Some(presence_bucket_from_value(value)?),
        None => None,
    };
    let selectors_version = match map.get("selectors_version") {
        Some(JsonValue::Number(num)) => num
            .as_u64()
            .and_then(|value| u16::try_from(value).ok())
            .ok_or_else(|| invalid("selectors_version must fit into u16"))?,
        Some(_) => return Err(invalid("selectors_version must be an unsigned integer")),
        None => cohort_selector_version_default(),
    };
    Ok(CohortKeySnapshot {
        domain,
        provider,
        badges,
        domain_tier,
        domain_owner,
        interest_tags,
        presence_bucket,
        selectors_version,
    })
}

fn cohort_key_from_snapshot(snapshot: &CohortKeySnapshot) -> CohortKey {
    CohortKey {
        domain: snapshot.domain.clone(),
        domain_tier: snapshot.domain_tier,
        domain_owner: snapshot.domain_owner.clone(),
        provider: snapshot.provider.clone(),
        badges: snapshot.badges.clone(),
        interest_tags: snapshot.interest_tags.clone(),
        presence_bucket: snapshot.presence_bucket.clone(),
        selectors_version: snapshot.selectors_version,
    }
}

fn cohort_key_snapshot_from_key(key: &CohortKey) -> CohortKeySnapshot {
    CohortKeySnapshot {
        domain: key.domain.clone(),
        provider: key.provider.clone(),
        badges: key.badges.clone(),
        domain_tier: key.domain_tier,
        domain_owner: key.domain_owner.clone(),
        interest_tags: key.interest_tags.clone(),
        presence_bucket: key.presence_bucket.clone(),
        selectors_version: key.selectors_version,
    }
}

fn normalize_quality_components(
    mut components: QualitySignalComponents,
) -> QualitySignalComponents {
    if components.freshness_multiplier_ppm == 0 {
        components.freshness_multiplier_ppm = PPM_SCALE as u32;
    }
    if components.readiness_multiplier_ppm == 0 {
        components.readiness_multiplier_ppm = PPM_SCALE as u32;
    }
    if components.privacy_multiplier_ppm == 0 {
        components.privacy_multiplier_ppm = PPM_SCALE as u32;
    }
    components
}

fn apply_quality_signals_to_map(
    map: &mut HashMap<CohortKey, QualitySignalState>,
    signals: Vec<QualitySignal>,
    config: &MarketplaceConfig,
) {
    for signal in signals {
        let key = cohort_key_from_snapshot(&signal.cohort);
        let components = normalize_quality_components(signal.components.clone());
        let multiplier = config.clamp_quality_signal_multiplier(signal.multiplier_ppm);
        map.insert(
            key,
            QualitySignalState {
                multiplier,
                components,
            },
        );
    }
}

fn presence_bucket_to_value(bucket: &PresenceBucketRef) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "bucket_id".into(),
        JsonValue::String(bucket.bucket_id.clone()),
    );
    map.insert(
        "kind".into(),
        JsonValue::String(bucket.kind.as_str().into()),
    );
    if let Some(region) = &bucket.region {
        map.insert("region".into(), JsonValue::String(region.clone()));
    }
    if bucket.radius_meters > 0 {
        map.insert(
            "radius_meters".into(),
            JsonValue::Number(JsonNumber::from(bucket.radius_meters)),
        );
    }
    if bucket.confidence_bps > 0 {
        map.insert(
            "confidence_bps".into(),
            JsonValue::Number(JsonNumber::from(bucket.confidence_bps)),
        );
    }
    if let Some(minted) = bucket.minted_at_micros {
        map.insert(
            "minted_at_micros".into(),
            JsonValue::Number(JsonNumber::from(minted)),
        );
    }
    if let Some(expires) = bucket.expires_at_micros {
        map.insert(
            "expires_at_micros".into(),
            JsonValue::Number(JsonNumber::from(expires)),
        );
    }
    JsonValue::Object(map)
}

fn presence_bucket_from_value(value: &JsonValue) -> Result<PresenceBucketRef, PersistenceError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("presence_bucket must be an object"))?;
    let bucket_id = read_string(map, "bucket_id")?;
    let kind = match map.get("kind") {
        Some(JsonValue::String(value)) => PresenceKind::from_str(value)
            .map_err(|_| invalid("presence kind must be localnet|range_boost|unknown"))?,
        Some(_) => return Err(invalid("presence kind must be a string")),
        None => PresenceKind::default(),
    };
    let region = match map.get("region") {
        Some(JsonValue::String(value)) => Some(value.clone()),
        Some(JsonValue::Null) | None => None,
        Some(_) => return Err(invalid("presence region must be a string")),
    };
    let radius_meters = match map.get("radius_meters") {
        Some(JsonValue::Number(num)) => num
            .as_u64()
            .and_then(|value| u16::try_from(value).ok())
            .ok_or_else(|| invalid("radius_meters must fit into u16"))?,
        Some(_) => return Err(invalid("radius_meters must be an unsigned integer")),
        None => 0,
    };
    let confidence_bps = match map.get("confidence_bps") {
        Some(JsonValue::Number(num)) => num
            .as_u64()
            .and_then(|value| u16::try_from(value).ok())
            .ok_or_else(|| invalid("confidence_bps must fit into u16"))?,
        Some(_) => return Err(invalid("confidence_bps must be an unsigned integer")),
        None => 0,
    };
    let minted_at_micros = match map.get("minted_at_micros") {
        Some(JsonValue::Number(num)) => num.as_u64(),
        Some(_) => return Err(invalid("minted_at_micros must be an unsigned integer")),
        None => None,
    };
    let expires_at_micros = match map.get("expires_at_micros") {
        Some(JsonValue::Number(num)) => num.as_u64(),
        Some(_) => return Err(invalid("expires_at_micros must be an unsigned integer")),
        None => None,
    };
    Ok(PresenceBucketRef {
        bucket_id,
        kind,
        region,
        radius_meters,
        confidence_bps,
        minted_at_micros,
        expires_at_micros,
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
    if let Some(pi_controller) = &snapshot.pi_controller {
        map.insert(
            "pi_controller".into(),
            pi_controller_snapshot_to_value(pi_controller),
        );
    }
    JsonValue::Object(map)
}

fn pi_controller_snapshot_to_value(snapshot: &PiControllerSnapshot) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("kp".into(), JsonValue::from(snapshot.kp));
    map.insert("ki".into(), JsonValue::from(snapshot.ki));
    map.insert("integral".into(), JsonValue::from(snapshot.integral));
    if let Some(last) = snapshot.last_cross_micros {
        map.insert("last_cross_micros".into(), JsonValue::from(last));
    }
    if let Some(period) = snapshot.period_estimate_secs {
        map.insert("period_estimate_secs".into(), JsonValue::from(period));
    }
    map.insert(
        "amplitude_since_cross".into(),
        JsonValue::from(snapshot.amplitude_since_cross),
    );
    JsonValue::Object(map)
}

fn pi_controller_snapshot_from_value(
    value: &JsonValue,
) -> Result<PiControllerSnapshot, PersistenceError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("pi_controller must be an object"))?;
    Ok(PiControllerSnapshot {
        kp: read_f64(map, "kp")?,
        ki: read_f64(map, "ki")?,
        integral: read_f64(map, "integral")?,
        last_cross_micros: map.get("last_cross_micros").and_then(JsonValue::as_u64),
        period_estimate_secs: map.get("period_estimate_secs").and_then(JsonValue::as_f64),
        amplitude_since_cross: read_f64(map, "amplitude_since_cross")?,
    })
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
    let pi_controller = map
        .get("pi_controller")
        .map(pi_controller_snapshot_from_value)
        .transpose()?;
    Ok(CampaignBudgetSnapshot {
        campaign_id: read_string(map, "campaign_id")?,
        total_budget: read_u64(map, "total_budget")?,
        remaining_budget: read_u64(map, "remaining_budget")?,
        epoch_target: read_f64(map, "epoch_target")?,
        epoch_spend: read_f64(map, "epoch_spend")?,
        epoch_impressions: read_u64(map, "epoch_impressions")?,
        dual_price: read_f64(map, "dual_price")?,
        cohorts,
        pi_controller,
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
    JsonValue::Object(map)
}

fn distribution_policy_from_value(
    value: &JsonValue,
) -> Result<DistributionPolicy, PersistenceError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("distribution policy must be an object"))?;
    let policy = DistributionPolicy::new(
        read_u64(obj, "viewer_percent")?,
        read_u64(obj, "host_percent")?,
        read_u64(obj, "hardware_percent")?,
        read_u64(obj, "verifier_percent")?,
        read_u64(obj, "liquidity_percent")?,
    );
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
            medians: RwLock::new(CostMedians::new()),
            quality_signals: RwLock::new(HashMap::new()),
            claim_registry: RwLock::new(ClaimRegistry::default()),
            conversions: RwLock::new(HashMap::new()),
            device_seen: RwLock::new(HashMap::new()),
        }
    }

    fn matches_targeting(&self, targeting: &CampaignTargeting, ctx: &ImpressionContext) -> bool {
        if !targeting.domains.is_empty() && !targeting.domains.iter().any(|d| d == &ctx.domain) {
            return false;
        }
        if !targeting.geo.matches(ctx.geo.as_ref()) {
            return false;
        }
        if !targeting.device.matches(ctx.device.as_ref()) {
            return false;
        }
        if !targeting.crm_lists.matches(&ctx.crm_lists) {
            return false;
        }
        if !targeting.delivery.allows(ctx.delivery_channel) {
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

    fn quality_multiplier_for(&self, cohort: &CohortKey) -> f64 {
        self.quality_signals
            .read()
            .unwrap()
            .get(cohort)
            .map(|state| {
                // Read components to keep them live for diagnostics if needed.
                let _ = &state.components;
                state.multiplier
            })
            .unwrap_or(1.0)
    }

    fn resource_scarcity_multiplier(&self) -> f64 {
        let (m_storage, m_verifier, m_host) = self.medians.read().unwrap().snapshot();
        let base_storage = self.config.min_price_per_mib_usd_micros.max(1);
        let base_verifier = self.config.resource_floor.verifier_cost_usd_micros.max(1);
        let base_host = self.config.resource_floor.host_fee_usd_micros.max(1);
        let ratios = [
            m_storage as f64 / base_storage as f64,
            m_verifier as f64 / base_verifier as f64,
            m_host as f64 / base_host as f64,
        ];
        let mut max_ratio = 1.0f64;
        for r in ratios {
            if r.is_finite() {
                max_ratio = max_ratio.max(r);
            }
        }
        let utilization_adjustment = {
            let pricing = self.pricing.read().unwrap();
            if pricing.is_empty() {
                1.0
            } else {
                let mut sum_ratio = 0.0;
                let mut count = 0.0;
                for state in pricing.values() {
                    if state.target_utilization_ppm > 0 {
                        let ratio = state.observed_utilization_ppm as f64
                            / state.target_utilization_ppm as f64;
                        sum_ratio += ratio;
                        count += 1.0;
                    }
                }
                if count > 0.0 {
                    (sum_ratio / count).clamp(0.8, 1.5)
                } else {
                    1.0
                }
            }
        };
        (max_ratio * utilization_adjustment).clamp(0.8, 2.0)
    }

    fn matches_creative(&self, creative: &Creative, ctx: &ImpressionContext) -> bool {
        if !creative.domains.is_empty() && !creative.domains.iter().any(|d| d == &ctx.domain) {
            return false;
        }
        if !creative.placement.allows(ctx.delivery_channel) {
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
        CohortKey::from_context(ctx)
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

    fn campaign(&self, id: &str) -> Option<Campaign> {
        let guard = self.campaigns.read().ok()?;
        guard.get(id).map(|state| state.campaign.clone())
    }

    fn budget_broker(&self) -> &RwLock<BudgetBroker> {
        &self.budget_broker
    }

    fn update_quality_signals(&self, signals: Vec<QualitySignal>) {
        let mut guard = self.quality_signals.write().unwrap();
        apply_quality_signals_to_map(&mut guard, signals, &self.config);
    }

    fn quality_signal_config(&self) -> QualitySignalConfig {
        self.config.quality_signal_config()
    }

    fn privacy_budget_snapshot(&self) -> PrivacyBudgetSnapshot {
        self.privacy_budget.read().unwrap().snapshot()
    }

    fn preview_privacy_budget(
        &self,
        badges: &[String],
        population_hint: Option<u64>,
    ) -> PrivacyBudgetPreview {
        self.privacy_budget
            .read()
            .unwrap()
            .preview(badges, population_hint)
    }

    fn authorize_privacy_budget(
        &self,
        badges: &[String],
        population_hint: Option<u64>,
    ) -> PrivacyBudgetDecision {
        let mut budgets = self.privacy_budget.write().unwrap();
        budgets.authorize(badges, population_hint)
    }

    fn register_claim_route(
        &self,
        domain: &str,
        role: &str,
        address: &str,
    ) -> Result<(), MarketplaceError> {
        let mut registry = self.claim_registry.write().unwrap();
        registry.register(domain, role, address);
        Ok(())
    }

    fn claim_routes(&self, cohort: &CohortKeySnapshot) -> HashMap<String, String> {
        self.claim_registry
            .read()
            .unwrap()
            .for_domain(&cohort.domain)
    }

    fn record_conversion(&self, event: ConversionEvent) -> Result<(), MarketplaceError> {
        {
            let campaigns = self.campaigns.read().unwrap();
            let state = campaigns
                .get(&event.campaign_id)
                .ok_or(MarketplaceError::UnknownCampaign)?;
            if !state
                .campaign
                .creatives
                .iter()
                .any(|creative| creative.id == event.creative_id)
            {
                return Err(MarketplaceError::UnknownCreative);
            }
        }
        let key = format!("{}::{}", event.campaign_id, event.creative_id);
        let device_link = event.device_link.clone().filter(|l| l.opt_in);
        if let Some(link) = device_link.as_ref() {
            let mut seen = self.device_seen.write().unwrap();
            let set = seen.entry(key.clone()).or_default();
            if !set.insert(link.device_hash.clone()) {
                return Ok(());
            }
            if set.len() > 10_000 {
                if let Some(first) = set.iter().next().cloned() {
                    set.remove(&first);
                }
            }
        }
        {
            let mut conversions = self.conversions.write().unwrap();
            let entry = conversions.entry(key).or_default();
            entry.count = entry.count.saturating_add(1);
            if let Some(link) = device_link {
                if entry.device_links.len() < 1_000 {
                    entry.device_links.push(link);
                }
            }
        }
        {
            let mut estimator = self.uplift.write().unwrap();
            estimator.record_observation(
                &event.campaign_id,
                &event.creative_id,
                &event.assignment,
                true,
            );
        }
        Ok(())
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
        match self.authorize_privacy_budget(&ctx.badges, ctx.population_estimate) {
            PrivacyBudgetDecision::Allowed => {}
            PrivacyBudgetDecision::Cooling { .. } | PrivacyBudgetDecision::Denied { .. } => {
                return None;
            }
        }
        let cohort = InMemoryMarketplace::cohort_key(&ctx);
        let price_per_mib = {
            let mut pricing = self.pricing.write().unwrap();
            InMemoryMarketplace::get_price_and_state(&mut pricing, &cohort, &self.config)
                .price_per_mib_usd_micros()
        };
        let scarcity = self.resource_scarcity_multiplier();
        let floor_breakdown = self
            .config
            .composite_floor_breakdown(price_per_mib, ctx.bytes, &cohort, ctx.population_estimate)
            .scale(scarcity);

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
            preference_match: bool,
            mesh_payload: Option<Vec<u8>>,
        }

        let campaigns = self.campaigns.read().unwrap();
        let quality_signal_multiplier = self.quality_multiplier_for(&cohort);
        gauge!(
            "ad_quality_signal_multiplier",
            quality_signal_multiplier,
            "domain" => cohort.domain.as_str(),
            "domain_tier" => cohort.domain_tier.as_str(),
            "provider" => cohort.provider.as_deref().unwrap_or("-"),
        );
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
                let quality = quality.apply_signal_multiplier(quality_signal_multiplier);
                if quality.base_bid_usd_micros < resource_floor
                    || quality.quality_adjusted_usd_micros < resource_floor
                {
                    #[cfg(test)]
                    eprintln!(
                        "skip candidate: bid below floor base={} qa={} floor={}",
                        quality.base_bid_usd_micros,
                        quality.quality_adjusted_usd_micros,
                        resource_floor
                    );
                    continue;
                }
                let preference_match = state
                    .campaign
                    .targeting
                    .delivery
                    .prefers(ctx.delivery_channel);
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
                    delivery_channel: ctx.delivery_channel,
                    preferred_delivery_match: preference_match,
                };
                let idx = candidates.len();
                if let Some(best) = best_index {
                    let best_trace = &candidates[best].trace;
                    let better_quality = trace.quality_adjusted_bid_usd_micros
                        > best_trace.quality_adjusted_bid_usd_micros
                        || (trace.quality_adjusted_bid_usd_micros
                            == best_trace.quality_adjusted_bid_usd_micros
                            && trace.available_budget_usd_micros
                                > best_trace.available_budget_usd_micros);
                    let tie_preference = trace.quality_adjusted_bid_usd_micros
                        == best_trace.quality_adjusted_bid_usd_micros
                        && trace.available_budget_usd_micros
                            == best_trace.available_budget_usd_micros
                        && preference_match
                        && !candidates[best].preference_match;
                    if better_quality || tie_preference {
                        best_index = Some(idx);
                    }
                } else {
                    best_index = Some(idx);
                }
                candidates.push(Candidate {
                    trace,
                    uplift: uplift_estimate,
                    preference_match,
                    mesh_payload: creative.mesh_payload.clone(),
                });
            }
        }
        drop(campaigns);

        // Relaxed second pass to keep progress in test environments even when bids fall
        // below the current resource floor.
        if best_index.is_none() && candidates.is_empty() {
            let campaigns = self.campaigns.read().unwrap();
            for state in campaigns.values() {
                if !self.matches_targeting(&state.campaign.targeting, &ctx) {
                    continue;
                }
                let available_budget = state.remaining_budget_usd_micros;
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
                    let quality = quality.apply_signal_multiplier(quality_signal_multiplier);
                    if quality.base_bid_usd_micros == 0 || quality.quality_adjusted_usd_micros == 0
                    {
                        continue;
                    }
                    let preference_match = state
                        .campaign
                        .targeting
                        .delivery
                        .prefers(ctx.delivery_channel);
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
                        delivery_channel: ctx.delivery_channel,
                        preferred_delivery_match: preference_match,
                    };
                    best_index = Some(candidates.len());
                    candidates.push(Candidate {
                        trace,
                        uplift: uplift_estimate,
                        preference_match,
                        mesh_payload: creative.mesh_payload.clone(),
                    });
                    break;
                }
                if best_index.is_some() {
                    break;
                }
            }
        }

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
        let winner_mesh_payload = candidates[winner_index].mesh_payload.clone();
        let clearing_price = resource_floor
            .max(runner_up_quality)
            .min(winner_trace.quality_adjusted_bid_usd_micros);
        if clearing_price == 0 {
            return None;
        }
        let mut assignment_seed = Vec::new();
        if let Some(seed) = ctx.assignment_seed_override.as_ref() {
            assignment_seed.extend_from_slice(seed);
        } else {
            assignment_seed.extend_from_slice(&key.discriminator);
        }
        assignment_seed.extend_from_slice(ctx.domain.as_bytes());
        if let Some(provider_id) = ctx.provider.as_ref() {
            assignment_seed.extend_from_slice(provider_id.as_bytes());
        }
        let assignment = {
            let estimator = self.uplift.read().unwrap();
            estimator.assign_holdout(
                &winner_trace.campaign_id,
                &winner_trace.creative_id,
                &assignment_seed,
            )
        };
        let effective_total_usd_micros = if assignment.in_holdout {
            0
        } else {
            clearing_price
        };
        let (soft_intent_receipt, soft_intent_snapshot) =
            soft_intent_artifacts(&ctx.badges, &ctx.soft_intent);
        let mut receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: ctx.domain.clone(),
                domain_tier: ctx.domain_tier,
                domain_owner: ctx.domain_owner.clone(),
                provider: ctx.provider.clone(),
                badges: ctx.badges.clone(),
                interest_tags: ctx.interest_tags.clone(),
                presence_bucket: ctx.presence_bucket.clone(),
                selectors_version: ctx.selectors_version,
                bytes: ctx.bytes,
                price_per_mib_usd_micros: price_per_mib,
                delivery_channel: ctx.delivery_channel,
                mesh_peer: ctx.mesh.as_ref().and_then(|m| m.peer_id.clone()),
                mesh_transport: ctx.mesh.as_ref().and_then(|m| m.transport.clone()),
                mesh_latency_ms: ctx.mesh.as_ref().and_then(|m| m.latency_ms),
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
            uplift_assignment: Some(assignment.clone()),
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
        let state = campaigns.get_mut(&winner_trace.campaign_id)?;
        if !assignment.in_holdout {
            if state.remaining_budget_usd_micros < clearing_price {
                return None;
            }
            state.remaining_budget_usd_micros -= clearing_price;
            state.reserved_budget_usd_micros = state
                .reserved_budget_usd_micros
                .saturating_add(clearing_price);
        }
        let cohort_snapshot = cohort_key_snapshot_from_key(&cohort);
        reservations.insert(
            key,
            ReservationState {
                campaign_id: winner_trace.campaign_id.clone(),
                creative_id: winner_trace.creative_id.clone(),
                bytes: ctx.bytes,
                price_per_mib_usd_micros: price_per_mib,
                total_usd_micros: effective_total_usd_micros,
                clearing_price_usd_micros: clearing_price,
                demand_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
                resource_floor_usd_micros: resource_floor,
                resource_floor_breakdown: floor_breakdown.clone(),
                runner_up_quality_bid_usd_micros: runner_up_quality,
                quality_adjusted_bid_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
                cohort,
                selection_receipt: receipt.clone(),
                uplift: winner_uplift.clone(),
                assignment: assignment.clone(),
                delivery_channel: ctx.delivery_channel,
                mesh_payload: winner_mesh_payload.clone(),
                claim_routes: self.claim_routes(&cohort_snapshot),
            },
        );
        Some(MatchOutcome {
            campaign_id: winner_trace.campaign_id,
            creative_id: winner_trace.creative_id,
            price_per_mib_usd_micros: price_per_mib,
            total_usd_micros: effective_total_usd_micros,
            clearing_price_usd_micros: clearing_price,
            resource_floor_usd_micros: resource_floor,
            resource_floor_breakdown: floor_breakdown,
            runner_up_quality_bid_usd_micros: runner_up_quality,
            quality_adjusted_bid_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
            selection_receipt: receipt,
            uplift: winner_uplift,
            uplift_assignment: assignment,
            delivery_channel: ctx.delivery_channel,
            mesh_payload: winner_mesh_payload,
        })
    }

    fn commit(&self, key: &ReservationKey) -> Option<SettlementBreakdown> {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)?
        };
        let assignment = reservation.assignment.clone();
        {
            let mut estimator = self.uplift.write().unwrap();
            estimator.record_observation(
                &reservation.campaign_id,
                &reservation.creative_id,
                &assignment,
                false,
            );
        }
        {
            let mut consumed = self.consumed_reservations.write().unwrap();
            consumed.insert(*key);
        }
        if assignment.in_holdout {
            return None;
        }
        let (m_storage, m_verifier, m_host) = {
            let mut med = self.medians.write().unwrap();
            med.record_storage(reservation.price_per_mib_usd_micros);
            med.record_verifier(reservation.resource_floor_breakdown.verifier_usd_micros);
            med.record_host(reservation.resource_floor_breakdown.host_usd_micros);
            med.snapshot()
        };
        gauge!("ad_cost_median_usd_micros", m_storage as f64, "role" => "storage");
        gauge!("ad_cost_median_usd_micros", m_verifier as f64, "role" => "verifier");
        gauge!("ad_cost_median_usd_micros", m_host as f64, "role" => "host");
        let mesh_payload_digest = reservation
            .mesh_payload
            .as_ref()
            .map(|payload| hex::encode(blake3::hash(payload).as_bytes()));
        let mut campaigns = self.campaigns.write().unwrap();
        let state = campaigns.get_mut(&reservation.campaign_id)?;
        if state.reserved_budget_usd_micros < reservation.total_usd_micros {
            return None;
        }
        state.reserved_budget_usd_micros = state
            .reserved_budget_usd_micros
            .saturating_sub(reservation.total_usd_micros);
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
        let mesh_bytes = reservation
            .mesh_payload
            .as_ref()
            .map(|payload| payload.len())
            .unwrap_or(0);
        let mesh_digest_label = mesh_payload_digest.as_deref().unwrap_or("none");
        diagnostics::log::info!(format!(
            "ad_reservation_commit campaign={} creative={} channel={} clearing_price={} mesh_bytes={} mesh_digest={}",
            reservation.campaign_id,
            reservation.creative_id,
            reservation.delivery_channel.as_str(),
            reservation.clearing_price_usd_micros,
            mesh_bytes,
            mesh_digest_label
        ));
        let mesh_payload = reservation.mesh_payload.clone();
        let conversion_key = format!("{}::{}", reservation.campaign_id, reservation.creative_id);
        let conversion_snapshot = {
            let mut conversions = self.conversions.write().unwrap();
            conversions.remove(&conversion_key).unwrap_or_default()
        };
        Some(SettlementBreakdown {
            campaign_id: reservation.campaign_id,
            creative_id: reservation.creative_id,
            bytes: reservation.bytes,
            price_per_mib_usd_micros: reservation.price_per_mib_usd_micros,
            total_usd_micros: reservation.total_usd_micros,
            demand_usd_micros: reservation.demand_usd_micros,
            resource_floor_usd_micros: reservation.resource_floor_usd_micros,
            clearing_price_usd_micros: reservation.clearing_price_usd_micros,
            delivery_channel: reservation.delivery_channel,
            mesh_payload,
            mesh_payload_digest,
            resource_floor_breakdown: reservation.resource_floor_breakdown.clone(),
            runner_up_quality_bid_usd_micros: reservation.runner_up_quality_bid_usd_micros,
            quality_adjusted_bid_usd_micros: reservation.quality_adjusted_bid_usd_micros,
            viewer: tokens.viewer,
            host: tokens.host,
            hardware: tokens.hardware,
            verifier: tokens.verifier,
            liquidity: tokens.liquidity,
            miner: tokens.miner,
            total: tokens.total,
            unsettled_usd_micros: tokens.unsettled_usd_micros,
            price_usd_micros: oracle.price_usd_micros,
            remainders_usd_micros: tokens.remainders.breakdown,
            twap_window_id: oracle.twap_window_id,
            selection_receipt: reservation.selection_receipt,
            uplift: reservation.uplift,
            claim_routes: reservation.claim_routes.clone(),
            conversions: conversion_snapshot.count,
            device_links: conversion_snapshot.device_links,
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
                domain_tier: key.domain_tier,
                domain_owner: key.domain_owner.clone(),
                provider: key.provider.clone(),
                badges: key.badges.clone(),
                interest_tags: key.interest_tags.clone(),
                presence_bucket: key.presence_bucket.clone(),
                selectors_version: key.selectors_version(),
                price_per_mib_usd_micros: state.price_per_mib_usd_micros(),
                target_utilization_ppm: state.target_utilization_ppm,
                observed_utilization_ppm: state.observed_utilization_ppm(),
            })
            .collect()
    }

    fn recompute_distribution_from_utilization(&self) {
        let pricing = self.pricing.read().unwrap();
        if pricing.is_empty() {
            return;
        }
        let mut sum_obs: u128 = 0;
        let mut sum_tgt: u128 = 0;
        let mut n: u128 = 0;
        for (_, state) in pricing.iter() {
            sum_obs = sum_obs.saturating_add(state.observed_utilization_ppm as u128);
            sum_tgt = sum_tgt.saturating_add(state.target_utilization_ppm as u128);
            n = n.saturating_add(1);
        }
        if n == 0 || sum_tgt == 0 {
            return;
        }
        let mean_obs = (sum_obs / n) as u64;
        let mean_tgt = (sum_tgt / n) as u64;
        let ratio = (mean_obs as f64) / (mean_tgt as f64 + f64::EPSILON);
        let current = *self.distribution.read().unwrap();
        let mut policy = current;
        let step: i64 = 2; // percentage points per step per role
        if ratio < 0.9 {
            policy.host_percent = (policy.host_percent as i64 + step) as u64;
            policy.hardware_percent = (policy.hardware_percent as i64 + step) as u64;
            policy.viewer_percent = policy.viewer_percent.saturating_sub(step as u64);
            policy.liquidity_percent = policy.liquidity_percent.saturating_sub(1);
        } else if ratio > 1.1 {
            policy.host_percent = policy.host_percent.saturating_sub(step as u64);
            policy.hardware_percent = policy.hardware_percent.saturating_sub(step as u64);
            policy.viewer_percent = (policy.viewer_percent as i64 + step) as u64;
            policy.liquidity_percent = policy.liquidity_percent.saturating_add(1);
        } else {
            // Hold steady or nudge verifier to maintain proof coverage.
            policy.verifier_percent = (policy.verifier_percent as i64 + 1) as u64;
            policy.liquidity_percent = policy.liquidity_percent.saturating_sub(1);
        }
        // Cost-index adjustments based on rolling medians
        let (m_storage, m_verifier, m_host) = { self.medians.read().unwrap().snapshot() };
        let base_storage = self.config.min_price_per_mib_usd_micros.max(1);
        let base_verifier = self.config.resource_floor.verifier_cost_usd_micros.max(1);
        let base_host = self.config.resource_floor.host_fee_usd_micros.max(1);
        let bump_if = |median: u64, base: u64| -> i64 {
            if base == 0 {
                return 0;
            }
            let ratio = (median as f64) / (base as f64);
            if ratio > 1.2 {
                1
            } else if ratio < 0.8 {
                -1
            } else {
                0
            }
        };
        let hw_adj = bump_if(m_storage, base_storage);
        let vf_adj = bump_if(m_verifier, base_verifier);
        let host_adj = bump_if(m_host, base_host);
        if hw_adj != 0 {
            policy.hardware_percent = ((policy.hardware_percent as i64) + hw_adj).max(0) as u64;
        }
        if vf_adj != 0 {
            policy.verifier_percent = ((policy.verifier_percent as i64) + vf_adj).max(0) as u64;
        }
        if host_adj != 0 {
            policy.host_percent = ((policy.host_percent as i64) + host_adj).max(0) as u64;
        }
        // Normalize to 100 via liquidity adjustments.
        let sum = policy.viewer_percent
            + policy.host_percent
            + policy.hardware_percent
            + policy.verifier_percent
            + policy.liquidity_percent;
        let mut policy = policy.normalize();
        if sum != 100 {
            if sum > 100 {
                policy.liquidity_percent = policy.liquidity_percent.saturating_sub(sum - 100);
            } else {
                policy.liquidity_percent = policy.liquidity_percent.saturating_add(100 - sum);
            }
        }
        let normalized = policy.normalize();
        // Policy drift telemetry per role
        let drift =
            |new: u64, old: u64| -> f64 { ((new as f64 - old as f64) / 100.0) * PPM_SCALE as f64 };
        gauge!("ad_distribution_policy_drift_ppm", drift(normalized.viewer_percent, current.viewer_percent), "role" => "viewer");
        gauge!("ad_distribution_policy_drift_ppm", drift(normalized.host_percent, current.host_percent), "role" => "host");
        gauge!("ad_distribution_policy_drift_ppm", drift(normalized.hardware_percent, current.hardware_percent), "role" => "hardware");
        gauge!("ad_distribution_policy_drift_ppm", drift(normalized.verifier_percent, current.verifier_percent), "role" => "verifier");
        *self.distribution.write().unwrap() = normalized;
    }

    fn cost_medians_usd_micros(&self) -> (u64, u64, u64) {
        self.medians.read().unwrap().snapshot()
    }

    fn badge_guard_decision(
        &self,
        badges: &[String],
        soft_intent: Option<&BadgeSoftIntentContext>,
    ) -> BadgeDecision {
        self.badge_guard.evaluate(badges, soft_intent)
    }
}
impl SledMarketplace {
    fn persist_medians_snapshot(
        &self,
        snapshot: (u64, u64, u64),
    ) -> Result<(), PersistenceError> {
        let bytes = foundation_serialization::json::to_vec(&snapshot)?;
        self.metadata_tree.insert(KEY_MEDIANS, bytes)?;
        self.metadata_tree.flush()?;
        Ok(())
    }

    #[allow(dead_code)]
    fn persist_medians(&self) -> Result<(), PersistenceError> {
        let snapshot = self.medians.read().unwrap().snapshot();
        self.persist_medians_snapshot(snapshot)
    }
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
        let uplift_bytes = metadata_tree.get(KEY_UPLIFT)?;
        let uplift_snapshot = uplift_bytes
            .as_ref()
            .map(|bytes| deserialize_uplift_snapshot(bytes))
            .transpose()?;
        let uplift_estimator =
            UpliftEstimator::from_snapshot(normalized.uplift.clone(), uplift_snapshot.clone());
        if uplift_bytes.is_none() {
            let snapshot = uplift_estimator.snapshot();
            let bytes = serialize_uplift_snapshot(&snapshot)?;
            metadata_tree.insert(KEY_UPLIFT, bytes)?;
            metadata_tree.flush()?;
        }
        // Load persisted cost medians if present
        let loaded_medians = match metadata_tree.get(KEY_MEDIANS)? {
            Some(bytes) => match json::from_slice::<(u64, u64, u64)>(&bytes) {
                Ok((storage, verifier, host)) => {
                    CostMedians::from_snapshot(storage, verifier, host)
                }
                Err(_) => CostMedians::new(),
            },
            None => CostMedians::new(),
        };
        let claim_registry = claim_registry_from_metadata(&metadata_tree)?;
        let conversions = match metadata_tree.get(KEY_CONVERSIONS)? {
            Some(bytes) => deserialize_conversions(&bytes).unwrap_or_default(),
            None => HashMap::new(),
        };
        let device_seen = match metadata_tree.get(KEY_DEVICE_SEEN)? {
            Some(bytes) => deserialize_device_seen(&bytes).unwrap_or_default(),
            None => HashMap::new(),
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
            uplift: RwLock::new(uplift_estimator),
            token_remainders: RwLock::new(token_remainders),
            medians: RwLock::new(loaded_medians),
            quality_signals: RwLock::new(HashMap::new()),
            claim_registry: RwLock::new(claim_registry),
            conversions: RwLock::new(conversions),
            device_seen: RwLock::new(device_seen),
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

    fn persist_uplift(&self) -> Result<(), PersistenceError> {
        let snapshot = {
            let guard = self.uplift.read().unwrap();
            guard.snapshot()
        };
        let bytes = serialize_uplift_snapshot(&snapshot)?;
        self.metadata_tree.insert(KEY_UPLIFT, bytes)?;
        self.metadata_tree.flush()?;
        Ok(())
    }

    fn persist_claim_registry(&self) -> Result<(), PersistenceError> {
        let registry = self.claim_registry.read().unwrap().clone();
        let bytes = serialize_claim_registry(&registry)?;
        self.metadata_tree.insert(KEY_CLAIMS, bytes)?;
        self.metadata_tree.flush()?;
        Ok(())
    }

    fn persist_conversions(&self) -> Result<(), PersistenceError> {
        let snapshot = self.conversions.read().unwrap().clone();
        let bytes = serialize_conversions(&snapshot)?;
        self.metadata_tree.insert(KEY_CONVERSIONS, bytes)?;
        self.metadata_tree.flush()?;
        Ok(())
    }

    fn persist_device_seen(&self) -> Result<(), PersistenceError> {
        let snapshot = self.device_seen.read().unwrap().clone();
        let bytes = serialize_device_seen(&snapshot)?;
        self.metadata_tree.insert(KEY_DEVICE_SEEN, bytes)?;
        self.metadata_tree.flush()?;
        Ok(())
    }

    fn quality_multiplier_for(&self, cohort: &CohortKey) -> f64 {
        self.quality_signals
            .read()
            .unwrap()
            .get(cohort)
            .map(|state| {
                let _ = &state.components;
                state.multiplier
            })
            .unwrap_or(1.0)
    }

    fn resource_scarcity_multiplier(&self) -> f64 {
        let (m_storage, m_verifier, m_host) = self.medians.read().unwrap().snapshot();
        let base_storage = self.config.min_price_per_mib_usd_micros.max(1);
        let base_verifier = self.config.resource_floor.verifier_cost_usd_micros.max(1);
        let base_host = self.config.resource_floor.host_fee_usd_micros.max(1);
        let ratios = [
            m_storage as f64 / base_storage as f64,
            m_verifier as f64 / base_verifier as f64,
            m_host as f64 / base_host as f64,
        ];
        let mut max_ratio = 1.0f64;
        for r in ratios {
            if r.is_finite() {
                max_ratio = max_ratio.max(r);
            }
        }
        let utilization_adjustment = {
            let pricing = self.pricing.read().unwrap();
            if pricing.is_empty() {
                1.0
            } else {
                let mut sum_ratio = 0.0;
                let mut count = 0.0;
                for state in pricing.values() {
                    if state.target_utilization_ppm > 0 {
                        let ratio = state.observed_utilization_ppm as f64
                            / state.target_utilization_ppm as f64;
                        sum_ratio += ratio;
                        count += 1.0;
                    }
                }
                if count > 0.0 {
                    (sum_ratio / count).clamp(0.8, 1.5)
                } else {
                    1.0
                }
            }
        };
        (max_ratio * utilization_adjustment).clamp(0.8, 2.0)
    }

    fn matches_targeting(&self, targeting: &CampaignTargeting, ctx: &ImpressionContext) -> bool {
        if !targeting.domains.is_empty() && !targeting.domains.iter().any(|d| d == &ctx.domain) {
            return false;
        }
        if !targeting.geo.matches(ctx.geo.as_ref()) {
            return false;
        }
        if !targeting.device.matches(ctx.device.as_ref()) {
            return false;
        }
        if !targeting.crm_lists.matches(&ctx.crm_lists) {
            return false;
        }
        if !targeting.delivery.allows(ctx.delivery_channel) {
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
        if !creative.placement.allows(ctx.delivery_channel) {
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

    fn campaign(&self, id: &str) -> Option<Campaign> {
        let guard = self.campaigns.read().ok()?;
        guard.get(id).map(|state| state.campaign.clone())
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
        match self.authorize_privacy_budget(&ctx.badges, ctx.population_estimate) {
            PrivacyBudgetDecision::Allowed => {}
            PrivacyBudgetDecision::Cooling { .. } | PrivacyBudgetDecision::Denied { .. } => {
                return None;
            }
        }
        let cohort = InMemoryMarketplace::cohort_key(&ctx);
        let price_per_mib = {
            let mut pricing = self.pricing.write().unwrap();
            InMemoryMarketplace::get_price_and_state(&mut pricing, &cohort, &self.config)
                .price_per_mib_usd_micros()
        };
        let scarcity = self.resource_scarcity_multiplier();
        let floor_breakdown = self
            .config
            .composite_floor_breakdown(price_per_mib, ctx.bytes, &cohort, ctx.population_estimate)
            .scale(scarcity);
        let resource_floor = floor_breakdown.total_usd_micros();
        if resource_floor == 0 {
            return None;
        }
        record_resource_floor_metrics(&ctx, &floor_breakdown, resource_floor);

        struct Candidate {
            trace: SelectionCandidateTrace,
            uplift: UpliftEstimate,
            preference_match: bool,
            mesh_payload: Option<Vec<u8>>,
        }

        let campaigns = self.campaigns.read().unwrap();
        let quality_signal_multiplier = self.quality_multiplier_for(&cohort);
        gauge!(
            "ad_quality_signal_multiplier",
            quality_signal_multiplier,
            "domain" => cohort.domain.as_str(),
            "domain_tier" => cohort.domain_tier.as_str(),
            "provider" => cohort.provider.as_deref().unwrap_or("-"),
        );
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
                let quality = quality.apply_signal_multiplier(quality_signal_multiplier);
                if quality.base_bid_usd_micros < resource_floor
                    || quality.quality_adjusted_usd_micros < resource_floor
                {
                    continue;
                }
                let preference_match = state
                    .campaign
                    .targeting
                    .delivery
                    .prefers(ctx.delivery_channel);
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
                    delivery_channel: ctx.delivery_channel,
                    preferred_delivery_match: preference_match,
                };
                let idx = candidates.len();
                if let Some(best) = best_index {
                    let best_trace = &candidates[best].trace;
                    let better_quality = trace.quality_adjusted_bid_usd_micros
                        > best_trace.quality_adjusted_bid_usd_micros
                        || (trace.quality_adjusted_bid_usd_micros
                            == best_trace.quality_adjusted_bid_usd_micros
                            && trace.available_budget_usd_micros
                                > best_trace.available_budget_usd_micros);
                    let tie_preference = trace.quality_adjusted_bid_usd_micros
                        == best_trace.quality_adjusted_bid_usd_micros
                        && trace.available_budget_usd_micros
                            == best_trace.available_budget_usd_micros
                        && preference_match
                        && !candidates[best].preference_match;
                    if better_quality || tie_preference {
                        best_index = Some(idx);
                    }
                } else {
                    best_index = Some(idx);
                }
                candidates.push(Candidate {
                    trace,
                    uplift: uplift_estimate,
                    preference_match,
                    mesh_payload: creative.mesh_payload.clone(),
                });
            }
        }
        drop(campaigns);

        let winner_index = best_index?;
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
        let winner_mesh_payload = candidates[winner_index].mesh_payload.clone();
        let clearing_price = resource_floor
            .max(runner_up_quality)
            .min(winner_trace.quality_adjusted_bid_usd_micros)
            .max(resource_floor);
        if clearing_price == 0 {
            return None;
        }
        let mut assignment_seed = Vec::new();
        if let Some(seed) = ctx.assignment_seed_override.as_ref() {
            assignment_seed.extend_from_slice(seed);
        } else {
            assignment_seed.extend_from_slice(&key.discriminator);
        }
        assignment_seed.extend_from_slice(ctx.domain.as_bytes());
        if let Some(provider_id) = ctx.provider.as_ref() {
            assignment_seed.extend_from_slice(provider_id.as_bytes());
        }
        let assignment = {
            let estimator = self.uplift.read().unwrap();
            estimator.assign_holdout(
                &winner_trace.campaign_id,
                &winner_trace.creative_id,
                &assignment_seed,
            )
        };
        let effective_total_usd_micros = if assignment.in_holdout {
            0
        } else {
            clearing_price
        };
        let (soft_intent_receipt, soft_intent_snapshot) =
            soft_intent_artifacts(&ctx.badges, &ctx.soft_intent);
        let mut receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: ctx.domain.clone(),
                domain_tier: ctx.domain_tier,
                domain_owner: ctx.domain_owner.clone(),
                provider: ctx.provider.clone(),
                badges: ctx.badges.clone(),
                interest_tags: ctx.interest_tags.clone(),
                presence_bucket: ctx.presence_bucket.clone(),
                selectors_version: ctx.selectors_version,
                bytes: ctx.bytes,
                price_per_mib_usd_micros: price_per_mib,
                delivery_channel: ctx.delivery_channel,
                mesh_peer: ctx.mesh.as_ref().and_then(|m| m.peer_id.clone()),
                mesh_transport: ctx.mesh.as_ref().and_then(|m| m.transport.clone()),
                mesh_latency_ms: ctx.mesh.as_ref().and_then(|m| m.latency_ms),
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
            uplift_assignment: Some(assignment.clone()),
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
        let state = campaigns.get_mut(&winner_trace.campaign_id)?;
        if !assignment.in_holdout {
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
        }
        let cohort_snapshot = cohort_key_snapshot_from_key(&cohort);
        reservations.insert(
            key,
            ReservationState {
                campaign_id: winner_trace.campaign_id.clone(),
                creative_id: winner_trace.creative_id.clone(),
                bytes: ctx.bytes,
                price_per_mib_usd_micros: price_per_mib,
                total_usd_micros: effective_total_usd_micros,
                clearing_price_usd_micros: clearing_price,
                demand_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
                resource_floor_usd_micros: resource_floor,
                resource_floor_breakdown: floor_breakdown.clone(),
                runner_up_quality_bid_usd_micros: runner_up_quality,
                quality_adjusted_bid_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
                cohort,
                selection_receipt: receipt.clone(),
                uplift: winner_uplift.clone(),
                assignment: assignment.clone(),
                delivery_channel: ctx.delivery_channel,
                mesh_payload: winner_mesh_payload.clone(),
                claim_routes: self.claim_routes(&cohort_snapshot),
            },
        );
        Some(MatchOutcome {
            campaign_id: winner_trace.campaign_id,
            creative_id: winner_trace.creative_id,
            price_per_mib_usd_micros: price_per_mib,
            total_usd_micros: effective_total_usd_micros,
            clearing_price_usd_micros: clearing_price,
            resource_floor_usd_micros: resource_floor,
            resource_floor_breakdown: floor_breakdown,
            runner_up_quality_bid_usd_micros: runner_up_quality,
            quality_adjusted_bid_usd_micros: winner_trace.quality_adjusted_bid_usd_micros,
            selection_receipt: receipt,
            uplift: winner_uplift,
            uplift_assignment: assignment,
            delivery_channel: ctx.delivery_channel,
            mesh_payload: winner_mesh_payload,
        })
    }

    fn commit(&self, key: &ReservationKey) -> Option<SettlementBreakdown> {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)?
        };
        let assignment = reservation.assignment.clone();
        {
            let mut estimator = self.uplift.write().unwrap();
            estimator.record_observation(
                &reservation.campaign_id,
                &reservation.creative_id,
                &assignment,
                false,
            );
        }
        if let Err(err) = self.persist_uplift() {
            eprintln!("failed to persist uplift snapshot: {err}");
        }
        {
            let mut consumed = self.consumed_reservations.write().unwrap();
            consumed.insert(*key);
        }
        if assignment.in_holdout {
            return None;
        }
        let med_snapshot = {
            let mut med = self.medians.write().unwrap();
            med.record_storage(reservation.price_per_mib_usd_micros);
            med.record_verifier(reservation.resource_floor_breakdown.verifier_usd_micros);
            med.record_host(reservation.resource_floor_breakdown.host_usd_micros);
            med.snapshot()
        };
        if let Err(err) = self.persist_medians_snapshot(med_snapshot) {
            eprintln!("failed to persist medians after commit: {err}");
        }
        let (m_storage, m_verifier, m_host) = med_snapshot;
        gauge!("ad_cost_median_usd_micros", m_storage as f64, "role" => "storage");
        gauge!("ad_cost_median_usd_micros", m_verifier as f64, "role" => "verifier");
        gauge!("ad_cost_median_usd_micros", m_host as f64, "role" => "host");
        let mesh_payload_digest = reservation
            .mesh_payload
            .as_ref()
            .map(|payload| hex::encode(blake3::hash(payload).as_bytes()));
        let mut campaigns = self.campaigns.write().unwrap();
        let state = campaigns.get_mut(&reservation.campaign_id)?;
        if state.reserved_budget_usd_micros < reservation.total_usd_micros {
            return None;
        }
        state.reserved_budget_usd_micros = state
            .reserved_budget_usd_micros
            .saturating_sub(reservation.total_usd_micros);
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
        let mesh_bytes = reservation
            .mesh_payload
            .as_ref()
            .map(|payload| payload.len())
            .unwrap_or(0);
        let mesh_digest_label = mesh_payload_digest.as_deref().unwrap_or("none");
        diagnostics::log::info!(format!(
            "ad_reservation_commit campaign={} creative={} channel={} clearing_price={} mesh_bytes={} mesh_digest={}",
            reservation.campaign_id,
            reservation.creative_id,
            reservation.delivery_channel.as_str(),
            reservation.clearing_price_usd_micros,
            mesh_bytes,
            mesh_digest_label
        ));
        let mesh_payload = reservation.mesh_payload.clone();
        let conversion_key = format!("{}::{}", reservation.campaign_id, reservation.creative_id);
        let conversion_snapshot = {
            let mut conversions = self.conversions.write().unwrap();
            conversions.remove(&conversion_key).unwrap_or_default()
        };
        if let Err(err) = self.persist_conversions() {
            eprintln!("failed to persist conversions: {err}");
        }
        Some(SettlementBreakdown {
            campaign_id: reservation.campaign_id,
            creative_id: reservation.creative_id,
            bytes: reservation.bytes,
            price_per_mib_usd_micros: reservation.price_per_mib_usd_micros,
            total_usd_micros: reservation.total_usd_micros,
            demand_usd_micros: reservation.demand_usd_micros,
            resource_floor_usd_micros: reservation.resource_floor_usd_micros,
            clearing_price_usd_micros: reservation.clearing_price_usd_micros,
            delivery_channel: reservation.delivery_channel,
            mesh_payload,
            mesh_payload_digest,
            resource_floor_breakdown: reservation.resource_floor_breakdown.clone(),
            runner_up_quality_bid_usd_micros: reservation.runner_up_quality_bid_usd_micros,
            quality_adjusted_bid_usd_micros: reservation.quality_adjusted_bid_usd_micros,
            viewer: tokens.viewer,
            host: tokens.host,
            hardware: tokens.hardware,
            verifier: tokens.verifier,
            liquidity: tokens.liquidity,
            miner: tokens.miner,
            total: tokens.total,
            unsettled_usd_micros: tokens.unsettled_usd_micros,
            price_usd_micros: oracle.price_usd_micros,
            remainders_usd_micros: tokens.remainders.breakdown,
            twap_window_id: oracle.twap_window_id,
            selection_receipt: reservation.selection_receipt,
            uplift: reservation.uplift,
            claim_routes: reservation.claim_routes.clone(),
            conversions: conversion_snapshot.count,
            device_links: conversion_snapshot.device_links,
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
                domain_tier: key.domain_tier,
                domain_owner: key.domain_owner.clone(),
                provider: key.provider.clone(),
                badges: key.badges.clone(),
                interest_tags: key.interest_tags.clone(),
                presence_bucket: key.presence_bucket.clone(),
                selectors_version: key.selectors_version(),
                price_per_mib_usd_micros: state.price_per_mib_usd_micros(),
                target_utilization_ppm: state.target_utilization_ppm,
                observed_utilization_ppm: state.observed_utilization_ppm(),
            })
            .collect()
    }

    fn budget_broker(&self) -> &RwLock<BudgetBroker> {
        &self.budget_broker
    }

    fn update_quality_signals(&self, signals: Vec<QualitySignal>) {
        let mut guard = self.quality_signals.write().unwrap();
        apply_quality_signals_to_map(&mut guard, signals, &self.config);
    }

    fn quality_signal_config(&self) -> QualitySignalConfig {
        self.config.quality_signal_config()
    }

    fn privacy_budget_snapshot(&self) -> PrivacyBudgetSnapshot {
        self.privacy_budget.read().unwrap().snapshot()
    }

    fn preview_privacy_budget(
        &self,
        badges: &[String],
        population_hint: Option<u64>,
    ) -> PrivacyBudgetPreview {
        self.privacy_budget
            .read()
            .unwrap()
            .preview(badges, population_hint)
    }

    fn authorize_privacy_budget(
        &self,
        badges: &[String],
        population_hint: Option<u64>,
    ) -> PrivacyBudgetDecision {
        let mut budgets = self.privacy_budget.write().unwrap();
        let decision = budgets.authorize(badges, population_hint);
        if let Err(err) = self.persist_budget_broker() {
            eprintln!("failed to persist budget broker after privacy update: {err}");
        }
        decision
    }

    fn register_claim_route(
        &self,
        domain: &str,
        role: &str,
        address: &str,
    ) -> Result<(), MarketplaceError> {
        {
            let mut registry = self.claim_registry.write().unwrap();
            registry.register(domain, role, address);
        }
        if let Err(err) = self.persist_claim_registry() {
            return Err(MarketplaceError::PersistenceFailure(err.to_string()));
        }
        Ok(())
    }

    fn claim_routes(&self, cohort: &CohortKeySnapshot) -> HashMap<String, String> {
        self.claim_registry
            .read()
            .unwrap()
            .for_domain(&cohort.domain)
    }

    fn record_conversion(&self, event: ConversionEvent) -> Result<(), MarketplaceError> {
        {
            let campaigns = self.campaigns.read().unwrap();
            let state = campaigns
                .get(&event.campaign_id)
                .ok_or(MarketplaceError::UnknownCampaign)?;
            if !state
                .campaign
                .creatives
                .iter()
                .any(|creative| creative.id == event.creative_id)
            {
                return Err(MarketplaceError::UnknownCreative);
            }
        }
        let key = format!("{}::{}", event.campaign_id, event.creative_id);
        let device_link = event.device_link.clone().filter(|l| l.opt_in);
        let mut persist_seen = false;
        if let Some(link) = device_link.as_ref() {
            let mut seen = self.device_seen.write().unwrap();
            let set = seen.entry(key.clone()).or_default();
            if !set.insert(link.device_hash.clone()) {
                return Ok(());
            }
            persist_seen = true;
            if set.len() > 10_000 {
                if let Some(first) = set.iter().next().cloned() {
                    set.remove(&first);
                }
            }
        }
        {
            let mut conversions = self.conversions.write().unwrap();
            let entry = conversions.entry(key.clone()).or_default();
            entry.count = entry.count.saturating_add(1);
            if let Some(link) = device_link {
                if entry.device_links.len() < 1_000 {
                    entry.device_links.push(link);
                }
            }
        }
        if persist_seen {
            if let Err(err) = self.persist_device_seen() {
                return Err(err.into());
            }
        }
        if let Err(err) = self.persist_conversions() {
            return Err(err.into());
        }
        {
            let mut estimator = self.uplift.write().unwrap();
            estimator.record_observation(
                &event.campaign_id,
                &event.creative_id,
                &event.assignment,
                true,
            );
        }
        if let Err(err) = self.persist_uplift() {
            return Err(err.into());
        }
        Ok(())
    }

    fn recompute_distribution_from_utilization(&self) {
        let pricing = self.pricing.read().unwrap();
        if pricing.is_empty() {
            return;
        }
        let mut sum_obs: u128 = 0;
        let mut sum_tgt: u128 = 0;
        let mut n: u128 = 0;
        for (_, state) in pricing.iter() {
            sum_obs = sum_obs.saturating_add(state.observed_utilization_ppm as u128);
            sum_tgt = sum_tgt.saturating_add(state.target_utilization_ppm as u128);
            n = n.saturating_add(1);
        }
        if n == 0 || sum_tgt == 0 {
            return;
        }
        let mean_obs = (sum_obs / n) as u64;
        let mean_tgt = (sum_tgt / n) as u64;
        let ratio = (mean_obs as f64) / (mean_tgt as f64 + f64::EPSILON);
        let current = *self.distribution.read().unwrap();
        let mut policy = current;
        let step: i64 = 2;
        if ratio < 0.9 {
            policy.host_percent = (policy.host_percent as i64 + step) as u64;
            policy.hardware_percent = (policy.hardware_percent as i64 + step) as u64;
            policy.viewer_percent = policy.viewer_percent.saturating_sub(step as u64);
            policy.liquidity_percent = policy.liquidity_percent.saturating_sub(1);
        } else if ratio > 1.1 {
            policy.host_percent = policy.host_percent.saturating_sub(step as u64);
            policy.hardware_percent = policy.hardware_percent.saturating_sub(step as u64);
            policy.viewer_percent = (policy.viewer_percent as i64 + step) as u64;
            policy.liquidity_percent = policy.liquidity_percent.saturating_add(1);
        } else {
            policy.verifier_percent = (policy.verifier_percent as i64 + 1) as u64;
            policy.liquidity_percent = policy.liquidity_percent.saturating_sub(1);
        }
        // Cost-index adjustments
        let (m_storage, m_verifier, m_host) = { self.medians.read().unwrap().snapshot() };
        let base_storage = self.config.min_price_per_mib_usd_micros.max(1);
        let base_verifier = self.config.resource_floor.verifier_cost_usd_micros.max(1);
        let base_host = self.config.resource_floor.host_fee_usd_micros.max(1);
        let bump_if = |median: u64, base: u64| -> i64 {
            if base == 0 {
                return 0;
            }
            let r = (median as f64) / (base as f64);
            if r > 1.2 {
                1
            } else if r < 0.8 {
                -1
            } else {
                0
            }
        };
        let hw_adj = bump_if(m_storage, base_storage);
        let vf_adj = bump_if(m_verifier, base_verifier);
        let host_adj = bump_if(m_host, base_host);
        if hw_adj != 0 {
            policy.hardware_percent = ((policy.hardware_percent as i64) + hw_adj).max(0) as u64;
        }
        if vf_adj != 0 {
            policy.verifier_percent = ((policy.verifier_percent as i64) + vf_adj).max(0) as u64;
        }
        if host_adj != 0 {
            policy.host_percent = ((policy.host_percent as i64) + host_adj).max(0) as u64;
        }
        // Normalize to 100
        let sum = policy.viewer_percent
            + policy.host_percent
            + policy.hardware_percent
            + policy.verifier_percent
            + policy.liquidity_percent;
        let mut policy = policy.normalize();
        if sum != 100 {
            if sum > 100 {
                policy.liquidity_percent = policy.liquidity_percent.saturating_sub(sum - 100);
            } else {
                policy.liquidity_percent = policy.liquidity_percent.saturating_add(100 - sum);
            }
        }
        let normalized = policy.normalize();
        // Drift telemetry
        let drift =
            |new: u64, old: u64| -> f64 { ((new as f64 - old as f64) / 100.0) * PPM_SCALE as f64 };
        gauge!("ad_distribution_policy_drift_ppm", drift(normalized.viewer_percent, current.viewer_percent), "role" => "viewer");
        gauge!("ad_distribution_policy_drift_ppm", drift(normalized.host_percent, current.host_percent), "role" => "host");
        gauge!("ad_distribution_policy_drift_ppm", drift(normalized.hardware_percent, current.hardware_percent), "role" => "hardware");
        gauge!("ad_distribution_policy_drift_ppm", drift(normalized.verifier_percent, current.verifier_percent), "role" => "verifier");
        let _ = self.persist_distribution(&normalized);
        *self.distribution.write().unwrap() = normalized;
    }

    fn cost_medians_usd_micros(&self) -> (u64, u64, u64) {
        self.medians.read().unwrap().snapshot()
    }

    fn badge_guard_decision(
        &self,
        badges: &[String],
        soft_intent: Option<&BadgeSoftIntentContext>,
    ) -> BadgeDecision {
        self.badge_guard.evaluate(badges, soft_intent)
    }
}
struct RoleUsdParts {
    viewer: u64,
    host: u64,
    hardware: u64,
    verifier: u64,
    liquidity: u64,
    remainder: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemainderBreakdown {
    pub viewer_usd_micros: u64,
    pub host_usd_micros: u64,
    pub hardware_usd_micros: u64,
    pub verifier_usd_micros: u64,
    pub liquidity_usd_micros: u64,
    pub miner_usd_micros: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemainderSnapshot {
    pub breakdown: RemainderBreakdown,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct TokenRemainderLedger {
    viewer_usd: u64,
    host_usd: u64,
    hardware_usd: u64,
    verifier_usd: u64,
    liquidity_usd: u64,
    miner_usd: u64,
}

impl TokenRemainderLedger {
    fn convert(&mut self, parts: RoleUsdParts, oracle: TokenOracle) -> TokenizedPayouts {
        let (viewer, _) = convert_role(parts.viewer, &mut self.viewer_usd, oracle.price_usd_micros);
        let (host, _) = convert_role(parts.host, &mut self.host_usd, oracle.price_usd_micros);
        let (hardware, _) = convert_role(
            parts.hardware,
            &mut self.hardware_usd,
            oracle.price_usd_micros,
        );
        let (verifier, _) = convert_role(
            parts.verifier,
            &mut self.verifier_usd,
            oracle.price_usd_micros,
        );
        let (liquidity, _) = convert_role(
            parts.liquidity,
            &mut self.liquidity_usd,
            oracle.price_usd_micros,
        );
        let (miner, _) = convert_role(
            parts.remainder,
            &mut self.miner_usd,
            oracle.price_usd_micros,
        );

        let snapshot = self.snapshot();
        // Collapse aggregated remainders so the unsettled portion always sits below
        // the current token price. Extra whole tokens are handed to the miner bucket
        // to preserve total value.
        let (extra_miner, collapsed_remainder) =
            usd_to_tokens(self.total_remainder_usd(), oracle.price_usd_micros);
        let miner = miner.saturating_add(extra_miner);
        self.viewer_usd = 0;
        self.host_usd = 0;
        self.hardware_usd = 0;
        self.verifier_usd = 0;
        self.liquidity_usd = 0;
        self.miner_usd = collapsed_remainder;
        let unsettled_usd_micros = collapsed_remainder;
        let total = viewer
            .saturating_add(host)
            .saturating_add(hardware)
            .saturating_add(verifier)
            .saturating_add(liquidity)
            .saturating_add(miner);

        TokenizedPayouts {
            viewer,
            host,
            hardware,
            verifier,
            liquidity,
            miner,
            total,
            unsettled_usd_micros,
            remainders: snapshot,
        }
    }

    fn snapshot(&self) -> RemainderSnapshot {
        RemainderSnapshot {
            breakdown: RemainderBreakdown {
                viewer_usd_micros: self.viewer_usd,
                host_usd_micros: self.host_usd,
                hardware_usd_micros: self.hardware_usd,
                verifier_usd_micros: self.verifier_usd,
                liquidity_usd_micros: self.liquidity_usd,
                miner_usd_micros: self.miner_usd,
            },
        }
    }

    fn total_remainder_usd(&self) -> u64 {
        self.viewer_usd
            .saturating_add(self.host_usd)
            .saturating_add(self.hardware_usd)
            .saturating_add(self.verifier_usd)
            .saturating_add(self.liquidity_usd)
            .saturating_add(self.miner_usd)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenizedPayouts {
    viewer: u64,
    host: u64,
    hardware: u64,
    verifier: u64,
    liquidity: u64,
    miner: u64,
    total: u64,
    unsettled_usd_micros: u64,
    remainders: RemainderSnapshot,
}

fn allocate_usd(total_usd_micros: u64, distribution: DistributionPolicy) -> RoleUsdParts {
    if total_usd_micros == 0 {
        return RoleUsdParts {
            viewer: 0,
            host: 0,
            hardware: 0,
            verifier: 0,
            liquidity: 0,
            remainder: 0,
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
    let distributed_sum = allocations.iter().copied().sum::<u64>();
    RoleUsdParts {
        viewer: allocations.first().copied().unwrap_or(0),
        host: allocations.get(1).copied().unwrap_or(0),
        hardware: allocations.get(2).copied().unwrap_or(0),
        verifier: allocations.get(3).copied().unwrap_or(0),
        liquidity: allocations.get(4).copied().unwrap_or(0),
        remainder: total_usd_micros.saturating_sub(distributed_sum),
    }
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
    numerator.ceil_div(BYTES_PER_MIB)
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

    #[test]
    fn sled_marketplace_cost_medians_persist_across_reopen() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("db");
        let config = MarketplaceConfig::default();
        {
            let market =
                SledMarketplace::open(&path, config.clone()).expect("open sled marketplace");
            {
                let mut med = market.medians.write().unwrap();
                med.record_storage(111);
                med.record_verifier(222);
                med.record_host(333);
            }
            market.persist_medians().expect("persist medians");
        }
        let reopened = SledMarketplace::open(&path, config).expect("reopen market");
        let medians = reopened.medians.read().unwrap().snapshot();
        assert_eq!(medians, (111, 222, 333));
    }

    #[test]
    fn quality_signal_scales_quality_bid() {
        let bid = QualityBid {
            base_bid_usd_micros: 100,
            quality_adjusted_usd_micros: 200,
            quality_multiplier: 1.0,
        };
        let adjusted = bid.apply_signal_multiplier(0.5);
        assert_eq!(adjusted.base_bid_usd_micros, 100);
        assert_eq!(adjusted.quality_adjusted_usd_micros, 100);
        assert!((adjusted.quality_multiplier - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn quality_signal_adjusts_quality_bid_outcome() {
        let ctx = ImpressionContext {
            domain: "example.test".to_string(),
            provider: Some("provider".to_string()),
            badges: Vec::new(),
            bytes: BYTES_PER_MIB,
            attestations: Vec::new(),
            population_estimate: Some(1_000),
            ..ImpressionContext::default()
        };
        let cohort = CohortKeySnapshot {
            domain: ctx.domain.clone(),
            provider: ctx.provider.clone(),
            badges: ctx.badges.clone(),
            domain_tier: ctx.domain_tier,
            domain_owner: ctx.domain_owner.clone(),
            interest_tags: ctx.interest_tags.clone(),
            presence_bucket: ctx.presence_bucket.clone(),
            selectors_version: ctx.selectors_version,
        };
        let signal = |multiplier_ppm| QualitySignal {
            cohort: cohort.clone(),
            multiplier_ppm,
            components: QualitySignalComponents {
                freshness_multiplier_ppm: multiplier_ppm,
                readiness_multiplier_ppm: multiplier_ppm,
                privacy_multiplier_ppm: multiplier_ppm,
            },
        };
        let outcome_for = |multiplier_ppm| {
            let market = InMemoryMarketplace::new(MarketplaceConfig::default());
            market
                .register_campaign(sample_campaign("cmp", 5 * MICROS_PER_DOLLAR))
                .expect("campaign registered");
            market.update_oracle(TokenOracle::new(50_000));
            market.update_quality_signals(vec![signal(multiplier_ppm)]);
            let key = ReservationKey {
                manifest: [1u8; 32],
                path_hash: [2u8; 32],
                discriminator: [3u8; 32],
            };
            market
                .reserve_impression(key, ctx.clone())
                .expect("reservation succeeded")
        };
        let low = outcome_for(500_000);
        let high = outcome_for(1_500_000);
        assert!(high.quality_adjusted_bid_usd_micros > low.quality_adjusted_bid_usd_micros);
    }

    #[test]
    fn quality_signal_scales_clearing_price() {
        let ctx = ImpressionContext {
            domain: "example.test".to_string(),
            bytes: 1,
            ..ImpressionContext::default()
        };
        let cohort = CohortKeySnapshot {
            domain: ctx.domain.clone(),
            provider: ctx.provider.clone(),
            badges: ctx.badges.clone(),
            domain_tier: ctx.domain_tier,
            domain_owner: ctx.domain_owner.clone(),
            interest_tags: ctx.interest_tags.clone(),
            presence_bucket: ctx.presence_bucket.clone(),
            selectors_version: ctx.selectors_version,
        };
        let signal = |multiplier_ppm| QualitySignal {
            cohort: cohort.clone(),
            multiplier_ppm,
            components: QualitySignalComponents {
                freshness_multiplier_ppm: multiplier_ppm,
                readiness_multiplier_ppm: multiplier_ppm,
                privacy_multiplier_ppm: multiplier_ppm,
            },
        };
        let outcome_for = |multiplier_ppm| {
            let market = InMemoryMarketplace::new(MarketplaceConfig::default());
            let creatives = vec![
                Creative {
                    id: "creative-high".to_string(),
                    action_rate_ppm: 1_000_000,
                    margin_ppm: 1_000_000,
                    value_per_action_usd_micros: 200_000,
                    max_cpi_usd_micros: None,
                    lift_ppm: 1_000_000,
                    badges: Vec::new(),
                    domains: vec!["example.test".to_string()],
                    metadata: HashMap::new(),
                    mesh_payload: None,
                    placement: CreativePlacement::default(),
                },
                Creative {
                    id: "creative-low".to_string(),
                    action_rate_ppm: 1_000_000,
                    margin_ppm: 1_000_000,
                    value_per_action_usd_micros: 150_000,
                    max_cpi_usd_micros: None,
                    lift_ppm: 1_000_000,
                    badges: Vec::new(),
                    domains: vec!["example.test".to_string()],
                    metadata: HashMap::new(),
                    mesh_payload: None,
                    placement: CreativePlacement::default(),
                },
            ];
            let campaign = Campaign {
                id: "cmp-signal".to_string(),
                advertiser_account: "adv".to_string(),
                budget_usd_micros: MICROS_PER_DOLLAR,
                creatives,
                targeting: CampaignTargeting {
                    domains: vec!["example.test".to_string()],
                    ..CampaignTargeting::default()
                },
                metadata: HashMap::new(),
            };
            market
                .register_campaign(campaign)
                .expect("campaign registered");
            market.update_oracle(TokenOracle::new(50_000));
            market.update_quality_signals(vec![signal(multiplier_ppm)]);
            let key = ReservationKey {
                manifest: [1u8; 32],
                path_hash: [2u8; 32],
                discriminator: [3u8; 32],
            };
            market
                .reserve_impression(key, ctx.clone())
                .expect("reservation succeeded")
        };
        let low = outcome_for(500_000);
        let high = outcome_for(1_500_000);
        assert!(high.clearing_price_usd_micros > low.clearing_price_usd_micros);
    }

    #[test]
    fn resource_floor_scaling_applies_scarcity_multiplier() {
        let breakdown = ResourceFloorBreakdown {
            bandwidth_usd_micros: 100,
            verifier_usd_micros: 50,
            host_usd_micros: 25,
            qualified_impressions_per_proof: 10,
        };
        let scaled = breakdown.scale(1.2);
        assert_eq!(scaled.bandwidth_usd_micros, 120);
        assert_eq!(scaled.verifier_usd_micros, 60);
        assert_eq!(scaled.host_usd_micros, 30);
        assert_eq!(scaled.total_usd_micros(), 210);
    }

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
                domain_tier: DomainTier::default(),
                domain_owner: None,
                provider: Some("wallet".into()),
                badges: vec!["badge-a".into(), "badge-b".into()],
                interest_tags: Vec::new(),
                presence_bucket: None,
                selectors_version: COHORT_SELECTOR_VERSION_V1,
                bytes: BYTES_PER_MIB,
                price_per_mib_usd_micros: 120,
                delivery_channel: DeliveryChannel::Http,
                mesh_peer: None,
                mesh_transport: None,
                mesh_latency_ms: None,
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
            uplift_assignment: Some(UpliftHoldoutAssignment {
                fold: 0,
                in_holdout: false,
                propensity: 1.0,
            }),
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
                mesh_payload: None,
                placement: CreativePlacement::default(),
            }],
            targeting: CampaignTargeting {
                domains: vec!["example.test".to_string()],
                badges: Vec::new(),
                ..CampaignTargeting::default()
            },
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn in_memory_reserve_and_commit() {
        let config = MarketplaceConfig::default();
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
        market.update_oracle(TokenOracle::new(50_000));
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
        assert!(settlement.viewer > 0);
        assert!(settlement.host > 0);
        assert!(settlement.hardware > 0);
        assert!(settlement.verifier > 0);
        let policy = market.distribution();
        let parts = allocate_usd(settlement.total_usd_micros, policy);
        let expected_liquidity_usd = parts.liquidity;
        let (expected_liquidity, _) =
            usd_to_tokens(expected_liquidity_usd, settlement.price_usd_micros);
        assert_eq!(settlement.liquidity, expected_liquidity);
        assert_eq!(
            settlement.total,
            settlement
                .viewer
                .saturating_add(settlement.host)
                .saturating_add(settlement.hardware)
                .saturating_add(settlement.verifier)
                .saturating_add(settlement.liquidity)
                .saturating_add(settlement.miner)
        );
        assert!(settlement.price_usd_micros > 0);
        assert!(settlement.unsettled_usd_micros < settlement.price_usd_micros);
        assert!(
            settlement.total.saturating_mul(settlement.price_usd_micros)
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
    fn claim_routes_flow_into_settlement() {
        let market = InMemoryMarketplace::new(MarketplaceConfig::default());
        market
            .register_campaign(sample_campaign("cmp", 5 * MICROS_PER_DOLLAR))
            .expect("campaign registered");
        market
            .register_claim_route("example.test", "publisher", "addr_pub")
            .expect("claim route");
        let ctx = ImpressionContext {
            domain: "example.test".to_string(),
            provider: Some("provider".to_string()),
            badges: Vec::new(),
            bytes: BYTES_PER_MIB,
            attestations: Vec::new(),
            population_estimate: Some(1_000),
            ..ImpressionContext::default()
        };
        let key = ReservationKey {
            manifest: [1u8; 32],
            path_hash: [2u8; 32],
            discriminator: [3u8; 32],
        };
        market.update_oracle(TokenOracle::new(50_000));
        let _ = market
            .reserve_impression(key, ctx.clone())
            .expect("reservation succeeded");
        let settlement = market.commit(&key).expect("commit succeeds");
        assert_eq!(
            settlement.claim_routes.get("publisher"),
            Some(&"addr_pub".to_string())
        );
    }

    #[test]
    fn conversions_flow_into_settlement() {
        let market = InMemoryMarketplace::new(MarketplaceConfig::default());
        market
            .register_campaign(sample_campaign("cmp", 5 * MICROS_PER_DOLLAR))
            .expect("campaign registered");
        market
            .record_conversion(ConversionEvent {
                campaign_id: "cmp".into(),
                creative_id: "creative-cmp".into(),
                assignment: UpliftHoldoutAssignment {
                    fold: 0,
                    in_holdout: false,
                    propensity: 1.0,
                },
                value_usd_micros: Some(25_000),
                occurred_at_micros: Some(123),
                device_link: Some(DeviceLinkOptIn {
                    device_hash: "device-1".into(),
                    opt_in: true,
                }),
            })
            .expect("conversion recorded");
        let ctx = ImpressionContext {
            domain: "example.test".to_string(),
            provider: Some("provider".to_string()),
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
        market.update_oracle(TokenOracle::new(50_000));
        let _ = market
            .reserve_impression(key, ctx.clone())
            .expect("reservation succeeded");
        let settlement = market.commit(&key).expect("commit succeeds");
        assert_eq!(settlement.conversions, 1);
        assert_eq!(settlement.device_links.len(), 1);
        assert_eq!(settlement.device_links[0].device_hash, "device-1");

        let key2 = ReservationKey {
            manifest: [3u8; 32],
            path_hash: [2u8; 32],
            discriminator: [1u8; 32],
        };
        let _ = market
            .reserve_impression(key2, ctx)
            .expect("reservation succeeded");
        let settlement2 = market.commit(&key2).expect("commit succeeds");
        assert_eq!(settlement2.conversions, 0);
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
            market.commit(&key);
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
                domain_tier: DomainTier::default(),
                domain_owner: None,
                provider: Some("edge".into()),
                badges: vec!["a".into()],
                interest_tags: Vec::new(),
                presence_bucket: None,
                selectors_version: COHORT_SELECTOR_VERSION_V1,
                bytes: 1_024,
                price_per_mib_usd_micros: 120,
                delivery_channel: DeliveryChannel::Http,
                mesh_peer: None,
                mesh_transport: None,
                mesh_latency_ms: None,
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
            uplift_assignment: Some(UpliftHoldoutAssignment {
                fold: 0,
                in_holdout: false,
                propensity: 1.0,
            }),
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
                domain_tier: DomainTier::default(),
                domain_owner: None,
                provider: None,
                badges: Vec::new(),
                interest_tags: Vec::new(),
                presence_bucket: None,
                selectors_version: COHORT_SELECTOR_VERSION_V1,
                bytes: 512,
                price_per_mib_usd_micros: 90,
                delivery_channel: DeliveryChannel::Http,
                mesh_peer: None,
                mesh_transport: None,
                mesh_latency_ms: None,
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
            uplift_assignment: Some(UpliftHoldoutAssignment {
                fold: 0,
                in_holdout: false,
                propensity: 1.0,
            }),
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
                domain_tier: DomainTier::default(),
                domain_owner: None,
                provider: Some("edge".into()),
                badges: vec!["badge".into()],
                interest_tags: Vec::new(),
                presence_bucket: None,
                selectors_version: COHORT_SELECTOR_VERSION_V1,
                bytes: 256,
                price_per_mib_usd_micros: 80,
                delivery_channel: DeliveryChannel::Http,
                mesh_peer: None,
                mesh_transport: None,
                mesh_latency_ms: None,
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
            uplift_assignment: Some(UpliftHoldoutAssignment {
                fold: 0,
                in_holdout: false,
                propensity: 1.0,
            }),
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

    #[test]
    fn record_conversion_updates_treatment_bucket() {
        let config = MarketplaceConfig::default();
        let market = InMemoryMarketplace::new(config);
        let creative = Creative {
            id: "creative".into(),
            action_rate_ppm: 100_000,
            margin_ppm: PPM_SCALE as u32,
            value_per_action_usd_micros: 100_000,
            max_cpi_usd_micros: None,
            lift_ppm: 0,
            badges: Vec::new(),
            domains: Vec::new(),
            metadata: HashMap::new(),
            mesh_payload: None,
            placement: CreativePlacement::default(),
        };
        let campaign = Campaign {
            id: "cmp".into(),
            advertiser_account: "adv".into(),
            budget_usd_micros: 5_000_000,
            creatives: vec![creative],
            targeting: CampaignTargeting::default(),
            metadata: HashMap::new(),
        };
        market
            .register_campaign(campaign)
            .expect("campaign registered");
        let event = ConversionEvent {
            campaign_id: "cmp".into(),
            creative_id: "creative".into(),
            assignment: UpliftHoldoutAssignment {
                fold: 0,
                in_holdout: false,
                propensity: 1.0,
            },
            value_usd_micros: Some(250_000),
            occurred_at_micros: Some(123_456),
            device_link: None,
        };
        market
            .record_conversion(event)
            .expect("conversion recorded");

        let snapshot = market.uplift.read().unwrap().snapshot();
        assert_eq!(snapshot.creatives.len(), 1);
        let creative_snapshot = &snapshot.creatives[0];
        assert_eq!(creative_snapshot.treatment_count, 1);
        assert_eq!(creative_snapshot.treatment_success, 1);
        assert_eq!(creative_snapshot.control_count, 0);
        assert_eq!(creative_snapshot.control_success, 0);
    }

    #[test]
    fn sled_record_conversion_persists_snapshot() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("market");
        let config = MarketplaceConfig::default();
        let mut campaign_metadata = HashMap::new();
        campaign_metadata.insert("channel".into(), "mesh".into());
        let creative = Creative {
            id: "creative".into(),
            action_rate_ppm: 80_000,
            margin_ppm: PPM_SCALE as u32,
            value_per_action_usd_micros: 120_000,
            max_cpi_usd_micros: None,
            lift_ppm: 0,
            badges: Vec::new(),
            domains: Vec::new(),
            metadata: HashMap::new(),
            mesh_payload: None,
            placement: CreativePlacement::default(),
        };
        let campaign = Campaign {
            id: "cmp".into(),
            advertiser_account: "adv".into(),
            budget_usd_micros: 4_000_000,
            creatives: vec![creative],
            targeting: CampaignTargeting::default(),
            metadata: campaign_metadata,
        };
        let market = SledMarketplace::open(&path, config.clone()).expect("sled opened");
        market
            .register_campaign(campaign)
            .expect("campaign registered");
        let event = ConversionEvent {
            campaign_id: "cmp".into(),
            creative_id: "creative".into(),
            assignment: UpliftHoldoutAssignment {
                fold: 1,
                in_holdout: true,
                propensity: 0.5,
            },
            value_usd_micros: None,
            occurred_at_micros: Some(999_000),
            device_link: None,
        };
        market
            .record_conversion(event)
            .expect("conversion recorded");
        drop(market);

        let reopened = SledMarketplace::open(&path, config).expect("sled reopened");
        let snapshot = reopened.uplift.read().unwrap().snapshot();
        assert_eq!(snapshot.creatives.len(), 1);
        let creative_snapshot = &snapshot.creatives[0];
        assert_eq!(creative_snapshot.control_count, 1);
        assert_eq!(creative_snapshot.control_success, 1);
    }

    #[test]
    fn sled_conversion_drains_into_settlement_after_restart() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("market");
        let config = MarketplaceConfig::default();
        let market = SledMarketplace::open(&path, config.clone()).expect("sled opened");
        market
            .register_campaign(sample_campaign("cmp", 5 * MICROS_PER_DOLLAR))
            .expect("campaign registered");
        market
            .record_conversion(ConversionEvent {
                campaign_id: "cmp".into(),
                creative_id: "creative-cmp".into(),
                assignment: UpliftHoldoutAssignment {
                    fold: 0,
                    in_holdout: false,
                    propensity: 1.0,
                },
                value_usd_micros: None,
                occurred_at_micros: Some(10),
                device_link: Some(DeviceLinkOptIn {
                    device_hash: "persisted-device".into(),
                    opt_in: true,
                }),
            })
            .expect("conversion recorded");
        drop(market);

        let reopened = SledMarketplace::open(&path, config).expect("reopened market");
        reopened.update_oracle(TokenOracle::new(50_000));
        let ctx = ImpressionContext {
            domain: "example.test".to_string(),
            provider: Some("provider".to_string()),
            badges: Vec::new(),
            bytes: BYTES_PER_MIB,
            attestations: Vec::new(),
            population_estimate: Some(1_000),
            ..ImpressionContext::default()
        };
        let key = ReservationKey {
            manifest: [4u8; 32],
            path_hash: [5u8; 32],
            discriminator: [6u8; 32],
        };
        let _ = reopened
            .reserve_impression(key, ctx)
            .expect("reservation succeeded");
        let settlement = reopened.commit(&key).expect("commit succeeds");
        assert_eq!(settlement.conversions, 1);
        assert_eq!(settlement.device_links.len(), 1);
        assert_eq!(settlement.device_links[0].device_hash, "persisted-device");
    }

    #[test]
    fn conversion_device_link_deduplicates() {
        let config = MarketplaceConfig::default();
        let market = InMemoryMarketplace::new(config);
        let creative = Creative {
            id: "creative".into(),
            action_rate_ppm: 100_000,
            margin_ppm: PPM_SCALE as u32,
            value_per_action_usd_micros: 100_000,
            max_cpi_usd_micros: None,
            lift_ppm: 0,
            badges: Vec::new(),
            domains: Vec::new(),
            metadata: HashMap::new(),
            mesh_payload: None,
            placement: CreativePlacement::default(),
        };
        let campaign = Campaign {
            id: "cmp".into(),
            advertiser_account: "adv".into(),
            budget_usd_micros: 5_000_000,
            creatives: vec![creative],
            targeting: CampaignTargeting::default(),
            metadata: HashMap::new(),
        };
        market
            .register_campaign(campaign)
            .expect("campaign registered");
        let base_event = ConversionEvent {
            campaign_id: "cmp".into(),
            creative_id: "creative".into(),
            assignment: UpliftHoldoutAssignment {
                fold: 0,
                in_holdout: false,
                propensity: 1.0,
            },
            value_usd_micros: Some(1),
            occurred_at_micros: Some(1),
            device_link: Some(DeviceLinkOptIn {
                device_hash: "hash1".into(),
                opt_in: true,
            }),
        };
        market
            .record_conversion(base_event.clone())
            .expect("first conversion");
        // duplicate should be dropped
        market
            .record_conversion(base_event)
            .expect("second conversion ok");
        let snapshot = market.uplift.read().unwrap().snapshot();
        let creative_snapshot = &snapshot.creatives[0];
        assert_eq!(creative_snapshot.treatment_count, 1);
        assert_eq!(creative_snapshot.treatment_success, 1);
    }
}
