use crypto_suite::hashing::blake3;
use foundation_metrics::{gauge, histogram};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::MICROS_PER_DOLLAR;

const PPM_SCALE_F64: f64 = 1_000_000.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpliftEstimatorConfig {
    pub treatment_prior: f64,
    pub control_prior: f64,
    pub propensity_prior: f64,
    pub holdout_fraction_ppm: u32,
    pub min_impressions: u64,
}

impl UpliftEstimatorConfig {
    pub fn normalized(mut self) -> Self {
        self.treatment_prior = self.treatment_prior.clamp(1e-6, 10.0);
        self.control_prior = self.control_prior.clamp(1e-6, 10.0);
        self.propensity_prior = self.propensity_prior.clamp(1e-6, 10.0);
        self.holdout_fraction_ppm = self.holdout_fraction_ppm.min(1_000_000);
        self.min_impressions = self.min_impressions.max(1);
        self
    }
}

impl Default for UpliftEstimatorConfig {
    fn default() -> Self {
        Self {
            treatment_prior: 2.0,
            control_prior: 2.0,
            propensity_prior: 1.0,
            holdout_fraction_ppm: 50_000,
            min_impressions: 500,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct UpliftSnapshot {
    pub generated_at_micros: u64,
    pub creatives: Vec<UpliftCreativeSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpliftCreativeSnapshot {
    pub key: String,
    pub treatment_count: u64,
    pub treatment_success: u64,
    pub control_count: u64,
    pub control_success: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folds: Vec<UpliftFoldSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct UpliftFoldSnapshot {
    pub fold_index: u8,
    pub treatment_count: u64,
    pub treatment_success: u64,
    pub control_count: u64,
    pub control_success: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
struct UpliftBucket {
    treatment_count: u64,
    treatment_success: u64,
    control_count: u64,
    control_success: u64,
}

impl UpliftBucket {
    fn impressions(&self) -> u64 {
        self.treatment_count + self.control_count
    }
}

#[derive(Clone, Debug)]
pub struct UpliftEstimator {
    config: UpliftEstimatorConfig,
    buckets: HashMap<String, [UpliftBucket; 2]>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct UpliftEstimate {
    pub lift_ppm: u32,
    pub baseline_action_rate_ppm: u32,
    pub propensity: f64,
    pub ece: f64,
    pub sample_size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpliftHoldoutAssignment {
    pub fold: u8,
    pub in_holdout: bool,
    pub propensity: f64,
}

impl UpliftEstimator {
    pub fn new(config: UpliftEstimatorConfig) -> Self {
        Self {
            config: config.normalized(),
            buckets: HashMap::new(),
        }
    }

    pub fn from_snapshot(config: UpliftEstimatorConfig, snapshot: Option<UpliftSnapshot>) -> Self {
        let mut estimator = Self::new(config);
        if let Some(snapshot) = snapshot {
            estimator.restore(&snapshot);
        }
        estimator
    }

    pub fn config(&self) -> &UpliftEstimatorConfig {
        &self.config
    }

    pub fn assign_holdout(
        &self,
        campaign_id: &str,
        creative_id: &str,
        impression_seed: &[u8],
    ) -> UpliftHoldoutAssignment {
        let mut hasher = blake3::Hasher::new();
        hasher.update(campaign_id.as_bytes());
        hasher.update(creative_id.as_bytes());
        hasher.update(impression_seed);
        let digest = hasher.finalize();
        let bytes = digest.as_bytes();
        let fold = bytes[0] & 0x01;
        let percentile = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) % 1_000_000;
        let holdout_fraction = self.config.holdout_fraction_ppm as f64 / PPM_SCALE_F64;
        let in_holdout = percentile < self.config.holdout_fraction_ppm;
        let propensity = if in_holdout {
            holdout_fraction.max(1e-6)
        } else {
            (1.0 - holdout_fraction).max(1e-6)
        };
        UpliftHoldoutAssignment {
            fold,
            in_holdout,
            propensity,
        }
    }

    pub fn record_observation(
        &mut self,
        campaign_id: &str,
        creative_id: &str,
        assignment: &UpliftHoldoutAssignment,
        converted: bool,
    ) {
        let key = key_for(campaign_id, creative_id);
        let buckets = self
            .buckets
            .entry(key.clone())
            .or_insert_with(|| [UpliftBucket::default(), UpliftBucket::default()]);
        let bucket = &mut buckets[assignment.fold as usize % 2];
        if assignment.in_holdout {
            bucket.control_count = bucket.control_count.saturating_add(1);
            if converted {
                bucket.control_success = bucket.control_success.saturating_add(1);
            }
        } else {
            bucket.treatment_count = bucket.treatment_count.saturating_add(1);
            if converted {
                bucket.treatment_success = bucket.treatment_success.saturating_add(1);
            }
        }
    }

    pub fn estimate(&self, campaign_id: &str, creative_id: &str) -> UpliftEstimate {
        let key = key_for(campaign_id, creative_id);
        let eval_fold = self.eval_fold(&key);
        let bucket = self
            .buckets
            .get(&key)
            .map(|folds| folds[eval_fold])
            .unwrap_or_default();
        self.estimate_from_bucket(bucket)
    }

    pub fn snapshot(&self) -> UpliftSnapshot {
        let creatives = self
            .buckets
            .iter()
            .map(|(key, folds)| {
                let per_fold = vec![
                    UpliftFoldSnapshot {
                        fold_index: 0,
                        treatment_count: folds[0].treatment_count,
                        treatment_success: folds[0].treatment_success,
                        control_count: folds[0].control_count,
                        control_success: folds[0].control_success,
                    },
                    UpliftFoldSnapshot {
                        fold_index: 1,
                        treatment_count: folds[1].treatment_count,
                        treatment_success: folds[1].treatment_success,
                        control_count: folds[1].control_count,
                        control_success: folds[1].control_success,
                    },
                ];
                let combined = UpliftBucket {
                    treatment_count: folds[0].treatment_count + folds[1].treatment_count,
                    treatment_success: folds[0].treatment_success + folds[1].treatment_success,
                    control_count: folds[0].control_count + folds[1].control_count,
                    control_success: folds[0].control_success + folds[1].control_success,
                };
                UpliftCreativeSnapshot {
                    key: key.clone(),
                    treatment_count: combined.treatment_count,
                    treatment_success: combined.treatment_success,
                    control_count: combined.control_count,
                    control_success: combined.control_success,
                    folds: per_fold,
                }
            })
            .collect();
        UpliftSnapshot {
            generated_at_micros: now_micros(),
            creatives,
        }
    }

    pub fn restore(&mut self, snapshot: &UpliftSnapshot) {
        self.buckets.clear();
        for creative in &snapshot.creatives {
            let mut folds = [UpliftBucket::default(), UpliftBucket::default()];
            if creative.folds.is_empty() {
                folds[0] = UpliftBucket {
                    treatment_count: creative.treatment_count,
                    treatment_success: creative.treatment_success,
                    control_count: creative.control_count,
                    control_success: creative.control_success,
                };
            } else {
                for fold in &creative.folds {
                    let idx = (fold.fold_index as usize) % 2;
                    folds[idx] = UpliftBucket {
                        treatment_count: fold.treatment_count,
                        treatment_success: fold.treatment_success,
                        control_count: fold.control_count,
                        control_success: fold.control_success,
                    };
                }
            }
            self.buckets.insert(creative.key.clone(), folds);
        }
    }

    fn eval_fold(&self, key: &str) -> usize {
        let digest = blake3::hash(key.as_bytes());
        ((digest.as_bytes()[0] & 0x01) ^ 0x01) as usize
    }

    fn estimate_from_bucket(&self, bucket: UpliftBucket) -> UpliftEstimate {
        let impressions = bucket.impressions();
        let treatment_rate = rate_with_prior(
            bucket.treatment_success,
            bucket.treatment_count,
            self.config.treatment_prior,
        );
        let control_rate = rate_with_prior(
            bucket.control_success,
            bucket.control_count,
            self.config.control_prior,
        );
        let lift = (treatment_rate - control_rate).clamp(-1.0, 1.0);
        let lift_ppm = ((lift * PPM_SCALE_F64).round()).clamp(-PPM_SCALE_F64, PPM_SCALE_F64) as i64;
        let propensity = rate_with_prior(
            bucket.treatment_count,
            bucket.impressions(),
            self.config.propensity_prior,
        )
        .clamp(1e-6, 1.0);
        let ece = if impressions < self.config.min_impressions {
            1.0
        } else {
            (lift.abs() * PPM_SCALE_F64 / MICROS_PER_DOLLAR as f64).min(1.0)
        };
        gauge!("ad_uplift_propensity", propensity, "sample" => impressions as i64);
        histogram!(
            "ad_uplift_lift_ppm",
            lift_ppm as f64,
            "impressions" => impressions as i64
        );
        UpliftEstimate {
            lift_ppm: lift_ppm.max(0) as u32,
            baseline_action_rate_ppm: (control_rate * PPM_SCALE_F64).round().max(0.0) as u32,
            propensity,
            ece,
            sample_size: impressions,
        }
    }
}

fn rate_with_prior(success: u64, count: u64, prior: f64) -> f64 {
    let numerator = success as f64 + prior;
    let denominator = count as f64 + 2.0 * prior;
    if denominator <= f64::EPSILON {
        prior / (2.0 * prior)
    } else {
        (numerator / denominator).clamp(0.0, 1.0)
    }
}

fn key_for(campaign_id: &str, creative_id: &str) -> String {
    format!("{}::{}", campaign_id, creative_id)
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
