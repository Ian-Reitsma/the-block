use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde_json::Value;

use crate::consensus::pos::PosState;

use super::RpcError;

static POS_STATE: Lazy<Mutex<PosState>> = Lazy::new(|| Mutex::new(PosState::default()));

fn get_id(params: &Value) -> Result<String, RpcError> {
    params
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or(RpcError {
            code: -32602,
            message: "missing id",
        })
}

fn get_amount(params: &Value) -> Result<u64, RpcError> {
    params
        .get("amount")
        .and_then(|v| v.as_u64())
        .ok_or(RpcError {
            code: -32602,
            message: "missing amount",
        })
}

pub fn register(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let mut pos = POS_STATE.lock().unwrap();
    pos.register(id);
    Ok(serde_json::json!({"status": "ok"}))
}

pub fn bond(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let amount = get_amount(params)?;
    let mut pos = POS_STATE.lock().unwrap();
    pos.bond(&id, amount);
    Ok(serde_json::json!({"stake": pos.stake_of(&id)}))
}

pub fn unbond(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let amount = get_amount(params)?;
    let mut pos = POS_STATE.lock().unwrap();
    pos.unbond(&id, amount);
    Ok(serde_json::json!({"stake": pos.stake_of(&id)}))
}

pub fn slash(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let amount = get_amount(params)?;
    let mut pos = POS_STATE.lock().unwrap();
    pos.slash(&id, amount);
    Ok(serde_json::json!({"stake": pos.stake_of(&id)}))
}

/// Expose for tests.
pub fn state() -> &'static Mutex<PosState> {
    &POS_STATE
}
