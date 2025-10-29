use foundation_metrics::{gauge, increment_counter};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug)]
pub struct BadgeGuardConfig {
    pub k_min: u64,
    pub forgetting: f64,
}

impl BadgeGuardConfig {
    pub fn normalized(mut self) -> Self {
        self.forgetting = self.forgetting.clamp(0.0, 1.0);
        self
    }
}

impl Default for BadgeGuardConfig {
    fn default() -> Self {
        Self {
            k_min: 500,
            forgetting: 0.2,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BadgeGuard {
    config: BadgeGuardConfig,
    populations: ArcPopulations,
}

type ArcPopulations = Arc<RwLock<HashMap<Vec<String>, BadgePopulationState>>>;

impl BadgeGuard {
    pub fn new(config: BadgeGuardConfig) -> Self {
        Self {
            config: config.normalized(),
            populations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn record(&self, badges: &[String], population_hint: Option<u64>) {
        let mut key: Vec<String> = badges.iter().cloned().collect();
        key.sort();
        let estimate = population_hint
            .unwrap_or(self.config.k_min)
            .max(self.config.k_min) as f64;
        let mut guard = self.populations.write().unwrap();
        let entry = guard
            .entry(key)
            .or_insert_with(|| BadgePopulationState::new(estimate));
        entry.update(estimate, self.config.forgetting);
    }

    pub fn evaluate(&self, badges: &[String]) -> BadgeDecision {
        if badges.is_empty() {
            return BadgeDecision::Allowed(Vec::new());
        }
        let mut relaxed: Vec<String> = badges.iter().cloned().collect();
        relaxed.sort();
        let mut dropped = 0usize;
        let populations = self.populations.read().unwrap();
        while !relaxed.is_empty() {
            if let Some(state) = populations.get(&relaxed) {
                gauge!(
                    "ad_badge_population",
                    state.population,
                    "badge_count" => relaxed.len() as i64,
                    "relaxed" => dropped as i64
                );
                if state.population >= self.config.k_min as f64 {
                    return BadgeDecision::Allowed(relaxed);
                }
            }
            relaxed.pop();
            dropped += 1;
        }
        increment_counter!("ad_badge_relax_block_total");
        BadgeDecision::Blocked
    }
}

#[derive(Debug)]
pub enum BadgeDecision {
    Allowed(Vec<String>),
    Blocked,
}

#[derive(Clone, Debug)]
struct BadgePopulationState {
    population: f64,
}

impl BadgePopulationState {
    fn new(population: f64) -> Self {
        Self { population }
    }

    fn update(&mut self, sample: f64, forgetting: f64) {
        let updated = forgetting.mul_add(sample, (1.0 - forgetting) * self.population);
        self.population = updated.max(sample);
    }
}
