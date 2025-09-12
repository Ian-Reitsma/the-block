use crate::header::{verify_pow, PowHeader};
use crate::light_client::{header_hash, verify, Header};
use crate::Bridge;
#[cfg(feature = "telemetry")]
use crate::BRIDGE_INVALID_PROOF_TOTAL;
use crate::{light_client::Proof, relayer::RelayerSet, RelayerProof};

pub fn lock(
    bridge: &mut Bridge,
    relayers: &mut RelayerSet,
    relayer: &str,
    user: &str,
    amount: u64,
    header: &PowHeader,
    proof: &Proof,
    rproof: &RelayerProof,
) -> bool {
    if !rproof.verify(user, amount) || !verify_pow(header) {
        #[cfg(feature = "telemetry")]
        {
            BRIDGE_INVALID_PROOF_TOTAL.inc();
        }
        relayers.slash(relayer, amount.min(1));
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
        return false;
    }
    let hh = header_hash(&h);
    if !bridge.verified_headers.insert(hh) {
        #[cfg(feature = "telemetry")]
        {
            BRIDGE_INVALID_PROOF_TOTAL.inc();
        }
        relayers.slash(relayer, amount.min(1));
        return false;
    }
    *bridge.locked.entry(user.to_string()).or_insert(0) += amount;
    true
}
