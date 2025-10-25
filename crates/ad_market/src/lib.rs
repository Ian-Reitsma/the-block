use foundation_serialization::{
    self,
    json::{self, Map as JsonMap, Number as JsonNumber, Value as JsonValue},
    Deserialize, Serialize,
};
use sled::{Config as SledConfig, Db as SledDb, Tree as SledTree};
use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};

const TREE_CAMPAIGNS: &str = "campaigns";
const TREE_METADATA: &str = "metadata";
const KEY_DISTRIBUTION: &[u8] = b"distribution";

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
            PersistenceError::Serialization(err) => write!(f, "serialization error: {err}"),
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
    Ok(DistributionPolicy::new(
        read_u64(obj, "viewer_percent")?,
        read_u64(obj, "host_percent")?,
        read_u64(obj, "hardware_percent")?,
        read_u64(obj, "verifier_percent")?,
        read_u64(obj, "liquidity_percent")?,
    ))
}

fn creative_to_value(creative: &Creative) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("id".into(), JsonValue::String(creative.id.clone()));
    map.insert(
        "price_per_mib_ct".into(),
        JsonValue::Number(JsonNumber::from(creative.price_per_mib_ct)),
    );
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
        price_per_mib_ct: read_u64(obj, "price_per_mib_ct")?,
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
        "budget_ct".into(),
        JsonValue::Number(JsonNumber::from(campaign.budget_ct)),
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
        budget_ct: read_u64(obj, "budget_ct")?,
        creatives,
        targeting,
        metadata: read_string_map(obj, "metadata")?,
    })
}

fn campaign_state_to_value(state: &CampaignState) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("campaign".into(), campaign_to_value(&state.campaign));
    map.insert(
        "remaining_budget_ct".into(),
        JsonValue::Number(JsonNumber::from(state.remaining_budget_ct)),
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
    let remaining = read_u64(obj, "remaining_budget_ct")?;
    Ok(CampaignState {
        campaign,
        remaining_budget_ct: remaining,
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

fn read_string(map: &JsonMap, key: &str) -> Result<String, PersistenceError> {
    map.get(key)
        .and_then(JsonValue::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| invalid(format!("{key} must be a string")))
}

fn read_u64(map: &JsonMap, key: &str) -> Result<u64, PersistenceError> {
    map.get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| invalid(format!("{key} must be a u64")))
}

fn read_string_vec(map: &JsonMap, key: &str) -> Result<Vec<String>, PersistenceError> {
    match map.get(key) {
        None => Ok(Vec::new()),
        Some(JsonValue::Array(items)) => items
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReservationKey {
    pub manifest: [u8; 32],
    pub path_hash: [u8; 32],
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CampaignTargeting {
    pub domains: Vec<String>,
    pub badges: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Creative {
    pub id: String,
    /// Price denominated in CT per mebibyte served.
    pub price_per_mib_ct: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub badges: Vec<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub domains: Vec<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub metadata: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Campaign {
    pub id: String,
    pub advertiser_account: String,
    pub budget_ct: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub creatives: Vec<Creative>,
    #[serde(default)]
    pub targeting: CampaignTargeting,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub metadata: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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
        Self::new(
            self.viewer_percent,
            self.host_percent,
            self.hardware_percent,
            self.verifier_percent,
            self.liquidity_percent,
        )
    }
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
    pub price_per_mib_ct: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettlementBreakdown {
    pub campaign_id: String,
    pub creative_id: String,
    pub bytes: u64,
    pub total_ct: u64,
    pub viewer_ct: u64,
    pub host_ct: u64,
    pub hardware_ct: u64,
    pub verifier_ct: u64,
    pub liquidity_ct: u64,
    pub miner_ct: u64,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignSummary {
    pub id: String,
    pub advertiser_account: String,
    pub remaining_budget_ct: u64,
    pub creatives: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CampaignState {
    campaign: Campaign,
    remaining_budget_ct: u64,
}

struct ReservationState {
    campaign_id: String,
    creative_id: String,
    bytes: u64,
    cost_ct: u64,
}

pub struct SledMarketplace {
    _db: SledDb,
    campaigns_tree: SledTree,
    metadata_tree: SledTree,
    campaigns: RwLock<HashMap<String, CampaignState>>,
    reservations: RwLock<HashMap<ReservationKey, ReservationState>>,
    pending: RwLock<HashMap<String, u64>>,
    distribution: RwLock<DistributionPolicy>,
}

pub struct InMemoryMarketplace {
    campaigns: RwLock<HashMap<String, CampaignState>>,
    reservations: RwLock<HashMap<ReservationKey, ReservationState>>,
    pending: RwLock<HashMap<String, u64>>,
    distribution: RwLock<DistributionPolicy>,
}

impl InMemoryMarketplace {
    pub fn new(distribution: DistributionPolicy) -> Self {
        Self {
            campaigns: RwLock::new(HashMap::new()),
            reservations: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashMap::new()),
            distribution: RwLock::new(distribution.normalize()),
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

    fn allocate(distribution: &DistributionPolicy, total: u64) -> SettlementBreakdownParts {
        let weights = [
            (0usize, distribution.viewer_percent),
            (1, distribution.host_percent),
            (2, distribution.hardware_percent),
            (3, distribution.verifier_percent),
            (4, distribution.liquidity_percent),
        ];
        let allocations = distribute_scalar(total, &weights);
        SettlementBreakdownParts {
            viewer: allocations.get(0).copied().unwrap_or(0),
            host: allocations.get(1).copied().unwrap_or(0),
            hardware: allocations.get(2).copied().unwrap_or(0),
            verifier: allocations.get(3).copied().unwrap_or(0),
            liquidity: allocations.get(4).copied().unwrap_or(0),
        }
    }
}

impl SledMarketplace {
    pub fn open<P: AsRef<Path>>(
        path: P,
        default_distribution: DistributionPolicy,
    ) -> Result<Self, PersistenceError> {
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
                let normalized = default_distribution.normalize();
                let payload = serialize_distribution(&normalized)?;
                metadata_tree.insert(KEY_DISTRIBUTION, payload)?;
                metadata_tree.flush()?;
                normalized
            }
        };

        let mut campaigns = HashMap::new();
        for entry in campaigns_tree.iter() {
            let (_key, value) = entry?;
            let state = deserialize_campaign_state(&value)?;
            campaigns.insert(state.campaign.id.clone(), state);
        }

        let marketplace = Self {
            _db: db,
            campaigns_tree,
            metadata_tree,
            campaigns: RwLock::new(campaigns),
            reservations: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashMap::new()),
            distribution: RwLock::new(distribution),
        };

        Ok(marketplace)
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

struct SettlementBreakdownParts {
    viewer: u64,
    host: u64,
    hardware: u64,
    verifier: u64,
    liquidity: u64,
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

fn cost_for_bytes(price_per_mib: u64, bytes: u64) -> u64 {
    if price_per_mib == 0 || bytes == 0 {
        return 0;
    }
    let bytes_per_mib = 1_048_576u64;
    let numerator = price_per_mib.saturating_mul(bytes);
    (numerator + bytes_per_mib - 1) / bytes_per_mib
}

impl Marketplace for InMemoryMarketplace {
    fn register_campaign(&self, campaign: Campaign) -> Result<(), MarketplaceError> {
        let mut guard = self.campaigns.write().unwrap();
        if guard.contains_key(&campaign.id) {
            return Err(MarketplaceError::DuplicateCampaign);
        }
        guard.insert(
            campaign.id.clone(),
            CampaignState {
                remaining_budget_ct: campaign.budget_ct,
                campaign,
            },
        );
        Ok(())
    }

    fn list_campaigns(&self) -> Vec<CampaignSummary> {
        let guard = self.campaigns.read().unwrap();
        guard
            .values()
            .map(|state| CampaignSummary {
                id: state.campaign.id.clone(),
                advertiser_account: state.campaign.advertiser_account.clone(),
                remaining_budget_ct: state.remaining_budget_ct,
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
        let campaigns = self.campaigns.read().unwrap();
        let mut pending = self.pending.write().unwrap();
        let mut best: Option<(MatchOutcome, String, u64)> = None;
        for (campaign_id, state) in campaigns.iter() {
            if !Self::matches_targeting(&state.campaign.targeting, &ctx) {
                continue;
            }
            let reserved = pending.get(campaign_id).copied().unwrap_or(0);
            let available_budget = state.remaining_budget_ct.saturating_sub(reserved);
            if available_budget == 0 {
                continue;
            }
            for creative in &state.campaign.creatives {
                if !Self::matches_creative(creative, &ctx) {
                    continue;
                }
                let cost = cost_for_bytes(creative.price_per_mib_ct, ctx.bytes);
                if cost == 0 || available_budget < cost {
                    continue;
                }
                let outcome = MatchOutcome {
                    campaign_id: state.campaign.id.clone(),
                    creative_id: creative.id.clone(),
                    price_per_mib_ct: creative.price_per_mib_ct,
                };
                match &mut best {
                    Some((current, current_id, current_cost)) => {
                        if creative.price_per_mib_ct > current.price_per_mib_ct
                            || (creative.price_per_mib_ct == current.price_per_mib_ct
                                && cost > *current_cost)
                        {
                            *current = outcome;
                            *current_id = campaign_id.clone();
                            *current_cost = cost;
                        }
                    }
                    None => {
                        best = Some((outcome, campaign_id.clone(), cost));
                    }
                }
            }
        }
        if let Some((outcome, campaign_id, cost_ct)) = best {
            let entry = pending.entry(campaign_id.clone()).or_insert(0);
            *entry = entry.saturating_add(cost_ct);
            drop(pending);
            drop(campaigns);
            let mut reservations = self.reservations.write().unwrap();
            reservations.insert(
                key,
                ReservationState {
                    campaign_id,
                    creative_id: outcome.creative_id.clone(),
                    bytes: ctx.bytes,
                    cost_ct,
                },
            );
            Some(outcome)
        } else {
            None
        }
    }

    fn commit(&self, key: &ReservationKey) -> Option<SettlementBreakdown> {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)
        }?;
        let mut campaigns = self.campaigns.write().unwrap();
        let mut pending = self.pending.write().unwrap();
        let state = campaigns.get_mut(&reservation.campaign_id)?;
        if state.remaining_budget_ct < reservation.cost_ct {
            return None;
        }
        state.remaining_budget_ct -= reservation.cost_ct;
        match pending.entry(reservation.campaign_id.clone()) {
            Entry::Occupied(mut entry) => {
                let value = entry.get_mut();
                *value = value.saturating_sub(reservation.cost_ct);
                if *value == 0 {
                    entry.remove();
                }
            }
            Entry::Vacant(_) => {}
        }
        drop(pending);
        let policy = *self.distribution.read().unwrap();
        let parts = InMemoryMarketplace::allocate(&policy, reservation.cost_ct);
        let distributed = parts
            .viewer
            .saturating_add(parts.host)
            .saturating_add(parts.hardware)
            .saturating_add(parts.verifier)
            .saturating_add(parts.liquidity);
        let miner = reservation.cost_ct.saturating_sub(distributed);
        Some(SettlementBreakdown {
            campaign_id: reservation.campaign_id,
            creative_id: reservation.creative_id,
            bytes: reservation.bytes,
            total_ct: reservation.cost_ct,
            viewer_ct: parts.viewer,
            host_ct: parts.host,
            hardware_ct: parts.hardware,
            verifier_ct: parts.verifier,
            liquidity_ct: parts.liquidity,
            miner_ct: miner,
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
                *value = value.saturating_sub(res.cost_ct);
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
}

impl Marketplace for SledMarketplace {
    fn register_campaign(&self, campaign: Campaign) -> Result<(), MarketplaceError> {
        let mut guard = self.campaigns.write().unwrap();
        if guard.contains_key(&campaign.id) {
            return Err(MarketplaceError::DuplicateCampaign);
        }
        let state = CampaignState {
            remaining_budget_ct: campaign.budget_ct,
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
                remaining_budget_ct: state.remaining_budget_ct,
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
        let campaigns = self.campaigns.read().unwrap();
        let mut pending = self.pending.write().unwrap();
        let mut best: Option<(MatchOutcome, String, u64)> = None;
        for (campaign_id, state) in campaigns.iter() {
            if !InMemoryMarketplace::matches_targeting(&state.campaign.targeting, &ctx) {
                continue;
            }
            let reserved = pending.get(campaign_id).copied().unwrap_or(0);
            let available_budget = state.remaining_budget_ct.saturating_sub(reserved);
            if available_budget == 0 {
                continue;
            }
            for creative in &state.campaign.creatives {
                if !InMemoryMarketplace::matches_creative(creative, &ctx) {
                    continue;
                }
                let cost = cost_for_bytes(creative.price_per_mib_ct, ctx.bytes);
                if cost == 0 || available_budget < cost {
                    continue;
                }
                let outcome = MatchOutcome {
                    campaign_id: state.campaign.id.clone(),
                    creative_id: creative.id.clone(),
                    price_per_mib_ct: creative.price_per_mib_ct,
                };
                match &mut best {
                    Some((current, current_id, current_cost)) => {
                        if creative.price_per_mib_ct > current.price_per_mib_ct
                            || (creative.price_per_mib_ct == current.price_per_mib_ct
                                && cost > *current_cost)
                        {
                            *current = outcome;
                            *current_id = campaign_id.clone();
                            *current_cost = cost;
                        }
                    }
                    None => {
                        best = Some((outcome, campaign_id.clone(), cost));
                    }
                }
            }
        }
        if let Some((outcome, campaign_id, cost_ct)) = best {
            let entry = pending.entry(campaign_id.clone()).or_insert(0);
            *entry = entry.saturating_add(cost_ct);
            drop(pending);
            drop(campaigns);
            let mut reservations = self.reservations.write().unwrap();
            reservations.insert(
                key,
                ReservationState {
                    campaign_id,
                    creative_id: outcome.creative_id.clone(),
                    bytes: ctx.bytes,
                    cost_ct,
                },
            );
            Some(outcome)
        } else {
            None
        }
    }

    fn commit(&self, key: &ReservationKey) -> Option<SettlementBreakdown> {
        let reservation = {
            let mut guard = self.reservations.write().unwrap();
            guard.remove(key)?
        };
        let mut campaigns = self.campaigns.write().unwrap();
        let mut pending = self.pending.write().unwrap();
        let state = campaigns.get_mut(&reservation.campaign_id)?;
        if state.remaining_budget_ct < reservation.cost_ct {
            return None;
        }
        state.remaining_budget_ct -= reservation.cost_ct;
        match pending.entry(reservation.campaign_id.clone()) {
            Entry::Occupied(mut entry) => {
                let value = entry.get_mut();
                *value = value.saturating_sub(reservation.cost_ct);
                if *value == 0 {
                    entry.remove();
                }
            }
            Entry::Vacant(_) => {}
        }
        drop(pending);
        let snapshot = state.clone();
        drop(campaigns);
        if let Err(err) = self.persist_campaign(&snapshot) {
            let mut campaigns = self.campaigns.write().unwrap();
            if let Some(state) = campaigns.get_mut(&snapshot.campaign.id) {
                state.remaining_budget_ct = state
                    .remaining_budget_ct
                    .saturating_add(reservation.cost_ct);
            }
            panic!(
                "failed to persist campaign {} after commit: {err}",
                snapshot.campaign.id
            );
        }
        let policy = *self.distribution.read().unwrap();
        let parts = InMemoryMarketplace::allocate(&policy, reservation.cost_ct);
        let distributed = parts
            .viewer
            .saturating_add(parts.host)
            .saturating_add(parts.hardware)
            .saturating_add(parts.verifier)
            .saturating_add(parts.liquidity);
        let miner = reservation.cost_ct.saturating_sub(distributed);
        Some(SettlementBreakdown {
            campaign_id: reservation.campaign_id,
            creative_id: reservation.creative_id,
            bytes: reservation.bytes,
            total_ct: reservation.cost_ct,
            viewer_ct: parts.viewer,
            host_ct: parts.host,
            hardware_ct: parts.hardware,
            verifier_ct: parts.verifier,
            liquidity_ct: parts.liquidity,
            miner_ct: miner,
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
                *value = value.saturating_sub(res.cost_ct);
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
        let previous = *guard;
        let normalized = policy.normalize();
        *guard = normalized;
        if let Err(err) = self.persist_distribution(&normalized) {
            *guard = previous;
            panic!("failed to persist ad_market distribution: {err}");
        }
    }
}

pub type MarketplaceHandle = Arc<dyn Marketplace>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier, Mutex,
    };
    use sys::tempfile::TempDir;

    #[test]
    fn reserves_and_commits_impression() {
        let market = InMemoryMarketplace::new(DistributionPolicy::new(40, 30, 20, 5, 5));
        market
            .register_campaign(Campaign {
                id: "cmp".to_string(),
                advertiser_account: "adv".to_string(),
                budget_ct: 10_000,
                creatives: vec![Creative {
                    id: "creative".to_string(),
                    price_per_mib_ct: 100,
                    badges: Vec::new(),
                    domains: vec!["example".to_string()],
                    metadata: HashMap::new(),
                }],
                targeting: CampaignTargeting {
                    domains: vec!["example".to_string()],
                    badges: Vec::new(),
                },
                metadata: HashMap::new(),
            })
            .expect("campaign registered");
        let key = ReservationKey {
            manifest: [1u8; 32],
            path_hash: [2u8; 32],
        };
        let ctx = ImpressionContext {
            domain: "example".to_string(),
            provider: Some("provider".to_string()),
            badges: Vec::new(),
            bytes: 1_048_576,
        };
        let outcome = market
            .reserve_impression(key, ctx)
            .expect("matched creative");
        assert_eq!(outcome.creative_id, "creative");
        let settlement = market.commit(&key).expect("settlement committed");
        assert_eq!(settlement.total_ct, 100);
        assert_eq!(settlement.viewer_ct, 40);
        assert_eq!(settlement.host_ct, 30);
        assert_eq!(settlement.hardware_ct, 20);
        assert_eq!(settlement.verifier_ct, 5);
        assert_eq!(settlement.liquidity_ct, 5);
        assert_eq!(settlement.miner_ct, 0);
        assert!(market.commit(&key).is_none());
    }

    #[test]
    fn sled_marketplace_persists_across_restarts() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("ad_market.db");
        let distribution = DistributionPolicy::new(40, 30, 20, 5, 5);
        let market = SledMarketplace::open(&path, distribution).expect("open sled market");
        market
            .register_campaign(Campaign {
                id: "cmp".to_string(),
                advertiser_account: "adv".to_string(),
                budget_ct: 10_000,
                creatives: vec![Creative {
                    id: "creative".to_string(),
                    price_per_mib_ct: 100,
                    badges: vec!["physical_presence".to_string()],
                    domains: vec!["example".to_string()],
                    metadata: HashMap::new(),
                }],
                targeting: CampaignTargeting {
                    domains: vec!["example".to_string()],
                    badges: vec!["physical_presence".to_string()],
                },
                metadata: HashMap::new(),
            })
            .expect("register campaign");
        let key = ReservationKey {
            manifest: [9u8; 32],
            path_hash: [8u8; 32],
        };
        let ctx = ImpressionContext {
            domain: "example".to_string(),
            provider: Some("provider-1".to_string()),
            badges: vec!["physical_presence".to_string()],
            bytes: 1_048_576,
        };
        let outcome = market
            .reserve_impression(key, ctx)
            .expect("reserve impression");
        assert_eq!(outcome.creative_id, "creative");
        market.commit(&key).expect("commit reservation");
        market.update_distribution(DistributionPolicy::new(50, 25, 15, 5, 5));
        drop(market);

        let reopened = SledMarketplace::open(&path, distribution).expect("reopen sled market");
        let campaigns = reopened.list_campaigns();
        assert_eq!(campaigns.len(), 1);
        assert_eq!(campaigns[0].id, "cmp");
        assert_eq!(campaigns[0].remaining_budget_ct, 9_900);
        let dist = reopened.distribution();
        assert_eq!(dist.viewer_percent, 50);
        assert_eq!(dist.host_percent, 25);
        assert_eq!(dist.hardware_percent, 15);
        assert_eq!(dist.verifier_percent, 5);
        assert_eq!(dist.liquidity_percent, 5);
    }

    #[test]
    fn sled_marketplace_reservations_are_atomic() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("atomic_reservations.db");
        let distribution = DistributionPolicy::new(40, 30, 20, 5, 5);
        let market =
            Arc::new(SledMarketplace::open(&path, distribution).expect("open sled market"));
        market
            .register_campaign(Campaign {
                id: "cmp-atomic".to_string(),
                advertiser_account: "adv".to_string(),
                budget_ct: 200,
                creatives: vec![Creative {
                    id: "creative".to_string(),
                    price_per_mib_ct: 100,
                    badges: Vec::new(),
                    domains: vec!["example.test".to_string()],
                    metadata: HashMap::new(),
                }],
                targeting: CampaignTargeting {
                    domains: vec!["example.test".to_string()],
                    badges: Vec::new(),
                },
                metadata: HashMap::new(),
            })
            .expect("campaign registered");

        let barrier = Arc::new(Barrier::new(4));
        let successes = Arc::new(AtomicUsize::new(0));
        let reserved_keys = Arc::new(Mutex::new(Vec::new()));
        let ctx = ImpressionContext {
            domain: "example.test".to_string(),
            provider: Some("provider-atomic".to_string()),
            badges: Vec::new(),
            bytes: 1_048_576,
        };

        let handles: Vec<_> = (0..4)
            .map(|idx| {
                let market = Arc::clone(&market);
                let barrier = Arc::clone(&barrier);
                let successes = Arc::clone(&successes);
                let reserved_keys = Arc::clone(&reserved_keys);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let key = ReservationKey {
                        manifest: [idx as u8; 32],
                        path_hash: [(idx + 1) as u8; 32],
                    };
                    barrier.wait();
                    if market.reserve_impression(key, ctx).is_some() {
                        successes.fetch_add(1, Ordering::SeqCst);
                        reserved_keys.lock().unwrap().push(key);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("thread join");
        }

        assert_eq!(
            successes.load(Ordering::SeqCst),
            2,
            "only funded reservations succeed"
        );

        let mut keys = reserved_keys.lock().unwrap();
        for key in keys.drain(..) {
            let settlement = market.commit(&key).expect("commit succeeds");
            assert_eq!(settlement.total_ct, 100);
        }

        // Budget exhausted; additional reservations should fail and pending ledger must be clear.
        let new_key = ReservationKey {
            manifest: [9u8; 32],
            path_hash: [8u8; 32],
        };
        let none = market.reserve_impression(new_key, ctx.clone());
        assert!(none.is_none(), "budget exhausted prevents new reservations");

        let campaigns = market.list_campaigns();
        assert_eq!(campaigns.len(), 1);
        assert_eq!(campaigns[0].remaining_budget_ct, 0);
    }

    #[test]
    fn cancel_releases_pending_budget() {
        let market = InMemoryMarketplace::new(DistributionPolicy::new(40, 30, 20, 5, 5));
        market
            .register_campaign(Campaign {
                id: "cmp-cancel".to_string(),
                advertiser_account: "adv".to_string(),
                budget_ct: 150,
                creatives: vec![Creative {
                    id: "creative".to_string(),
                    price_per_mib_ct: 100,
                    badges: Vec::new(),
                    domains: vec!["example.test".to_string()],
                    metadata: HashMap::new(),
                }],
                targeting: CampaignTargeting {
                    domains: vec!["example.test".to_string()],
                    badges: Vec::new(),
                },
                metadata: HashMap::new(),
            })
            .expect("campaign registered");

        let ctx = ImpressionContext {
            domain: "example.test".to_string(),
            provider: Some("provider-cancel".to_string()),
            badges: Vec::new(),
            bytes: 1_048_576,
        };
        let key_one = ReservationKey {
            manifest: [1u8; 32],
            path_hash: [2u8; 32],
        };
        assert!(market.reserve_impression(key_one, ctx.clone()).is_some());

        // Cancel the reservation and ensure budget becomes available again.
        let cancel_key = ReservationKey {
            manifest: [1u8; 32],
            path_hash: [2u8; 32],
        };
        market.cancel(&cancel_key);

        let key_two = ReservationKey {
            manifest: [3u8; 32],
            path_hash: [4u8; 32],
        };
        assert!(
            market.reserve_impression(key_two, ctx).is_some(),
            "budget should be reusable after cancellation"
        );
    }
}
