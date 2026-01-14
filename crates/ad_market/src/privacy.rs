use foundation_metrics::{gauge, increment_counter};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrivacyBudgetConfig {
    pub max_epsilon: f64,
    pub max_delta: f64,
    pub default_epsilon_cost: f64,
    pub default_delta_cost: f64,
    pub cool_off_impressions: u64,
    pub forgetting: f64,
}

impl PrivacyBudgetConfig {
    pub fn normalized(mut self) -> Self {
        self.max_epsilon = self.max_epsilon.clamp(1e-6, 10.0);
        self.max_delta = self.max_delta.clamp(1e-12, 1e-1);
        self.default_epsilon_cost = self.default_epsilon_cost.clamp(1e-6, self.max_epsilon);
        self.default_delta_cost = self.default_delta_cost.clamp(1e-12, self.max_delta);
        self.cool_off_impressions = self.cool_off_impressions.max(1);
        self.forgetting = self.forgetting.clamp(0.0, 1.0);
        self
    }
}

impl Default for PrivacyBudgetConfig {
    fn default() -> Self {
        Self {
            max_epsilon: 1.0,
            max_delta: 1e-6,
            default_epsilon_cost: 0.01,
            default_delta_cost: 1e-8,
            cool_off_impressions: 10_000,
            forgetting: 0.05,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PrivacyBudgetSnapshot {
    pub generated_at_micros: u64,
    pub families: Vec<PrivacyBudgetFamilySnapshot>,
    pub max_epsilon: f64,
    pub max_delta: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrivacyBudgetFamilySnapshot {
    pub family: String,
    pub epsilon_spent: f64,
    pub delta_spent: f64,
    pub impressions_tracked: u64,
    pub cooldown_remaining: u64,
    #[serde(default)]
    pub accepted_total: u64,
    #[serde(default)]
    pub denied_total: u64,
    #[serde(default)]
    pub cooling_total: u64,
}

#[derive(Clone, Debug)]
pub struct PrivacyBudgetManager {
    config: PrivacyBudgetConfig,
    families: HashMap<String, PrivacyBudgetState>,
}

#[derive(Clone, Debug)]
struct PrivacyBudgetState {
    epsilon_spent: f64,
    delta_spent: f64,
    impressions_tracked: u64,
    cooldown_remaining: u64,
    accepted_total: u64,
    denied_total: u64,
    cooling_total: u64,
}

impl PrivacyBudgetState {
    fn new() -> Self {
        Self {
            epsilon_spent: 0.0,
            delta_spent: 0.0,
            impressions_tracked: 0,
            cooldown_remaining: 0,
            accepted_total: 0,
            denied_total: 0,
            cooling_total: 0,
        }
    }

    fn decay(&mut self, forgetting: f64) {
        if forgetting <= f64::EPSILON {
            return;
        }
        self.epsilon_spent *= 1.0 - forgetting;
        self.delta_spent *= 1.0 - forgetting;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrivacyBudgetDecision {
    Allowed,
    Cooling { family: String },
    Denied { family: String },
}

#[derive(Clone, Debug)]
pub struct PrivacyBudgetPreview {
    pub decision: PrivacyBudgetDecision,
    pub remaining_ppm: u32,
    pub denied_ppm: u32,
    pub cooldown_remaining: u64,
}

impl PrivacyBudgetManager {
    pub fn new(config: PrivacyBudgetConfig) -> Self {
        Self {
            config: config.normalized(),
            families: HashMap::new(),
        }
    }

    pub fn config(&self) -> &PrivacyBudgetConfig {
        &self.config
    }

    pub fn preview(&self, badges: &[String], population_hint: Option<u64>) -> PrivacyBudgetPreview {
        if badges.is_empty() {
            return PrivacyBudgetPreview {
                decision: PrivacyBudgetDecision::Allowed,
                remaining_ppm: 1_000_000,
                denied_ppm: 0,
                cooldown_remaining: 0,
            };
        }
        let families: HashSet<String> = badges.iter().map(|badge| family_for(badge)).collect();
        if families.is_empty() {
            return PrivacyBudgetPreview {
                decision: PrivacyBudgetDecision::Allowed,
                remaining_ppm: 1_000_000,
                denied_ppm: 0,
                cooldown_remaining: 0,
            };
        }
        let (epsilon_cost, delta_cost) = self.estimate_cost(population_hint);
        let mut remaining_ppm = 1_000_000u32;
        let mut denied_ppm = 0u32;
        let mut decision = PrivacyBudgetDecision::Allowed;
        let mut cooldown_remaining = 0u64;

        for family in families {
            let state = self
                .families
                .get(&family)
                .cloned()
                .unwrap_or_else(PrivacyBudgetState::new);
            let epsilon_remaining = if self.config.max_epsilon <= f64::EPSILON {
                0.0
            } else {
                (self.config.max_epsilon - state.epsilon_spent) / self.config.max_epsilon
            };
            let delta_remaining = if self.config.max_delta <= f64::EPSILON {
                0.0
            } else {
                (self.config.max_delta - state.delta_spent) / self.config.max_delta
            };
            let remaining_ratio = epsilon_remaining.min(delta_remaining).clamp(0.0, 1.0);
            remaining_ppm = remaining_ppm.min((remaining_ratio * 1_000_000f64).round() as u32);

            let decisions = state.accepted_total + state.denied_total + state.cooling_total;
            if decisions > 0 {
                let denied_ratio =
                    (state.denied_total + state.cooling_total) as f64 / decisions as f64;
                denied_ppm = denied_ppm.max((denied_ratio * 1_000_000f64).round() as u32);
            }

            if state.cooldown_remaining > 0 {
                decision = PrivacyBudgetDecision::Cooling {
                    family: family.clone(),
                };
                cooldown_remaining = cooldown_remaining.max(state.cooldown_remaining);
                continue;
            }
            let epsilon_next = state.epsilon_spent + epsilon_cost;
            let delta_next = state.delta_spent + delta_cost;
            if epsilon_next > self.config.max_epsilon || delta_next > self.config.max_delta {
                decision = PrivacyBudgetDecision::Denied {
                    family: family.clone(),
                };
            }
        }

        PrivacyBudgetPreview {
            decision,
            remaining_ppm,
            denied_ppm,
            cooldown_remaining,
        }
    }

    pub fn authorize(
        &mut self,
        badges: &[String],
        population_hint: Option<u64>,
    ) -> PrivacyBudgetDecision {
        if badges.is_empty() {
            return PrivacyBudgetDecision::Allowed;
        }
        let families: HashSet<String> = badges.iter().map(|badge| family_for(badge)).collect();
        if families.is_empty() {
            return PrivacyBudgetDecision::Allowed;
        }
        let (epsilon_cost, delta_cost) = self.estimate_cost(population_hint);

        for family in &families {
            let state = self
                .families
                .entry(family.clone())
                .or_insert_with(PrivacyBudgetState::new);
            if state.cooldown_remaining > 0 {
                state.cooldown_remaining = state.cooldown_remaining.saturating_sub(1);
                state.cooling_total = state.cooling_total.saturating_add(1);
                increment_counter!(
                    "ad_privacy_budget_total",
                    "family" => family.as_str(),
                    "result" => "cooling"
                );
                return PrivacyBudgetDecision::Cooling {
                    family: family.clone(),
                };
            }
            let epsilon_next = state.epsilon_spent + epsilon_cost;
            let delta_next = state.delta_spent + delta_cost;
            if epsilon_next > self.config.max_epsilon || delta_next > self.config.max_delta {
                state.cooldown_remaining = self.config.cool_off_impressions;
                state.epsilon_spent = 0.0;
                state.delta_spent = 0.0;
                state.impressions_tracked = 0;
                state.denied_total = state.denied_total.saturating_add(1);
                increment_counter!(
                    "ad_privacy_budget_total",
                    "family" => family.as_str(),
                    "result" => "revoked"
                );
                return PrivacyBudgetDecision::Denied {
                    family: family.clone(),
                };
            }
        }

        for family in families {
            let state = self
                .families
                .entry(family.clone())
                .or_insert_with(PrivacyBudgetState::new);
            state.decay(self.config.forgetting);
            state.epsilon_spent += epsilon_cost;
            state.delta_spent += delta_cost;
            state.impressions_tracked = state.impressions_tracked.saturating_add(1);
            state.accepted_total = state.accepted_total.saturating_add(1);
            gauge!(
                "ad_privacy_budget_remaining",
                (self.config.max_epsilon - state.epsilon_spent).max(0.0),
                "family" => family.as_str(),
                "metric" => "epsilon"
            );
            gauge!(
                "ad_privacy_budget_remaining",
                (self.config.max_delta - state.delta_spent).max(0.0),
                "family" => family.as_str(),
                "metric" => "delta"
            );
            increment_counter!(
                "ad_privacy_budget_total",
                "family" => family.as_str(),
                "result" => "accepted"
            );
        }
        PrivacyBudgetDecision::Allowed
    }

    pub fn snapshot(&self) -> PrivacyBudgetSnapshot {
        let families = self
            .families
            .iter()
            .map(|(family, state)| PrivacyBudgetFamilySnapshot {
                family: family.clone(),
                epsilon_spent: state.epsilon_spent,
                delta_spent: state.delta_spent,
                impressions_tracked: state.impressions_tracked,
                cooldown_remaining: state.cooldown_remaining,
                accepted_total: state.accepted_total,
                denied_total: state.denied_total,
                cooling_total: state.cooling_total,
            })
            .collect();
        PrivacyBudgetSnapshot {
            generated_at_micros: now_micros(),
            families,
            max_epsilon: self.config.max_epsilon,
            max_delta: self.config.max_delta,
        }
    }

    fn estimate_cost(&self, population_hint: Option<u64>) -> (f64, f64) {
        let population = population_hint.unwrap_or(1).max(1) as f64;
        let epsilon = (self.config.default_epsilon_cost / population).max(1e-12);
        let delta = (self.config.default_delta_cost / population).max(1e-12);
        (epsilon, delta)
    }
}

fn family_for(badge: &str) -> String {
    badge
        .split(':')
        .next()
        .map(str::to_lowercase)
        .unwrap_or_else(|| "global".to_string())
}

pub fn badge_family(badge: &str) -> String {
    family_for(badge)
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
