pub mod ann;

use ann::{SoftIntentReceipt, WalletAnnIndexSnapshot};
use foundation_metrics::{gauge, increment_counter};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct BadgeGuardConfig {
    pub k_min: u64,
    pub forgetting: f64,
    pub soft_intent_required: bool,
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
            soft_intent_required: false,
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
        let mut key = badges.to_vec();
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

    pub fn evaluate(
        &self,
        badges: &[String],
        soft_intent: Option<&BadgeSoftIntentContext>,
    ) -> BadgeDecision {
        if badges.is_empty() {
            return BadgeDecision::Allowed {
                required: Vec::new(),
                proof: None,
            };
        }
        let mut relaxed = badges.to_vec();
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
                    let proof =
                        build_soft_intent(self.config.soft_intent_required, soft_intent, &relaxed);
                    if self.config.soft_intent_required && proof.is_none() {
                        increment_counter!("ad_badge_soft_intent_missing_total");
                        break;
                    }
                    return BadgeDecision::Allowed {
                        required: relaxed,
                        proof,
                    };
                }
            }
            relaxed.pop();
            dropped += 1;
        }
        increment_counter!("ad_badge_relax_block_total");
        BadgeDecision::Blocked
    }
}

#[derive(Debug, Clone)]
pub enum BadgeDecision {
    Allowed {
        required: Vec<String>,
        proof: Option<SoftIntentReceipt>,
    },
    Blocked,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BadgeSoftIntentContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallet_index: Option<WalletAnnIndexSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<SoftIntentReceipt>,
}

#[derive(Clone, Debug)]
struct BadgePopulationState {
    population: f64,
}

fn build_soft_intent(
    required: bool,
    context: Option<&BadgeSoftIntentContext>,
    badges: &[String],
) -> Option<SoftIntentReceipt> {
    let ctx = context?;
    let snapshot = match ctx.wallet_index.as_ref() {
        Some(snapshot) => snapshot,
        None => return if required { None } else { ctx.proof.clone() },
    };
    if let Some(receipt) = ctx.proof.as_ref() {
        if ann::verify_receipt(snapshot, receipt, badges) {
            return Some(receipt.clone());
        }
        return None;
    }
    ann::build_proof(snapshot, badges)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot() -> WalletAnnIndexSnapshot {
        let badges = vec!["badge.alpha".to_string(), "badge.beta".to_string()];
        let query = ann::hash_badges(&badges);
        WalletAnnIndexSnapshot::new([0xAB; 32], vec![query, [0x11; 32], [0x22; 32]], 16)
    }

    #[test]
    fn allows_without_soft_intent_requirement() {
        let guard = BadgeGuard::new(BadgeGuardConfig {
            k_min: 1,
            forgetting: 0.5,
            soft_intent_required: false,
        });
        let badges = vec!["badge.alpha".to_string()];
        guard.record(&badges, Some(10));
        match guard.evaluate(&badges, None) {
            BadgeDecision::Allowed { proof, .. } => assert!(proof.is_none()),
            BadgeDecision::Blocked => panic!("expected guard to allow without soft intent"),
        }
    }

    #[test]
    fn blocks_when_soft_intent_missing() {
        let guard = BadgeGuard::new(BadgeGuardConfig {
            k_min: 1,
            forgetting: 0.5,
            soft_intent_required: true,
        });
        let badges = vec!["badge.alpha".to_string()];
        guard.record(&badges, Some(10));
        assert!(matches!(
            guard.evaluate(&badges, None),
            BadgeDecision::Blocked
        ));
    }

    #[test]
    fn accepts_valid_soft_intent_proof() {
        let guard = BadgeGuard::new(BadgeGuardConfig {
            k_min: 1,
            forgetting: 0.2,
            soft_intent_required: true,
        });
        let badges = vec!["badge.alpha".to_string(), "badge.beta".to_string()];
        let snapshot = sample_snapshot();
        guard.record(&badges, Some(10));
        let proof = ann::build_proof(&snapshot, &badges).expect("proof");
        let decision = guard.evaluate(
            &badges,
            Some(&BadgeSoftIntentContext {
                wallet_index: Some(snapshot.clone()),
                proof: Some(proof.clone()),
            }),
        );
        match decision {
            BadgeDecision::Allowed {
                proof: Some(result),
                ..
            } => assert_eq!(result, proof),
            other => panic!("unexpected decision: {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_soft_intent_proof() {
        let guard = BadgeGuard::new(BadgeGuardConfig {
            k_min: 1,
            forgetting: 0.2,
            soft_intent_required: true,
        });
        let badges = vec!["badge.alpha".to_string(), "badge.beta".to_string()];
        let snapshot = sample_snapshot();
        guard.record(&badges, Some(10));
        let mut proof = ann::build_proof(&snapshot, &badges).expect("proof");
        proof.proof.ciphertext[0] ^= 0xFF;
        let decision = guard.evaluate(
            &badges,
            Some(&BadgeSoftIntentContext {
                wallet_index: Some(snapshot),
                proof: Some(proof),
            }),
        );
        assert!(matches!(decision, BadgeDecision::Blocked));
    }
}
