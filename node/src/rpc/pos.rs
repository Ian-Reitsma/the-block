use std::collections::HashSet;
use std::sync::Mutex;

use concurrency::Lazy;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey, SIGNATURE_LENGTH};
use foundation_serialization::json::Value;

use crate::consensus::pos::PosState;

use super::RpcError;

static POS_STATE: Lazy<Mutex<PosState>> = Lazy::new(|| Mutex::new(PosState::default()));

struct SignerPayload {
    approvals: Vec<(VerifyingKey, Signature)>,
    threshold: usize,
}

fn get_id(params: &Value) -> Result<String, RpcError> {
    params
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| RpcError::new(-32602, "missing id"))
}

fn get_amount(params: &Value) -> Result<u64, RpcError> {
    params
        .get("amount")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::new(-32602, "missing amount"))
}

fn get_role(params: &Value) -> String {
    params
        .get("role")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "validator".to_string())
}

fn get_sig(params: &Value) -> Result<Vec<u8>, RpcError> {
    let sig_hex = params
        .get("sig")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::new(-32602, "missing sig"))?;
    hex::decode(sig_hex).map_err(|_| RpcError::new(-32602, "invalid sig"))
}

fn parse_key(hex: &str, err: &'static str) -> Result<VerifyingKey, RpcError> {
    let bytes = hex::decode(hex).map_err(|_| RpcError::new(-32602, err))?;
    let raw: [u8; 32] = bytes.try_into().map_err(|_| RpcError::new(-32602, err))?;
    VerifyingKey::from_bytes(&raw).map_err(|_| RpcError::new(-32602, err))
}

fn parse_signers(params: &Value, id_key: &VerifyingKey) -> Result<SignerPayload, RpcError> {
    if let Some(signers) = params.get("signers") {
        let entries = signers
            .as_array()
            .ok_or_else(|| RpcError::new(-32602, "invalid signers"))?;
        if entries.is_empty() {
            return Err(RpcError::new(-32602, "missing signers"));
        }
        let mut approvals = Vec::with_capacity(entries.len());
        for entry in entries {
            let pk_hex = entry
                .get("pk")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::new(-32602, "invalid signers"))?;
            let sig_hex = entry
                .get("sig")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::new(-32602, "invalid signers"))?;
            let pk = parse_key(pk_hex, "invalid signer pk")?;
            let sig_bytes =
                hex::decode(sig_hex).map_err(|_| RpcError::new(-32602, "invalid sig"))?;
            let arr: [u8; SIGNATURE_LENGTH] = sig_bytes
                .as_slice()
                .try_into()
                .map_err(|_| RpcError::new(-32602, "invalid sig"))?;
            let sig = Signature::from_bytes(&arr);
            approvals.push((pk, sig));
        }
        let threshold = params
            .get("threshold")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or_else(|| approvals.len());
        if threshold == 0 || threshold > approvals.len() {
            return Err(RpcError::new(-32602, "invalid threshold"));
        }
        if !approvals.iter().any(|(pk, _)| pk == id_key) {
            return Err(RpcError::new(-32602, "id not authorized"));
        }
        Ok(SignerPayload {
            approvals,
            threshold,
        })
    } else {
        let sig_bytes = get_sig(params)?;
        if sig_bytes.len() != SIGNATURE_LENGTH {
            return Err(RpcError::new(-32602, "invalid sig"));
        }
        let arr: [u8; SIGNATURE_LENGTH] = sig_bytes
            .as_slice()
            .try_into()
            .map_err(|_| RpcError::new(-32602, "invalid sig"))?;
        let sig = Signature::from_bytes(&arr);
        Ok(SignerPayload {
            approvals: vec![(id_key.clone(), sig)],
            threshold: 1,
        })
    }
}

fn verify(action: &str, role: &str, amount: u64, payload: &SignerPayload) -> Result<(), RpcError> {
    let msg = format!("{action}:{role}:{amount}");
    let mut seen = HashSet::new();
    let mut valid = 0usize;
    for (pk, sig) in &payload.approvals {
        let key_bytes = pk.to_bytes();
        if !seen.insert(key_bytes) {
            continue;
        }
        if pk.verify(msg.as_bytes(), sig).is_ok() {
            valid += 1;
            if valid >= payload.threshold {
                return Ok(());
            }
        }
    }
    Err(RpcError::new(-32602, "bad signature"))
}

pub fn register(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let mut pos = POS_STATE.lock().unwrap_or_else(|e| e.into_inner());
    pos.register(id);
    Ok(foundation_serialization::json!({"status": "ok"}))
}

pub fn bond(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let role = get_role(params);
    let amount = get_amount(params)?;
    let id_key = parse_key(&id, "invalid id")?;
    let payload = parse_signers(params, &id_key)?;
    verify("bond", &role, amount, &payload)?;
    let mut pos = POS_STATE.lock().unwrap_or_else(|e| e.into_inner());
    pos.bond(&id, &role, amount);
    Ok(foundation_serialization::json!({"stake": pos.stake_of(&id, &role)}))
}

pub fn unbond(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let role = get_role(params);
    let amount = get_amount(params)?;
    let id_key = parse_key(&id, "invalid id")?;
    let payload = parse_signers(params, &id_key)?;
    verify("unbond", &role, amount, &payload)?;
    let mut pos = POS_STATE.lock().unwrap_or_else(|e| e.into_inner());
    pos.unbond(&id, &role, amount);
    Ok(foundation_serialization::json!({"stake": pos.stake_of(&id, &role)}))
}

pub fn slash(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let role = get_role(params);
    let amount = get_amount(params)?;
    let mut pos = POS_STATE.lock().unwrap_or_else(|e| e.into_inner());
    pos.slash(&id, &role, amount);
    Ok(foundation_serialization::json!({"stake": pos.stake_of(&id, &role)}))
}

/// Expose for tests.
pub fn state() -> &'static Mutex<PosState> {
    &POS_STATE
}

pub fn role(params: &Value) -> Result<Value, RpcError> {
    let id = get_id(params)?;
    let role = get_role(params);
    let pos = POS_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let stake = pos.stake_of(&id, &role);
    Ok(foundation_serialization::json!({
        "id": id.clone(),
        "role": role.clone(),
        "stake": stake,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_suite::signatures::{ed25519::SigningKey, Signer};

    fn reset_state() {
        let mut state = POS_STATE.lock().unwrap();
        *state = PosState::default();
    }

    fn signing_key(byte: u8) -> SigningKey {
        SigningKey::from_bytes(&[byte; 32])
    }

    #[test]
    fn legacy_single_signature_still_works() {
        reset_state();
        let sk = signing_key(7);
        let pk = sk.verifying_key();
        let role = "validator";
        let amount = 10u64;
        let msg = format!("bond:{role}:{amount}");
        let sig = sk.sign(msg.as_bytes());
        let params = foundation_serialization::json!({
            "id": hex::encode(pk.to_bytes()),
            "role": role,
            "amount": amount,
            "sig": hex::encode(sig.to_bytes()),
        });
        let res = bond(&params).expect("bond succeeds");
        assert_eq!(res["stake"].as_u64().unwrap(), amount);
    }

    #[test]
    fn multisig_threshold_enforced() {
        reset_state();
        let signer_a = signing_key(1);
        let signer_b = signing_key(2);
        let signer_c = signing_key(3);
        let amount = 42u64;
        let role = "gateway";
        let msg = format!("bond:{role}:{amount}");
        let approvals = [
            (&signer_a, signer_a.sign(msg.as_bytes())),
            (&signer_b, signer_b.sign(msg.as_bytes())),
            (&signer_c, signer_c.sign(b"other")),
        ];
        let params = foundation_serialization::json!({
            "id": hex::encode(signer_a.verifying_key().to_bytes()),
            "role": role,
            "amount": amount,
            "threshold": 2,
            "signers": [
                {"pk": hex::encode(approvals[0].0.verifying_key().to_bytes()), "sig": hex::encode(approvals[0].1.to_bytes())},
                {"pk": hex::encode(approvals[1].0.verifying_key().to_bytes()), "sig": hex::encode(approvals[1].1.to_bytes())},
                {"pk": hex::encode(approvals[2].0.verifying_key().to_bytes()), "sig": hex::encode(approvals[2].1.to_bytes())},
            ],
        });
        let res = bond(&params).expect("multisig bond");
        assert_eq!(res["stake"].as_u64().unwrap(), amount);

        // Fails when fewer than threshold signatures are valid.
        reset_state();
        let bad_params = foundation_serialization::json!({
            "id": hex::encode(signer_a.verifying_key().to_bytes()),
            "role": role,
            "amount": amount,
            "threshold": 2,
            "signers": [
                {"pk": hex::encode(signer_a.verifying_key().to_bytes()), "sig": hex::encode(approvals[0].1.to_bytes())},
                {"pk": hex::encode(signer_b.verifying_key().to_bytes()), "sig": "00"},
            ],
        });
        assert!(matches!(bond(&bad_params), Err(_)));
    }
}
