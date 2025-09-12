use super::RpcError;
use bridges::{
    header::PowHeader,
    light_client::{Header, Proof},
    relayer::RelayerSet,
    Bridge, RelayerProof,
};
use once_cell::sync::Lazy;
use serde_json::json;
use std::sync::Mutex;

static BRIDGE: Lazy<Mutex<Bridge>> = Lazy::new(|| Mutex::new(Bridge::default()));
static RELAYERS: Lazy<Mutex<RelayerSet>> = Lazy::new(|| Mutex::new(RelayerSet::default()));

pub fn relayer_status(id: &str) -> serde_json::Value {
    let guard = RELAYERS.lock().unwrap();
    if let Some(r) = guard.status(id) {
        json!({"stake": r.stake, "slashes": r.slashes})
    } else {
        json!({"stake": 0, "slashes": 0})
    }
}

pub fn verify_deposit(
    relayer: &str,
    user: &str,
    amount: u64,
    header: PowHeader,
    proof: Proof,
    rproof: RelayerProof,
) -> Result<serde_json::Value, RpcError> {
    let mut b = BRIDGE.lock().map_err(|_| RpcError {
        code: -32000,
        message: "bridge busy",
    })?;
    let mut rs = RELAYERS.lock().map_err(|_| RpcError {
        code: -32001,
        message: "relayer busy",
    })?;
    if !rs.status(relayer).is_some() {
        rs.stake(relayer, 0);
    }
    if b.deposit_with_relayer(&mut rs, relayer, user, amount, &header, &proof, &rproof) {
        Ok(json!({"status": "ok"}))
    } else {
        Err(RpcError {
            code: -32002,
            message: "invalid proof",
        })
    }
}
