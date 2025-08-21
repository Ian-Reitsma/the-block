#![allow(clippy::unwrap_used, clippy::expect_used)]
use proptest::prelude::*;
use the_block::compute_market::{Workload, WorkloadRunner};

proptest! {
    #[test]
    fn transcode_hash_matches(data in proptest::collection::vec(any::<u8>(), 0..64)) {
        let runner = WorkloadRunner::new();
        let w = Workload::Transcode(data.clone());
        let out = runner.run(&w);
        let mut h = blake3::Hasher::new();
        h.update(&data);
        prop_assert_eq!(out, *h.finalize().as_bytes());
    }

    #[test]
    fn inference_hash_matches(data in proptest::collection::vec(any::<u8>(), 0..64)) {
        let runner = WorkloadRunner::new();
        let w = Workload::Inference(data.clone());
        let out = runner.run(&w);
        let mut h = blake3::Hasher::new();
        h.update(&data);
        prop_assert_eq!(out, *h.finalize().as_bytes());
    }
}
