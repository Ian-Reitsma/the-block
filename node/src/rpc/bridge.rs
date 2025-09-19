use super::RpcError;
use bridges::{
    header::PowHeader, light_client::Proof, relayer::RelayerSet, Bridge, RelayerBundle,
    RelayerProof,
};
use hex;
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
    proofs: Vec<RelayerProof>,
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
    if proofs.is_empty() {
        return Err(RpcError {
            code: -32602,
            message: "no relayer proofs",
        });
    }
    let bundle = RelayerBundle::new(proofs);
    if b.deposit_with_relayer(&mut rs, relayer, user, amount, &header, &proof, &bundle) {
        Ok(json!({"status": "ok"}))
    } else {
        Err(RpcError {
            code: -32002,
            message: "invalid proof",
        })
    }
}

pub fn request_withdrawal(
    relayer: &str,
    user: &str,
    amount: u64,
    proofs: Vec<RelayerProof>,
) -> Result<serde_json::Value, RpcError> {
    if proofs.is_empty() {
        return Err(RpcError {
            code: -32602,
            message: "no relayer proofs",
        });
    }
    let mut b = BRIDGE.lock().map_err(|_| RpcError {
        code: -32000,
        message: "bridge busy",
    })?;
    let mut rs = RELAYERS.lock().map_err(|_| RpcError {
        code: -32001,
        message: "relayer busy",
    })?;
    let bundle = RelayerBundle::new(proofs);
    let commitment = bundle.aggregate_commitment(user, amount);
    if b.unlock_with_relayer(&mut rs, relayer, user, amount, &bundle) {
        Ok(json!({
            "status": "pending",
            "commitment": hex::encode(commitment),
        }))
    } else {
        Err(RpcError {
            code: -32003,
            message: "withdrawal rejected",
        })
    }
}

pub fn challenge_withdrawal(commitment_hex: &str) -> Result<serde_json::Value, RpcError> {
    let bytes = hex::decode(commitment_hex).map_err(|_| RpcError {
        code: -32602,
        message: "invalid commitment",
    })?;
    if bytes.len() != 32 {
        return Err(RpcError {
            code: -32602,
            message: "invalid commitment",
        });
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    let mut b = BRIDGE.lock().map_err(|_| RpcError {
        code: -32000,
        message: "bridge busy",
    })?;
    let mut rs = RELAYERS.lock().map_err(|_| RpcError {
        code: -32001,
        message: "relayer busy",
    })?;
    if b.challenge_withdrawal(&mut rs, key) {
        Ok(json!({"status": "challenged"}))
    } else {
        Err(RpcError {
            code: -32004,
            message: "no matching withdrawal",
        })
    }
}

pub fn finalize_withdrawal(commitment_hex: &str) -> Result<serde_json::Value, RpcError> {
    let bytes = hex::decode(commitment_hex).map_err(|_| RpcError {
        code: -32602,
        message: "invalid commitment",
    })?;
    if bytes.len() != 32 {
        return Err(RpcError {
            code: -32602,
            message: "invalid commitment",
        });
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    let mut b = BRIDGE.lock().map_err(|_| RpcError {
        code: -32000,
        message: "bridge busy",
    })?;
    if b.finalize_withdrawal(key) {
        Ok(json!({"status": "finalized"}))
    } else {
        Err(RpcError {
            code: -32005,
            message: "not ready",
        })
    }
}
