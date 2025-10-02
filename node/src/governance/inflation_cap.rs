#![forbid(unsafe_code)]

use crypto_suite::hashing::blake3::Hasher;
#[cfg(test)]
use crypto_suite::zk::groth16::Groth16Bn256;
use crypto_suite::zk::groth16::PreparedVerifyingKey;
use inflation::proof::{verify as verify_proof, InflationProof};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct WeekTuple {
    pub week: u64,
    pub s_start: u64,
    pub s_end: u64,
    pub rho_calc: f64,
    pub sigma_s: f64,
}

fn hash_leaf(t: &WeekTuple) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(&t.week.to_le_bytes());
    h.update(&t.s_start.to_le_bytes());
    h.update(&t.s_end.to_le_bytes());
    h.update(&t.rho_calc.to_le_bytes());
    h.update(&t.sigma_s.to_le_bytes());
    h.finalize().into()
}

/// Compute a simple binary Merkle root over the weekly tuples.
pub fn merkle_root(weeks: &[WeekTuple]) -> [u8; 32] {
    if weeks.is_empty() {
        return [0u8; 32];
    }
    let mut hashes: Vec<[u8; 32]> = weeks.iter().map(hash_leaf).collect();
    while hashes.len() > 1 {
        let mut next = Vec::with_capacity((hashes.len() + 1) / 2);
        for pair in hashes.chunks(2) {
            let mut h = Hasher::new();
            h.update(&pair[0]);
            if pair.len() == 2 {
                h.update(&pair[1]);
            } else {
                h.update(&pair[0]);
            }
            next.push(h.finalize().into());
        }
        hashes = next;
    }
    hashes[0]
}

/// Verify and submit a quarterly inflation-cap proof.
pub fn submit_proof(proof: &InflationProof, pvk: &PreparedVerifyingKey) -> bool {
    verify_proof(proof, pvk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use inflation::proof::{prove, setup};

    #[test]
    fn root_deterministic() {
        let weeks = vec![WeekTuple {
            week: 1,
            s_start: 1000,
            s_end: 1010,
            rho_calc: 0.01,
            sigma_s: 0.1,
        }];
        let r1 = merkle_root(&weeks);
        let r2 = merkle_root(&weeks);
        assert_eq!(r1, r2);
    }

    #[test]
    fn submits_proof() {
        let params = setup().expect("parameters");
        let pvk = Groth16Bn256::prepare_verifying_key(&params);
        let proof = prove(&params, 100, 200).unwrap();
        assert!(submit_proof(&proof, &pvk));
    }
}
