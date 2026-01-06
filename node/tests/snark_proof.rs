#![cfg(feature = "integration-tests")]
use sys::tempfile::tempdir;
use the_block::compute_market::{
    scheduler,
    settlement::{SettleMode, Settlement},
    snark, ExecutionReceipt, Job, Market, Offer, Workload,
};

#[test]
fn invalid_proof_rejected() {
    scheduler::reset_for_test();
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let mut market = Market::new();
    let offer = Offer {
        job_id: "job1".into(),
        provider: "prov".into(),
        provider_bond: 1,
        consumer_bond: 1,
        units: 1,
        price_per_unit: 5,
        fee_pct: 100,
        capability: scheduler::Capability::default(),
        reputation: 0,
        reputation_multiplier: 1.0,
    };
    market.post_offer(offer).unwrap();
    let wasm = b"program".to_vec();
    let expected = the_block::compute_market::workloads::snark::run(&wasm);
    let job = Job {
        job_id: "job1".into(),
        buyer: "buyer".into(),
        slices: vec![expected],
        price_per_unit: 5,
        consumer_bond: 1,
        workloads: vec![Workload::Snark(wasm.clone())],
        capability: scheduler::Capability::default(),
        deadline: u64::MAX,
        priority: scheduler::Priority::Normal,
    };
    market.submit_job(job).unwrap();
    // Craft invalid proof by tampering with a valid bundle
    let mut bundle = snark::prove(&wasm, &expected).expect("generate snark proof");
    bundle.output_commitment = [0u8; 32];
    let receipt = ExecutionReceipt {
        reference: expected,
        output: expected,
        payout: 5,
        proof: Some(bundle),
    };
    assert!(market.submit_slice("job1", receipt).is_err());
    Settlement::shutdown();
}
