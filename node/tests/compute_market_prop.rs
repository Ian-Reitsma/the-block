#![cfg(feature = "integration-tests")]
use crypto_suite::hashing::blake3::Hasher;
use testkit::tb_prop_test;
use the_block::compute_market::{scheduler, *};

tb_prop_test!(match_and_finalize_payout, |runner| {
    runner
        .add_random_case("market executions", 20, |rng| {
            let slices = rng.range_usize(1..=8);
            let price = rng.range_u64(1..=50) + 1;
            let mut market = Market::new();
            let job_id = format!("job_prop_{slices}_{price}");
            let offer = Offer {
                job_id: job_id.clone(),
                provider: "prov".into(),
                provider_bond: 1,
                consumer_bond: 1,
                units: slices as u64,
                price_per_unit: price,
                fee_pct_ct: 100,
                capability: scheduler::Capability::default(),
                reputation: 0,
                reputation_multiplier: 1.0,
            };
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
            let job = Job {
                job_id: job_id.clone(),
                buyer: "buyer".into(),
                slices: refs,
                price_per_unit: price,
                consumer_bond: 1,
                workloads: wls,
                capability: scheduler::Capability::default(),
                deadline: u64::MAX,
                priority: scheduler::Priority::Normal,
            };
            market.submit_job(job).unwrap();
            let total = runtime::block_on(market.execute_job(&job_id)).unwrap();
            assert_eq!(total, price * slices as u64);
            assert_eq!(market.finalize_job(&job_id).unwrap(), (1, 1));
        })
        .expect("register random case");
});
