use super::RpcError;
use crate::Blockchain;
use std::sync::{Arc, Mutex};

pub fn status(bc: &Arc<Mutex<Blockchain>>) -> Result<serde_json::Value, RpcError> {
    let j = bc.lock().unwrap().config.jurisdiction.clone();
    Ok(serde_json::json!({"jurisdiction": j}))
}

pub fn set(bc: &Arc<Mutex<Blockchain>>, path: &str) -> Result<serde_json::Value, RpcError> {
    match jurisdiction::PolicyPack::load(path) {
        Ok(pack) => {
            let mut g = bc.lock().unwrap();
            g.config.jurisdiction = Some(pack.region.clone());
            g.save_config();
            let _ = jurisdiction::log_law_enforcement_request(
                "le_jurisdiction.log",
                &format!("rpc set {}", pack.region),
            );
            Ok(serde_json::json!({"status": "ok", "jurisdiction": pack.region}))
        }
        Err(_) => Err(RpcError {
            code: -32070,
            message: "load failed",
        }),
    }
}
