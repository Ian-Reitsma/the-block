use crypto_suite::signatures::ed25519::SigningKey;
use crypto_suite::transactions::{domain_tag_for, TransactionSigner};
use crypto_suite::zk::groth16::{
    BellmanConstraintSystem, Bn256, Circuit, FieldElement, Groth16Bn256, SynthesisError,
};
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
fn groth16_inhouse_verifies_constraints() {
    #[derive(Clone)]
    struct MulCircuit {
        left: Option<FieldElement>,
        right: Option<FieldElement>,
        product: Option<FieldElement>,
    }

    impl Circuit for MulCircuit {
        fn synthesize<CS: BellmanConstraintSystem<Bn256>>(
            self,
            cs: &mut CS,
        ) -> Result<(), SynthesisError> {
            let left_val = self.left.ok_or(SynthesisError::AssignmentMissing)?;
            let right_val = self.right.ok_or(SynthesisError::AssignmentMissing)?;
            let product_val = self.product.ok_or(SynthesisError::AssignmentMissing)?;

            let left_var = cs.alloc(|| "left".to_string(), || Ok(left_val.clone()))?;
            let right_var = cs.alloc(|| "right".to_string(), || Ok(right_val.clone()))?;
            let prod_var = cs.alloc_input(|| "product".to_string(), || Ok(product_val.clone()))?;

            cs.enforce(
                || "left * right = product".to_string(),
                |lc| lc + left_var,
                |lc| lc + right_var,
                |lc| lc + prod_var,
            );

            Ok(())
        }
    }

    let mut rng = StdRng::seed_from_u64(42);
    let left = FieldElement::from_u64(3);
    let right = FieldElement::from_u64(4);
    let product = FieldElement::from_u64(12);
    let circuit = MulCircuit {
        left: Some(left.clone()),
        right: Some(right.clone()),
        product: Some(product.clone()),
    };

    let params = Groth16Bn256::setup(circuit.clone(), &mut rng).expect("setup");
    let proof = Groth16Bn256::prove(&params, circuit.clone(), &mut rng).expect("prove");
    let pvk = Groth16Bn256::prepare_verifying_key(&params);
    let inputs = vec![product.clone()];

    let suite_ok = Groth16Bn256::verify(&pvk, &proof, &inputs).expect("suite verify");
    assert!(suite_ok);

    let wrong_inputs = vec![FieldElement::from_u64(11)];
    let suite_bad = Groth16Bn256::verify(&pvk, &proof, &wrong_inputs).expect("suite verify");
    assert!(!suite_bad);

    let (public_inputs, aux) = proof.inner();
    assert_eq!(public_inputs, &[product]);
    assert_eq!(aux.len(), 2);
}
