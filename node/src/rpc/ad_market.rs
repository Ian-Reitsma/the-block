#![forbid(unsafe_code)]

use crate::ad_readiness::AdReadinessHandle;
use ad_market::{DistributionPolicy, MarketplaceHandle};
use foundation_rpc::RpcError;
use foundation_serialization::json::{Map, Number, Value};

fn unavailable() -> Value {
    let mut map = Map::new();
    map.insert("status".into(), Value::String("unavailable".into()));
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
    let items: Vec<Value> = campaigns
        .into_iter()
        .map(|campaign| {
            let mut entry = Map::new();
            entry.insert("id".into(), Value::String(campaign.id));
            entry.insert(
                "advertiser_account".into(),
                Value::String(campaign.advertiser_account),
            );
            entry.insert(
                "remaining_budget_usd_micros".into(),
                Value::Number(Number::from(campaign.remaining_budget_usd_micros)),
            );
            entry.insert(
                "creatives".into(),
                Value::Array(campaign.creatives.into_iter().map(Value::String).collect()),
            );
            Value::Object(entry)
        })
        .collect();
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
            Value::Object(entry)
        })
        .collect();
    root.insert("cohort_prices".into(), Value::Array(pricing));
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
    let snapshot = handle.snapshot();
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
    let mut oracle_map = Map::new();
    oracle_map.insert(
        "snapshot_ct_price_usd_micros".into(),
        Value::Number(Number::from(snapshot.ct_price_usd_micros)),
    );
    oracle_map.insert(
        "snapshot_it_price_usd_micros".into(),
        Value::Number(Number::from(snapshot.it_price_usd_micros)),
    );
    if let Some(handle) = market {
        let oracle = handle.oracle();
        oracle_map.insert(
            "market_ct_price_usd_micros".into(),
            Value::Number(Number::from(oracle.ct_price_usd_micros)),
        );
        oracle_map.insert(
            "market_it_price_usd_micros".into(),
            Value::Number(Number::from(oracle.it_price_usd_micros)),
        );
        root.insert(
            "distribution".into(),
            distribution_to_value(handle.distribution()),
        );
    }
    root.insert("oracle".into(), Value::Object(oracle_map));
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
