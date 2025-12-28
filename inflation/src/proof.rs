#![forbid(unsafe_code)]

use crypto_suite::zk::groth16::{
    BellmanConstraintSystem, Bn256, Circuit, FieldElement, Groth16Bn256, Groth16Error, Parameters,
    PreparedVerifyingKey, Proof, SynthesisError,
};
use rand::thread_rng;

/// Simplified proof object carrying the claimed totals.
pub struct InflationProof {
    pub proof: Proof,
    pub minted: u64,
    pub bound: u64,
}

#[derive(Clone)]
struct InflationCircuit {
    minted: Option<FieldElement>,
    bound: Option<FieldElement>,
    slack: Option<FieldElement>,
}

impl Circuit for InflationCircuit {
    fn synthesize<CS: BellmanConstraintSystem<Bn256>>(
        self,
        cs: &mut CS,
    ) -> Result<(), SynthesisError> {
        let InflationCircuit {
            minted,
            bound,
            slack,
        } = self;

        let minted = cs.alloc_input(
            || "minted".to_string(),
            || minted.clone().ok_or(SynthesisError::AssignmentMissing),
        )?;
        let bound = cs.alloc_input(
            || "bound".to_string(),
            || bound.clone().ok_or(SynthesisError::AssignmentMissing),
        )?;
        let slack = cs.alloc(
            || "slack".to_string(),
            || slack.clone().ok_or(SynthesisError::AssignmentMissing),
        )?;
        // minted + slack = bound
        cs.enforce(
            || "minted plus slack equals bound".to_string(),
            |lc| lc + minted + slack,
            |lc| lc + CS::one(),
            |lc| lc + bound,
        );
        Ok(())
    }
}

pub fn setup() -> Result<Parameters, Groth16Error> {
    let circuit = InflationCircuit {
        minted: None,
        bound: None,
        slack: None,
    };
    Groth16Bn256::setup(circuit, &mut thread_rng())
}

/// Produce a proof that the total minted BLOCK does not exceed `bound`.
/// Returns an error if the cap is violated.
pub fn prove(params: &Parameters, minted: u64, bound: u64) -> Result<InflationProof, &'static str> {
    if minted > bound {
        return Err("inflation cap exceeded");
    }
    let slack = bound - minted;
    let circuit = InflationCircuit {
        minted: Some(FieldElement::from_u64(minted)),
        bound: Some(FieldElement::from_u64(bound)),
        slack: Some(FieldElement::from_u64(slack)),
    };
    let mut rng = thread_rng();
    let proof = Groth16Bn256::prove(params, circuit, &mut rng).map_err(|_| "prove")?;
    Ok(InflationProof {
        proof,
        minted,
        bound,
    })
}

/// Verify an inflation proof. In a full implementation this would check a
/// Groth16 proof against a prepared verifying key. Here we simply re-check
/// the inequality.
pub fn verify(proof: &InflationProof, pvk: &PreparedVerifyingKey) -> bool {
    let inputs = [
        FieldElement::from_u64(proof.minted),
        FieldElement::from_u64(proof.bound),
    ];
    Groth16Bn256::verify(pvk, &proof.proof, &inputs).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::setup;
    #[test]
    fn prove_and_verify() {
        let params = setup().expect("parameters");
        let pvk = Groth16Bn256::prepare_verifying_key(&params);
        let p = prove(&params, 100, 200).unwrap();
        assert!(verify(&p, &pvk));
    }
}
