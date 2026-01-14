#![forbid(unsafe_code)]

use crate::ad_readiness::{AdReadinessSnapshot, FreshnessHistogramPpm as ReadinessHistogram};
use ad_market::{
    badge_family, CohortKeySnapshot, FreshnessHistogramPpm as WeightHistogram,
    PrivacyBudgetSnapshot, QualitySignal, QualitySignalComponents, QualitySignalConfig,
};
use std::collections::HashSet;

pub struct QualitySignalReport {
    pub signal: QualitySignal,
    pub readiness_streak_windows: u64,
    pub freshness_score_ppm: Option<(String, u32)>,
    pub privacy_score_ppm: u32,
}

pub fn quality_signal_for_cohort(
    config: &QualitySignalConfig,
    readiness: Option<&AdReadinessSnapshot>,
    privacy: Option<&PrivacyBudgetSnapshot>,
    cohort: &CohortKeySnapshot,
) -> QualitySignalReport {
    let readiness_streak_windows = readiness.map(|snap| snap.ready_streak_windows).unwrap_or(0);
    let (freshness_ppm, freshness_metric) = freshness_component_ppm(config, readiness, cohort);
    let readiness_ppm = if readiness.is_some() {
        readiness_component_ppm(config, readiness_streak_windows)
    } else {
        1_000_000
    };
    let mut privacy_badges = cohort.badges.clone();
    if let Some(bucket) = cohort.presence_bucket.as_ref() {
        privacy_badges.push(format!("presence:{}", bucket.kind.as_str()));
    }
    let (privacy_ppm, privacy_score_ppm) = privacy_component_ppm(config, privacy, &privacy_badges);

    let composite_ppm = composite_multiplier_ppm(config, freshness_ppm, readiness_ppm, privacy_ppm);
    let components = QualitySignalComponents {
        freshness_multiplier_ppm: freshness_ppm,
        readiness_multiplier_ppm: readiness_ppm,
        privacy_multiplier_ppm: privacy_ppm,
    };
    QualitySignalReport {
        signal: QualitySignal {
            cohort: cohort.clone(),
            multiplier_ppm: composite_ppm,
            components,
        },
        readiness_streak_windows,
        freshness_score_ppm: freshness_metric,
        privacy_score_ppm,
    }
}

pub fn freshness_scores_for_snapshot(
    config: &QualitySignalConfig,
    readiness: &AdReadinessSnapshot,
) -> Vec<(String, u32)> {
    let mut scores = Vec::new();
    let Some(segment) = readiness.segment_readiness.as_ref() else {
        return scores;
    };
    for (bucket_id, entry) in &segment.presence_buckets {
        let ppm = weighted_freshness_ppm(&entry.freshness_histogram, &config.freshness_weights_ppm);
        scores.push((bucket_id.clone(), ppm));
    }
    scores
}

pub fn privacy_score_ppm(snapshot: Option<&PrivacyBudgetSnapshot>) -> u32 {
    let Some(snapshot) = snapshot else {
        return 1_000_000;
    };
    let mut remaining_ppm = 1_000_000u32;
    for family in &snapshot.families {
        let epsilon_remaining = if snapshot.max_epsilon <= f64::EPSILON {
            0.0
        } else {
            (snapshot.max_epsilon - family.epsilon_spent) / snapshot.max_epsilon
        };
        let delta_remaining = if snapshot.max_delta <= f64::EPSILON {
            0.0
        } else {
            (snapshot.max_delta - family.delta_spent) / snapshot.max_delta
        };
        let ratio = epsilon_remaining.min(delta_remaining).clamp(0.0, 1.0);
        remaining_ppm = remaining_ppm.min((ratio * 1_000_000f64).round() as u32);
    }
    remaining_ppm
}

fn freshness_component_ppm(
    config: &QualitySignalConfig,
    readiness: Option<&AdReadinessSnapshot>,
    cohort: &CohortKeySnapshot,
) -> (u32, Option<(String, u32)>) {
    let Some(bucket) = cohort.presence_bucket.as_ref() else {
        return (1_000_000, None);
    };
    let histogram = readiness
        .and_then(|snap| snap.segment_readiness.as_ref())
        .and_then(|seg| seg.presence_buckets.get(&bucket.bucket_id))
        .map(|entry| &entry.freshness_histogram);
    let Some(hist) = histogram else {
        return (1_000_000, None);
    };
    let ppm = weighted_freshness_ppm(hist, &config.freshness_weights_ppm);
    (ppm, Some((bucket.bucket_id.clone(), ppm)))
}

fn weighted_freshness_ppm(histogram: &ReadinessHistogram, weights: &WeightHistogram) -> u32 {
    let weighted = histogram.under_1h_ppm as u64 * weights.under_1h_ppm as u64
        + histogram.hours_1_to_6_ppm as u64 * weights.hours_1_to_6_ppm as u64
        + histogram.hours_6_to_24_ppm as u64 * weights.hours_6_to_24_ppm as u64
        + histogram.over_24h_ppm as u64 * weights.over_24h_ppm as u64;
    (weighted / 1_000_000).min(u64::from(u32::MAX)) as u32
}

fn readiness_component_ppm(config: &QualitySignalConfig, streak_windows: u64) -> u32 {
    let target = config.readiness_target_windows.max(1);
    let ratio = (streak_windows as f64 / target as f64)
        .clamp(config.readiness_floor_ppm as f64 / 1_000_000f64, 1.0);
    (ratio * 1_000_000f64).round().min(u32::MAX as f64) as u32
}

fn privacy_component_ppm(
    config: &QualitySignalConfig,
    privacy: Option<&PrivacyBudgetSnapshot>,
    badges: &[String],
) -> (u32, u32) {
    if badges.is_empty() {
        return (1_000_000, 1_000_000);
    }
    let Some(snapshot) = privacy else {
        return (1_000_000, 1_000_000);
    };
    let mut remaining_ppm = 1_000_000u32;
    let mut denied_ppm = 0u32;
    let mut families: HashSet<String> = HashSet::new();
    for badge in badges {
        families.insert(badge_family(badge));
    }
    for family in families {
        let entry = snapshot.families.iter().find(|f| f.family == family);
        let Some(entry) = entry else {
            continue;
        };
        let epsilon_remaining = if snapshot.max_epsilon <= f64::EPSILON {
            0.0
        } else {
            (snapshot.max_epsilon - entry.epsilon_spent) / snapshot.max_epsilon
        };
        let delta_remaining = if snapshot.max_delta <= f64::EPSILON {
            0.0
        } else {
            (snapshot.max_delta - entry.delta_spent) / snapshot.max_delta
        };
        let remaining_ratio = epsilon_remaining.min(delta_remaining).clamp(0.0, 1.0);
        remaining_ppm = remaining_ppm.min((remaining_ratio * 1_000_000f64).round() as u32);
        if entry.cooldown_remaining > 0 {
            denied_ppm = denied_ppm.max(1_000_000);
        }
        let decisions = entry.accepted_total + entry.denied_total + entry.cooling_total;
        if decisions > 0 {
            let denied_ratio = (entry.denied_total + entry.cooling_total) as f64 / decisions as f64;
            denied_ppm = denied_ppm.max((denied_ratio * 1_000_000f64).round() as u32);
        }
    }
    let capped = remaining_ppm.min(1_000_000u32.saturating_sub(denied_ppm));
    let privacy_ppm = capped.max(config.privacy_floor_ppm);
    (privacy_ppm, capped)
}

fn composite_multiplier_ppm(
    config: &QualitySignalConfig,
    freshness_ppm: u32,
    readiness_ppm: u32,
    privacy_ppm: u32,
) -> u32 {
    let product = freshness_ppm as f64 * readiness_ppm as f64 * privacy_ppm as f64;
    product
        .cbrt()
        .clamp(
            config.cohort_floor_ppm as f64,
            config.cohort_ceiling_ppm as f64,
        )
        .round()
        .min(u32::MAX as f64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use ad_market::{
        DomainTier, MarketplaceConfig, PresenceBucketRef, PresenceKind, PrivacyBudgetFamilySnapshot,
    };

    #[test]
    fn quality_signal_uses_presence_privacy_family() {
        let config = MarketplaceConfig::default().quality_signal_config();
        let bucket = PresenceBucketRef {
            bucket_id: "presence-1".to_string(),
            kind: PresenceKind::LocalNet,
            region: None,
            radius_meters: 0,
            confidence_bps: 0,
            minted_at_micros: None,
            expires_at_micros: None,
        };
        let cohort = CohortKeySnapshot {
            domain: "example.test".to_string(),
            provider: None,
            badges: Vec::new(),
            domain_tier: DomainTier::Premium,
            domain_owner: None,
            interest_tags: Vec::new(),
            presence_bucket: Some(bucket),
            selectors_version: 0,
        };
        let privacy_snapshot = PrivacyBudgetSnapshot {
            generated_at_micros: 0,
            families: vec![PrivacyBudgetFamilySnapshot {
                family: "presence".to_string(),
                epsilon_spent: 1.0,
                delta_spent: 1e-6,
                impressions_tracked: 1,
                cooldown_remaining: 0,
                accepted_total: 1,
                denied_total: 1,
                cooling_total: 0,
            }],
            max_epsilon: 1.0,
            max_delta: 1e-6,
        };
        let report = quality_signal_for_cohort(&config, None, Some(&privacy_snapshot), &cohort);
        assert_eq!(report.privacy_score_ppm, 0);
        assert_eq!(
            report.signal.components.privacy_multiplier_ppm,
            config.privacy_floor_ppm
        );
    }
}
