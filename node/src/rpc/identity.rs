use crate::governance::GovStore;
use crate::identity::{
    handle_registry::{HandleError, HandleRegistry},
    DidError, DidRecord, DidRegistry,
};
use crate::transaction::TxDidAnchor;
use serde_json::Value;

pub fn register_handle(params: &Value, reg: &mut HandleRegistry) -> Result<Value, HandleError> {
    let handle = params
        .get("handle")
        .and_then(|v| v.as_str())
        .ok_or(HandleError::Reserved)?;
    let pubkey = params
        .get("pubkey")
        .and_then(|v| v.as_str())
        .ok_or(HandleError::BadSig)?;
    let sig = params
        .get("sig")
        .and_then(|v| v.as_str())
        .ok_or(HandleError::BadSig)?;
    #[cfg(feature = "pq-crypto")]
    let pq_pubkey = params.get("pq_pubkey").and_then(|v| v.as_str());
    let nonce = params
        .get("nonce")
        .and_then(|v| v.as_u64())
        .ok_or(HandleError::LowNonce)?;
    let pk_bytes = hex::decode(pubkey).map_err(|_| HandleError::BadSig)?;
    let sig_bytes = hex::decode(sig).map_err(|_| HandleError::BadSig)?;
    #[cfg(feature = "pq-crypto")]
    let pq_bytes = pq_pubkey.map(|s| hex::decode(s).ok()).flatten();
    #[cfg(feature = "pq-crypto")]
    let addr = reg.register_handle(handle, &pk_bytes, pq_bytes.as_deref(), &sig_bytes, nonce)?;
    #[cfg(not(feature = "pq-crypto"))]
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

fn did_record_json(record: DidRecord) -> Value {
    serde_json::json!({
        "address": record.address,
        "document": record.document,
        "hash": hex::encode(record.hash),
        "nonce": record.nonce,
        "updated_at": record.updated_at,
        "public_key": hex::encode(record.public_key),
        "remote_attestation": record.remote_attestation.map(|att| {
            serde_json::json!({"signer": att.signer, "signature": att.signature})
        }),
    })
}

pub fn anchor_did(
    params: &Value,
    reg: &mut DidRegistry,
    gov: &GovStore,
) -> Result<Value, DidError> {
    let tx: TxDidAnchor =
        serde_json::from_value(params.clone()).map_err(|_| DidError::InvalidRequest)?;
    let record = reg.anchor(&tx, Some(gov))?;
    Ok(did_record_json(record))
}

pub fn resolve_did(params: &Value, reg: &DidRegistry) -> Value {
    let address = params.get("address").and_then(|v| v.as_str()).unwrap_or("");
    match reg.resolve(address) {
        Some(record) => did_record_json(record),
        None => serde_json::json!({
            "address": address,
            "document": Value::Null,
            "hash": Value::Null,
            "nonce": Value::Null,
            "updated_at": Value::Null,
        }),
    }
}
