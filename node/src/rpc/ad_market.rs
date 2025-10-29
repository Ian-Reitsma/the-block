#![forbid(unsafe_code)]

use crate::ad_readiness::AdReadinessHandle;
use ad_market::{
    BudgetBrokerConfig, BudgetBrokerSnapshot, CampaignBudgetSnapshot, CohortBudgetSnapshot,
    CohortPriceSnapshot, DistributionPolicy, MarketplaceHandle,
};
use foundation_rpc::RpcError;
use foundation_serialization::json::{Map, Number, Value};

fn unavailable() -> Value {
    let mut map = Map::new();
    map.insert("status".into(), Value::String("unavailable".into()));
    Value::Object(map)
}

fn number_from_f64(value: f64) -> Number {
    Number::from_f64(value).unwrap_or_else(|| Number::from(0))
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
        "max_kappa".into(),
        Value::Number(number_from_f64(config.max_kappa)),
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
    Value::Object(map)
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
    root.insert(
        "last_updated".into(),
        Value::Number(Number::from(snapshot.last_updated)),
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
