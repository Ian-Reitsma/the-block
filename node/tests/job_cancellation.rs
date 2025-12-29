#![cfg(feature = "integration-tests")]
use the_block::compute_market::{
    courier, scheduler,
    scheduler::{CancelReason, Capability, Priority},
    settlement::{Settlement, SettleMode},
    Job, Market, Offer, Workload,
};
use sys::tempfile::tempdir;

fn sample_offer_job(id: &str) -> (Offer, Job) {
    let offer = Offer {
        job_id: id.into(),
        provider: "prov".into(),
        provider_bond: 1,
        consumer_bond: 1,
        units: 1,
        price_per_unit: 1,
        fee_pct: 0,
        capability: Capability::default(),
        reputation: 0,
        reputation_multiplier: 1.0,
    };
    let job = Job {
        job_id: id.into(),
        buyer: "buyer".into(),
        slices: vec![[0u8; 32]],
        price_per_unit: 1,
        consumer_bond: 1,
        workloads: vec![Workload::Transcode(vec![1])],
        capability: Capability::default(),
        deadline: u64::MAX,
        priority: Priority::Normal,
    };
    (offer, job)
}

#[test]
fn cancel_releases_resources() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    scheduler::reset_for_test();
    let (offer, job) = sample_offer_job("j1");
    let mut m = Market::new();
    m.post_offer(offer).unwrap();
    m.submit_job(job).unwrap();
    courier::reserve_resources("j1");
    assert!(courier::is_reserved("j1"));
    let _ = m.cancel_job("j1", CancelReason::Client);
    assert!(!courier::is_reserved("j1"));
}

#[test]
fn cancel_after_completion_noop() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    scheduler::reset_for_test();
    let (offer, job) = sample_offer_job("j2");
    let mut m = Market::new();
    m.post_offer(offer).unwrap();
    m.submit_job(job).unwrap();
    scheduler::end_job("j2");
    assert!(m.cancel_job("j2", CancelReason::Client).is_none());
}
