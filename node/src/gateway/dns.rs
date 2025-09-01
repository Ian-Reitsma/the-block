use crate::simple_db::SimpleDb;
use crate::ERR_DNS_SIG_INVALID;
use ed25519_dalek::{Signature, Verifier, VerifyingKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH};
use hex;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::convert::TryInto;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

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
    let pk: [u8; PUBLIC_KEY_LENGTH] = pk_vec
        .as_slice()
        .try_into()
        .map_err(|_| DnsError::SigInvalid)?;
    let sig_bytes: [u8; SIGNATURE_LENGTH] = sig_vec
        .as_slice()
        .try_into()
        .map_err(|_| DnsError::SigInvalid)?;
    let vk = VerifyingKey::from_bytes(&pk).map_err(|_| DnsError::SigInvalid)?;
    let sig = Signature::from_bytes(&sig_bytes);
    let mut msg = Vec::new();
    msg.extend(domain.as_bytes());
    msg.extend(txt.as_bytes());
    vk.verify(&msg, &sig).map_err(|_| DnsError::SigInvalid)?;
    let mut db = DNS_DB.lock().unwrap();
    db.insert(&format!("dns_records/{}", domain), txt.as_bytes().to_vec());
    db.insert(
        &format!("dns_reads/{}", domain),
        0u64.to_le_bytes().to_vec(),
    );
    db.insert(&format!("dns_last/{}", domain), 0u64.to_le_bytes().to_vec());
    Ok(serde_json::json!({"status":"ok"}))
}

pub fn gateway_policy(params: &Value) -> serde_json::Value {
    let domain = params.get("domain").and_then(|v| v.as_str()).unwrap_or("");
    let key = format!("dns_records/{}", domain);
    let mut db = DNS_DB.lock().unwrap();
    if let Some(bytes) = db.get(&key) {
        if let Ok(txt) = String::from_utf8(bytes) {
            let reads_key = format!("dns_reads/{}", domain);
            let last_key = format!("dns_last/{}", domain);
            let mut reads = db
                .get(&reads_key)
                .map(|v| u64::from_le_bytes(v.as_slice().try_into().unwrap_or([0; 8])))
                .unwrap_or(0);
            reads += 1;
            db.insert(&reads_key, reads.to_le_bytes().to_vec());
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            db.insert(&last_key, ts.to_le_bytes().to_vec());
            return serde_json::json!({
                "record": txt,
                "reads_total": reads,
                "last_access_ts": ts,
            });
        }
    }
    serde_json::json!({
        "record": null,
        "reads_total": 0,
        "last_access_ts": 0,
    })
}
