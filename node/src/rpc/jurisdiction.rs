#![deny(warnings)]

use super::RpcError;
use crate::Blockchain;
use crypto_suite::signatures::ed25519::VerifyingKey;
use std::sync::{Arc, Mutex};

pub fn status(
    bc: &Arc<Mutex<Blockchain>>,
) -> Result<foundation_serialization::json::Value, RpcError> {
    let j = bc.lock().unwrap().config.jurisdiction.clone();
    if let Some(ref region) = j {
        if let Some(pack) = jurisdiction::PolicyPack::template(region) {
            return Ok(foundation_serialization::json!({
                "jurisdiction": pack.region,
                "consent_required": pack.consent_required,
                "features": pack.features,
            }));
        }
    }
    Ok(foundation_serialization::json!({"jurisdiction": j}))
}

pub fn set(
    bc: &Arc<Mutex<Blockchain>>,
    path: &str,
) -> Result<foundation_serialization::json::Value, RpcError> {
    let pack_res = if path.starts_with("http") {
        // placeholder: use zero key for demo
        let pk = VerifyingKey::from_bytes(&[0u8; 32]).unwrap();
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
            Ok(foundation_serialization::json!({
                "status": "ok",
                "jurisdiction": pack.region,
            }))
        }
        Err(_) => Err(RpcError::new(-32070, "load failed")),
    }
}

pub fn policy_diff(
    bc: &Arc<Mutex<Blockchain>>,
    path: &str,
) -> Result<foundation_serialization::json::Value, RpcError> {
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
    let load_error = || RpcError::new(-32070, "load failed");
    let new_pack = if std::path::Path::new(path).exists() {
        jurisdiction::PolicyPack::load(path).map_err(|_| load_error())
    } else {
        jurisdiction::PolicyPack::template(path).ok_or_else(load_error)
    }?
    .resolve();
    let diff = jurisdiction::PolicyPack::diff(&current, &new_pack);
    Ok(diff.to_json_value())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::tempfile::tempdir;

    #[test]
    fn policy_diff_reports_missing_pack() {
        let dir = tempdir().unwrap();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let err = policy_diff(&bc, "definitely-missing-template").unwrap_err();
        assert_eq!(err.code, -32070);
        assert_eq!(err.message(), "load failed");
    }
}
