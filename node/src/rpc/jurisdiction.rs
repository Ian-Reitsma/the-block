#![deny(warnings)]

use super::RpcError;
use crate::Blockchain;
use std::sync::{Arc, Mutex};

pub fn status(bc: &Arc<Mutex<Blockchain>>) -> Result<serde_json::Value, RpcError> {
    let j = bc.lock().unwrap().config.jurisdiction.clone();
    if let Some(ref region) = j {
        if let Some(pack) = jurisdiction::PolicyPack::template(region) {
            return Ok(serde_json::json!({
                "jurisdiction": pack.region,
                "consent_required": pack.consent_required,
                "features": pack.features,
            }));
        }
    }
    Ok(serde_json::json!({"jurisdiction": j}))
}

pub fn set(bc: &Arc<Mutex<Blockchain>>, path: &str) -> Result<serde_json::Value, RpcError> {
    let pack_res = if path.starts_with("http") {
        // placeholder: use zero key for demo
        let pk = ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap();
        jurisdiction::fetch_signed(path, &pk)
    } else if std::path::Path::new(path).exists() {
        jurisdiction::PolicyPack::load(path)
    } else {
        jurisdiction::PolicyPack::template(path)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "template"))
    };
    match pack_res {
        Ok(pack) => {
            let mut g = bc.lock().unwrap();
            g.config.jurisdiction = Some(pack.region.clone());
            g.save_config();
            let _ = jurisdiction::log_law_enforcement_request(
                "le_jurisdiction.log",
                &format!("rpc set {}", pack.region),
            );
            Ok(serde_json::json!({
                "status": "ok",
                "jurisdiction": pack.region,
            }))
        }
        Err(_) => Err(RpcError {
            code: -32070,
            message: "load failed",
        }),
    }
}

pub fn policy_diff(bc: &Arc<Mutex<Blockchain>>, path: &str) -> Result<serde_json::Value, RpcError> {
    let current = {
        let g = bc.lock().unwrap();
        g.config
            .jurisdiction
            .as_ref()
            .and_then(|r| jurisdiction::PolicyPack::template(r))
            .unwrap_or(jurisdiction::PolicyPack {
                region: "".into(),
                consent_required: false,
                features: vec![],
                parent: None,
            })
            .resolve()
    };
    let new_pack = if std::path::Path::new(path).exists() {
        jurisdiction::PolicyPack::load(path)
    } else {
        jurisdiction::PolicyPack::template(path)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "template"))
    }?
    .resolve();
    Ok(jurisdiction::PolicyPack::diff(&current, &new_pack))
}
