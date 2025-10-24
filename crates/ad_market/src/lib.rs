use foundation_serialization::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

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

pub struct InMemoryMarketplace {
    campaigns: RwLock<HashMap<String, CampaignState>>,
    reservations: RwLock<HashMap<ReservationKey, ReservationState>>,
    distribution: RwLock<DistributionPolicy>,
}

impl InMemoryMarketplace {
    pub fn new(distribution: DistributionPolicy) -> Self {
        Self {
            campaigns: RwLock::new(HashMap::new()),
            reservations: RwLock::new(HashMap::new()),
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
        let mut best: Option<(MatchOutcome, String, u64)> = None;
        for state in campaigns.values() {
            if !Self::matches_targeting(&state.campaign.targeting, &ctx) {
                continue;
            }
            for creative in &state.campaign.creatives {
                if !Self::matches_creative(creative, &ctx) {
                    continue;
                }
                let cost = cost_for_bytes(creative.price_per_mib_ct, ctx.bytes);
                if cost == 0 || state.remaining_budget_ct < cost {
                    continue;
                }
                let outcome = MatchOutcome {
                    campaign_id: state.campaign.id.clone(),
                    creative_id: creative.id.clone(),
                    price_per_mib_ct: creative.price_per_mib_ct,
                };
                match &mut best {
                    Some((current, _, current_cost)) => {
                        if creative.price_per_mib_ct > current.price_per_mib_ct
                            || (creative.price_per_mib_ct == current.price_per_mib_ct
                                && cost > *current_cost)
                        {
                            *current = outcome;
                            *current_cost = cost;
                        }
                    }
                    None => {
                        best = Some((outcome, state.campaign.id.clone(), cost));
                    }
                }
            }
        }
        drop(campaigns);
        if let Some((outcome, campaign_id, cost_ct)) = best {
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
        let state = campaigns.get_mut(&reservation.campaign_id)?;
        if state.remaining_budget_ct < reservation.cost_ct {
            return None;
        }
        state.remaining_budget_ct -= reservation.cost_ct;
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
        let mut guard = self.reservations.write().unwrap();
        guard.remove(key);
    }

    fn distribution(&self) -> DistributionPolicy {
        *self.distribution.read().unwrap()
    }

    fn update_distribution(&self, policy: DistributionPolicy) {
        let mut guard = self.distribution.write().unwrap();
        *guard = policy.normalize();
    }
}

pub type MarketplaceHandle = Arc<dyn Marketplace>;

#[cfg(test)]
mod tests {
    use super::*;

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
}
