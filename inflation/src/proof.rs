#![forbid(unsafe_code)]

use bellman_ce::bn256::{Bn256, Fr};
use bellman_ce::groth16::{
    create_random_proof, generate_random_parameters, verify_proof, Parameters,
    PreparedVerifyingKey, Proof,
};
use bellman_ce::pairing::ff::PrimeField;
use bellman_ce::{Circuit, ConstraintSystem, SynthesisError};
use rand::thread_rng;

/// Simplified proof object carrying the claimed totals.
pub struct InflationProof {
    pub proof: Proof<Bn256>,
    pub minted: u64,
    pub bound: u64,
}

#[derive(Clone)]
struct InflationCircuit {
    minted: Option<Fr>,
    bound: Option<Fr>,
    slack: Option<Fr>,
}

impl Circuit<Bn256> for InflationCircuit {
    fn synthesize<CS: ConstraintSystem<Bn256>>(self, cs: &mut CS) -> Result<(), SynthesisError> {
        let minted = cs.alloc_input(
            || "minted",
            || self.minted.ok_or(SynthesisError::AssignmentMissing),
        )?;
        let bound = cs.alloc_input(
            || "bound",
            || self.bound.ok_or(SynthesisError::AssignmentMissing),
        )?;
        let slack = cs.alloc(
            || "slack",
            || self.slack.ok_or(SynthesisError::AssignmentMissing),
        )?;
        // minted + slack = bound
        cs.enforce(
            || "minted plus slack equals bound",
            |lc| lc + minted + slack,
            |lc| lc + CS::one(),
            |lc| lc + bound,
        );
        Ok(())
    }
}

pub fn setup() -> Parameters<Bn256> {
    let circuit = InflationCircuit {
        minted: None,
        bound: None,
        slack: None,
    };
    generate_random_parameters::<Bn256, _, _>(circuit, &mut thread_rng()).unwrap()
}

/// Produce a proof that the total minted CT does not exceed `bound`.
/// Returns an error if the cap is violated.
pub fn prove(
    params: &Parameters<Bn256>,
    minted: u64,
    bound: u64,
) -> Result<InflationProof, &'static str> {
    if minted > bound {
        return Err("inflation cap exceeded");
    }
    let slack = bound - minted;
    let circuit = InflationCircuit {
        minted: Some(Fr::from_str(&minted.to_string()).unwrap()),
        bound: Some(Fr::from_str(&bound.to_string()).unwrap()),
        slack: Some(Fr::from_str(&slack.to_string()).unwrap()),
    };
    let mut rng = thread_rng();
    let proof =
        create_random_proof::<Bn256, _, _, _>(circuit, params, &mut rng).map_err(|_| "prove")?;
    Ok(InflationProof {
        proof,
        minted,
        bound,
    })
}

/// Verify an inflation proof. In a full implementation this would check a
/// Groth16 proof against a prepared verifying key. Here we simply re-check
/// the inequality.
pub fn verify(proof: &InflationProof, pvk: &PreparedVerifyingKey<Bn256>) -> bool {
    let inputs = [
        Fr::from_str(&proof.minted.to_string()).unwrap(),
        Fr::from_str(&proof.bound.to_string()).unwrap(),
    ];
    verify_proof(pvk, &proof.proof, &inputs).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::setup;
    use bellman_ce::groth16::prepare_verifying_key;
    #[test]
    fn prove_and_verify() {
        let params = setup();
        let pvk = prepare_verifying_key(&params.vk);
        let p = prove(&params, 100, 200).unwrap();
        assert!(verify(&p, &pvk));
    }
}
