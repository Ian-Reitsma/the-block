#![deny(warnings)]

use super::RpcError;
use crate::Blockchain;
use std::io::ErrorKind;
use std::sync::{Arc, Mutex};

use foundation_serialization::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde", untagged)]
pub enum JurisdictionStatusResponse {
    Detailed {
        jurisdiction: String,
        consent_required: bool,
        features: Vec<String>,
    },
    Summary {
        #[serde(skip_serializing_if = "Option::is_none")]
        jurisdiction: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct JurisdictionSetResponse {
    pub status: &'static str,
    pub jurisdiction: String,
}

pub fn status(bc: &Arc<Mutex<Blockchain>>) -> Result<JurisdictionStatusResponse, RpcError> {
    let j = bc.lock().unwrap().config.jurisdiction.clone();
    if let Some(ref region) = j {
        if let Some(pack) = jurisdiction::PolicyPack::template(region) {
            return Ok(JurisdictionStatusResponse::Detailed {
                jurisdiction: pack.region,
                consent_required: pack.consent_required,
                features: pack.features,
            });
        }
    }
    Ok(JurisdictionStatusResponse::Summary { jurisdiction: j })
}

pub fn set(bc: &Arc<Mutex<Blockchain>>, path: &str) -> Result<JurisdictionSetResponse, RpcError> {
    let pack_res: Result<_, RpcError> = if path.starts_with("http") {
        let registry = jurisdiction::KeyRegistry::load_default()
            .map_err(|_| RpcError::new(-32071, "jurisdiction registry missing or unreadable"))?;
        jurisdiction::fetch_signed(path, &registry).map_err(|err| {
            let code = if err.kind() == ErrorKind::PermissionDenied {
                -32072
            } else {
                -32070
            };
            RpcError::new(code, "jurisdiction signature rejected")
        })
    } else if std::path::Path::new(path).exists() {
        jurisdiction::PolicyPack::load(path).map_err(|_| RpcError::new(-32070, "load failed"))
    } else {
        jurisdiction::PolicyPack::template(path).ok_or_else(|| RpcError::new(-32070, "load failed"))
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
            Ok(JurisdictionSetResponse {
                status: "ok",
                jurisdiction: pack.region,
            })
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
