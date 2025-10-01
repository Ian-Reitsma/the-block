use crypto_suite::signatures::ed25519::SigningKey;
use crypto_suite::transactions::{domain_tag_for, TransactionSigner};
use crypto_suite::zk::groth16::{FieldElement, Groth16Bn256};
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};

#[test]
fn ed25519_signature_is_deterministic() {
    let mut rng = StdRng::seed_from_u64(0xFACE_CAFE);
    let signing_key = SigningKey::generate(&mut rng);
    let signer = TransactionSigner::from_chain_id(1);
    let mut payload = vec![0u8; 64];
    rng.fill_bytes(&mut payload);

    let sig_a = signer.sign(&signing_key, &payload);
    let sig_b = signer.sign(&signing_key, &payload);
    assert_eq!(
        sig_a.to_bytes(),
        sig_b.to_bytes(),
        "signing must be deterministic"
    );

    let verifying_key = signing_key.verifying_key();
    assert!(
        signer.verify(&verifying_key, &payload, &sig_a).is_ok(),
        "signature must verify"
    );

    assert!(
        signer.verify(&verifying_key, b"altered", &sig_a).is_err(),
        "altered payload rejected"
    );
}

#[test]
fn domain_separation_differs_across_chains() {
    let mut rng = StdRng::seed_from_u64(7);
    let signing_key = SigningKey::generate(&mut rng);
    let payload = b"domain-test";

    let signer_a = TransactionSigner::from_chain_id(1);
    let signer_b = TransactionSigner::from_chain_id(2);
    let sig = signer_a.sign(&signing_key, payload);
    let verifying_key = signing_key.verifying_key();

    assert!(signer_a.verify(&verifying_key, payload, &sig).is_ok());
    assert!(signer_b.verify(&verifying_key, payload, &sig).is_err());

    let tag_a: [u8; 16] = domain_tag_for(1).into();
    let tag_b: [u8; 16] = domain_tag_for(2).into();
    assert_ne!(tag_a, tag_b);
}

#[test]
fn groth16_verification_matches_bellman() {
    use bellman_ce::pairing::bn256::{Bn256, Fr};
    use bellman_ce::pairing::ff::{Field, PrimeField};
    use bellman_ce::{Circuit, ConstraintSystem, SynthesisError};

    #[derive(Clone)]
    struct MulCircuit {
        left: Option<Fr>,
        right: Option<Fr>,
        product: Option<Fr>,
    }

    impl Circuit<Bn256> for MulCircuit {
        fn synthesize<CS: ConstraintSystem<Bn256>>(
            self,
            cs: &mut CS,
        ) -> Result<(), SynthesisError> {
            let left_val = self.left.ok_or(SynthesisError::AssignmentMissing)?;
            let right_val = self.right.ok_or(SynthesisError::AssignmentMissing)?;
            let product_val = self.product.ok_or(SynthesisError::AssignmentMissing)?;

            let left_var = cs.alloc(|| "left", || Ok(left_val))?;
            let right_var = cs.alloc(|| "right", || Ok(right_val))?;
            let prod_var = cs.alloc_input(|| "product", || Ok(product_val))?;

            cs.enforce(
                || "left * right = product",
                |lc| lc + left_var,
                |lc| lc + right_var,
                |lc| lc + prod_var,
            );

            Ok(())
        }
    }

    let mut rng = StdRng::seed_from_u64(42);
    let left = Fr::from_str("3").expect("left");
    let right = Fr::from_str("4").expect("right");
    let mut product = left;
    product.mul_assign(&right);
    let circuit = MulCircuit {
        left: Some(left),
        right: Some(right),
        product: Some(product),
    };

    let params = Groth16Bn256::setup(circuit.clone(), &mut rng).expect("setup");
    let proof = Groth16Bn256::prove(&params, circuit.clone(), &mut rng).expect("prove");
    let pvk = Groth16Bn256::prepare_verifying_key(&params);
    let inputs = vec![FieldElement::from(product)];

    let suite_ok = Groth16Bn256::verify(&pvk, &proof, &inputs).expect("suite verify");

    let raw_inputs: Vec<_> = inputs.iter().map(|f| f.clone_inner()).collect();
    let direct_ok = bellman_ce::groth16::verify_proof(pvk.inner(), proof.inner(), &raw_inputs)
        .expect("direct verify");

    assert_eq!(suite_ok, direct_ok);
    assert!(suite_ok);
}
