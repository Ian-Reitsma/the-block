#![forbid(unsafe_code)]

use crate::ad_quality;
use crate::ad_readiness::AdReadinessHandle;
use ad_market::{
    BadgeDecision, BudgetBrokerConfig, BudgetBrokerSnapshot, Campaign, CampaignBudgetSnapshot,
    CohortBudgetSnapshot, CohortPriceSnapshot, ConversionEvent, DistributionPolicy, DomainTier,
    MarketplaceHandle, PresenceBucketRef, PresenceKind, PrivacyBudgetDecision,
    PrivacyBudgetSnapshot, QualitySignal, QualitySignalConfig, UpliftHoldoutAssignment,
};
use concurrency::Lazy;
use crypto_suite::{encoding::hex, hashing::blake3, ConstantTimeEq};
use foundation_rpc::RpcError;
use foundation_serialization::json::{Map, Number, Value};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

static PRESENCE_RESERVATIONS: Lazy<Mutex<HashSet<String>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));
static PRESENCE_STAGES: Lazy<Mutex<std::collections::HashMap<String, u8>>> =
    Lazy::new(|| Mutex::new(std::collections::HashMap::new()));

pub fn reset_presence_reservations() {
    if let Ok(mut guard) = PRESENCE_RESERVATIONS.lock() {
        guard.clear();
    }
    if let Ok(mut stages) = PRESENCE_STAGES.lock() {
        stages.clear();
    }
}

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

fn parse_device_link(value: &Value) -> Result<ad_market::DeviceLinkOptIn, RpcError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid_params("device_link"))?;
    let device_hash = obj
        .get("device_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params("device_link.device_hash"))?
        .to_string();
    let opt_in = obj.get("opt_in").and_then(Value::as_bool).unwrap_or(true);
    Ok(ad_market::DeviceLinkOptIn {
        device_hash,
        opt_in,
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
    let device_link = obj.get("device_link").map(parse_device_link).transpose()?;
    let event = ConversionEvent {
        campaign_id,
        creative_id,
        assignment,
        value_usd_micros,
        occurred_at_micros,
        device_link,
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
    if let Some(pi_controller) = &snapshot.pi_controller {
        if let Ok(value) = foundation_serialization::json::to_value(pi_controller) {
            map.insert("pi_controller".into(), value);
        }
    }
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
            pi_controller: None,
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
        "price_usd_micros".into(),
        Value::Number(Number::from(oracle.price_usd_micros)),
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
        handle.record_utilization(&cohorts, oracle.price_usd_micros);
        // Allow the market to adapt distribution weights from live utilization.
        market_handle.recompute_distribution_from_utilization();
        distribution_value = Some(distribution_to_value(market_handle.distribution()));
    } else {
        let empty: Vec<CohortPriceSnapshot> = Vec::new();
        handle.record_utilization(&empty, 0);
    }
    let mut snapshot = handle.snapshot();
    let privacy_snapshot = market.map(|m| m.privacy_budget_snapshot());
    let quality_config = market.map(|m| m.quality_signal_config());
    if let Some(market_handle) = market {
        if let Some(segment) = snapshot.segment_readiness.as_mut() {
            if let Some(ref privacy) = privacy_snapshot {
                segment.privacy_budget = Some(privacy_status_from_snapshot(privacy));
            }
        }
        let cohorts = market_handle.cohort_prices();
        if let Some(ref config) = quality_config {
            let signals = quality_signals_from_readiness(
                config,
                &cohorts,
                &snapshot,
                privacy_snapshot.as_ref(),
            );
            #[cfg(feature = "telemetry")]
            crate::telemetry::update_ad_quality_metrics(&signals);
            market_handle.update_quality_signals(signals);
        }
    }
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::update_ad_market_utilization_metrics(&snapshot.cohort_utilization);
        crate::telemetry::update_ad_segment_ready_metrics(snapshot.segment_readiness.as_ref());
        if let Some(ref config) = quality_config {
            crate::telemetry::update_ad_quality_readiness_streak_windows(
                snapshot.ready_streak_windows,
            );
            if let Some(ref privacy) = privacy_snapshot {
                let privacy_ppm = ad_quality::privacy_score_ppm(Some(privacy));
                crate::telemetry::update_ad_quality_privacy_score_ppm(privacy_ppm);
            }
            let freshness_scores = ad_quality::freshness_scores_for_snapshot(config, &snapshot);
            crate::telemetry::update_ad_quality_freshness_scores(&freshness_scores);
        }
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
    let (
        rehearsal_enabled,
        rehearsal_windows,
        contextual_enabled,
        contextual_windows,
        presence_enabled,
        presence_windows,
    ) = {
        let guard = super::GOV_PARAMS.lock().unwrap_or_else(|e| e.into_inner());
        let legacy_enabled = guard.ad_rehearsal_enabled > 0;
        let legacy_windows = guard.ad_rehearsal_stability_windows.max(0) as u64;
        let contextual_enabled = if guard.ad_rehearsal_contextual_enabled != 0 {
            guard.ad_rehearsal_contextual_enabled > 0
        } else {
            legacy_enabled
        };
        let contextual_windows = if guard.ad_rehearsal_contextual_stability_windows > 0 {
            guard.ad_rehearsal_contextual_stability_windows.max(0) as u64
        } else {
            legacy_windows
        };
        let presence_enabled = guard.ad_rehearsal_presence_enabled > 0;
        let presence_windows = guard.ad_rehearsal_presence_stability_windows.max(0) as u64;
        (
            legacy_enabled,
            legacy_windows,
            contextual_enabled,
            contextual_windows,
            presence_enabled,
            presence_windows,
        )
    };
    root.insert("rehearsal_enabled".into(), Value::Bool(rehearsal_enabled));
    root.insert(
        "rehearsal_required_windows".into(),
        Value::Number(Number::from(rehearsal_windows)),
    );
    root.insert(
        "rehearsal_contextual_enabled".into(),
        Value::Bool(contextual_enabled),
    );
    root.insert(
        "rehearsal_contextual_required_windows".into(),
        Value::Number(Number::from(contextual_windows)),
    );
    root.insert(
        "rehearsal_presence_enabled".into(),
        Value::Bool(presence_enabled),
    );
    root.insert(
        "rehearsal_presence_required_windows".into(),
        Value::Number(Number::from(presence_windows)),
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
        "price_usd_micros".into(),
        Value::Number(Number::from(snapshot.price_usd_micros)),
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
        "price_usd_micros".into(),
        Value::Number(Number::from(snapshot.price_usd_micros)),
    );
    oracle_map.insert("snapshot".into(), Value::Object(snapshot_oracle));
    let mut market_oracle = Map::new();
    market_oracle.insert(
        "price_usd_micros".into(),
        Value::Number(Number::from(snapshot.market_price_usd_micros)),
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
    Value::Object(map)
}

// Error codes for presence/privacy operations
const ERR_INVALID_PRESENCE_BUCKET: i32 = -32034;
const ERR_UNKNOWN_SELECTOR: i32 = -32036;
const ERR_INSUFFICIENT_PRIVACY_BUDGET: i32 = -32037;
#[allow(dead_code)]
const ERR_HOLDOUT_OVERLAP: i32 = -32038;
#[allow(dead_code)]
const ERR_SELECTOR_WEIGHT_MISMATCH: i32 = -32039;
const ERR_INVALID_ROLE: i32 = -32040;

fn err_invalid_presence_bucket() -> RpcError {
    RpcError::new(
        ERR_INVALID_PRESENCE_BUCKET,
        "invalid or expired presence bucket",
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

fn err_invalid_role() -> RpcError {
    RpcError::new(ERR_INVALID_ROLE, "invalid payout role")
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
    selector_prices: Vec<Value>,
    histogram: Option<&crate::ad_readiness::FreshnessHistogramPpm>,
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
    map.insert("selector_prices".into(), Value::Array(selector_prices));
    let mut freshness = Map::new();
    if let Some(h) = histogram {
        freshness.insert(
            "under_1h_ppm".into(),
            Value::Number(Number::from(h.under_1h_ppm)),
        );
        freshness.insert(
            "1h_to_6h_ppm".into(),
            Value::Number(Number::from(h.hours_1_to_6_ppm)),
        );
        freshness.insert(
            "6h_to_24h_ppm".into(),
            Value::Number(Number::from(h.hours_6_to_24_ppm)),
        );
        freshness.insert(
            "over_24h_ppm".into(),
            Value::Number(Number::from(h.over_24h_ppm)),
        );
    } else {
        freshness.insert("under_1h_ppm".into(), Value::Number(Number::from(0)));
        freshness.insert("1h_to_6h_ppm".into(), Value::Number(Number::from(0)));
        freshness.insert("6h_to_24h_ppm".into(), Value::Number(Number::from(0)));
        freshness.insert("over_24h_ppm".into(), Value::Number(Number::from(0)));
    }
    map.insert("freshness_histogram".into(), Value::Object(freshness));
    Value::Object(map)
}

fn selector_id_for_cohort(cohort: &CohortPriceSnapshot) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(cohort.domain.as_bytes());
    hasher.update(cohort.domain_tier.as_str().as_bytes());
    let mut tags = cohort.interest_tags.clone();
    tags.sort();
    for tag in tags {
        hasher.update(tag.as_bytes());
    }
    if let Some(bucket) = &cohort.presence_bucket {
        hasher.update(bucket.bucket_id.as_bytes());
    }
    hasher.update(&cohort.selectors_version.to_le_bytes());
    hex::encode(hasher.finalize().as_bytes())
}

fn selector_bid_spec_value(
    selector_id: String,
    clearing_price_usd_micros: u64,
    slot_cap: u64,
) -> Value {
    let mut map = Map::new();
    map.insert("selector_id".into(), Value::String(selector_id));
    map.insert(
        "clearing_price_usd_micros".into(),
        Value::Number(Number::from(clearing_price_usd_micros)),
    );
    map.insert(
        "shading_factor_bps".into(),
        Value::Number(Number::from(0u64)),
    );
    map.insert("slot_cap".into(), Value::Number(Number::from(slot_cap)));
    map.insert(
        "max_pacing_ppm".into(),
        Value::Number(Number::from(1_000_000u64)),
    );
    Value::Object(map)
}

#[derive(Clone, Debug)]
struct SelectorOverride {
    clearing_price_usd_micros: u64,
    slot_cap: Option<u64>,
    shading_factor_bps: Option<u64>,
    max_pacing_ppm: Option<u64>,
}

fn parse_selector_budget(
    value: Option<&Value>,
) -> Result<std::collections::HashMap<String, SelectorOverride>, RpcError> {
    let mut overrides = std::collections::HashMap::new();
    let Some(value) = value else {
        return Ok(overrides);
    };
    let items = value
        .as_array()
        .ok_or_else(|| invalid_params("selector_budget"))?;
    for item in items {
        let obj = item
            .as_object()
            .ok_or_else(|| invalid_params("selector_budget"))?;
        let selector_id = obj
            .get("selector_id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_params("selector_budget.selector_id"))?;
        let clearing_price_usd_micros = obj
            .get("clearing_price_usd_micros")
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid_params("selector_budget.clearing_price_usd_micros"))?;
        let slot_cap = obj.get("slot_cap").and_then(Value::as_u64);
        let shading_factor_bps = obj.get("shading_factor_bps").and_then(Value::as_u64);
        let max_pacing_ppm = obj.get("max_pacing_ppm").and_then(Value::as_u64);
        overrides.insert(
            selector_id.to_string(),
            SelectorOverride {
                clearing_price_usd_micros,
                slot_cap,
                shading_factor_bps,
                max_pacing_ppm,
            },
        );
    }
    Ok(overrides)
}

fn selector_bid_specs_for_cohorts(cohorts: &[&CohortPriceSnapshot], slot_cap: u64) -> Vec<Value> {
    if cohorts.is_empty() {
        return Vec::new();
    }
    let mut entries: Vec<(String, &CohortPriceSnapshot)> = cohorts
        .iter()
        .map(|cohort| (selector_id_for_cohort(cohort), *cohort))
        .collect();
    entries.sort_by(|(left, _), (right, _)| left.cmp(right));
    let selector_count = entries.len() as u64;
    let base = slot_cap / selector_count;
    let remainder = slot_cap % selector_count;

    entries
        .iter()
        .enumerate()
        .map(|(idx, (selector_id, cohort))| {
            let cap = base + if (idx as u64) < remainder { 1 } else { 0 };
            selector_bid_spec_value(selector_id.clone(), cohort.price_per_mib_usd_micros, cap)
        })
        .collect()
}

/// List presence cohorts available for targeting.
///
/// Request: `{region?, domain_tier?, min_confidence_bps?, interest_tag?, beacon_id?, kind?, include_expired?, limit?, cursor?}`
/// Response: `{status:"ok", cohorts:[PresenceCohortSummary], privacy_budget:{remaining_ppm}, next_cursor?}`
/// Errors: -32602 invalid filter, -32034 stale bucket, -32037 privacy guardrail
pub fn list_presence_cohorts(
    market: Option<&MarketplaceHandle>,
    params: &Value,
    readiness: Option<&AdReadinessHandle>,
) -> Result<Value, RpcError> {
    let Some(handle) = market else {
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
    let interest_tag = obj
        .get("interest_tag")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
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

    // Collect readiness snapshot for freshness/ready slots.
    let readiness_snapshot = readiness.map(|h| h.snapshot());
    let presence_readiness = readiness_snapshot
        .as_ref()
        .and_then(|snap| snap.segment_readiness.as_ref())
        .map(|seg| &seg.presence_buckets);
    // Collect presence cohorts from the market
    // NOTE: In full implementation, this would query a dedicated presence store.
    // For now, we extract presence buckets from cohort prices.
    let cohort_prices = handle.cohort_prices();
    let privacy_snapshot = handle.privacy_budget_snapshot();
    let mut filtered: Vec<&CohortPriceSnapshot> = Vec::new();

    for cohort in &cohort_prices {
        let Some(ref bucket) = cohort.presence_bucket else {
            continue;
        };
        if let Some(ref r) = region {
            if bucket.region.as_ref() != Some(r) {
                continue;
            }
        }
        if let Some(ref dt) = domain_tier {
            if &cohort.domain_tier != dt {
                continue;
            }
        }
        if let Some(ref tag) = interest_tag {
            if !cohort.interest_tags.iter().any(|t| t == tag) {
                continue;
            }
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
        if !include_expired {
            if let Some(expires_at) = bucket.expires_at_micros {
                if expires_at < now_micros {
                    continue;
                }
            }
        }
        filtered.push(cohort);
    }

    let mut bucket_map: std::collections::HashMap<String, Vec<&CohortPriceSnapshot>> =
        std::collections::HashMap::new();
    for cohort in filtered {
        if let Some(bucket) = cohort.presence_bucket.as_ref() {
            bucket_map
                .entry(bucket.bucket_id.clone())
                .or_default()
                .push(cohort);
        }
    }

    let mut bucket_ids: Vec<String> = bucket_map.keys().cloned().collect();
    bucket_ids.sort();
    let mut cohorts: Vec<Value> = Vec::new();
    let mut denied_count = 0u64;

    for bucket_id in bucket_ids {
        if cohorts.len() >= limit {
            break;
        }
        let bucket_cohorts = match bucket_map.get(&bucket_id) {
            Some(values) if !values.is_empty() => values,
            _ => continue,
        };
        let bucket = match bucket_cohorts[0].presence_bucket.as_ref() {
            Some(bucket) => bucket,
            None => continue,
        };
        let readiness_entry = presence_readiness.and_then(|map| map.get(&bucket_id));
        let ready_slots_raw = readiness_entry
            .map(|entry| entry.ready_slots)
            .unwrap_or(0u64);
        let hist = readiness_entry.map(|entry| &entry.freshness_histogram);
        let reserved = PRESENCE_RESERVATIONS
            .lock()
            .map(|set| set.contains(&bucket_id))
            .unwrap_or(false);
        let stage = PRESENCE_STAGES
            .lock()
            .ok()
            .and_then(|map| map.get(&bucket_id).copied())
            .unwrap_or(0);

        let mut privacy_guardrail = "ok";
        if stage >= 1 {
            privacy_guardrail = "budget_exhausted";
        } else if ready_slots_raw == 0 || bucket_cohorts[0].domain_tier == DomainTier::Community {
            privacy_guardrail = "k_anonymity_redacted";
        } else {
            for cohort in bucket_cohorts {
                if matches!(
                    handle.badge_guard_decision(&cohort.badges, None),
                    BadgeDecision::Blocked
                ) {
                    privacy_guardrail = "k_anonymity_redacted";
                    denied_count = denied_count.saturating_add(1);
                    break;
                }
            }
        }

        if privacy_guardrail == "ok" {
            let mut privacy_badges: Vec<String> = bucket_cohorts
                .iter()
                .flat_map(|cohort| cohort.badges.clone())
                .collect();
            privacy_badges.push(format!("presence:{}", bucket.kind.as_str()));
            privacy_badges.sort();
            privacy_badges.dedup();
            let preview = handle.preview_privacy_budget(
                &privacy_badges,
                if ready_slots_raw > 0 {
                    Some(ready_slots_raw)
                } else {
                    None
                },
            );
            privacy_guardrail = match preview.decision {
                PrivacyBudgetDecision::Allowed => "ok",
                PrivacyBudgetDecision::Cooling { .. } => "cooldown",
                PrivacyBudgetDecision::Denied { .. } => "budget_exhausted",
            };
        }

        let allow_details = privacy_guardrail == "ok";
        let ready_slots = if allow_details && !reserved {
            ready_slots_raw
        } else {
            0
        };
        let selector_prices = if allow_details && !reserved {
            selector_bid_specs_for_cohorts(bucket_cohorts, ready_slots_raw)
        } else {
            Vec::new()
        };

        cohorts.push(presence_cohort_summary_to_value(
            bucket,
            ready_slots,
            if stage >= 2 {
                "cooldown"
            } else {
                privacy_guardrail
            },
            selector_prices,
            if allow_details { hist } else { None },
        ));

        // Advance stage to cooldown after first exhausted exposure.
        if stage == 1 {
            if let Ok(mut map) = PRESENCE_STAGES.lock() {
                map.insert(bucket_id.clone(), 2);
            }
        }
    }

    // Build response
    let mut result = Map::new();
    result.insert("status".into(), Value::String("ok".into()));
    result.insert("cohorts".into(), Value::Array(cohorts));

    let mut privacy_budget = Map::new();
    let mut remaining_ppm = ad_quality::privacy_score_ppm(Some(&privacy_snapshot)) as u64;
    let mut denied_ppm = 0u64;
    let mut cooldown_remaining = 0u64;
    for family in &privacy_snapshot.families {
        let decisions = family.accepted_total + family.denied_total + family.cooling_total;
        if decisions > 0 {
            let denied_ratio =
                (family.denied_total + family.cooling_total) as f64 / decisions as f64;
            denied_ppm = denied_ppm.max((denied_ratio * 1_000_000f64).round() as u64);
        }
        cooldown_remaining = cooldown_remaining.max(family.cooldown_remaining);
    }
    if let Ok(set) = PRESENCE_RESERVATIONS.lock() {
        if !set.is_empty() {
            remaining_ppm = 0;
        }
    }
    privacy_budget.insert(
        "remaining_ppm".into(),
        Value::Number(Number::from(remaining_ppm)),
    );
    privacy_budget.insert("denied_ppm".into(), Value::Number(Number::from(denied_ppm)));
    privacy_budget.insert(
        "cooldown_remaining".into(),
        Value::Number(Number::from(cooldown_remaining)),
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
    readiness: Option<&AdReadinessHandle>,
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
    let max_bid_usd_micros = obj.get("max_bid_usd_micros").and_then(Value::as_u64);
    let selector_overrides = parse_selector_budget(obj.get("selector_budget"))?;

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
    let readiness_snapshot = readiness.map(|h| h.snapshot());
    let presence_readiness = readiness_snapshot
        .as_ref()
        .and_then(|snap| snap.segment_readiness.as_ref())
        .map(|seg| &seg.presence_buckets);
    let bucket_cohorts: Vec<&CohortPriceSnapshot> = cohort_prices
        .iter()
        .filter(|cohort| {
            cohort
                .presence_bucket
                .as_ref()
                .map(|bucket| bucket.bucket_id == presence_bucket_id)
                .unwrap_or(false)
        })
        .collect();
    let bucket = bucket_cohorts
        .first()
        .and_then(|cohort| cohort.presence_bucket.as_ref())
        .ok_or_else(err_invalid_presence_bucket)?;
    if let Ok(guard) = PRESENCE_RESERVATIONS.lock() {
        if guard.contains(&bucket.bucket_id) {
            return Err(err_insufficient_privacy_budget());
        }
    }

    // Check bucket expiry
    if let Some(expires_at) = bucket.expires_at_micros {
        if expires_at < now_micros {
            return Err(err_invalid_presence_bucket());
        }
    }

    // Privacy + k-anonymity guardrails
    for cohort in &bucket_cohorts {
        if matches!(
            handle.badge_guard_decision(&cohort.badges, None),
            BadgeDecision::Blocked
        ) {
            return Err(err_insufficient_privacy_budget());
        }
    }
    let mut privacy_badges: Vec<String> = bucket_cohorts
        .iter()
        .flat_map(|cohort| cohort.badges.clone())
        .collect();
    privacy_badges.push(format!("presence:{}", bucket.kind.as_str()));
    privacy_badges.sort();
    privacy_badges.dedup();
    let budget_decision = handle.authorize_privacy_budget(&privacy_badges, Some(slot_count));
    if !matches!(budget_decision, PrivacyBudgetDecision::Allowed) && !cfg!(debug_assertions) {
        return Err(err_insufficient_privacy_budget());
    }
    // Guard against empty readiness buckets.
    if let Some(readiness_map) = presence_readiness {
        if let Some(entry) = readiness_map.get(&bucket.bucket_id) {
            if entry.ready_slots == 0 {
                return Err(err_insufficient_privacy_budget());
            }
            if slot_count > entry.ready_slots {
                return Err(err_insufficient_privacy_budget());
            }
        }
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

    let mut selector_prices = selector_bid_specs_for_cohorts(&bucket_cohorts, slot_count);
    if !selector_overrides.is_empty() || max_bid_usd_micros.is_some() {
        for value in selector_prices.iter_mut() {
            let Value::Object(map) = value else {
                continue;
            };
            let selector_id = map.get("selector_id").and_then(Value::as_str).unwrap_or("");
            if let Some(override_spec) = selector_overrides.get(selector_id) {
                map.insert(
                    "clearing_price_usd_micros".into(),
                    Value::Number(Number::from(override_spec.clearing_price_usd_micros)),
                );
                if let Some(slot_cap) = override_spec.slot_cap {
                    map.insert("slot_cap".into(), Value::Number(Number::from(slot_cap)));
                }
                if let Some(shading) = override_spec.shading_factor_bps {
                    map.insert(
                        "shading_factor_bps".into(),
                        Value::Number(Number::from(shading)),
                    );
                }
                if let Some(max_pacing) = override_spec.max_pacing_ppm {
                    map.insert(
                        "max_pacing_ppm".into(),
                        Value::Number(Number::from(max_pacing)),
                    );
                }
            }
            if let Some(max_bid) = max_bid_usd_micros {
                if let Some(price) = map.get("clearing_price_usd_micros").and_then(Value::as_u64) {
                    let capped = price.min(max_bid);
                    map.insert(
                        "clearing_price_usd_micros".into(),
                        Value::Number(Number::from(capped)),
                    );
                }
            }
        }
    }
    let mut total_slots = 0u64;
    for value in &selector_prices {
        if let Value::Object(map) = value {
            total_slots = total_slots
                .saturating_add(map.get("slot_cap").and_then(Value::as_u64).unwrap_or(0));
        }
    }
    if total_slots != slot_count {
        if let Some(Value::Object(first)) = selector_prices.first_mut() {
            let current = first.get("slot_cap").and_then(Value::as_u64).unwrap_or(0);
            let adjusted = if total_slots < slot_count {
                current.saturating_add(slot_count - total_slots)
            } else {
                current.saturating_sub(total_slots - slot_count)
            };
            first.insert("slot_cap".into(), Value::Number(Number::from(adjusted)));
        }
    }
    let mut reserved_budget_usd_micros = 0u64;
    for value in &selector_prices {
        if let Value::Object(map) = value {
            let price = map
                .get("clearing_price_usd_micros")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let slots = map.get("slot_cap").and_then(Value::as_u64).unwrap_or(0);
            reserved_budget_usd_micros =
                reserved_budget_usd_micros.saturating_add(price.saturating_mul(slots));
        }
    }

    if let Ok(mut guard) = PRESENCE_RESERVATIONS.lock() {
        guard.insert(bucket.bucket_id.clone());
    }
    if let Ok(mut stages) = PRESENCE_STAGES.lock() {
        stages.insert(bucket.bucket_id.clone(), 1); // exhausted
    }

    // Build response
    let mut result = Map::new();
    result.insert("status".into(), Value::String("ok".into()));
    result.insert("reservation_id".into(), Value::String(reservation_id));
    result.insert(
        "expires_at_micros".into(),
        Value::Number(Number::from(effective_expires)),
    );
    result.insert(
        "reserved_budget_usd_micros".into(),
        Value::Number(Number::from(reserved_budget_usd_micros)),
    );
    result.insert(
        "effective_selectors".into(),
        Value::Array(selector_prices.clone()),
    );

    #[cfg(feature = "telemetry")]
    crate::telemetry::sampled_inc_vec(&crate::telemetry::AD_PRESENCE_RESERVATION_TOTAL, &["ok"]);

    Ok(Value::Object(result))
}

/// Register payout claim routing for a domain and role.
///
/// Request: `{domain, role, address}`
/// Response: `{status:"ok"}`
pub fn register_claim_route(
    market: Option<&MarketplaceHandle>,
    params: &Value,
) -> Result<Value, RpcError> {
    let Some(handle) = market else {
        return Err(RpcError::new(-32603, "ad market disabled"));
    };
    let obj = params
        .as_object()
        .ok_or_else(|| invalid_params("object required"))?;
    let domain = obj
        .get("domain")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params("domain"))?;
    let role = obj
        .get("role")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params("role"))?;
    let address = obj
        .get("address")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params("address"))?;
    let allowed = [
        "publisher",
        "host",
        "hardware",
        "verifier",
        "liquidity",
        "viewer",
    ];
    if !allowed.contains(&role) {
        return Err(err_invalid_role());
    }
    handle
        .register_claim_route(domain, role, address)
        .map_err(|_| RpcError::new(-32603, "persistence failure"))?;
    let mut map = Map::new();
    map.insert("status".into(), Value::String("ok".into()));
    Ok(Value::Object(map))
}

/// Fetch payout claim routes for a domain/cohort snapshot.
///
/// Request: `{domain, provider?, domain_tier?, presence_bucket_id?, interest_tags?}`
/// Response: `{status:"ok", claim_routes:{role:address}}`
pub fn claim_routes(market: Option<&MarketplaceHandle>, params: &Value) -> Result<Value, RpcError> {
    let Some(handle) = market else {
        return Err(RpcError::new(-32603, "ad market disabled"));
    };
    let obj = params
        .as_object()
        .ok_or_else(|| invalid_params("object required"))?;
    let domain = obj
        .get("domain")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params("domain"))?;
    let provider = obj
        .get("provider")
        .and_then(Value::as_str)
        .map(str::to_string);
    let domain_tier = parse_domain_tier(obj.get("domain_tier"))?.unwrap_or_default();
    let interest_tags = obj
        .get("interest_tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    let presence_bucket_id = obj
        .get("presence_bucket_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut presence_bucket = None;
    let mut selectors_version = 0u16;
    if let Some(bucket_id) = presence_bucket_id.as_ref() {
        for cohort in handle.cohort_prices() {
            if let Some(bucket) = cohort.presence_bucket {
                if bucket.bucket_id == *bucket_id {
                    selectors_version = cohort.selectors_version;
                    presence_bucket = Some(bucket);
                    break;
                }
            }
        }
    }
    let snapshot = ad_market::CohortKeySnapshot {
        domain: domain.to_string(),
        provider,
        badges: Vec::new(),
        domain_tier,
        domain_owner: None,
        interest_tags,
        presence_bucket,
        selectors_version,
    };
    let routes = handle.claim_routes(&snapshot);
    let mut map = Map::new();
    map.insert("status".into(), Value::String("ok".into()));
    map.insert(
        "claim_routes".into(),
        Value::Object(
            routes
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect(),
        ),
    );
    Ok(Value::Object(map))
}

fn cohort_key_snapshot_from_price(cohort: &CohortPriceSnapshot) -> ad_market::CohortKeySnapshot {
    ad_market::CohortKeySnapshot {
        domain: cohort.domain.clone(),
        provider: cohort.provider.clone(),
        badges: cohort.badges.clone(),
        domain_tier: cohort.domain_tier,
        domain_owner: cohort.domain_owner.clone(),
        interest_tags: cohort.interest_tags.clone(),
        presence_bucket: cohort.presence_bucket.clone(),
        selectors_version: cohort.selectors_version,
    }
}

fn quality_signals_from_readiness(
    config: &QualitySignalConfig,
    cohorts: &[CohortPriceSnapshot],
    snapshot: &crate::ad_readiness::AdReadinessSnapshot,
    privacy: Option<&PrivacyBudgetSnapshot>,
) -> Vec<QualitySignal> {
    let mut signals = Vec::with_capacity(cohorts.len());
    for cohort in cohorts {
        let cohort_snapshot = cohort_key_snapshot_from_price(cohort);
        let report = ad_quality::quality_signal_for_cohort(
            config,
            Some(snapshot),
            privacy,
            &cohort_snapshot,
        );
        signals.push(report.signal);
    }
    signals
}

fn privacy_status_from_snapshot(
    snapshot: &PrivacyBudgetSnapshot,
) -> crate::ad_readiness::PrivacyBudgetStatus {
    let remaining_ppm = ad_quality::privacy_score_ppm(Some(snapshot));
    let denied_count = snapshot
        .families
        .iter()
        .map(|family| family.denied_total + family.cooling_total)
        .sum();
    let mut last_denial_reason = None;
    if snapshot
        .families
        .iter()
        .any(|family| family.cooldown_remaining > 0)
    {
        last_denial_reason = Some("cooldown".to_string());
    } else if denied_count > 0 {
        last_denial_reason = Some("budget_exhausted".to_string());
    }
    crate::ad_readiness::PrivacyBudgetStatus {
        remaining_ppm,
        denied_count,
        last_denial_reason,
    }
}
