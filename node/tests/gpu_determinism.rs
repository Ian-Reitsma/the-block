#![cfg(feature = "integration-tests")]
use the_block::compute_market::{workloads, Workload, WorkloadRunner};

#[test]
fn gpu_hash_matches_cpu() {
    let runner = WorkloadRunner::new();
    let data = b"hello".to_vec();
    let cpu = workloads::hash_bytes(&data);
    let gpu = runtime::block_on(runner.run(0, Workload::GpuHash(data)));
    assert_eq!(gpu.output, cpu);
}
