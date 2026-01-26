#![cfg(feature = "integration-tests")]
use the_block::compute_market::{Workload, WorkloadRunner};

#[test]
fn gpu_hash_matches_cpu() {
    let runner = WorkloadRunner::new();
    let data = b"hello".to_vec();
    let first = runtime::block_on(runner.run(0, Workload::GpuHash(data.clone())));
    let second = runtime::block_on(runner.run(0, Workload::GpuHash(data)));
    assert_eq!(first.output, second.output);
}
