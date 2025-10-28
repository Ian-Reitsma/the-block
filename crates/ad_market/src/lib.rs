use foundation_serialization::{
    self,
    json::{self, Map as JsonMap, Number as JsonNumber, Value as JsonValue},
    Deserialize, Serialize,
};
use sled::{Config as SledConfig, Db as SledDb, Tree as SledTree};
use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, RwLock};

const TREE_CAMPAIGNS: &str = "campaigns";
const TREE_METADATA: &str = "metadata";
const KEY_DISTRIBUTION: &[u8] = b"distribution";

const MICROS_PER_DOLLAR: u64 = 1_000_000;
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
        }
    }

    pub fn with_liquidity_split(mut self, split_ct_ppm: u32) -> Self {
        self.liquidity_split_ct_ppm = split_ct_ppm.min(PPM_SCALE as u32);
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
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenOracle {
    pub ct_price_usd_micros: u64,
    pub it_price_usd_micros: u64,
}

impl TokenOracle {
    pub fn new(ct_price_usd_micros: u64, it_price_usd_micros: u64) -> Self {
        Self {
            ct_price_usd_micros: ct_price_usd_micros.max(1),
            it_price_usd_micros: it_price_usd_micros.max(1),
        }
    }
}

impl Default for TokenOracle {
    fn default() -> Self {
        Self {
            ct_price_usd_micros: MICROS_PER_DOLLAR,
            it_price_usd_micros: MICROS_PER_DOLLAR,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MarketplaceConfig {
    pub distribution: DistributionPolicy,
    pub default_price_per_mib_usd_micros: u64,
    pub target_utilization_ppm: u32,
    pub learning_rate_ppm: u32,
    pub smoothing_ppm: u32,
    pub min_price_per_mib_usd_micros: u64,
    pub max_price_per_mib_usd_micros: u64,
    pub default_oracle: TokenOracle,
}

impl MarketplaceConfig {
    pub fn normalized(self) -> Self {
        let mut normalized = self;
        normalized.default_price_per_mib_usd_micros =
            normalized.default_price_per_mib_usd_micros.clamp(
                normalized.min_price_per_mib_usd_micros,
                normalized.max_price_per_mib_usd_micros,
            );
        normalized
    }
}

impl Default for MarketplaceConfig {
    fn default() -> Self {
        Self {
            distribution: DistributionPolicy::default(),
            default_price_per_mib_usd_micros: MICROS_PER_DOLLAR,
            target_utilization_ppm: 900_000,
            learning_rate_ppm: 50_000,
            smoothing_ppm: 200_000,
            min_price_per_mib_usd_micros: 10_000,
            max_price_per_mib_usd_micros: 1_000 * MICROS_PER_DOLLAR,
            default_oracle: TokenOracle::default(),
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
struct CohortPricingState {
    price_per_mib_usd_micros: u64,
    target_utilization_ppm: u32,
    learning_rate_ppm: u32,
    smoothing_ppm: u32,
    ema_supply_usd_micros: f64,
    ema_demand_usd_micros: f64,
    min_price_per_mib_usd_micros: u64,
    max_price_per_mib_usd_micros: u64,
    observed_utilization_ppm: u32,
}

impl CohortPricingState {
    fn new(config: &MarketplaceConfig) -> Self {
        Self {
            price_per_mib_usd_micros: config.default_price_per_mib_usd_micros,
            target_utilization_ppm: config.target_utilization_ppm,
            learning_rate_ppm: config.learning_rate_ppm,
            smoothing_ppm: config.smoothing_ppm,
            ema_supply_usd_micros: 0.0,
            ema_demand_usd_micros: 0.0,
            min_price_per_mib_usd_micros: config.min_price_per_mib_usd_micros,
            max_price_per_mib_usd_micros: config.max_price_per_mib_usd_micros,
            observed_utilization_ppm: 0,
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
        let target = (self.target_utilization_ppm as f64 / PPM_SCALE as f64).clamp(0.0, 1.0);
        let delta = utilization - target;
        let learning = self.learning_rate_ppm as f64 / PPM_SCALE as f64;
        let factor = (learning * delta).exp();
        let updated = (self.price_per_mib_usd_micros as f64 * factor).round() as u64;
        self.price_per_mib_usd_micros = updated
            .clamp(
                self.min_price_per_mib_usd_micros,
                self.max_price_per_mib_usd_micros,
            )
            .max(1);
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
}

#[derive(Clone, Debug, Default)]
pub struct ImpressionContext {
    pub domain: String,
    pub provider: Option<String>,
    pub badges: Vec<String>,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct MatchOutcome {
    pub campaign_id: String,
    pub creative_id: String,
    pub price_per_mib_usd_micros: u64,
    pub total_usd_micros: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettlementBreakdown {
    pub campaign_id: String,
    pub creative_id: String,
    pub bytes: u64,
    pub price_per_mib_usd_micros: u64,
    pub total_usd_micros: u64,
    pub demand_usd_micros: u64,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CampaignState {
    campaign: Campaign,
    remaining_budget_usd_micros: u64,
}

struct ReservationState {
    campaign_id: String,
    creative_id: String,
    bytes: u64,
    price_per_mib_usd_micros: u64,
    total_usd_micros: u64,
    demand_usd_micros: u64,
    cohort: CohortKey,
}

pub struct InMemoryMarketplace {
    config: MarketplaceConfig,
    campaigns: RwLock<HashMap<String, CampaignState>>,
    reservations: RwLock<HashMap<ReservationKey, ReservationState>>,
    pending: RwLock<HashMap<String, u64>>,
    distribution: RwLock<DistributionPolicy>,
    pricing: RwLock<HashMap<CohortKey, CohortPricingState>>,
    oracle: RwLock<TokenOracle>,
}

pub struct SledMarketplace {
    _db: SledDb,
    campaigns_tree: SledTree,
    metadata_tree: SledTree,
    config: MarketplaceConfig,
    campaigns: RwLock<HashMap<String, CampaignState>>,
    reservations: RwLock<HashMap<ReservationKey, ReservationState>>,
    pending: RwLock<HashMap<String, u64>>,
    distribution: RwLock<DistributionPolicy>,
    pricing: RwLock<HashMap<CohortKey, CohortPricingState>>,
    oracle: RwLock<TokenOracle>,
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
    Ok(CampaignState {
        campaign,
        remaining_budget_usd_micros: remaining,
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
    Ok(policy.normalize())
}
impl InMemoryMarketplace {
    pub fn new(config: MarketplaceConfig) -> Self {
        let normalized = config.normalized();
        Self {
            config: normalized.clone(),
            campaigns: RwLock::new(HashMap::new()),
            reservations: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashMap::new()),
            distribution: RwLock::new(normalized.distribution.normalize()),
            pricing: RwLock::new(HashMap::new()),
            oracle: RwLock::new(normalized.default_oracle),
        }
    }

    fn matches_targeting(targeting: &CampaignTargeting, ctx: &ImpressionContext) -> bool {
        if !targeting.domains.is_empty() && !targeting.domains.iter().any(|d| d == &ctx.domain) {
            return false;
        }
        if !targeting.badges.is_empty() {
            let ctx_badges: HashSet<&str> = ctx.badges.iter().map(String::as_str).collect();
            if targeting
                .badges
                .iter()
                .any(|badge| !ctx_badges.contains(badge.as_str()))
            {
                return false;
            }
        }
        true
    }

    fn matches_creative(creative: &Creative, ctx: &ImpressionContext) -> bool {
        if !creative.domains.is_empty() && !creative.domains.iter().any(|d| d == &ctx.domain) {
            return false;
        }
        if !creative.badges.is_empty() {
            let ctx_badges: HashSet<&str> = ctx.badges.iter().map(String::as_str).collect();
            if creative
                .badges
                .iter()
                .any(|badge| !ctx_badges.contains(badge.as_str()))
            {
                return false;
            }
        }
        true
    }

    fn cohort_key(ctx: &ImpressionContext) -> CohortKey {
        CohortKey::new(ctx.domain.clone(), ctx.provider.clone(), ctx.badges.clone())
    }

    fn get_price_and_state<'a>(
        pricing: &'a mut HashMap<CohortKey, CohortPricingState>,
        key: &CohortKey,
        config: &MarketplaceConfig,
    ) -> &'a mut CohortPricingState {
        pricing
            .entry(key.clone())
            .or_insert_with(|| CohortPricingState::new(config))
    }
}

impl Marketplace for InMemoryMarketplace {
    fn register_campaign(&self, campaign: Campaign) -> Result<(), MarketplaceError> {
        let mut guard = self.campaigns.write().unwrap();
        if guard.contains_key(&campaign.id) {
            return Err(MarketplaceError::DuplicateCampaign);
        }
        let state = CampaignState {
            remaining_budget_usd_micros: campaign.budget_usd_micros,
            campaign,
        };
        guard.insert(state.campaign.id.clone(), state);
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
            })
            .collect()
    }

    fn reserve_impression(
        &self,
        key: ReservationKey,
        ctx: ImpressionContext,
    ) -> Option<MatchOutcome> {
        let cohort = InMemoryMarketplace::cohort_key(&ctx);
        let price_per_mib = {
            let mut pricing = self.pricing.write().unwrap();
            InMemoryMarketplace::get_price_and_state(&mut pricing, &cohort, &self.config)
                .price_per_mib_usd_micros()
        };
        let cost = usd_cost_for_bytes(price_per_mib, ctx.bytes);
        if cost == 0 {
            return None;
        }
        let campaigns = self.campaigns.read().unwrap();
        let pending = self.pending.read().unwrap();
        let mut best: Option<(MatchOutcome, String, u64, u64)> = None;
        for (campaign_id, state) in campaigns.iter() {
            if !InMemoryMarketplace::matches_targeting(&state.campaign.targeting, &ctx) {
                continue;
            }
            let reserved = pending.get(campaign_id).copied().unwrap_or(0);
            let available_budget = state.remaining_budget_usd_micros.saturating_sub(reserved);
            if available_budget < cost {
                continue;
            }
            for creative in &state.campaign.creatives {
                if !InMemoryMarketplace::matches_creative(creative, &ctx) {
                    continue;
                }
                let willingness = creative.willingness_to_pay_usd_micros();
                let demand = willingness.min(available_budget);
                if demand < cost {
                    continue;
                }
                let outcome = MatchOutcome {
                    campaign_id: state.campaign.id.clone(),
                    creative_id: creative.id.clone(),
                    price_per_mib_usd_micros: price_per_mib,
                    total_usd_micros: cost,
                };
                match &mut best {
                    Some((current, _, current_cost, current_demand)) => {
                        if demand > *current_demand
                            || (demand == *current_demand && available_budget > *current_cost)
                        {
                            *current = outcome;
                            *current_cost = available_budget;
                            *current_demand = demand;
                        }
                    }
                    None => {
                        best = Some((outcome, campaign_id.clone(), available_budget, demand));
                    }
                }
            }
        }
        drop(pending);
        drop(campaigns);
        let Some((outcome, campaign_id, _, demand)) = best else {
            return None;
        };
        let mut reservations = self.reservations.write().unwrap();
        if reservations.contains_key(&key) {
            return None;
        }
        let mut pending = self.pending.write().unwrap();
        let campaigns = self.campaigns.read().unwrap();
        let Some(state) = campaigns.get(&campaign_id) else {
            return None;
        };
        let reserved = pending.get(&campaign_id).copied().unwrap_or(0);
        if state.remaining_budget_usd_micros.saturating_sub(reserved) < outcome.total_usd_micros {
            return None;
        }
        reservations.insert(
            key,
            ReservationState {
                campaign_id: campaign_id.clone(),
                creative_id: outcome.creative_id.clone(),
                bytes: ctx.bytes,
                price_per_mib_usd_micros: price_per_mib,
                total_usd_micros: outcome.total_usd_micros,
                demand_usd_micros: demand,
                cohort,
            },
        );
        let entry = pending.entry(campaign_id).or_insert(0);
        *entry = entry.saturating_add(outcome.total_usd_micros);
        Some(outcome)
    }

    fn commit(&self, key: &ReservationKey) -> Option<SettlementBreakdown> {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)?
        };
        let mut campaigns = self.campaigns.write().unwrap();
        let mut pending = self.pending.write().unwrap();
        let state = campaigns.get_mut(&reservation.campaign_id)?;
        if state.remaining_budget_usd_micros < reservation.total_usd_micros {
            return None;
        }
        state.remaining_budget_usd_micros -= reservation.total_usd_micros;
        match pending.entry(reservation.campaign_id.clone()) {
            Entry::Occupied(mut entry) => {
                let value = entry.get_mut();
                *value = value.saturating_sub(reservation.total_usd_micros);
                if *value == 0 {
                    entry.remove();
                }
            }
            Entry::Vacant(_) => {}
        }
        drop(pending);
        drop(campaigns);
        {
            let mut pricing = self.pricing.write().unwrap();
            if let Some(state) = pricing.get_mut(&reservation.cohort) {
                state.record(reservation.demand_usd_micros, reservation.total_usd_micros);
            }
        }
        let policy = *self.distribution.read().unwrap();
        let oracle = *self.oracle.read().unwrap();
        let parts = allocate_usd(reservation.total_usd_micros, &policy);
        let (liquidity_ct_usd, liquidity_it_usd) = split_liquidity_usd(parts.liquidity, policy);
        let (viewer_ct, viewer_ct_rem) = usd_to_tokens(parts.viewer, oracle.ct_price_usd_micros);
        let (host_ct, host_ct_rem) = usd_to_tokens(parts.host, oracle.ct_price_usd_micros);
        let (hardware_ct, hardware_ct_rem) =
            usd_to_tokens(parts.hardware, oracle.ct_price_usd_micros);
        let (verifier_ct, verifier_ct_rem) =
            usd_to_tokens(parts.verifier, oracle.ct_price_usd_micros);
        let (liquidity_ct, liquidity_ct_rem) =
            usd_to_tokens(liquidity_ct_usd, oracle.ct_price_usd_micros);
        let mut ct_remainder_usd = parts
            .remainder
            .saturating_add(viewer_ct_rem)
            .saturating_add(host_ct_rem)
            .saturating_add(hardware_ct_rem)
            .saturating_add(verifier_ct_rem)
            .saturating_add(liquidity_ct_rem);
        let (miner_ct, miner_ct_rem) = usd_to_tokens(ct_remainder_usd, oracle.ct_price_usd_micros);
        ct_remainder_usd = miner_ct_rem;

        let total_ct = viewer_ct
            .saturating_add(host_ct)
            .saturating_add(hardware_ct)
            .saturating_add(verifier_ct)
            .saturating_add(liquidity_ct)
            .saturating_add(miner_ct);

        let (host_it, _) = usd_to_tokens(parts.host, oracle.it_price_usd_micros);
        let (hardware_it, _) = usd_to_tokens(parts.hardware, oracle.it_price_usd_micros);
        let (verifier_it, _) = usd_to_tokens(parts.verifier, oracle.it_price_usd_micros);
        let (liquidity_it, _) = usd_to_tokens(liquidity_it_usd, oracle.it_price_usd_micros);
        let (miner_it, unsettled_after_it) =
            usd_to_tokens(ct_remainder_usd, oracle.it_price_usd_micros);
        let remainder_usd = unsettled_after_it;
        Some(SettlementBreakdown {
            campaign_id: reservation.campaign_id,
            creative_id: reservation.creative_id,
            bytes: reservation.bytes,
            price_per_mib_usd_micros: reservation.price_per_mib_usd_micros,
            total_usd_micros: reservation.total_usd_micros,
            demand_usd_micros: reservation.demand_usd_micros,
            viewer_ct,
            host_ct,
            hardware_ct,
            verifier_ct,
            host_it,
            hardware_it,
            verifier_it,
            liquidity_ct,
            liquidity_it,
            miner_ct,
            miner_it,
            total_ct,
            unsettled_usd_micros: remainder_usd,
            ct_price_usd_micros: oracle.ct_price_usd_micros,
            it_price_usd_micros: oracle.it_price_usd_micros,
        })
    }

    fn cancel(&self, key: &ReservationKey) {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)
        };
        if let Some(res) = reservation {
            let mut pending = self.pending.write().unwrap();
            if let Entry::Occupied(mut entry) = pending.entry(res.campaign_id) {
                let value = entry.get_mut();
                *value = value.saturating_sub(res.total_usd_micros);
                if *value == 0 {
                    entry.remove();
                }
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
        Ok(Self {
            _db: db,
            campaigns_tree,
            metadata_tree,
            config: normalized.clone(),
            campaigns: RwLock::new(campaigns),
            reservations: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashMap::new()),
            distribution: RwLock::new(distribution),
            pricing: RwLock::new(HashMap::new()),
            oracle: RwLock::new(normalized.default_oracle),
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
}

impl Marketplace for SledMarketplace {
    fn register_campaign(&self, campaign: Campaign) -> Result<(), MarketplaceError> {
        let mut guard = self.campaigns.write().unwrap();
        if guard.contains_key(&campaign.id) {
            return Err(MarketplaceError::DuplicateCampaign);
        }
        let state = CampaignState {
            remaining_budget_usd_micros: campaign.budget_usd_micros,
            campaign,
        };
        guard.insert(state.campaign.id.clone(), state.clone());
        drop(guard);
        if let Err(err) = self.persist_campaign(&state) {
            let mut guard = self.campaigns.write().unwrap();
            guard.remove(&state.campaign.id);
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
            })
            .collect()
    }

    fn reserve_impression(
        &self,
        key: ReservationKey,
        ctx: ImpressionContext,
    ) -> Option<MatchOutcome> {
        let cohort = InMemoryMarketplace::cohort_key(&ctx);
        let price_per_mib = {
            let mut pricing = self.pricing.write().unwrap();
            InMemoryMarketplace::get_price_and_state(&mut pricing, &cohort, &self.config)
                .price_per_mib_usd_micros()
        };
        let cost = usd_cost_for_bytes(price_per_mib, ctx.bytes);
        if cost == 0 {
            return None;
        }
        let campaigns = self.campaigns.read().unwrap();
        let pending = self.pending.read().unwrap();
        let mut best: Option<(MatchOutcome, String, u64, u64)> = None;
        for (campaign_id, state) in campaigns.iter() {
            if !InMemoryMarketplace::matches_targeting(&state.campaign.targeting, &ctx) {
                continue;
            }
            let reserved = pending.get(campaign_id).copied().unwrap_or(0);
            let available_budget = state.remaining_budget_usd_micros.saturating_sub(reserved);
            if available_budget < cost {
                continue;
            }
            for creative in &state.campaign.creatives {
                if !InMemoryMarketplace::matches_creative(creative, &ctx) {
                    continue;
                }
                let willingness = creative.willingness_to_pay_usd_micros();
                let demand = willingness.min(available_budget);
                if demand < cost {
                    continue;
                }
                let outcome = MatchOutcome {
                    campaign_id: state.campaign.id.clone(),
                    creative_id: creative.id.clone(),
                    price_per_mib_usd_micros: price_per_mib,
                    total_usd_micros: cost,
                };
                match &mut best {
                    Some((current, _, current_budget, current_demand)) => {
                        if demand > *current_demand
                            || (demand == *current_demand && available_budget > *current_budget)
                        {
                            *current = outcome;
                            *current_budget = available_budget;
                            *current_demand = demand;
                        }
                    }
                    None => {
                        best = Some((outcome, campaign_id.clone(), available_budget, demand));
                    }
                }
            }
        }
        drop(pending);
        drop(campaigns);
        let Some((outcome, campaign_id, _, demand)) = best else {
            return None;
        };
        let mut reservations = self.reservations.write().unwrap();
        if reservations.contains_key(&key) {
            return None;
        }
        let mut pending = self.pending.write().unwrap();
        let campaigns = self.campaigns.read().unwrap();
        let Some(state) = campaigns.get(&campaign_id) else {
            return None;
        };
        let reserved = pending.get(&campaign_id).copied().unwrap_or(0);
        if state.remaining_budget_usd_micros.saturating_sub(reserved) < outcome.total_usd_micros {
            return None;
        }
        reservations.insert(
            key,
            ReservationState {
                campaign_id: campaign_id.clone(),
                creative_id: outcome.creative_id.clone(),
                bytes: ctx.bytes,
                price_per_mib_usd_micros: price_per_mib,
                total_usd_micros: outcome.total_usd_micros,
                demand_usd_micros: demand,
                cohort,
            },
        );
        let entry = pending.entry(campaign_id).or_insert(0);
        *entry = entry.saturating_add(outcome.total_usd_micros);
        Some(outcome)
    }

    fn commit(&self, key: &ReservationKey) -> Option<SettlementBreakdown> {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)?
        };
        let mut campaigns = self.campaigns.write().unwrap();
        let mut pending = self.pending.write().unwrap();
        let state = campaigns.get_mut(&reservation.campaign_id)?;
        if state.remaining_budget_usd_micros < reservation.total_usd_micros {
            return None;
        }
        state.remaining_budget_usd_micros -= reservation.total_usd_micros;
        match pending.entry(reservation.campaign_id.clone()) {
            Entry::Occupied(mut entry) => {
                let value = entry.get_mut();
                *value = value.saturating_sub(reservation.total_usd_micros);
                if *value == 0 {
                    entry.remove();
                }
            }
            Entry::Vacant(_) => {}
        }
        let snapshot = state.clone();
        drop(pending);
        drop(campaigns);
        if let Err(err) = self.persist_campaign(&snapshot) {
            let mut campaigns = self.campaigns.write().unwrap();
            if let Some(state) = campaigns.get_mut(&snapshot.campaign.id) {
                state.remaining_budget_usd_micros = state
                    .remaining_budget_usd_micros
                    .saturating_add(reservation.total_usd_micros);
            }
            panic!(
                "failed to persist campaign {} after commit: {err}",
                snapshot.campaign.id
            );
        }
        {
            let mut pricing = self.pricing.write().unwrap();
            if let Some(state) = pricing.get_mut(&reservation.cohort) {
                state.record(reservation.demand_usd_micros, reservation.total_usd_micros);
            }
        }
        let policy = *self.distribution.read().unwrap();
        let oracle = *self.oracle.read().unwrap();
        let parts = allocate_usd(reservation.total_usd_micros, &policy);
        let (liquidity_ct_usd, liquidity_it_usd) = split_liquidity_usd(parts.liquidity, policy);
        let (viewer_ct, viewer_ct_rem) = usd_to_tokens(parts.viewer, oracle.ct_price_usd_micros);
        let (host_ct, host_ct_rem) = usd_to_tokens(parts.host, oracle.ct_price_usd_micros);
        let (hardware_ct, hardware_ct_rem) =
            usd_to_tokens(parts.hardware, oracle.ct_price_usd_micros);
        let (verifier_ct, verifier_ct_rem) =
            usd_to_tokens(parts.verifier, oracle.ct_price_usd_micros);
        let (liquidity_ct, liquidity_ct_rem) =
            usd_to_tokens(liquidity_ct_usd, oracle.ct_price_usd_micros);
        let mut ct_remainder_usd = parts
            .remainder
            .saturating_add(viewer_ct_rem)
            .saturating_add(host_ct_rem)
            .saturating_add(hardware_ct_rem)
            .saturating_add(verifier_ct_rem)
            .saturating_add(liquidity_ct_rem);
        let (miner_ct, miner_ct_rem) = usd_to_tokens(ct_remainder_usd, oracle.ct_price_usd_micros);
        ct_remainder_usd = miner_ct_rem;

        let total_ct = viewer_ct
            .saturating_add(host_ct)
            .saturating_add(hardware_ct)
            .saturating_add(verifier_ct)
            .saturating_add(liquidity_ct)
            .saturating_add(miner_ct);

        let (host_it, _) = usd_to_tokens(parts.host, oracle.it_price_usd_micros);
        let (hardware_it, _) = usd_to_tokens(parts.hardware, oracle.it_price_usd_micros);
        let (verifier_it, _) = usd_to_tokens(parts.verifier, oracle.it_price_usd_micros);
        let (liquidity_it, _) = usd_to_tokens(liquidity_it_usd, oracle.it_price_usd_micros);
        let (miner_it, unsettled_after_it) =
            usd_to_tokens(ct_remainder_usd, oracle.it_price_usd_micros);
        let remainder_usd = unsettled_after_it;
        Some(SettlementBreakdown {
            campaign_id: reservation.campaign_id,
            creative_id: reservation.creative_id,
            bytes: reservation.bytes,
            price_per_mib_usd_micros: reservation.price_per_mib_usd_micros,
            total_usd_micros: reservation.total_usd_micros,
            demand_usd_micros: reservation.demand_usd_micros,
            viewer_ct,
            host_ct,
            hardware_ct,
            verifier_ct,
            host_it,
            hardware_it,
            verifier_it,
            liquidity_ct,
            liquidity_it,
            miner_ct,
            miner_it,
            total_ct,
            unsettled_usd_micros: remainder_usd,
            ct_price_usd_micros: oracle.ct_price_usd_micros,
            it_price_usd_micros: oracle.it_price_usd_micros,
        })
    }

    fn cancel(&self, key: &ReservationKey) {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)
        };
        if let Some(res) = reservation {
            let mut pending = self.pending.write().unwrap();
            if let Entry::Occupied(mut entry) = pending.entry(res.campaign_id) {
                let value = entry.get_mut();
                *value = value.saturating_sub(res.total_usd_micros);
                if *value == 0 {
                    entry.remove();
                }
            }
        }
    }

    fn distribution(&self) -> DistributionPolicy {
        *self.distribution.read().unwrap()
    }

    fn update_distribution(&self, policy: DistributionPolicy) {
        if let Err(err) = self.persist_distribution(&policy.normalize()) {
            panic!("failed to persist ad_market distribution: {err}");
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
}
struct RoleUsdParts {
    viewer: u64,
    host: u64,
    hardware: u64,
    verifier: u64,
    liquidity: u64,
    remainder: u64,
}

fn allocate_usd(total_usd_micros: u64, distribution: &DistributionPolicy) -> RoleUsdParts {
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
    RoleUsdParts {
        viewer: allocations.get(0).copied().unwrap_or(0),
        host: allocations.get(1).copied().unwrap_or(0),
        hardware: allocations.get(2).copied().unwrap_or(0),
        verifier: allocations.get(3).copied().unwrap_or(0),
        liquidity: allocations.get(4).copied().unwrap_or(0),
        remainder: total_usd_micros.saturating_sub(allocations.iter().copied().sum::<u64>()),
    }
}

fn split_liquidity_usd(total_liquidity_usd: u64, policy: DistributionPolicy) -> (u64, u64) {
    if total_liquidity_usd == 0 {
        return (0, 0);
    }
    let ct_usd = (u128::from(total_liquidity_usd) * u128::from(policy.liquidity_split_ct_ppm))
        / u128::from(PPM_SCALE);
    let ct_usd = ct_usd as u64;
    let it_usd = total_liquidity_usd.saturating_sub(ct_usd);
    (ct_usd, it_usd)
}

fn usd_to_tokens(amount_usd_micros: u64, price_usd_micros: u64) -> (u64, u64) {
    if price_usd_micros == 0 {
        return (0, amount_usd_micros);
    }
    let tokens = amount_usd_micros / price_usd_micros;
    let remainder = amount_usd_micros.saturating_sub(tokens.saturating_mul(price_usd_micros));
    (tokens, remainder)
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
        let market = InMemoryMarketplace::new(MarketplaceConfig::default());
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
        };
        market.update_oracle(TokenOracle::new(50_000, 25_000));
        let outcome = market
            .reserve_impression(key, ctx.clone())
            .expect("reservation succeeded");
        assert!(outcome.total_usd_micros > 0);
        let settlement = market.commit(&key).expect("commit succeeds");
        assert_eq!(settlement.bytes, BYTES_PER_MIB);
        assert_eq!(settlement.total_usd_micros, outcome.total_usd_micros);
        assert!(settlement.viewer_ct > 0);
        assert!(settlement.host_ct > 0);
        assert!(settlement.hardware_ct > 0);
        assert!(settlement.verifier_ct > 0);
        let policy = market.distribution();
        let parts = allocate_usd(settlement.total_usd_micros, &policy);
        let (expected_liquidity_ct_usd, expected_liquidity_it_usd) =
            split_liquidity_usd(parts.liquidity, policy);
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
        };
        let key = ReservationKey {
            manifest: [9u8; 32],
            path_hash: [8u8; 32],
            discriminator: [7u8; 32],
        };
        market.reserve_impression(key, ctx).expect("reserved");
        market.cancel(&key);
        let summary = market.list_campaigns();
        assert_eq!(
            summary[0].remaining_budget_usd_micros,
            2 * MICROS_PER_DOLLAR
        );
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
            learning_rate_ppm: 200_000,
            smoothing_ppm: 500_000,
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
}
