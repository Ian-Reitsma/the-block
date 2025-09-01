#![allow(clippy::unwrap_used)]
use serial_test::serial;
use the_block::compute_market::{
    settlement::{SettleMode, Settlement},
    Job, Market, Offer, SliceProof, Workload,
};
use the_block::telemetry::INDUSTRIAL_REJECTED_TOTAL;

#[test]
#[serial]
fn slashes_on_deadline_miss() {
    let dir = tempfile::tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 0, 0.0, 0);
    Settlement::set_balance("prov", 100);
    let mut market = Market::new();
    let job_id = "sla".to_string();
    let offer = Offer {
        job_id: job_id.clone(),
        provider: "prov".into(),
        provider_bond: 50,
        consumer_bond: 1,
        capacity: 1,
        price: 5,
    };
    market.post_offer(offer).unwrap();
    let hash = [0u8; 32];
    let job = Job {
        job_id: job_id.clone(),
        buyer: "buyer".into(),
        slices: vec![hash],
        price_per_slice: 5,
        consumer_bond: 1,
        workloads: vec![Workload::Transcode(vec![])],
        gpu_required: false,
        deadline: 0,
    };
    market.submit_job(job).unwrap();
    let proof = SliceProof {
        reference: hash,
        output: hash,
        payout: 5,
    };
    assert!(market.submit_slice(&job_id, proof).is_err());
    assert_eq!(Settlement::balance("prov"), 50);
    assert_eq!(
        INDUSTRIAL_REJECTED_TOTAL.with_label_values(&["SLA"]).get(),
        1
    );
    Settlement::shutdown();
}
