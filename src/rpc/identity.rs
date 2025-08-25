use crate::identity::handle_registry::{HandleRegistry, HandleError};
use serde_json::Value;

pub fn register_handle(params: &Value, reg: &mut HandleRegistry) -> Result<Value, HandleError> {
    let handle = params.get("handle").and_then(|v| v.as_str()).ok_or(HandleError::Reserved)?;
    let pubkey = params.get("pubkey").and_then(|v| v.as_str()).ok_or(HandleError::BadSig)?;
    let sig = params.get("sig").and_then(|v| v.as_str()).ok_or(HandleError::BadSig)?;
    let nonce = params.get("nonce").and_then(|v| v.as_u64()).ok_or(HandleError::LowNonce)?;
    let pk_bytes = hex::decode(pubkey).map_err(|_| HandleError::BadSig)?;
    let sig_bytes = hex::decode(sig).map_err(|_| HandleError::BadSig)?;
    let addr = reg.register_handle(handle, &pk_bytes, &sig_bytes, nonce)?;
    Ok(serde_json::json!({"address": addr}))
}

pub fn resolve_handle(params: &Value, reg: &HandleRegistry) -> Value {
    let handle = params.get("handle").and_then(|v| v.as_str()).unwrap_or("");
    let addr = reg.resolve_handle(handle);
    serde_json::json!({"address": addr})
}

pub fn whoami(params: &Value, reg: &HandleRegistry) -> Value {
    let addr = params.get("address").and_then(|v| v.as_str()).unwrap_or("");
    let handle = reg.handle_of(addr);
    serde_json::json!({"address": addr, "handle": handle})
}
