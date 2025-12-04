#![forbid(unsafe_code)]

use crate::ad_readiness::AdReadinessHandle;
use ad_market::{
    BudgetBrokerConfig, BudgetBrokerSnapshot, Campaign, CampaignBudgetSnapshot,
    CohortBudgetSnapshot, CohortPriceSnapshot, ConversionEvent, DistributionPolicy,
    DomainTier, MarketplaceHandle, PresenceBucketRef, PresenceKind, UpliftHoldoutAssignment,
};
use concurrency::Lazy;
use crypto_suite::{encoding::hex, hashing::blake3, ConstantTimeEq};
use foundation_rpc::RpcError;
use foundation_serialization::json::{Map, Number, Value};
use std::collections::BTreeMap;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn unavailable() -> Value {
    let mut map = Map::new();
    map.insert("status".into(), Value::String("unavailable".into()));
    Value::Object(map)
}

fn number_from_f64(value: f64) -> Number {
    Number::from_f64(value).unwrap_or_else(|| Number::from(0))
}

fn invalid_params(message: &'static str) -> RpcError {
    RpcError::new(-32602, message)
}

fn parse_non_empty_string(value: &Value, field: &'static str) -> Result<String, RpcError> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Ok(s.trim().to_string()),
        _ => Err(invalid_params(field)),
    }
}

fn parse_optional_u64(value: Option<&Value>, field: &'static str) -> Result<Option<u64>, RpcError> {
    match value {
        Some(Value::Number(n)) => n.as_u64().ok_or_else(|| invalid_params(field)).map(Some),
        Some(_) => Err(invalid_params(field)),
        None => Ok(None),
    }
}

fn parse_assignment(value: &Value) -> Result<UpliftHoldoutAssignment, RpcError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid_params("assignment"))?;
    let fold = obj
        .get("fold")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_params("assignment.fold"))?;
    if fold > u64::from(u8::MAX) {
        return Err(invalid_params("assignment.fold"));
    }
    let in_holdout = obj
        .get("in_holdout")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid_params("assignment.in_holdout"))?;
    let propensity = obj
        .get("propensity")
        .and_then(Value::as_f64)
        .ok_or_else(|| invalid_params("assignment.propensity"))?;
    Ok(UpliftHoldoutAssignment {
        fold: fold as u8,
        in_holdout,
        propensity,
    })
}

fn parse_conversion_params(params: &Value) -> Result<(ConversionEvent, String), RpcError> {
    let obj = params
        .as_object()
        .ok_or_else(|| invalid_params("object required"))?;
    let campaign_id = obj
        .get("campaign_id")
        .map(|v| parse_non_empty_string(v, "campaign_id"))
        .ok_or_else(|| invalid_params("campaign_id"))??;
    let creative_id = obj
        .get("creative_id")
        .map(|v| parse_non_empty_string(v, "creative_id"))
        .ok_or_else(|| invalid_params("creative_id"))??;
    let advertiser_account = obj
        .get("advertiser_account")
        .map(|v| parse_non_empty_string(v, "advertiser_account"))
        .ok_or_else(|| invalid_params("advertiser_account"))??;
    let assignment_value = obj
        .get("assignment")
        .ok_or_else(|| invalid_params("assignment"))?;
    let assignment = parse_assignment(assignment_value)?;
    let value_usd_micros = parse_optional_u64(obj.get("value_usd_micros"), "value_usd_micros")?;
    let occurred_at_micros =
        parse_optional_u64(obj.get("occurred_at_micros"), "occurred_at_micros")?;
    let event = ConversionEvent {
        campaign_id,
        creative_id,
        assignment,
        value_usd_micros,
        occurred_at_micros,
    };
    Ok((event, advertiser_account))
}

const CONVERSION_TOKEN_HASH_KEY: &str = "conversion_token_hash";

#[derive(Clone, Debug)]
struct ConversionErrorRecord {
    code: String,
    occurred_at: u64,
}

#[derive(Clone, Debug, Default)]
struct ConversionSummaryStats {
    authenticated: u64,
    rejected: BTreeMap<String, u64>,
    last_error: Option<ConversionErrorRecord>,
    last_authenticated_at: Option<u64>,
}

static CONVERSION_STATS: Lazy<RwLock<ConversionSummaryStats>> =
    Lazy::new(|| RwLock::new(ConversionSummaryStats::default()));

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn record_conversion_success() {
    #[cfg(feature = "telemetry")]
    crate::telemetry::sampled_inc_vec(&crate::telemetry::AD_CONVERSION_TOTAL, &["accepted", "ok"]);
    let mut guard = CONVERSION_STATS.write().expect("conversion stats poisoned");
    guard.authenticated = guard.authenticated.saturating_add(1);
    guard.last_authenticated_at = Some(now_ts());
}

fn record_conversion_error(code: &str) {
    #[cfg(feature = "telemetry")]
    crate::telemetry::sampled_inc_vec(&crate::telemetry::AD_CONVERSION_TOTAL, &["rejected", code]);
    let mut guard = CONVERSION_STATS.write().expect("conversion stats poisoned");
    let entry = guard.rejected.entry(code.to_string()).or_insert(0);
    *entry = entry.saturating_add(1);
    guard.last_error = Some(ConversionErrorRecord {
        code: code.to_string(),
        occurred_at: now_ts(),
    });
}

fn conversion_summary_value() -> Value {
    let guard = CONVERSION_STATS.read().expect("conversion stats poisoned");
    let mut map = Map::new();
    map.insert(
        "authenticated_total".into(),
        Value::Number(Number::from(guard.authenticated)),
    );
    let rejected_total: u64 = guard.rejected.values().copied().sum();
    map.insert(
        "rejected_total".into(),
        Value::Number(Number::from(rejected_total)),
    );
    let mut errors = Map::new();
    for (code, count) in guard.rejected.iter() {
        errors.insert(code.clone(), Value::Number(Number::from(*count)));
    }
    map.insert("error_counts".into(), Value::Object(errors));
    if let Some(ts) = guard.last_authenticated_at {
        map.insert(
            "last_authenticated_at".into(),
            Value::Number(Number::from(ts)),
        );
    }
    if let Some(last_error) = guard.last_error.clone() {
        let mut last = Map::new();
        last.insert("code".into(), Value::String(last_error.code));
        last.insert(
            "occurred_at".into(),
            Value::Number(Number::from(last_error.occurred_at)),
        );
        map.insert("last_error".into(), Value::Object(last));
    }
    Value::Object(map)
}

fn err_auth_required() -> RpcError {
    RpcError::new(-32030, "advertiser authorization required")
}

fn err_advertiser_mismatch() -> RpcError {
    RpcError::new(-32031, "advertiser mismatch")
}

fn err_token_missing() -> RpcError {
    RpcError::new(-32032, "conversion token missing")
}

fn err_token_invalid() -> RpcError {
    RpcError::new(-32033, "invalid advertiser token")
}

fn parse_advertiser_auth(auth: Option<&str>) -> Result<(String, String), RpcError> {
    let header = auth.ok_or_else(err_auth_required)?;
    let header = header.trim();
    let Some(rest) = header.strip_prefix("Advertiser ") else {
        return Err(err_auth_required());
    };
    let mut parts = rest.splitn(2, ':');
    let account = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(err_auth_required)?;
    let token = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(err_auth_required)?;
    Ok((account.to_string(), token.to_string()))
}

fn campaign_summary_to_value(summary: &ad_market::CampaignSummary) -> Value {
    let mut entry = Map::new();
    entry.insert("id".into(), Value::String(summary.id.clone()));
    entry.insert(
        "advertiser_account".into(),
        Value::String(summary.advertiser_account.clone()),
    );
    entry.insert(
        "remaining_budget_usd_micros".into(),
        Value::Number(Number::from(summary.remaining_budget_usd_micros)),
    );
    entry.insert(
        "reserved_budget_usd_micros".into(),
        Value::Number(Number::from(summary.reserved_budget_usd_micros)),
    );
    entry.insert(
        "creatives".into(),
        Value::Array(
            summary
                .creatives
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );
    Value::Object(entry)
}

fn budget_config_to_value(config: &BudgetBrokerConfig) -> Value {
    let mut map = Map::new();
    map.insert(
        "epoch_impressions".into(),
        Value::Number(Number::from(config.epoch_impressions)),
    );
    map.insert(
        "step_size".into(),
        Value::Number(number_from_f64(config.step_size)),
    );
    map.insert(
        "dual_step".into(),
        Value::Number(number_from_f64(config.dual_step)),
    );
    map.insert(
        "dual_forgetting".into(),
        Value::Number(number_from_f64(config.dual_forgetting)),
    );
    map.insert(
        "max_kappa".into(),
        Value::Number(number_from_f64(config.max_kappa)),
    );
    map.insert(
        "min_kappa".into(),
        Value::Number(number_from_f64(config.min_kappa)),
    );
    map.insert(
        "shadow_price_cap".into(),
        Value::Number(number_from_f64(config.shadow_price_cap)),
    );
    map.insert(
        "smoothing".into(),
        Value::Number(number_from_f64(config.smoothing)),
    );
    map.insert(
        "epochs_per_budget".into(),
        Value::Number(Number::from(config.epochs_per_budget)),
    );
    Value::Object(map)
}

fn cohort_budget_snapshot_to_value(snapshot: &CohortBudgetSnapshot) -> Value {
    let mut map = Map::new();
    map.insert(
        "domain".into(),
        Value::String(snapshot.cohort.domain.clone()),
    );
    if let Some(provider) = &snapshot.cohort.provider {
        map.insert("provider".into(), Value::String(provider.clone()));
    }
    map.insert(
        "badges".into(),
        Value::Array(
            snapshot
                .cohort
                .badges
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );
    map.insert(
        "kappa".into(),
        Value::Number(number_from_f64(snapshot.kappa)),
    );
    map.insert(
        "smoothed_error".into(),
        Value::Number(number_from_f64(snapshot.smoothed_error)),
    );
    map.insert(
        "realized_spend".into(),
        Value::Number(number_from_f64(snapshot.realized_spend)),
    );
    Value::Object(map)
}

fn campaign_budget_snapshot_to_value(snapshot: &CampaignBudgetSnapshot) -> Value {
    let mut map = Map::new();
    map.insert(
        "campaign_id".into(),
        Value::String(snapshot.campaign_id.clone()),
    );
    map.insert(
        "total_budget".into(),
        Value::Number(Number::from(snapshot.total_budget)),
    );
    map.insert(
        "remaining_budget".into(),
        Value::Number(Number::from(snapshot.remaining_budget)),
    );
    map.insert(
        "epoch_target".into(),
        Value::Number(number_from_f64(snapshot.epoch_target)),
    );
    map.insert(
        "epoch_spend".into(),
        Value::Number(number_from_f64(snapshot.epoch_spend)),
    );
    map.insert(
        "epoch_impressions".into(),
        Value::Number(Number::from(snapshot.epoch_impressions)),
    );
    map.insert(
        "dual_price".into(),
        Value::Number(number_from_f64(snapshot.dual_price)),
    );
    map.insert(
        "cohorts".into(),
        Value::Array(
            snapshot
                .cohorts
                .iter()
                .map(cohort_budget_snapshot_to_value)
                .collect(),
        ),
    );
    Value::Object(map)
}

fn budget_snapshot_to_value(snapshot: &BudgetBrokerSnapshot) -> Value {
    let mut map = Map::new();
    map.insert("config".into(), budget_config_to_value(&snapshot.config));
    let analytics = ad_market::budget_snapshot_analytics(snapshot);
    map.insert(
        "campaigns".into(),
        Value::Array(
            snapshot
                .campaigns
                .iter()
                .map(campaign_budget_snapshot_to_value)
                .collect(),
        ),
    );
    map.insert(
        "generated_at_micros".into(),
        Value::Number(Number::from(snapshot.generated_at_micros)),
    );
    map.insert(
        "summary".into(),
        budget_snapshot_summary_with_analytics(&analytics, &snapshot.config),
    );
    map.insert(
        "pacing".into(),
        budget_snapshot_pacing(snapshot, &analytics),
    );
    map.insert("conversion_summary".into(), conversion_summary_value());
    Value::Object(map)
}

fn budget_snapshot_summary_with_analytics(
    analytics: &ad_market::BudgetBrokerAnalytics,
    config: &BudgetBrokerConfig,
) -> Value {
    let mut map = Map::new();
    map.insert(
        "campaign_count".into(),
        Value::Number(Number::from(analytics.campaign_count)),
    );
    map.insert(
        "cohort_count".into(),
        Value::Number(Number::from(analytics.cohort_count)),
    );
    map.insert(
        "mean_kappa".into(),
        Value::Number(number_from_f64(analytics.mean_kappa)),
    );
    map.insert(
        "min_kappa".into(),
        Value::Number(number_from_f64(analytics.min_kappa)),
    );
    map.insert(
        "max_kappa".into(),
        Value::Number(number_from_f64(analytics.max_kappa)),
    );
    map.insert(
        "mean_smoothed_error".into(),
        Value::Number(number_from_f64(analytics.mean_smoothed_error)),
    );
    map.insert(
        "max_abs_smoothed_error".into(),
        Value::Number(number_from_f64(analytics.max_abs_smoothed_error)),
    );
    map.insert(
        "realized_spend_total".into(),
        Value::Number(number_from_f64(analytics.realized_spend_total)),
    );
    map.insert(
        "epoch_target_total".into(),
        Value::Number(number_from_f64(analytics.epoch_target_total)),
    );
    map.insert(
        "epoch_spend_total".into(),
        Value::Number(number_from_f64(analytics.epoch_spend_total)),
    );
    map.insert(
        "dual_price_max".into(),
        Value::Number(number_from_f64(analytics.dual_price_max)),
    );
    map.insert(
        "config_step_size".into(),
        Value::Number(number_from_f64(config.step_size)),
    );
    map.insert(
        "config_dual_step".into(),
        Value::Number(number_from_f64(config.dual_step)),
    );
    map.insert(
        "config_dual_forgetting".into(),
        Value::Number(number_from_f64(config.dual_forgetting)),
    );
    map.insert(
        "config_max_kappa".into(),
        Value::Number(number_from_f64(config.max_kappa)),
    );
    map.insert(
        "config_min_kappa".into(),
        Value::Number(number_from_f64(config.min_kappa)),
    );
    map.insert(
        "config_shadow_price_cap".into(),
        Value::Number(number_from_f64(config.shadow_price_cap)),
    );
    map.insert(
        "config_smoothing".into(),
        Value::Number(number_from_f64(config.smoothing)),
    );
    Value::Object(map)
}

fn budget_snapshot_pacing(
    snapshot: &BudgetBrokerSnapshot,
    analytics: &ad_market::BudgetBrokerAnalytics,
) -> Value {
    let mut map = Map::new();
    map.insert(
        "step_size".into(),
        Value::Number(number_from_f64(snapshot.config.step_size)),
    );
    map.insert(
        "dual_step".into(),
        Value::Number(number_from_f64(snapshot.config.dual_step)),
    );
    map.insert(
        "dual_forgetting".into(),
        Value::Number(number_from_f64(snapshot.config.dual_forgetting)),
    );
    map.insert(
        "max_kappa_config".into(),
        Value::Number(number_from_f64(snapshot.config.max_kappa)),
    );
    map.insert(
        "min_kappa_config".into(),
        Value::Number(number_from_f64(snapshot.config.min_kappa)),
    );
    map.insert(
        "shadow_price_cap_config".into(),
        Value::Number(number_from_f64(snapshot.config.shadow_price_cap)),
    );
    map.insert(
        "smoothing".into(),
        Value::Number(number_from_f64(snapshot.config.smoothing)),
    );
    map.insert(
        "epochs_per_budget".into(),
        Value::Number(Number::from(snapshot.config.epochs_per_budget)),
    );
    map.insert(
        "campaign_count".into(),
        Value::Number(Number::from(analytics.campaign_count)),
    );
    map.insert(
        "cohort_count".into(),
        Value::Number(Number::from(analytics.cohort_count)),
    );
    map.insert(
        "mean_kappa".into(),
        Value::Number(number_from_f64(analytics.mean_kappa)),
    );
    map.insert(
        "max_kappa_observed".into(),
        Value::Number(number_from_f64(analytics.max_kappa)),
    );
    map.insert(
        "mean_smoothed_error".into(),
        Value::Number(number_from_f64(analytics.mean_smoothed_error)),
    );
    map.insert(
        "max_abs_smoothed_error".into(),
        Value::Number(number_from_f64(analytics.max_abs_smoothed_error)),
    );
    map.insert(
        "dual_price_max".into(),
        Value::Number(number_from_f64(analytics.dual_price_max)),
    );
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cohort_snapshot(
        domain: &str,
        kappa: f64,
        error: f64,
        realized: f64,
    ) -> ad_market::CohortBudgetSnapshot {
        ad_market::CohortBudgetSnapshot {
            cohort: ad_market::CohortKeySnapshot {
                domain: domain.to_string(),
                provider: Some("wallet".into()),
                badges: vec!["badge".into()],
                ..Default::default()
            },
            kappa,
            smoothed_error: error,
            realized_spend: realized,
        }
    }

    fn campaign_snapshot(
        campaign_id: &str,
        epoch_target: f64,
        epoch_spend: f64,
        dual_price: f64,
        cohort: ad_market::CohortBudgetSnapshot,
    ) -> ad_market::CampaignBudgetSnapshot {
        ad_market::CampaignBudgetSnapshot {
            campaign_id: campaign_id.into(),
            total_budget: 5_000_000,
            remaining_budget: 4_000_000,
            epoch_target,
            epoch_spend,
            epoch_impressions: 25,
            dual_price,
            cohorts: vec![cohort],
        }
    }

    #[test]
    fn pacing_delta_matches_partial_snapshot_merge() {
        let config = ad_market::BudgetBrokerConfig::default();
        let base = ad_market::BudgetBrokerSnapshot {
            generated_at_micros: 50,
            config: config.clone(),
            campaigns: vec![
                campaign_snapshot(
                    "cmp-a",
                    120_000.0,
                    90_000.0,
                    0.4,
                    cohort_snapshot("example.com", 0.7, 0.08, 60_000.0),
                ),
                campaign_snapshot(
                    "cmp-b",
                    150_000.0,
                    110_000.0,
                    0.5,
                    cohort_snapshot("news.example", 0.9, 0.05, 100_000.0),
                ),
            ],
        };
        let partial = ad_market::BudgetBrokerSnapshot {
            generated_at_micros: 60,
            config: config.clone(),
            campaigns: vec![campaign_snapshot(
                "cmp-a",
                120_000.0,
                105_000.0,
                0.65,
                cohort_snapshot("example.com", 0.8, 0.06, 75_000.0),
            )],
        };
        let merged = ad_market::merge_budget_snapshots(&base, &partial);
        let base_analytics = ad_market::budget_snapshot_analytics(&base);
        let merged_analytics = ad_market::budget_snapshot_analytics(&merged);
        let delta = ad_market::budget_snapshot_pacing_delta(&base, &merged);

        assert!(
            (delta.mean_kappa_delta - (merged_analytics.mean_kappa - base_analytics.mean_kappa))
                .abs()
                < 1e-9
        );
        assert!(
            (delta.epoch_spend_total_delta
                - (merged_analytics.epoch_spend_total - base_analytics.epoch_spend_total))
                .abs()
                < 1e-6
        );
        assert_eq!(delta.campaign_count_delta, 0);

        let pacing_json = budget_snapshot_pacing(&merged, &merged_analytics);
        let mean_kappa_json = pacing_json
            .get("mean_kappa")
            .and_then(Value::as_f64)
            .expect("mean kappa json");
        assert!((mean_kappa_json - merged_analytics.mean_kappa).abs() < 1e-9);
    }
}

pub fn inventory(market: Option<&MarketplaceHandle>) -> Value {
    let Some(handle) = market else {
        return unavailable();
    };
    let campaigns = handle.list_campaigns();
    let distribution = handle.distribution();
    let oracle = handle.oracle();
    let mut root = Map::new();
    root.insert("status".into(), Value::String("ok".into()));
    root.insert("distribution".into(), distribution_to_value(distribution));
    let mut oracle_map = Map::new();
    oracle_map.insert(
        "ct_price_usd_micros".into(),
        Value::Number(Number::from(oracle.ct_price_usd_micros)),
    );
    oracle_map.insert(
        "it_price_usd_micros".into(),
        Value::Number(Number::from(oracle.it_price_usd_micros)),
    );
    root.insert("oracle".into(), Value::Object(oracle_map));
    let items: Vec<Value> = campaigns.iter().map(campaign_summary_to_value).collect();
    root.insert("campaigns".into(), Value::Array(items));
    let pricing: Vec<Value> = handle
        .cohort_prices()
        .into_iter()
        .map(|snapshot| {
            let mut entry = Map::new();
            entry.insert("domain".into(), Value::String(snapshot.domain));
            if let Some(provider) = snapshot.provider {
                entry.insert("provider".into(), Value::String(provider));
            }
            entry.insert(
                "badges".into(),
                Value::Array(snapshot.badges.into_iter().map(Value::String).collect()),
            );
            entry.insert(
                "price_per_mib_usd_micros".into(),
                Value::Number(Number::from(snapshot.price_per_mib_usd_micros)),
            );
            entry.insert(
                "target_utilization_ppm".into(),
                Value::Number(Number::from(snapshot.target_utilization_ppm)),
            );
            entry.insert(
                "observed_utilization_ppm".into(),
                Value::Number(Number::from(snapshot.observed_utilization_ppm)),
            );
            Value::Object(entry)
        })
        .collect();
    root.insert("cohort_prices".into(), Value::Array(pricing));
    Value::Object(root)
}

pub fn list_campaigns(market: Option<&MarketplaceHandle>) -> Value {
    let Some(handle) = market else {
        return unavailable();
    };
    let mut root = Map::new();
    root.insert("status".into(), Value::String("ok".into()));
    let campaigns: Vec<Value> = handle
        .list_campaigns()
        .iter()
        .map(campaign_summary_to_value)
        .collect();
    root.insert("campaigns".into(), Value::Array(campaigns));
    Value::Object(root)
}

pub fn budget(market: Option<&MarketplaceHandle>) -> Value {
    let Some(handle) = market else {
        return unavailable();
    };
    let snapshot = handle.budget_snapshot();
    #[cfg(feature = "telemetry")]
    crate::telemetry::update_ad_budget_metrics(&snapshot);
    let mut root = Map::new();
    root.insert("status".into(), Value::String("ok".into()));
    if let Value::Object(map) = budget_snapshot_to_value(&snapshot) {
        for (key, value) in map {
            root.insert(key, value);
        }
    }
    Value::Object(root)
}

pub fn broker_state(market: Option<&MarketplaceHandle>) -> Value {
    let Some(handle) = market else {
        return unavailable();
    };
    let snapshot = handle.budget_snapshot();
    #[cfg(feature = "telemetry")]
    crate::telemetry::update_ad_budget_metrics(&snapshot);
    let mut root = Map::new();
    root.insert("status".into(), Value::String("ok".into()));
    if let Value::Object(map) = budget_snapshot_to_value(&snapshot) {
        for (key, value) in map {
            root.insert(key, value);
        }
    }
    Value::Object(root)
}

pub fn distribution(market: Option<&MarketplaceHandle>) -> Value {
    let Some(handle) = market else {
        return unavailable();
    };
    let mut map = Map::new();
    map.insert("status".into(), Value::String("ok".into()));
    map.insert(
        "distribution".into(),
        distribution_to_value(handle.distribution()),
    );
    Value::Object(map)
}

pub fn readiness(
    market: Option<&MarketplaceHandle>,
    readiness: Option<&AdReadinessHandle>,
) -> Value {
    let Some(handle) = readiness else {
        return unavailable();
    };
    let mut distribution_value = None;
    if let Some(market_handle) = market {
        let oracle = market_handle.oracle();
        let cohorts = market_handle.cohort_prices();
        handle.record_utilization(
            &cohorts,
            oracle.ct_price_usd_micros,
            oracle.it_price_usd_micros,
        );
        // Allow the market to adapt distribution weights from live utilization.
        market_handle.recompute_distribution_from_utilization();
        distribution_value = Some(distribution_to_value(market_handle.distribution()));
    } else {
        let empty: Vec<CohortPriceSnapshot> = Vec::new();
        handle.record_utilization(&empty, 0, 0);
    }
    let snapshot = handle.snapshot();
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::update_ad_market_utilization_metrics(&snapshot.cohort_utilization);
    }
    let mut root = Map::new();
    root.insert("status".into(), Value::String("ok".into()));
    root.insert("ready".into(), Value::Bool(snapshot.ready));
    root.insert(
        "window_secs".into(),
        Value::Number(Number::from(snapshot.window_secs)),
    );
    root.insert(
        "unique_viewers".into(),
        Value::Number(Number::from(snapshot.unique_viewers)),
    );
    root.insert(
        "host_count".into(),
        Value::Number(Number::from(snapshot.host_count)),
    );
    root.insert(
        "provider_count".into(),
        Value::Number(Number::from(snapshot.provider_count)),
    );
    let mut thresholds = Map::new();
    thresholds.insert(
        "min_unique_viewers".into(),
        Value::Number(Number::from(snapshot.min_unique_viewers)),
    );
    thresholds.insert(
        "min_host_count".into(),
        Value::Number(Number::from(snapshot.min_host_count)),
    );
    thresholds.insert(
        "min_provider_count".into(),
        Value::Number(Number::from(snapshot.min_provider_count)),
    );
    root.insert("thresholds".into(), Value::Object(thresholds));
    // Dynamic readiness configuration surface
    let cfg = handle.config();
    let mut dynamic = Map::new();
    dynamic.insert(
        "use_percentile_thresholds".into(),
        Value::Bool(cfg.use_percentile_thresholds),
    );
    dynamic.insert(
        "viewer_percentile".into(),
        Value::Number(Number::from(cfg.viewer_percentile as u64)),
    );
    dynamic.insert(
        "host_percentile".into(),
        Value::Number(Number::from(cfg.host_percentile as u64)),
    );
    dynamic.insert(
        "provider_percentile".into(),
        Value::Number(Number::from(cfg.provider_percentile as u64)),
    );
    dynamic.insert(
        "ema_smoothing_ppm".into(),
        Value::Number(Number::from(cfg.ema_smoothing_ppm as u64)),
    );
    dynamic.insert(
        "floor_unique_viewers".into(),
        Value::Number(Number::from(cfg.floor_unique_viewers)),
    );
    dynamic.insert(
        "floor_host_count".into(),
        Value::Number(Number::from(cfg.floor_host_count)),
    );
    dynamic.insert(
        "floor_provider_count".into(),
        Value::Number(Number::from(cfg.floor_provider_count)),
    );
    dynamic.insert(
        "cap_unique_viewers".into(),
        Value::Number(Number::from(cfg.cap_unique_viewers)),
    );
    dynamic.insert(
        "cap_host_count".into(),
        Value::Number(Number::from(cfg.cap_host_count)),
    );
    dynamic.insert(
        "cap_provider_count".into(),
        Value::Number(Number::from(cfg.cap_provider_count)),
    );
    dynamic.insert(
        "percentile_buckets".into(),
        Value::Number(Number::from(cfg.percentile_buckets as u64)),
    );
    root.insert("dynamic".into(), Value::Object(dynamic));
    root.insert(
        "last_updated".into(),
        Value::Number(Number::from(snapshot.last_updated)),
    );
    root.insert(
        "ready_streak_windows".into(),
        Value::Number(Number::from(snapshot.ready_streak_windows)),
    );
    // Rehearsal fields from governance params snapshot
    let (rehearsal_enabled, rehearsal_windows) = {
        let guard = super::GOV_PARAMS.lock().unwrap_or_else(|e| e.into_inner());
        (
            guard.ad_rehearsal_enabled > 0,
            guard.ad_rehearsal_stability_windows.max(0) as u64,
        )
    };
    root.insert("rehearsal_enabled".into(), Value::Bool(rehearsal_enabled));
    root.insert(
        "rehearsal_required_windows".into(),
        Value::Number(Number::from(rehearsal_windows)),
    );
    root.insert(
        "total_usd_micros".into(),
        Value::Number(Number::from(snapshot.total_usd_micros)),
    );
    root.insert(
        "settlement_count".into(),
        Value::Number(Number::from(snapshot.settlement_count)),
    );
    root.insert(
        "ct_price_usd_micros".into(),
        Value::Number(Number::from(snapshot.ct_price_usd_micros)),
    );
    root.insert(
        "it_price_usd_micros".into(),
        Value::Number(Number::from(snapshot.it_price_usd_micros)),
    );
    let blockers: Vec<Value> = snapshot.blockers.into_iter().map(Value::String).collect();
    root.insert("blockers".into(), Value::Array(blockers));
    if let Some(distribution) = distribution_value {
        root.insert("distribution".into(), distribution);
    }
    root.insert("conversion_summary".into(), conversion_summary_value());
    let mut oracle_map = Map::new();
    let mut snapshot_oracle = Map::new();
    snapshot_oracle.insert(
        "ct_price_usd_micros".into(),
        Value::Number(Number::from(snapshot.ct_price_usd_micros)),
    );
    snapshot_oracle.insert(
        "it_price_usd_micros".into(),
        Value::Number(Number::from(snapshot.it_price_usd_micros)),
    );
    oracle_map.insert("snapshot".into(), Value::Object(snapshot_oracle));
    let mut market_oracle = Map::new();
    market_oracle.insert(
        "ct_price_usd_micros".into(),
        Value::Number(Number::from(snapshot.market_ct_price_usd_micros)),
    );
    market_oracle.insert(
        "it_price_usd_micros".into(),
        Value::Number(Number::from(snapshot.market_it_price_usd_micros)),
    );
    oracle_map.insert("market".into(), Value::Object(market_oracle));
    root.insert("oracle".into(), Value::Object(oracle_map));
    if let Some(summary) = snapshot.utilization_summary {
        let mut utilization = Map::new();
        utilization.insert(
            "cohort_count".into(),
            Value::Number(Number::from(summary.cohort_count)),
        );
        utilization.insert(
            "mean_ppm".into(),
            Value::Number(Number::from(summary.mean_ppm)),
        );
        utilization.insert(
            "min_ppm".into(),
            Value::Number(Number::from(summary.min_ppm as u64)),
        );
        utilization.insert(
            "max_ppm".into(),
            Value::Number(Number::from(summary.max_ppm as u64)),
        );
        utilization.insert(
            "last_updated".into(),
            Value::Number(Number::from(summary.last_updated)),
        );
        let cohorts: Vec<Value> = snapshot
            .cohort_utilization
            .into_iter()
            .map(|entry| {
                let mut cohort = Map::new();
                cohort.insert("domain".into(), Value::String(entry.domain));
                if let Some(provider) = entry.provider {
                    cohort.insert("provider".into(), Value::String(provider));
                }
                cohort.insert(
                    "badges".into(),
                    Value::Array(entry.badges.into_iter().map(Value::String).collect()),
                );
                cohort.insert(
                    "price_per_mib_usd_micros".into(),
                    Value::Number(Number::from(entry.price_per_mib_usd_micros)),
                );
                cohort.insert(
                    "target_utilization_ppm".into(),
                    Value::Number(Number::from(entry.target_utilization_ppm)),
                );
                cohort.insert(
                    "observed_utilization_ppm".into(),
                    Value::Number(Number::from(entry.observed_utilization_ppm)),
                );
                cohort.insert(
                    "delta_utilization_ppm".into(),
                    Value::Number(Number::from(entry.delta_ppm)),
                );
                Value::Object(cohort)
            })
            .collect();
        utilization.insert("cohorts".into(), Value::Array(cohorts));
        root.insert("utilization".into(), Value::Object(utilization));
    }
    Value::Object(root)
}

pub fn register_campaign(
    market: Option<&MarketplaceHandle>,
    params: &Value,
) -> Result<Value, RpcError> {
    let Some(handle) = market else {
        return Err(RpcError::new(-32603, "ad market disabled"));
    };
    let campaign = ad_market::campaign_from_value(params)
        .map_err(|_| RpcError::new(-32602, "invalid params"))?;
    match handle.register_campaign(campaign) {
        Ok(()) => {
            let mut map = Map::new();
            map.insert("status".into(), Value::String("ok".into()));
            Ok(Value::Object(map))
        }
        Err(ad_market::MarketplaceError::DuplicateCampaign) => {
            Err(RpcError::new(-32000, "campaign already exists"))
        }
        Err(ad_market::MarketplaceError::PersistenceFailure(_)) => {
            Err(RpcError::new(-32603, "persistence failure"))
        }
        Err(_) => Err(RpcError::new(-32603, "internal error")),
    }
}

pub fn record_conversion(
    market: Option<&MarketplaceHandle>,
    params: &Value,
    auth: Option<&str>,
) -> Result<Value, RpcError> {
    let Some(handle) = market else {
        record_conversion_error("market_disabled");
        return Err(RpcError::new(-32603, "ad market disabled"));
    };
    let (event, advertiser_account) = match parse_conversion_params(params) {
        Ok(value) => value,
        Err(err) => {
            record_conversion_error("invalid_params");
            return Err(err);
        }
    };
    let (auth_account, token) = match parse_advertiser_auth(auth) {
        Ok(value) => value,
        Err(err) => {
            record_conversion_error("auth_required");
            return Err(err);
        }
    };
    if advertiser_account != auth_account {
        record_conversion_error("advertiser_mismatch");
        return Err(err_advertiser_mismatch());
    }
    let campaign: Campaign = match handle.campaign(&event.campaign_id) {
        Some(c) => c,
        None => {
            record_conversion_error("unknown_campaign");
            return Err(RpcError::new(-32001, "unknown campaign"));
        }
    };
    if campaign.advertiser_account != advertiser_account {
        record_conversion_error("advertiser_mismatch");
        return Err(err_advertiser_mismatch());
    }
    let expected_hash = match campaign.metadata.get(CONVERSION_TOKEN_HASH_KEY) {
        Some(hash) => hash,
        None => {
            record_conversion_error("token_missing");
            return Err(err_token_missing());
        }
    };
    let provided_hash = blake3::hash(token.as_bytes());
    let provided_hex = hex::encode(provided_hash.as_bytes());
    if expected_hash.len() != provided_hex.len()
        || !bool::from(expected_hash.as_bytes().ct_eq(provided_hex.as_bytes()))
    {
        record_conversion_error("token_invalid");
        return Err(err_token_invalid());
    }
    match handle.record_conversion(event) {
        Ok(()) => {
            record_conversion_success();
            let mut map = Map::new();
            map.insert("status".into(), Value::String("ok".into()));
            map.insert("conversion_summary".into(), conversion_summary_value());
            Ok(Value::Object(map))
        }
        Err(ad_market::MarketplaceError::UnknownCampaign) => {
            record_conversion_error("unknown_campaign");
            Err(RpcError::new(-32001, "unknown campaign"))
        }
        Err(ad_market::MarketplaceError::UnknownCreative) => {
            record_conversion_error("unknown_creative");
            Err(RpcError::new(-32002, "unknown creative"))
        }
        Err(ad_market::MarketplaceError::PersistenceFailure(_)) => {
            record_conversion_error("persistence_failure");
            Err(RpcError::new(-32603, "persistence failure"))
        }
        Err(_) => {
            record_conversion_error("internal_error");
            Err(RpcError::new(-32603, "internal error"))
        }
    }
}

fn distribution_to_value(policy: DistributionPolicy) -> Value {
    let mut map = Map::new();
    map.insert(
        "viewer_percent".into(),
        Value::Number(Number::from(policy.viewer_percent)),
    );
    map.insert(
        "host_percent".into(),
        Value::Number(Number::from(policy.host_percent)),
    );
    map.insert(
        "hardware_percent".into(),
        Value::Number(Number::from(policy.hardware_percent)),
    );
    map.insert(
        "verifier_percent".into(),
        Value::Number(Number::from(policy.verifier_percent)),
    );
    map.insert(
        "liquidity_percent".into(),
        Value::Number(Number::from(policy.liquidity_percent)),
    );
    map.insert(
        "liquidity_split_ct_ppm".into(),
        Value::Number(Number::from(policy.liquidity_split_ct_ppm)),
    );
    Value::Object(map)
}

// Error codes for presence/privacy operations
const ERR_INVALID_PRESENCE_BUCKET: i32 = -32034;
const ERR_FORBIDDEN_SELECTOR_COMBO: i32 = -32035;
const ERR_UNKNOWN_SELECTOR: i32 = -32036;
const ERR_INSUFFICIENT_PRIVACY_BUDGET: i32 = -32037;
#[allow(dead_code)]
const ERR_HOLDOUT_OVERLAP: i32 = -32038;
#[allow(dead_code)]
const ERR_SELECTOR_WEIGHT_MISMATCH: i32 = -32039;

fn err_invalid_presence_bucket() -> RpcError {
    RpcError::new(ERR_INVALID_PRESENCE_BUCKET, "invalid or expired presence bucket")
}

fn err_forbidden_selector_combo() -> RpcError {
    RpcError::new(
        ERR_FORBIDDEN_SELECTOR_COMBO,
        "selector combination violates privacy policy",
    )
}

#[allow(dead_code)]
fn err_unknown_selector() -> RpcError {
    RpcError::new(ERR_UNKNOWN_SELECTOR, "unknown interest tag or domain tier")
}

fn err_insufficient_privacy_budget() -> RpcError {
    RpcError::new(
        ERR_INSUFFICIENT_PRIVACY_BUDGET,
        "insufficient privacy budget for request",
    )
}

/// Parse optional domain tier filter from request params.
fn parse_domain_tier(value: Option<&Value>) -> Result<Option<DomainTier>, RpcError> {
    match value {
        Some(Value::String(s)) => match s.as_str() {
            "premium" => Ok(Some(DomainTier::Premium)),
            "reserved" => Ok(Some(DomainTier::Reserved)),
            "community" => Ok(Some(DomainTier::Community)),
            "unverified" => Ok(Some(DomainTier::Unverified)),
            _ => Err(invalid_params("domain_tier")),
        },
        Some(_) => Err(invalid_params("domain_tier")),
        None => Ok(None),
    }
}

/// Parse optional presence kind filter from request params.
fn parse_presence_kind(value: Option<&Value>) -> Result<Option<PresenceKind>, RpcError> {
    match value {
        Some(Value::String(s)) => match s.as_str() {
            "localnet" => Ok(Some(PresenceKind::LocalNet)),
            "range_boost" => Ok(Some(PresenceKind::RangeBoost)),
            _ => Err(invalid_params("kind")),
        },
        Some(_) => Err(invalid_params("kind")),
        None => Ok(None),
    }
}

/// Serialize a PresenceBucketRef to JSON.
fn presence_bucket_to_value(bucket: &PresenceBucketRef) -> Value {
    let mut map = Map::new();
    map.insert("bucket_id".into(), Value::String(bucket.bucket_id.clone()));
    map.insert("kind".into(), Value::String(bucket.kind.as_str().into()));
    if let Some(ref region) = bucket.region {
        map.insert("region".into(), Value::String(region.clone()));
    }
    map.insert(
        "radius_meters".into(),
        Value::Number(Number::from(bucket.radius_meters)),
    );
    map.insert(
        "confidence_bps".into(),
        Value::Number(Number::from(bucket.confidence_bps)),
    );
    if let Some(minted_at) = bucket.minted_at_micros {
        map.insert(
            "minted_at_micros".into(),
            Value::Number(Number::from(minted_at)),
        );
    }
    if let Some(expires_at) = bucket.expires_at_micros {
        map.insert(
            "expires_at_micros".into(),
            Value::Number(Number::from(expires_at)),
        );
    }
    Value::Object(map)
}

/// Serialize a presence cohort summary to JSON.
fn presence_cohort_summary_to_value(
    bucket: &PresenceBucketRef,
    ready_slots: u64,
    privacy_guardrail: &str,
) -> Value {
    let mut map = Map::new();
    map.insert("bucket".into(), presence_bucket_to_value(bucket));
    map.insert(
        "ready_slots".into(),
        Value::Number(Number::from(ready_slots)),
    );
    map.insert(
        "privacy_guardrail".into(),
        Value::String(privacy_guardrail.into()),
    );
    map.insert("selector_prices".into(), Value::Array(Vec::new()));
    // Freshness histogram (placeholder)
    let mut freshness = Map::new();
    freshness.insert("under_1h_ppm".into(), Value::Number(Number::from(0)));
    freshness.insert("1h_to_6h_ppm".into(), Value::Number(Number::from(0)));
    freshness.insert("6h_to_24h_ppm".into(), Value::Number(Number::from(0)));
    freshness.insert("over_24h_ppm".into(), Value::Number(Number::from(0)));
    map.insert("freshness_histogram".into(), Value::Object(freshness));
    Value::Object(map)
}

/// List presence cohorts available for targeting.
///
/// Request: `{region?, domain_tier?, min_confidence_bps?, interest_tag?, beacon_id?, kind?, include_expired?, limit?, cursor?}`
/// Response: `{status:"ok", cohorts:[PresenceCohortSummary], privacy_budget:{remaining_ppm}, next_cursor?}`
/// Errors: -32602 invalid filter, -32034 stale bucket, -32037 privacy guardrail
pub fn list_presence_cohorts(
    market: Option<&MarketplaceHandle>,
    params: &Value,
) -> Result<Value, RpcError> {
    let Some(_handle) = market else {
        return Err(RpcError::new(-32603, "ad market disabled"));
    };

    let empty_map = Map::new();
    let obj = params.as_object().unwrap_or(&empty_map);

    // Parse filter parameters
    let region = obj
        .get("region")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let domain_tier = parse_domain_tier(obj.get("domain_tier"))?;
    let min_confidence_bps = obj
        .get("min_confidence_bps")
        .and_then(Value::as_u64)
        .map(|v| v.min(10000) as u16);
    let _interest_tag = obj.get("interest_tag").and_then(Value::as_str);
    let beacon_id = obj
        .get("beacon_id")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let kind = parse_presence_kind(obj.get("kind"))?;
    let include_expired = obj
        .get("include_expired")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let limit = obj
        .get("limit")
        .and_then(Value::as_u64)
        .map(|v| v.min(1000) as usize)
        .unwrap_or(100);
    let _cursor = obj.get("cursor").and_then(Value::as_str);

    // Get current timestamp for expiry checks
    let now_micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);

    // Collect presence cohorts from the market
    // NOTE: In full implementation, this would query a dedicated presence store.
    // For now, we extract presence buckets from cohort prices.
    let cohort_prices = _handle.cohort_prices();
    let mut cohorts: Vec<Value> = Vec::new();
    let mut seen_buckets: std::collections::HashSet<String> = std::collections::HashSet::new();
    let denied_count = 0u64;

    for cohort in cohort_prices {
        if cohorts.len() >= limit {
            break;
        }
        if let Some(ref bucket) = cohort.presence_bucket {
            // Dedup by bucket_id
            if seen_buckets.contains(&bucket.bucket_id) {
                continue;
            }
            seen_buckets.insert(bucket.bucket_id.clone());

            // Apply filters
            if let Some(ref r) = region {
                if bucket.region.as_ref() != Some(r) {
                    continue;
                }
            }
            if let Some(ref _dt) = domain_tier {
                // Domain tier filtering would require cohort key inspection
                // Placeholder: skip if we can't verify
            }
            if let Some(min_conf) = min_confidence_bps {
                if bucket.confidence_bps < min_conf {
                    continue;
                }
            }
            if let Some(ref b) = beacon_id {
                // Beacon filtering not directly available on bucket ref
                if !bucket.bucket_id.contains(b) {
                    continue;
                }
            }
            if let Some(ref k) = kind {
                if &bucket.kind != k {
                    continue;
                }
            }
            // Check expiry
            if !include_expired {
                if let Some(expires_at) = bucket.expires_at_micros {
                    if expires_at < now_micros {
                        continue;
                    }
                }
            }

            // Privacy guardrail check (placeholder for k-anonymity)
            // In full implementation, check against PrivacyBudgetManager
            let privacy_guardrail = "ok";
            let ready_slots = 0u64; // Would come from readiness snapshot

            cohorts.push(presence_cohort_summary_to_value(
                bucket,
                ready_slots,
                privacy_guardrail,
            ));
        }
    }

    // Build response
    let mut result = Map::new();
    result.insert("status".into(), Value::String("ok".into()));
    result.insert("cohorts".into(), Value::Array(cohorts));

    let mut privacy_budget = Map::new();
    privacy_budget.insert(
        "remaining_ppm".into(),
        Value::Number(Number::from(1_000_000u64)), // Placeholder
    );
    privacy_budget.insert(
        "denied_count".into(),
        Value::Number(Number::from(denied_count)),
    );
    result.insert("privacy_budget".into(), Value::Object(privacy_budget));

    Ok(Value::Object(result))
}

/// Reserve presence slots for a campaign.
///
/// Request: `{campaign_id, presence_bucket_id, slot_count, expires_at_micros?, selector_budget?, max_bid_usd_micros?}`
/// Response: `{status:"ok", reservation_id, expires_at_micros, reserved_budget_usd_micros?, effective_selectors?}`
/// Errors: -32001 unknown campaign, -32034 invalid bucket, -32035 forbidden combo, -32037 privacy budget
pub fn reserve_presence(
    market: Option<&MarketplaceHandle>,
    params: &Value,
) -> Result<Value, RpcError> {
    let Some(handle) = market else {
        return Err(RpcError::new(-32603, "ad market disabled"));
    };

    let obj = params
        .as_object()
        .ok_or_else(|| invalid_params("object required"))?;

    // Parse required fields
    let campaign_id = obj
        .get("campaign_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params("campaign_id"))?;
    let presence_bucket_id = obj
        .get("presence_bucket_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params("presence_bucket_id"))?;
    let slot_count = obj
        .get("slot_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_params("slot_count"))?;
    let expires_at_micros = obj.get("expires_at_micros").and_then(Value::as_u64);
    let _max_bid_usd_micros = obj.get("max_bid_usd_micros").and_then(Value::as_u64);

    // Verify campaign exists
    let _campaign = handle
        .campaign(campaign_id)
        .ok_or_else(|| RpcError::new(-32001, "unknown campaign"))?;

    // Get current timestamp
    let now_micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);

    // Find the presence bucket in current cohorts
    let cohort_prices = handle.cohort_prices();
    let mut found_bucket: Option<PresenceBucketRef> = None;
    for cohort in &cohort_prices {
        if let Some(ref bucket) = cohort.presence_bucket {
            if bucket.bucket_id == presence_bucket_id {
                found_bucket = Some(bucket.clone());
                break;
            }
        }
    }

    let bucket = found_bucket.ok_or_else(err_invalid_presence_bucket)?;

    // Check bucket expiry
    if let Some(expires_at) = bucket.expires_at_micros {
        if expires_at < now_micros {
            return Err(err_invalid_presence_bucket());
        }
    }

    // Privacy policy check (placeholder)
    // In full implementation, check PrivacyBudgetManager for selector combo violations
    let privacy_check_passed = true;
    if !privacy_check_passed {
        return Err(err_forbidden_selector_combo());
    }

    // Privacy budget check (placeholder)
    let budget_available = true;
    if !budget_available {
        return Err(err_insufficient_privacy_budget());
    }

    // Generate reservation ID
    let mut hasher = blake3::Hasher::new();
    hasher.update(campaign_id.as_bytes());
    hasher.update(presence_bucket_id.as_bytes());
    hasher.update(&slot_count.to_le_bytes());
    hasher.update(&now_micros.to_le_bytes());
    let reservation_id = hex::encode(&hasher.finalize().as_bytes()[..16]);

    // Compute expiry
    let default_ttl_micros = 86_400_000_000u64; // 24 hours in micros
    let effective_expires = expires_at_micros
        .or(bucket.expires_at_micros)
        .unwrap_or(now_micros + default_ttl_micros);

    // Build response
    let mut result = Map::new();
    result.insert("status".into(), Value::String("ok".into()));
    result.insert(
        "reservation_id".into(),
        Value::String(reservation_id),
    );
    result.insert(
        "expires_at_micros".into(),
        Value::Number(Number::from(effective_expires)),
    );
    result.insert(
        "reserved_budget_usd_micros".into(),
        Value::Number(Number::from(0u64)), // Placeholder
    );
    result.insert("effective_selectors".into(), Value::Array(Vec::new()));

    #[cfg(feature = "telemetry")]
    crate::telemetry::sampled_inc_vec(
        &crate::telemetry::AD_PRESENCE_RESERVATION_TOTAL,
        &["ok"],
    );

    Ok(Value::Object(result))
}
