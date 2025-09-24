use super::RpcError;
use crate::{
    bridge::{Bridge, BridgeError},
    simple_db::names,
    SimpleDb,
};
use bridges::{header::PowHeader, light_client::Proof, RelayerBundle, RelayerProof};
use once_cell::sync::Lazy;
use serde_json::json;
use std::sync::Mutex;

static SERVICE: Lazy<Mutex<Bridge>> = Lazy::new(|| {
    let path = std::env::var("TB_BRIDGE_DB_PATH").unwrap_or_else(|_| "state/bridge_db".into());
    let db = SimpleDb::open_named(names::RPC_BRIDGE, &path);
    Mutex::new(Bridge::with_db(db))
});

fn guard() -> Result<std::sync::MutexGuard<'static, Bridge>, RpcError> {
    SERVICE.lock().map_err(|_| RpcError {
        code: -32000,
        message: "bridge busy",
    })
}

fn convert_err(err: BridgeError) -> RpcError {
    let (code, message) = match err {
        BridgeError::InvalidProof => (-32002, "invalid proof"),
        BridgeError::Replay => (-32006, "proof replay"),
        BridgeError::DuplicateWithdrawal => (-32007, "withdrawal already pending"),
        BridgeError::WithdrawalMissing => (-32008, "withdrawal not found"),
        BridgeError::AlreadyChallenged => (-32009, "withdrawal already challenged"),
        BridgeError::ChallengeWindowOpen => (-32010, "challenge window open"),
        BridgeError::UnauthorizedRelease => (-32011, "release not authorized"),
        BridgeError::UnknownChannel(_) => (-32012, "unknown bridge channel"),
        BridgeError::Storage(_) => (-32013, "bridge storage failure"),
    };
    RpcError { code, message }
}

pub fn relayer_status(asset: Option<&str>, relayer: &str) -> serde_json::Value {
    if let Ok(bridge) = SERVICE.lock() {
        if let Some((asset_id, stake, slashes, bond)) = bridge.relayer_status(relayer, asset) {
            return json!({
                "asset": asset_id,
                "stake": stake,
                "slashes": slashes,
                "bond": bond,
            });
        }
    }
    json!({
        "asset": asset.unwrap_or_default(),
        "stake": 0,
        "slashes": 0,
        "bond": 0,
    })
}

pub fn bond_relayer(relayer: &str, amount: u64) -> Result<serde_json::Value, RpcError> {
    let mut bridge = guard()?;
    bridge.bond_relayer(relayer, amount).map_err(convert_err)?;
    Ok(json!({ "status": "ok" }))
}

pub fn verify_deposit(
    asset: &str,
    relayer: &str,
    user: &str,
    amount: u64,
    header: PowHeader,
    proof: Proof,
    proofs: Vec<RelayerProof>,
) -> Result<serde_json::Value, RpcError> {
    if proofs.is_empty() {
        return Err(RpcError {
            code: -32602,
            message: "no relayer proofs",
        });
    }
    let bundle = RelayerBundle::new(proofs);
    let mut bridge = guard()?;
    let receipt = bridge
        .deposit(asset, relayer, user, amount, &header, &proof, &bundle)
        .map_err(convert_err)?;
    Ok(json!({
        "status": "ok",
        "nonce": receipt.nonce,
        "commitment": hex::encode(receipt.relayer_commitment),
        "recorded_at": receipt.recorded_at,
    }))
}

pub fn request_withdrawal(
    asset: &str,
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
    let bundle = RelayerBundle::new(proofs);
    let mut bridge = guard()?;
    let commitment = bridge
        .request_withdrawal(asset, relayer, user, amount, &bundle)
        .map_err(convert_err)?;
    Ok(json!({
        "status": "pending",
        "commitment": hex::encode(commitment),
    }))
}

pub fn challenge_withdrawal(
    asset: &str,
    commitment_hex: &str,
    challenger: &str,
) -> Result<serde_json::Value, RpcError> {
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
    let mut bridge = guard()?;
    let record = bridge
        .challenge_withdrawal(asset, key, challenger)
        .map_err(convert_err)?;
    Ok(json!({
        "status": "challenged",
        "challenger": record.challenger,
        "timestamp": record.challenged_at,
    }))
}

pub fn finalize_withdrawal(
    asset: &str,
    commitment_hex: &str,
) -> Result<serde_json::Value, RpcError> {
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
    let mut bridge = guard()?;
    bridge
        .finalize_withdrawal(asset, key)
        .map_err(convert_err)?;
    Ok(json!({"status": "finalized"}))
}

pub fn pending_withdrawals(asset: Option<&str>) -> Result<serde_json::Value, RpcError> {
    let bridge = guard()?;
    Ok(json!({
        "withdrawals": bridge.pending_withdrawals(asset),
    }))
}

pub fn active_challenges(asset: Option<&str>) -> Result<serde_json::Value, RpcError> {
    let bridge = guard()?;
    let challenges: Vec<_> = bridge
        .challenges(asset)
        .into_iter()
        .map(|c| {
            json!({
                "asset": c.asset,
                "commitment": hex::encode(c.commitment),
                "challenger": c.challenger,
                "timestamp": c.challenged_at,
            })
        })
        .collect();
    Ok(json!({ "challenges": challenges }))
}

pub fn relayer_quorum(asset: &str) -> Result<serde_json::Value, RpcError> {
    let bridge = guard()?;
    bridge.relayer_quorum(asset).ok_or(RpcError {
        code: -32012,
        message: "unknown bridge channel",
    })
}

pub fn deposit_history(
    asset: &str,
    cursor: Option<u64>,
    limit: usize,
) -> Result<serde_json::Value, RpcError> {
    let bridge = guard()?;
    let receipts: Vec<_> = bridge
        .deposit_history(asset, cursor, limit)
        .into_iter()
        .map(|r| {
            json!({
                "asset": r.asset,
                "nonce": r.nonce,
                "user": r.user,
                "amount": r.amount,
                "relayer": r.relayer,
                "header_hash": hex::encode(r.header_hash),
                "commitment": hex::encode(r.relayer_commitment),
                "fingerprint": hex::encode(r.proof_fingerprint),
                "relayers": r.bundle_relayers,
                "recorded_at": r.recorded_at,
            })
        })
        .collect();
    Ok(json!({ "receipts": receipts }))
}

pub fn slash_log() -> Result<serde_json::Value, RpcError> {
    let bridge = guard()?;
    let records: Vec<_> = bridge
        .slash_log()
        .iter()
        .map(|r| {
            json!({
                "relayer": r.relayer,
                "asset": r.asset,
                "slashes": r.slashes,
                "remaining_bond": r.remaining_bond,
                "timestamp": r.occurred_at,
            })
        })
        .collect();
    Ok(json!({ "slashes": records }))
}
