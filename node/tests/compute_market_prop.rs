#![cfg(feature = "integration-tests")]
use blake3::Hasher;
use proptest::prelude::*;
use the_block::compute_market::{scheduler, *};

proptest! {
    #[test]
    fn match_and_finalize_payout(slices in 1usize..5, price in 1u64..10) {
        let mut market = Market::new();
        let job_id = "job_prop".to_string();
        let offer = Offer { job_id: job_id.clone(), provider: "prov".into(), provider_bond: 1, consumer_bond: 1, units: slices as u64, price_per_unit: price, fee_pct_ct: 100, capability: scheduler::Capability::default(), reputation: 0, reputation_multiplier: 1.0 };
        market.post_offer(offer).unwrap();
        let mut refs = Vec::new();
        let mut wls = Vec::new();
        for i in 0..slices {
            let data = vec![i as u8];
            let mut h = Hasher::new();
            h.update(&data);
            refs.push(*h.finalize().as_bytes());
            wls.push(Workload::Transcode(data));
        }
        let job = Job { job_id: job_id.clone(), buyer: "buyer".into(), slices: refs, price_per_unit: price, consumer_bond: 1, workloads: wls, capability: scheduler::Capability::default(), deadline: u64::MAX, priority: scheduler::Priority::Normal };
        market.submit_job(job).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let total = rt.block_on(market.execute_job(&job_id)).unwrap();
        prop_assert_eq!(total, price * slices as u64);
        prop_assert_eq!(market.finalize_job(&job_id).unwrap(), (1,1));
    }
}
