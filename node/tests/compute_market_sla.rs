#![cfg(feature = "integration-tests")]
use the_block::compute_market::scheduler::Capability;
use the_block::compute_market::{scheduler, ExecutionReceipt, Job, Market, Offer, Workload};

#[test]
fn job_timeout_and_resubmit_penalizes() {
    scheduler::reset_for_test();
    let mut market = Market::new();
    let offer = Offer {
        job_id: "job1".into(),
        provider: "prov".into(),
        provider_bond: 5,
        consumer_bond: 5,
        units: 1,
        price_per_unit: 1,
        fee_pct_ct: 0,
        capability: Capability {
            cpu_cores: 1,
            ..Default::default()
        },
        reputation: 0,
        reputation_multiplier: 1.0,
    };
    market.post_offer(offer).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let job = Job {
        job_id: "job1".into(),
        buyer: "buyer".into(),
        slices: vec![[0u8; 32]],
        price_per_unit: 1,
        consumer_bond: 5,
        workloads: vec![Workload::Transcode(vec![])],
        capability: Capability {
            cpu_cores: 1,
            ..Default::default()
        },
        deadline: now + 1,
        priority: scheduler::Priority::Normal,
    };
    market.submit_job(job).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    let proof = ExecutionReceipt {
        reference: [0u8; 32],
        output: [0u8; 32],
        payout: 1,
        proof: None,
    };
    assert!(market.submit_slice("job1", proof).is_err());
    #[cfg(feature = "telemetry")]
    assert_eq!(the_block::telemetry::COMPUTE_JOB_TIMEOUT_TOTAL.value(), 1);
    // resubmit
    let offer2 = Offer {
        job_id: "job1".into(),
        provider: "prov".into(),
        provider_bond: 5,
        consumer_bond: 5,
        units: 1,
        price_per_unit: 1,
        fee_pct_ct: 0,
        capability: Capability {
            cpu_cores: 1,
            ..Default::default()
        },
        reputation: 0,
        reputation_multiplier: 1.0,
    };
    market.post_offer(offer2).unwrap();
    let job2 = Job {
        job_id: "job1".into(),
        buyer: "buyer".into(),
        slices: vec![[0u8; 32]],
        price_per_unit: 1,
        consumer_bond: 5,
        workloads: vec![Workload::Transcode(vec![])],
        capability: Capability {
            cpu_cores: 1,
            ..Default::default()
        },
        deadline: now + 10,
        priority: scheduler::Priority::Normal,
    };
    market.submit_job(job2).unwrap();
    #[cfg(feature = "telemetry")]
    assert_eq!(the_block::telemetry::JOB_RESUBMITTED_TOTAL.value(), 1);
}
