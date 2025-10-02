#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crypto_suite::hashing::blake3;
use proptest::prelude::*;
use the_block::compute_market::{Workload, WorkloadRunner};

proptest! {
    #[test]
    fn transcode_hash_matches(data in proptest::collection::vec(any::<u8>(), 0..64)) {
        let runner = WorkloadRunner::new();
        let w = Workload::Transcode(data.clone());
        let out = runtime::block_on(runner.run(0, w));
        let mut h = blake3::Hasher::new();
        h.update(&data);
        prop_assert_eq!(out, *h.finalize().as_bytes());
    }

    #[test]
    fn inference_hash_matches(data in proptest::collection::vec(any::<u8>(), 0..64)) {
        let runner = WorkloadRunner::new();
        let w = Workload::Inference(data.clone());
        let out = runtime::block_on(runner.run(0, w));
        let mut h = blake3::Hasher::new();
        h.update(&data);
        prop_assert_eq!(out, *h.finalize().as_bytes());
    }
}
