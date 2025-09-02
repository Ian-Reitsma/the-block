use std::sync::Mutex;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
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

fn get_role(params: &Value) -> String {
    params
        .get("role")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "validator".to_string())
}

fn get_sig(params: &Value) -> Result<Vec<u8>, RpcError> {
    let sig_hex = params.get("sig").and_then(|v| v.as_str()).ok_or(RpcError {
        code: -32602,
        message: "missing sig",
    })?;
    hex::decode(sig_hex).map_err(|_| RpcError {
        code: -32602,
        message: "invalid sig",
    })
}

fn verify(action: &str, id: &str, role: &str, amount: u64, sig: &[u8]) -> Result<(), RpcError> {
    let pk_bytes = hex::decode(id).map_err(|_| RpcError {
        code: -32602,
        message: "invalid id",
    })?;
    let pk = VerifyingKey::from_bytes(&pk_bytes.try_into().map_err(|_| RpcError {
        code: -32602,
        message: "invalid id",
    })?)
    .map_err(|_| RpcError {
        code: -32602,
        message: "invalid id",
    })?;
    let msg = format!("{action}:{role}:{amount}");
    let sig = Signature::from_bytes(&sig.try_into().map_err(|_| RpcError {
        code: -32602,
        message: "invalid sig",
    })?);
    pk.verify(msg.as_bytes(), &sig).map_err(|_| RpcError {
        code: -32602,
        message: "bad signature",
    })
}

pub fn register(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let mut pos = POS_STATE.lock().unwrap_or_else(|e| e.into_inner());
    pos.register(id);
    Ok(serde_json::json!({"status": "ok"}))
}

pub fn bond(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let role = get_role(params);
    let amount = get_amount(params)?;
    let sig = get_sig(params)?;
    verify("bond", &id, &role, amount, &sig)?;
    let mut pos = POS_STATE.lock().unwrap_or_else(|e| e.into_inner());
    pos.bond(&id, &role, amount);
    Ok(serde_json::json!({"stake": pos.stake_of(&id, &role)}))
}

pub fn unbond(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let role = get_role(params);
    let amount = get_amount(params)?;
    let sig = get_sig(params)?;
    verify("unbond", &id, &role, amount, &sig)?;
    let mut pos = POS_STATE.lock().unwrap_or_else(|e| e.into_inner());
    pos.unbond(&id, &role, amount);
    Ok(serde_json::json!({"stake": pos.stake_of(&id, &role)}))
}

pub fn slash(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let role = get_role(params);
    let amount = get_amount(params)?;
    let mut pos = POS_STATE.lock().unwrap_or_else(|e| e.into_inner());
    pos.slash(&id, &role, amount);
    Ok(serde_json::json!({"stake": pos.stake_of(&id, &role)}))
}

/// Expose for tests.
pub fn state() -> &'static Mutex<PosState> {
    &POS_STATE
}

pub fn role(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let role = get_role(params);
    let pos = POS_STATE.lock().unwrap_or_else(|e| e.into_inner());
    Ok(serde_json::json!({"id": id, "role": role, "stake": pos.stake_of(&id, &role)}))
}
