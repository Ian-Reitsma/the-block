#![forbid(unsafe_code)]

use crate::energy;
use crypto_suite::hex;
use energy_market::{EnergyCredit, EnergyProvider, EnergyReceipt, MeterReading, ProviderId, H256};
use foundation_rpc::Params;
use foundation_serialization::json::{Map, Number, Value};

fn error_value(message: impl Into<String>) -> Value {
    let mut map = Map::new();
    map.insert("error".into(), Value::String(message.into()));
    Value::Object(map)
}

fn number(value: u64) -> Value {
    Value::Number(Number::from(value))
}

fn provider_value(provider: &EnergyProvider) -> Value {
    let mut map = Map::new();
    map.insert(
        "provider_id".into(),
        Value::String(provider.provider_id.clone()),
    );
    map.insert("owner".into(), Value::String(provider.owner.clone()));
    map.insert(
        "jurisdiction".into(),
        Value::String(provider.location.clone()),
    );
    map.insert("capacity_kwh".into(), number(provider.capacity_kwh));
    map.insert("available_kwh".into(), number(provider.available_kwh));
    map.insert("price_per_kwh".into(), number(provider.price_per_kwh));
    map.insert(
        "reputation_score".into(),
        Value::Number(Number::from_f64(provider.reputation_score).unwrap_or(Number::from(0u64))),
    );
    map.insert(
        "meter_address".into(),
        Value::String(provider.meter_address.clone()),
    );
    map.insert(
        "total_delivered_kwh".into(),
        number(provider.total_delivered_kwh),
    );
    map.insert("staked_balance".into(), number(provider.staked_balance));
    Value::Object(map)
}

fn receipt_value(receipt: &EnergyReceipt) -> Value {
    let mut map = Map::new();
    map.insert("buyer".into(), Value::String(receipt.buyer.clone()));
    map.insert("seller".into(), Value::String(receipt.seller.clone()));
    map.insert("kwh_delivered".into(), number(receipt.kwh_delivered));
    map.insert("price_paid".into(), number(receipt.price_paid));
    map.insert("block_settled".into(), number(receipt.block_settled));
    map.insert("treasury_fee".into(), number(receipt.treasury_fee));
    map.insert("slash_applied".into(), number(receipt.slash_applied));
    map.insert(
        "meter_hash".into(),
        Value::String(hex::encode(receipt.meter_reading_hash)),
    );
    Value::Object(map)
}

fn credit_value(credit: &EnergyCredit) -> Value {
    let mut map = Map::new();
    map.insert("provider".into(), Value::String(credit.provider.clone()));
    map.insert("amount_kwh".into(), number(credit.amount_kwh));
    map.insert("timestamp".into(), number(credit.timestamp));
    map.insert(
        "meter_hash".into(),
        Value::String(hex::encode(credit.meter_reading_hash)),
    );
    Value::Object(map)
}

fn params_object(params: &Params) -> Result<&Map, Value> {
    params
        .as_map()
        .ok_or_else(|| error_value("parameters must be an object"))
}

fn require_string(params: &Map, key: &str) -> Result<String, Value> {
    params
        .get(key)
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| error_value(format!("missing or invalid '{key}'")))
}

fn require_u64(params: &Map, key: &str) -> Result<u64, Value> {
    params
        .get(key)
        .and_then(|value| value.as_u64())
        .ok_or_else(|| error_value(format!("missing or invalid '{key}'")))
}

fn decode_hash(hex_value: &str) -> Result<H256, Value> {
    let bytes = hex::decode(hex_value)
        .map_err(|_| error_value("meter hash must be hex-encoded 32 bytes"))?;
    if bytes.len() != 32 {
        return Err(error_value("meter hash must be 32 bytes"));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn decode_signature(hex_value: &str) -> Result<Vec<u8>, Value> {
    hex::decode(hex_value).map_err(|_| error_value("signature must be hex encoded"))
}

pub fn register(params: &Params) -> Value {
    let params = match params_object(params) {
        Ok(map) => map,
        Err(err) => return err,
    };
    let capacity = match require_u64(params, "capacity_kwh") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let price = match require_u64(params, "price_per_kwh") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let stake = match require_u64(params, "stake") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let meter_address = match require_string(params, "meter_address") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let jurisdiction = match require_string(params, "jurisdiction") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let owner = require_string(params, "owner").unwrap_or_else(|_| "anonymous".into());
    match energy::register_provider(owner, capacity, price, meter_address, jurisdiction, stake) {
        Ok(provider) => provider_value(&provider),
        Err(err) => error_value(err.to_string()),
    }
}

pub fn market_state(filter_provider: Option<&str>) -> Value {
    let snapshot = energy::market_snapshot();
    let providers: Vec<Value> = snapshot
        .providers
        .into_iter()
        .filter(|provider| {
            filter_provider
                .map(|target| provider.provider_id == target)
                .unwrap_or(true)
        })
        .map(|provider| provider_value(&provider))
        .collect();
    let credits: Vec<Value> = snapshot
        .credits
        .into_iter()
        .filter(|credit| {
            filter_provider
                .map(|target| credit.provider == target)
                .unwrap_or(true)
        })
        .map(|credit| credit_value(&credit))
        .collect();
    let receipts: Vec<Value> = snapshot
        .receipts
        .into_iter()
        .filter(|receipt| {
            filter_provider
                .map(|target| receipt.seller == target)
                .unwrap_or(true)
        })
        .map(|receipt| receipt_value(&receipt))
        .collect();
    let mut map = Map::new();
    map.insert("status".into(), Value::String("ok".into()));
    map.insert("providers".into(), Value::Array(providers));
    map.insert("credits".into(), Value::Array(credits));
    map.insert("receipts".into(), Value::Array(receipts));
    Value::Object(map)
}

pub fn settle(params: &Params, block: u64) -> Value {
    let params = match params_object(params) {
        Ok(map) => map,
        Err(err) => return err,
    };
    let provider_id = match require_string(params, "provider_id") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let buyer = require_string(params, "buyer").unwrap_or_else(|_| "anonymous".into());
    let kwh = match require_u64(params, "kwh_consumed") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let meter_hash = match require_string(params, "meter_hash").and_then(|hash| decode_hash(&hash))
    {
        Ok(hash) => hash,
        Err(err) => return err,
    };
    match energy::settle_energy_delivery(buyer, &provider_id, kwh, block, meter_hash) {
        Ok(receipt) => receipt_value(&receipt),
        Err(err) => error_value(err.to_string()),
    }
}

pub fn submit_reading(params: &Params, block: u64) -> Value {
    let params = match params_object(params) {
        Ok(map) => map,
        Err(err) => return err,
    };
    let provider_id = match require_string(params, "provider_id") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let meter_address = match require_string(params, "meter_address") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let kwh_reading = match require_u64(params, "kwh_reading") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let timestamp = match require_u64(params, "timestamp") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let signature = match require_string(params, "signature").and_then(|sig| decode_signature(&sig))
    {
        Ok(value) => value,
        Err(err) => return err,
    };
    let reading = MeterReading {
        provider_id,
        meter_address,
        total_kwh: kwh_reading,
        timestamp,
        signature,
    };
    match energy::submit_meter_reading(reading, block) {
        Ok(credit) => credit_value(&credit),
        Err(err) => error_value(err.to_string()),
    }
}
