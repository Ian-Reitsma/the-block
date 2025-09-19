#![cfg(feature = "integration-tests")]
use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore]
fn run_example_workloads() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    for file in [
        "examples/workloads/cpu_only.json",
        "examples/workloads/gpu_inference.json",
        "examples/workloads/multi_gpu.json",
    ] {
        let status = Command::new("cargo")
            .args([
                "run",
                "-p",
                "the_block",
                "--features",
                "telemetry",
                "--example",
                "run_workload",
                file,
            ])
            .current_dir(&root)
            .status()
            .expect("run example");
        assert!(status.success(), "example {} failed", file);
    }
}
