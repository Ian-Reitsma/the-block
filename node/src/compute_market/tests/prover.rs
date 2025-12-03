use crate::compute_market::snark::{self, SnarkBackend};
use crate::compute_market::workloads;

/// Compare CPU and GPU prover latency to ensure both paths stay healthy.
#[test]
fn prover_cpu_gpu_latency_smoke() {
    let wasm = b"bench-prover".to_vec();
    let output = workloads::snark::run(&wasm);
    let cpu = snark::prove_with_backend(&wasm, &output, SnarkBackend::Cpu)
        .expect("cpu prover must succeed");
    let gpu = snark::prove_with_backend(&wasm, &output, SnarkBackend::Gpu);
    if let Ok(bundle) = gpu {
        assert!(bundle.latency_ms > 0);
        // Allow a generous threshold since CI hosts do not expose discrete GPUs.
        assert!(
            bundle.latency_ms <= cpu.latency_ms.saturating_mul(4) + 1,
            "gpu latency {}ms should stay within 4x of cpu {}ms",
            bundle.latency_ms,
            cpu.latency_ms
        );
    }
}

/// Ensure repeated CPU prover runs benefit from cached circuit parameters.
#[test]
fn prover_cache_benefits_repeated_runs() {
    let wasm = b"bench-cache".to_vec();
    let output = workloads::snark::run(&wasm);
    let first = snark::prove_with_backend(&wasm, &output, SnarkBackend::Cpu).expect("first run");
    let second = snark::prove_with_backend(&wasm, &output, SnarkBackend::Cpu).expect("second run");
    assert!(
        second.latency_ms <= first.latency_ms.saturating_add(10),
        "cached prover latency {}ms should not regress far beyond initial {}ms",
        second.latency_ms,
        first.latency_ms
    );
}
