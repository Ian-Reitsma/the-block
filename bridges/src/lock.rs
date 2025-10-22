use crate::header::{verify_pow, PowHeader};
use crate::light_client::{header_hash, verify, Header};
use crate::Bridge;
#[cfg(feature = "telemetry")]
use crate::BRIDGE_INVALID_PROOF_TOTAL;
use crate::{light_client::Proof, relayer::RelayerSet, RelayerBundle};
use foundation_serialization::json;

pub fn lock(
    bridge: &mut Bridge,
    relayers: &mut RelayerSet,
    relayer: &str,
    user: &str,
    amount: u64,
    header: &PowHeader,
    proof: &Proof,
    bundle: &RelayerBundle,
) -> bool {
    relayers.mark_duty_assignment(relayer);
    let (valid, invalid) = bundle.verify(user, amount);
    for rel in invalid {
        relayers.slash(&rel, 1);
    }
    let has_primary = bundle.relayer_ids().iter().any(|id| id == relayer);
    let pow_ok = verify_pow(header);
    if valid < bridge.cfg.relayer_quorum || !has_primary || !pow_ok {
        #[cfg(feature = "telemetry")]
        {
            BRIDGE_INVALID_PROOF_TOTAL.inc();
        }
        relayers.slash(relayer, amount.min(1));
        relayers.mark_duty_failure(relayer);
        return false;
    }
    let h = Header {
        chain_id: header.chain_id.clone(),
        height: header.height,
        merkle_root: header.merkle_root,
        signature: header.signature,
    };
    if !verify(&h, proof) {
        #[cfg(feature = "telemetry")]
        {
            BRIDGE_INVALID_PROOF_TOTAL.inc();
        }
        relayers.slash(relayer, amount.min(1));
        relayers.mark_duty_failure(relayer);
        return false;
    }
    let hh = header_hash(&h);
    if !bridge.verified_headers.insert(hh) {
        #[cfg(feature = "telemetry")]
        {
            BRIDGE_INVALID_PROOF_TOTAL.inc();
        }
        relayers.slash(relayer, amount.min(1));
        relayers.mark_duty_failure(relayer);
        return false;
    }
    // Persist the full header for audit and replay protection.
    let dir = &bridge.cfg.headers_dir;
    let _ = std::fs::create_dir_all(dir);
    let path = std::path::Path::new(dir).join(format!("{}.json", crypto_suite::hex::encode(&hh)));
    let header_value = header.to_value();
    let rendered = json::to_string_value_pretty(&header_value);
    let _ = std::fs::write(&path, rendered.as_bytes());
    *bridge.locked.entry(user.to_string()).or_insert(0) += amount;
    relayers.mark_duty_completion(relayer);
    true
}
