use crate::CohortKey;
use crypto_suite::hashing::blake3;
use foundation_metrics::{gauge, histogram, increment_counter};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BudgetBrokerConfig {
    pub epoch_impressions: u64,
    pub step_size: f64,
    pub dual_step: f64,
    pub dual_forgetting: f64,
    pub max_kappa: f64,
    pub min_kappa: f64,
    pub shadow_price_cap: f64,
    pub smoothing: f64,
    pub epochs_per_budget: u64,
}

impl BudgetBrokerConfig {
    pub fn normalized(mut self) -> Self {
        self.epoch_impressions = self.epoch_impressions.max(1);
        self.step_size = self.step_size.clamp(1e-6, 1.0);
        self.dual_step = self.dual_step.clamp(1e-6, 1.0);
        self.dual_forgetting = self.dual_forgetting.clamp(0.0, 1.0);
        self.max_kappa = self.max_kappa.clamp(0.1, 10.0);
        self.min_kappa = self.min_kappa.clamp(0.0, self.max_kappa);
        self.shadow_price_cap = self
            .shadow_price_cap
            .clamp(self.min_kappa.max(0.0), self.max_kappa * 2.0);
        self.smoothing = self.smoothing.clamp(0.0, 1.0);
        self.epochs_per_budget = self.epochs_per_budget.max(1);
        self
    }
}

impl Default for BudgetBrokerConfig {
    fn default() -> Self {
        Self {
            epoch_impressions: 10_000,
            step_size: 0.05,
            dual_step: 0.02,
            dual_forgetting: 0.85,
            max_kappa: 2.0,
            min_kappa: 0.25,
            shadow_price_cap: 4.0,
            smoothing: 0.2,
            epochs_per_budget: 96,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BudgetBroker {
    config: BudgetBrokerConfig,
    campaigns: HashMap<String, CampaignBudgetState>,
}

#[derive(Clone, Copy, Debug)]
pub struct BidShadingGuidance {
    pub kappa: f64,
    pub shadow_price: f64,
    pub dual_price: f64,
}

impl BidShadingGuidance {
    pub fn scaling_factor(&self) -> f64 {
        let base = self.kappa.clamp(0.0, 10.0);
        if !self.shadow_price.is_finite() || self.shadow_price <= f64::EPSILON {
            return base;
        }
        (base / (1.0 + self.shadow_price)).clamp(0.0, 10.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BidShadingApplication {
    pub requested_kappa: f64,
    pub applied_multiplier: f64,
    pub shadow_price: f64,
    pub dual_price: f64,
}

impl BudgetBroker {
    pub fn new(config: BudgetBrokerConfig) -> Self {
        Self {
            config: config.normalized(),
            campaigns: HashMap::new(),
        }
    }

    pub fn config(&self) -> &BudgetBrokerConfig {
        &self.config
    }

    pub fn snapshot(&self) -> BudgetBrokerSnapshot {
        BudgetBrokerSnapshot {
            generated_at_micros: now_micros(),
            config: self.config.clone(),
            campaigns: self
                .campaigns
                .values()
                .map(CampaignBudgetState::snapshot)
                .collect(),
        }
    }

    pub fn restore(config: BudgetBrokerConfig, snapshot: &BudgetBrokerSnapshot) -> Self {
        let mut broker = BudgetBroker::new(config);
        let normalized = broker.config.clone();
        for campaign in &snapshot.campaigns {
            broker.campaigns.insert(
                campaign.campaign_id.clone(),
                CampaignBudgetState::from_snapshot(campaign, &normalized),
            );
        }
        broker
    }

    pub fn ensure_registered(&mut self, campaign_id: &str, budget_usd_micros: u64) {
        if let Some(state) = self.campaigns.get_mut(campaign_id) {
            state.reconcile_budget(budget_usd_micros, &self.config);
        } else {
            self.register_campaign(campaign_id, budget_usd_micros);
        }
    }

    pub fn register_campaign(&mut self, campaign_id: &str, budget_usd_micros: u64) {
        if self.campaigns.contains_key(campaign_id) {
            self.update_budget(campaign_id, budget_usd_micros);
            return;
        }
        let config = self.config.clone();
        self.campaigns.insert(
            campaign_id.to_string(),
            CampaignBudgetState::new(campaign_id.to_string(), budget_usd_micros, config.clone()),
        );
    }

    pub fn update_budget(&mut self, campaign_id: &str, budget_usd_micros: u64) {
        if let Some(state) = self.campaigns.get_mut(campaign_id) {
            state.set_budget(budget_usd_micros, &self.config);
        } else {
            self.register_campaign(campaign_id, budget_usd_micros);
        }
    }

    pub fn remove_campaign(&mut self, campaign_id: &str) {
        self.campaigns.remove(campaign_id);
    }

    pub(crate) fn guidance_for(
        &mut self,
        campaign_id: &str,
        cohort: &CohortKey,
    ) -> BidShadingGuidance {
        self.ensure_campaign(campaign_id);
        let state = self
            .campaigns
            .get_mut(campaign_id)
            .expect("campaign state must exist");
        state.ensure_cohort(cohort.clone());
        let clamped = state
            .kappa_for(cohort)
            .clamp(self.config.min_kappa, self.config.max_kappa);
        if let Some(cohort_state) = state.cohorts.get_mut(cohort) {
            cohort_state.kappa = clamped;
        }
        gauge!(
            "ad_budget_kappa",
            clamped,
            "campaign" => campaign_id,
            "cohort_hash" => state.cohort_hash(cohort)
        );
        let guidance = BidShadingGuidance {
            kappa: clamped,
            shadow_price: state.dual_price,
            dual_price: state.dual_price,
        };
        gauge!(
            "ad_budget_shading_multiplier",
            guidance.scaling_factor(),
            "campaign" => campaign_id,
            "cohort_hash" => state.cohort_hash(cohort)
        );
        guidance
    }

    pub(crate) fn record_reservation(&mut self, campaign_id: &str, cohort: &CohortKey, spend: u64) {
        if let Some(state) = self.campaigns.get_mut(campaign_id) {
            state.record_spend(cohort, spend, &self.config);
        }
    }

    fn ensure_campaign(&mut self, campaign_id: &str) {
        if !self.campaigns.contains_key(campaign_id) {
            self.register_campaign(campaign_id, 0);
        }
    }
}

#[derive(Clone, Debug)]
struct CampaignBudgetState {
    campaign_id: String,
    total_budget: u64,
    remaining_budget: u64,
    epoch_target: f64,
    epoch_spend: f64,
    epoch_impressions: u64,
    dual_price: f64,
    cohorts: HashMap<CohortKey, CohortBudgetState>,
}

impl CampaignBudgetState {
    fn new(campaign_id: String, budget_usd_micros: u64, config: BudgetBrokerConfig) -> Self {
        let epoch_target = compute_epoch_target(budget_usd_micros, config.epochs_per_budget);
        Self {
            campaign_id,
            total_budget: budget_usd_micros,
            remaining_budget: budget_usd_micros,
            epoch_target,
            epoch_spend: 0.0,
            epoch_impressions: 0,
            dual_price: 0.0,
            cohorts: HashMap::new(),
        }
    }

    fn set_budget(&mut self, budget_usd_micros: u64, config: &BudgetBrokerConfig) {
        self.total_budget = budget_usd_micros;
        self.remaining_budget = budget_usd_micros;
        self.epoch_target = compute_epoch_target(budget_usd_micros, config.epochs_per_budget);
        self.epoch_spend = 0.0;
        self.epoch_impressions = 0;
        self.dual_price = 0.0;
        for cohort in self.cohorts.values_mut() {
            cohort.reset_epoch();
        }
    }

    fn ensure_cohort(&mut self, cohort: CohortKey) {
        self.cohorts
            .entry(cohort)
            .or_insert_with(CohortBudgetState::new);
    }

    fn kappa_for(&self, cohort: &CohortKey) -> f64 {
        self.cohorts
            .get(cohort)
            .map(|state| state.kappa)
            .unwrap_or(1.0)
    }

    fn cohort_hash(&self, cohort: &CohortKey) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.campaign_id.as_bytes());
        hasher.update(cohort.domain.as_bytes());
        if let Some(provider) = &cohort.provider {
            hasher.update(provider.as_bytes());
        }
        for badge in &cohort.badges {
            hasher.update(badge.as_bytes());
        }
        hasher.finalize().to_hex().to_hex_string()
    }

    fn record_spend(&mut self, cohort: &CohortKey, spend: u64, config: &BudgetBrokerConfig) {
        let spend = spend as f64;
        if spend <= 0.0 {
            return;
        }
        if self.remaining_budget > 0 {
            self.remaining_budget = self.remaining_budget.saturating_sub(spend as u64);
        }
        if self.total_budget > 0 {
            let spent = self.total_budget.saturating_sub(self.remaining_budget);
            let progress = spent as f64 / self.total_budget as f64;
            gauge!(
                "ad_budget_progress",
                progress,
                "campaign" => self.campaign_id.as_str()
            );
        }
        self.epoch_spend += spend;
        self.epoch_impressions = self.epoch_impressions.saturating_add(1);
        let active_cohorts = self.cohorts.len().max(1) as f64;
        let target_per_cohort = (self.epoch_target / active_cohorts).max(1.0);
        if let Some(state) = self.cohorts.get_mut(cohort) {
            state.record_spend(spend, target_per_cohort, config);
        }
        let total_error = (self.epoch_spend - self.epoch_target) / self.epoch_target.max(1.0);
        let forgetting = config.dual_forgetting.clamp(0.0, 1.0);
        let updated = self.dual_price * forgetting + config.dual_step * total_error;
        self.dual_price = updated.clamp(0.0, config.shadow_price_cap);
        gauge!(
            "ad_budget_dual_price",
            self.dual_price,
            "campaign" => self.campaign_id.as_str()
        );
        gauge!(
            "ad_budget_shadow_price",
            self.dual_price,
            "campaign" => self.campaign_id.as_str()
        );
        if self.dual_price + f64::EPSILON >= config.shadow_price_cap {
            increment_counter!(
                "ad_budget_shadow_price_spike_total",
                "campaign" => self.campaign_id.as_str()
            );
        }
        if let Some(state) = self.cohorts.get_mut(cohort) {
            state.apply_primal_dual(self.dual_price, config);
        }
        if self.epoch_impressions >= config.epoch_impressions
            || self.epoch_spend >= self.epoch_target
        {
            self.reset_epoch(config);
        }
    }

    fn reset_epoch(&mut self, config: &BudgetBrokerConfig) {
        self.epoch_impressions = 0;
        self.epoch_spend = 0.0;
        self.epoch_target = compute_epoch_target(self.remaining_budget, config.epochs_per_budget);
        for cohort in self.cohorts.values_mut() {
            cohort.reset_epoch();
        }
    }

    fn reconcile_budget(&mut self, budget_usd_micros: u64, config: &BudgetBrokerConfig) {
        if self.total_budget != budget_usd_micros {
            self.total_budget = budget_usd_micros;
        }
        if self.remaining_budget > budget_usd_micros {
            self.remaining_budget = budget_usd_micros;
        }
        self.epoch_target = compute_epoch_target(self.total_budget, config.epochs_per_budget);
    }

    fn snapshot(&self) -> CampaignBudgetSnapshot {
        CampaignBudgetSnapshot {
            campaign_id: self.campaign_id.clone(),
            total_budget: self.total_budget,
            remaining_budget: self.remaining_budget,
            epoch_target: self.epoch_target,
            epoch_spend: self.epoch_spend,
            epoch_impressions: self.epoch_impressions,
            dual_price: self.dual_price,
            cohorts: self
                .cohorts
                .iter()
                .map(|(key, state)| CohortBudgetSnapshot {
                    cohort: CohortKeySnapshot::from_key(key),
                    kappa: state.kappa,
                    smoothed_error: state.smoothed_error,
                    realized_spend: state.realized_spend,
                })
                .collect(),
        }
    }

    fn from_snapshot(snapshot: &CampaignBudgetSnapshot, config: &BudgetBrokerConfig) -> Self {
        let mut cohorts = HashMap::new();
        for entry in &snapshot.cohorts {
            cohorts.insert(
                entry.cohort.clone().into_key(),
                CohortBudgetState {
                    kappa: entry.kappa.clamp(0.0, config.max_kappa),
                    smoothed_error: entry.smoothed_error,
                    realized_spend: entry.realized_spend.max(0.0),
                },
            );
        }
        Self {
            campaign_id: snapshot.campaign_id.clone(),
            total_budget: snapshot.total_budget,
            remaining_budget: snapshot.remaining_budget.min(snapshot.total_budget),
            epoch_target: snapshot.epoch_target.max(1.0),
            epoch_spend: snapshot.epoch_spend.max(0.0),
            epoch_impressions: snapshot.epoch_impressions,
            dual_price: snapshot
                .dual_price
                .clamp(0.0, config.shadow_price_cap.max(config.max_kappa)),
            cohorts,
        }
    }
}

#[derive(Clone, Debug)]
struct CohortBudgetState {
    kappa: f64,
    smoothed_error: f64,
    realized_spend: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BudgetBrokerSnapshot {
    pub generated_at_micros: u64,
    pub config: BudgetBrokerConfig,
    pub campaigns: Vec<CampaignBudgetSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BudgetBrokerAnalytics {
    pub campaign_count: u64,
    pub cohort_count: u64,
    pub mean_kappa: f64,
    pub min_kappa: f64,
    pub max_kappa: f64,
    pub mean_smoothed_error: f64,
    pub max_abs_smoothed_error: f64,
    pub realized_spend_total: f64,
    pub epoch_target_total: f64,
    pub epoch_spend_total: f64,
    pub dual_price_max: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BudgetBrokerPacingDelta {
    pub generated_at_micros: u64,
    pub campaign_count_delta: i64,
    pub cohort_count_delta: i64,
    pub mean_kappa_delta: f64,
    pub max_kappa_delta: f64,
    pub mean_smoothed_error_delta: f64,
    pub max_abs_smoothed_error_delta: f64,
    pub realized_spend_total_delta: f64,
    pub epoch_target_total_delta: f64,
    pub epoch_spend_total_delta: f64,
    pub dual_price_max_delta: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignBudgetSnapshot {
    pub campaign_id: String,
    pub total_budget: u64,
    pub remaining_budget: u64,
    pub epoch_target: f64,
    pub epoch_spend: f64,
    pub epoch_impressions: u64,
    pub dual_price: f64,
    pub cohorts: Vec<CohortBudgetSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CohortBudgetSnapshot {
    pub cohort: CohortKeySnapshot,
    pub kappa: f64,
    pub smoothed_error: f64,
    pub realized_spend: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CohortKeySnapshot {
    pub domain: String,
    pub provider: Option<String>,
    pub badges: Vec<String>,
}

impl CohortBudgetState {
    fn new() -> Self {
        Self {
            kappa: 1.0,
            smoothed_error: 0.0,
            realized_spend: 0.0,
        }
    }

    fn record_spend(&mut self, spend: f64, target: f64, config: &BudgetBrokerConfig) {
        self.realized_spend += spend;
        let error = (self.realized_spend - target) / target.max(1.0);
        self.smoothed_error = config
            .smoothing
            .mul_add(error, (1.0 - config.smoothing) * self.smoothed_error);
        histogram!(
            "ad_budget_epoch_error",
            self.smoothed_error,
            "target_usd" => format!("{target:.0}")
        );
    }

    fn apply_primal_dual(&mut self, shadow_price: f64, config: &BudgetBrokerConfig) {
        let gradient = shadow_price + self.smoothed_error;
        if !gradient.is_finite() {
            return;
        }
        let updated =
            (self.kappa - config.step_size * gradient).clamp(config.min_kappa, config.max_kappa);
        self.kappa = updated;
        gauge!("ad_budget_kappa_adjustment", self.kappa);
        gauge!("ad_budget_kappa_gradient", gradient);
    }

    fn reset_epoch(&mut self) {
        self.realized_spend = 0.0;
        self.smoothed_error = 0.0;
    }
}

pub fn compute_budget_analytics(snapshot: &BudgetBrokerSnapshot) -> BudgetBrokerAnalytics {
    let mut cohort_count: u64 = 0;
    let mut kappa_total = 0.0f64;
    let mut error_total = 0.0f64;
    let mut kappa_min = f64::INFINITY;
    let mut kappa_max = 0.0f64;
    let mut error_max = 0.0f64;
    let mut realized_total = 0.0f64;
    let mut epoch_target_total = 0.0f64;
    let mut epoch_spend_total = 0.0f64;
    let mut dual_price_max = 0.0f64;
    for campaign in &snapshot.campaigns {
        epoch_target_total += campaign.epoch_target;
        epoch_spend_total += campaign.epoch_spend;
        dual_price_max = dual_price_max.max(campaign.dual_price);
        for cohort in &campaign.cohorts {
            cohort_count += 1;
            kappa_total += cohort.kappa;
            error_total += cohort.smoothed_error;
            kappa_min = kappa_min.min(cohort.kappa);
            kappa_max = kappa_max.max(cohort.kappa);
            error_max = error_max.max(cohort.smoothed_error.abs());
            realized_total += cohort.realized_spend;
        }
    }
    let campaign_count = snapshot.campaigns.len() as u64;
    let cohort_count_f = cohort_count.max(1) as f64;
    let mean_kappa = kappa_total / cohort_count_f;
    let mean_smoothed_error = error_total / cohort_count_f;
    BudgetBrokerAnalytics {
        campaign_count,
        cohort_count,
        mean_kappa,
        min_kappa: if kappa_min.is_finite() {
            kappa_min
        } else {
            0.0
        },
        max_kappa: kappa_max,
        mean_smoothed_error,
        max_abs_smoothed_error: error_max,
        realized_spend_total: realized_total,
        epoch_target_total,
        epoch_spend_total,
        dual_price_max,
    }
}

pub fn merge_budget_snapshots(
    base: &BudgetBrokerSnapshot,
    update: &BudgetBrokerSnapshot,
) -> BudgetBrokerSnapshot {
    let mut merged: HashMap<String, CampaignBudgetSnapshot> = base
        .campaigns
        .iter()
        .cloned()
        .map(|campaign| (campaign.campaign_id.clone(), campaign))
        .collect();
    for campaign in &update.campaigns {
        merged.insert(campaign.campaign_id.clone(), campaign.clone());
    }
    let mut campaigns: Vec<_> = merged.into_values().collect();
    campaigns.sort_by(|a, b| a.campaign_id.cmp(&b.campaign_id));
    let config = if update.campaigns.is_empty() {
        base.config.clone()
    } else {
        update.config.clone()
    };
    BudgetBrokerSnapshot {
        generated_at_micros: update.generated_at_micros.max(base.generated_at_micros),
        config,
        campaigns,
    }
}

pub fn budget_snapshot_pacing_delta(
    previous: &BudgetBrokerSnapshot,
    current: &BudgetBrokerSnapshot,
) -> BudgetBrokerPacingDelta {
    let prev = compute_budget_analytics(previous);
    let curr = compute_budget_analytics(current);
    BudgetBrokerPacingDelta {
        generated_at_micros: current.generated_at_micros,
        campaign_count_delta: curr.campaign_count as i64 - prev.campaign_count as i64,
        cohort_count_delta: curr.cohort_count as i64 - prev.cohort_count as i64,
        mean_kappa_delta: curr.mean_kappa - prev.mean_kappa,
        max_kappa_delta: curr.max_kappa - prev.max_kappa,
        mean_smoothed_error_delta: curr.mean_smoothed_error - prev.mean_smoothed_error,
        max_abs_smoothed_error_delta: curr.max_abs_smoothed_error - prev.max_abs_smoothed_error,
        realized_spend_total_delta: curr.realized_spend_total - prev.realized_spend_total,
        epoch_target_total_delta: curr.epoch_target_total - prev.epoch_target_total,
        epoch_spend_total_delta: curr.epoch_spend_total - prev.epoch_spend_total,
        dual_price_max_delta: curr.dual_price_max - prev.dual_price_max,
    }
}

impl CohortKeySnapshot {
    pub(crate) fn from_key(key: &CohortKey) -> Self {
        Self {
            domain: key.domain.clone(),
            provider: key.provider.clone(),
            badges: key.badges.clone(),
        }
    }

    pub(crate) fn into_key(mut self) -> CohortKey {
        self.badges.sort();
        self.badges.dedup();
        CohortKey::new(self.domain, self.provider, self.badges)
    }
}

fn compute_epoch_target(budget: u64, epochs_per_budget: u64) -> f64 {
    if epochs_per_budget == 0 {
        return budget as f64;
    }
    (budget as f64 / epochs_per_budget as f64).max(1.0)
}

fn now_micros() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration
            .as_secs()
            .saturating_mul(1_000_000)
            .saturating_add(duration.subsec_micros() as u64),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestMetricsRecorder;

    fn pacing_cohort(domain: &str, kappa: f64, error: f64, realized: f64) -> CohortBudgetSnapshot {
        CohortBudgetSnapshot {
            cohort: CohortKeySnapshot {
                domain: domain.to_string(),
                provider: Some("wallet".into()),
                badges: vec!["badge".into()],
            },
            kappa,
            smoothed_error: error,
            realized_spend: realized,
        }
    }

    fn pacing_campaign(
        campaign_id: &str,
        epoch_target: f64,
        epoch_spend: f64,
        dual_price: f64,
        cohort: CohortBudgetSnapshot,
    ) -> CampaignBudgetSnapshot {
        CampaignBudgetSnapshot {
            campaign_id: campaign_id.into(),
            total_budget: 5_000_000,
            remaining_budget: 4_000_000,
            epoch_target,
            epoch_spend,
            epoch_impressions: 50,
            dual_price,
            cohorts: vec![cohort],
        }
    }

    fn sample_config() -> BudgetBrokerConfig {
        BudgetBrokerConfig {
            epoch_impressions: 8,
            step_size: 0.1,
            dual_step: 0.05,
            dual_forgetting: 0.75,
            max_kappa: 3.0,
            min_kappa: 0.2,
            shadow_price_cap: 1.0,
            smoothing: 0.25,
            epochs_per_budget: 16,
        }
    }

    fn sample_cohort() -> CohortKey {
        CohortKey::new(
            "example.com".into(),
            Some("wallet".into()),
            vec!["badge-1".into(), "badge-2".into()],
        )
    }

    #[test]
    fn snapshot_round_trips_budget_state() {
        let config = sample_config();
        let cohort = sample_cohort();
        let mut broker = BudgetBroker::new(config.clone());
        broker.ensure_registered("campaign-a", 2_000_000);
        broker.record_reservation("campaign-a", &cohort, 500_000);

        let snapshot = broker.snapshot();
        assert_eq!(snapshot.campaigns.len(), 1);
        let original = snapshot.campaigns.first().expect("campaign snapshot");
        assert!(original.remaining_budget < original.total_budget);

        let mut restored = BudgetBroker::restore(config.clone(), &snapshot);
        let guidance = restored.guidance_for("campaign-a", &cohort);
        assert!(guidance.kappa >= 0.0);
        assert!(guidance.scaling_factor() >= 0.0);
        restored.record_reservation("campaign-a", &cohort, 100_000);
        let restored_snapshot = restored.snapshot();
        let restored_campaign = restored_snapshot
            .campaigns
            .first()
            .expect("restored campaign snapshot");
        assert_eq!(restored_campaign.total_budget, original.total_budget);
        assert_eq!(
            restored_campaign.remaining_budget,
            original.remaining_budget.saturating_sub(100_000)
        );
        assert_eq!(
            restored_campaign
                .cohorts
                .first()
                .map(|c| c.cohort.domain.clone()),
            Some(cohort.domain.clone())
        );
    }

    #[test]
    fn budget_metrics_recorded_under_load() {
        let Some(recorder) = TestMetricsRecorder::install() else {
            eprintln!("skipping metrics assertion; recorder already installed elsewhere");
            return;
        };
        recorder.reset();
        let mut broker = BudgetBroker::new(sample_config());
        let cohort = sample_cohort();
        broker.ensure_registered("cmp-load", 5_000_000);
        let guidance = broker.guidance_for("cmp-load", &cohort);
        assert!(guidance.scaling_factor() >= 0.0);
        for _ in 0..16 {
            broker.record_reservation("cmp-load", &cohort, 250_000);
        }
        let histograms = recorder.histograms();
        assert!(histograms
            .iter()
            .any(|event| event.name == "ad_budget_epoch_error"));
        let gauges = recorder.gauges();
        assert!(gauges
            .iter()
            .any(|event| event.name == "ad_budget_progress"));
        assert!(gauges
            .iter()
            .any(|event| event.name == "ad_budget_shadow_price"));
        assert!(gauges
            .iter()
            .any(|event| event.name == "ad_budget_kappa_gradient"));
    }

    #[test]
    fn merge_budget_snapshots_preserves_previous_entries() {
        let config = BudgetBrokerConfig::default();
        let base = BudgetBrokerSnapshot {
            generated_at_micros: 10,
            config: config.clone(),
            campaigns: vec![
                pacing_campaign(
                    "cmp-a",
                    210_000.0,
                    150_000.0,
                    0.4,
                    pacing_cohort("example.com", 0.8, 0.1, 120_000.0),
                ),
                pacing_campaign(
                    "cmp-b",
                    160_000.0,
                    90_000.0,
                    0.55,
                    pacing_cohort("news.example", 0.6, 0.05, 80_000.0),
                ),
            ],
        };
        let update = BudgetBrokerSnapshot {
            generated_at_micros: 20,
            config: config.clone(),
            campaigns: vec![pacing_campaign(
                "cmp-a",
                210_000.0,
                190_000.0,
                0.75,
                pacing_cohort("example.com", 0.9, 0.08, 150_000.0),
            )],
        };

        let merged = merge_budget_snapshots(&base, &update);
        assert_eq!(merged.campaigns.len(), 2);
        assert!(merged
            .campaigns
            .iter()
            .any(|campaign| campaign.campaign_id == "cmp-b"));
        let updated = merged
            .campaigns
            .iter()
            .find(|campaign| campaign.campaign_id == "cmp-a")
            .expect("cmp-a present");
        assert!((updated.epoch_spend - 190_000.0).abs() < f64::EPSILON);
        assert!((updated.dual_price - 0.75).abs() < f64::EPSILON);
        assert_eq!(merged.generated_at_micros, 20);
    }

    #[test]
    fn pacing_delta_tracks_partial_updates() {
        let config = BudgetBrokerConfig::default();
        let base = BudgetBrokerSnapshot {
            generated_at_micros: 10,
            config: config.clone(),
            campaigns: vec![
                pacing_campaign(
                    "cmp-a",
                    210_000.0,
                    150_000.0,
                    0.4,
                    pacing_cohort("example.com", 0.8, 0.1, 120_000.0),
                ),
                pacing_campaign(
                    "cmp-b",
                    160_000.0,
                    90_000.0,
                    0.55,
                    pacing_cohort("news.example", 0.6, 0.05, 80_000.0),
                ),
            ],
        };
        let update = BudgetBrokerSnapshot {
            generated_at_micros: 20,
            config: config.clone(),
            campaigns: vec![pacing_campaign(
                "cmp-a",
                210_000.0,
                190_000.0,
                0.75,
                pacing_cohort("example.com", 0.9, 0.08, 150_000.0),
            )],
        };
        let merged = merge_budget_snapshots(&base, &update);
        let delta = budget_snapshot_pacing_delta(&base, &merged);
        assert_eq!(delta.campaign_count_delta, 0);
        assert_eq!(delta.cohort_count_delta, 0);
        assert!((delta.mean_kappa_delta - 0.05).abs() < 1e-9);
        assert!((delta.max_kappa_delta - 0.1).abs() < 1e-9);
        assert!((delta.mean_smoothed_error_delta + 0.01).abs() < 1e-9);
        assert!((delta.max_abs_smoothed_error_delta + 0.02).abs() < 1e-9);
        assert!((delta.realized_spend_total_delta - 30_000.0).abs() < 1e-6);
        assert!((delta.epoch_spend_total_delta - 40_000.0).abs() < 1e-6);
        assert!((delta.epoch_target_total_delta - 0.0).abs() < 1e-6);
        assert!((delta.dual_price_max_delta - 0.20).abs() < 1e-9);
        assert_eq!(delta.generated_at_micros, 20);
    }
}
