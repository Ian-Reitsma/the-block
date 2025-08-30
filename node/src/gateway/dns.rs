use crate::simple_db::SimpleDb;
use crate::ERR_DNS_SIG_INVALID;
use ed25519_dalek::{Signature, Verifier, VerifyingKey, SIGNATURE_LENGTH, PUBLIC_KEY_LENGTH};
use once_cell::sync::Lazy;
use serde_json::Value;
use std::convert::TryInto;
use std::sync::Mutex;
use hex;

static DNS_DB: Lazy<Mutex<SimpleDb>> = Lazy::new(|| {
    let path = std::env::var("TB_DNS_DB_PATH").unwrap_or_else(|_| "dns_db".into());
    Mutex::new(SimpleDb::open(&path))
});

pub enum DnsError {
    SigInvalid,
}

impl DnsError {
    pub fn code(&self) -> i32 {
        -(ERR_DNS_SIG_INVALID as i32)
    }
    pub fn message(&self) -> &'static str {
        "ERR_DNS_SIG_INVALID"
    }
}

pub fn publish_record(params: &Value) -> Result<serde_json::Value, DnsError> {
    let domain = params.get("domain").and_then(|v| v.as_str()).unwrap_or("");
    let txt = params.get("txt").and_then(|v| v.as_str()).unwrap_or("");
    let pk_hex = params.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");
    let sig_hex = params.get("sig").and_then(|v| v.as_str()).unwrap_or("");
    let pk_vec = hex::decode(pk_hex).ok().ok_or(DnsError::SigInvalid)?;
    let sig_vec = hex::decode(sig_hex).ok().ok_or(DnsError::SigInvalid)?;
    let pk: [u8; PUBLIC_KEY_LENGTH] = pk_vec.as_slice().try_into().map_err(|_| DnsError::SigInvalid)?;
    let sig_bytes: [u8; SIGNATURE_LENGTH] = sig_vec.as_slice().try_into().map_err(|_| DnsError::SigInvalid)?;
    let vk = VerifyingKey::from_bytes(&pk).map_err(|_| DnsError::SigInvalid)?;
    let sig = Signature::from_bytes(&sig_bytes);
    let mut msg = Vec::new();
    msg.extend(domain.as_bytes());
    msg.extend(txt.as_bytes());
    vk.verify(&msg, &sig).map_err(|_| DnsError::SigInvalid)?;
    DNS_DB
        .lock()
        .unwrap()
        .insert(&format!("dns_records/{}", domain), txt.as_bytes().to_vec());
    Ok(serde_json::json!({"status":"ok"}))
}

pub fn gateway_policy(params: &Value) -> serde_json::Value {
    let domain = params.get("domain").and_then(|v| v.as_str()).unwrap_or("");
    let key = format!("dns_records/{}", domain);
    let db = DNS_DB.lock().unwrap();
    if let Some(bytes) = db.get(&key) {
        if let Ok(txt) = String::from_utf8(bytes) {
            return serde_json::json!({"record": txt});
        }
    }
    serde_json::json!({"record": null})
}
