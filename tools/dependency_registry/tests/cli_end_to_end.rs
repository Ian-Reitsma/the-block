use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use dependency_registry::{run_cli, Cli};
use foundation_serialization::json;

fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/simple_workspace")
        .join(relative)
}

fn unique_temp_path(label: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    path.push(format!("dependency_registry_cli_{label}_{nanos}"));
    let _ = fs::remove_dir_all(&path);
    path
}

#[test]
fn check_mode_reports_drift_and_writes_telemetry() {
    let manifest = fixture_path("Cargo.toml");
    let policy = fixture_path("policy.toml");
    let baseline = fixture_path("outdated_registry.json");

    let out_dir = unique_temp_path("check");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let cli = Cli {
        manifest_path: Some(manifest.clone()),
        config: policy.clone(),
        positional_config: None,
        check: true,
        diff: None,
        explain: None,
        max_depth: None,
        baseline: baseline.clone(),
        out_dir: out_dir.clone(),
        snapshot: None,
        manifest_out: out_dir.join("manifest.txt"),
    };

    let error = run_cli(&cli).err().expect("check execution should fail");
    let message = error.to_string();
    assert!(
        message.contains("+ dep_b 0.1.0"),
        "drift diagnostics should list the missing dependency"
    );
    assert!(
        message.contains("~ dep_a 0.1.0 tier"),
        "drift diagnostics should include field-level changes"
    );
    assert!(
        message.contains("+ root package deep_dep"),
        "root package additions should be reported"
    );

    let telemetry_path = out_dir.join("dependency-check.telemetry");
    let telemetry = fs::read_to_string(&telemetry_path).expect("check telemetry written");
    assert!(
        telemetry.contains("dependency_registry_check_status"),
        "status metric exported"
    );
    assert!(
        telemetry.contains("status=\"drift\""),
        "status metric labelled as drift"
    );
    assert!(
        telemetry.contains("detail=\"add=1,remove=0,field=1,policy=1,root_add=3,root_remove=0\""),
        "detail label encodes drift counts"
    );
    assert!(
        telemetry.contains("dependency_registry_check_counts{kind=\"root_additions\"} 3"),
        "per-kind counters emitted"
    );
    assert!(
        telemetry.contains("dependency_registry_check_counts{kind=\"policy_changes\"} 1"),
        "policy change counter emitted"
    );

    let _ = fs::remove_dir_all(out_dir);
}

#[test]
fn generates_all_artifacts_via_cli_runner() {
    let manifest = fixture_path("Cargo.toml");
    let policy = fixture_path("policy.toml");

    let out_dir = unique_temp_path("artifacts");
    fs::create_dir_all(&out_dir).expect("create out dir");
    let snapshot_path = out_dir.join("snapshot.json");
    let manifest_out = out_dir.join("first_party_manifest.txt");
    let markdown_path = out_dir.join("inventory.md");

    std::env::set_var("TB_DEPENDENCY_REGISTRY_DOC_PATH", &markdown_path);

    let cli = Cli {
        manifest_path: Some(manifest.clone()),
        config: policy.clone(),
        positional_config: None,
        check: false,
        diff: None,
        explain: None,
        max_depth: None,
        baseline: out_dir.join("baseline.json"),
        out_dir: out_dir.clone(),
        snapshot: Some(snapshot_path.clone()),
        manifest_out: manifest_out.clone(),
    };

    let artifacts = run_cli(&cli).expect("cli execution should succeed");

    let registry_bytes = fs::read(&artifacts.registry_path).expect("registry written");
    let registry_value = json::value_from_slice(&registry_bytes).expect("registry json parse");
    let entries = registry_value
        .as_object()
        .and_then(|obj| obj.get("entries"))
        .and_then(|value| value.as_array())
        .expect("entries array present");
    assert!(entries.len() >= 4, "expected workspace entries");

    let violations_bytes = fs::read(&artifacts.violations_path).expect("violations written");
    let violations_value = json::value_from_slice(&violations_bytes).expect("violations json");
    let violation_entries = violations_value
        .as_array()
        .expect("violations entries array");
    assert!(
        violation_entries
            .iter()
            .any(|value| value.to_string().contains("dep_b")),
        "violations should contain dep_b"
    );

    let metrics = fs::read_to_string(&artifacts.telemetry_path).expect("metrics written");
    assert!(
        metrics.contains("dependency_policy_violation_total"),
        "telemetry counters emitted"
    );

    let manifest_body = fs::read_to_string(&artifacts.manifest_path).expect("manifest written");
    assert!(
        manifest_body.lines().any(|line| line.trim() == "dep_b"),
        "manifest lists dependency names"
    );

    let markdown_body = fs::read_to_string(&artifacts.markdown_path).expect("markdown written");
    assert!(markdown_body.contains("| Tier | Crate | Version |"));

    assert!(
        artifacts
            .snapshot_path
            .as_ref()
            .map(|path| path.exists())
            .unwrap_or(false),
        "snapshot emitted"
    );

    std::env::remove_var("TB_DEPENDENCY_REGISTRY_DOC_PATH");

    let _ = fs::remove_dir_all(out_dir);
}
