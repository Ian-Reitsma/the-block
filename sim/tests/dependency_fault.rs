#![cfg(feature = "dependency-fault")]

use std::str::FromStr;
use std::time::Duration;

use tb_sim::dependency_fault_harness::{
    run_simulation, BackendSelections, CodingBackendChoice, FaultSpec, SimulationRequest,
    StorageBackendChoice,
};

#[test]
fn baseline_simulation_generates_artifacts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let mut request = SimulationRequest::default();
    request.duration = Duration::from_millis(50);
    request.iterations = 1;
    request.persist_logs = false;
    request.output_root = temp.path().to_path_buf();
    request.label = Some("baseline-test".into());
    let summary = run_simulation(&request).expect("simulation");
    assert!(!summary.reports.is_empty());
    for report in summary.reports {
        assert!(report.metrics_path.exists(), "metrics missing");
        assert!(report.summary_path.exists(), "summary missing");
    }
}

#[test]
fn simulation_records_faults() {
    let temp = tempfile::tempdir().expect("temp dir");
    let mut request = SimulationRequest::default();
    request.selections.coding = CodingBackendChoice::Xor;
    request.selections.storage = StorageBackendChoice::Memory;
    request.duration = Duration::from_millis(30);
    request.iterations = 1;
    request.output_root = temp.path().to_path_buf();
    request.persist_logs = false;
    request.faults = vec![FaultSpec::from_str("transport:timeout").unwrap()];
    let summary = run_simulation(&request).expect("simulation");
    let faulted = summary
        .reports
        .into_iter()
        .find(|report| report.metrics.scenario == "faulted")
        .expect("faulted scenario");
    assert!(faulted.metrics.transport_failures > 0);
    assert!(!faulted.metrics.fault_events.is_empty());
}
