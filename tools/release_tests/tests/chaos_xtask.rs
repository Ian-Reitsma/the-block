use foundation_serialization::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use sys::tempfile::tempdir;

fn repo_root() -> PathBuf {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse");
    assert!(output.status.success(), "git rev-parse failed");
    PathBuf::from(String::from_utf8(output.stdout).expect("utf8").trim())
}

fn read_json(path: &Path) -> json::Value {
    let data = fs::read(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    json::from_slice(&data).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
}

#[test]
fn chaos_xtask_archives_and_reports_failover() {
    let repo = repo_root();
    let temp = tempdir().expect("temp dir");
    let chaos_dir = temp.path().join("chaos");
    let archive_dir = temp.path().join("archive");
    let publish_dir = temp.path().join("publish");

    let status = Command::new("cargo")
        .current_dir(&repo)
        .env("TB_SIM_SEED", "1337")
        .args(["run", "-p", "xtask", "--bin", "xtask", "--", "chaos"])
        .arg("--out-dir")
        .arg(chaos_dir.as_os_str())
        .arg("--steps")
        .arg("12")
        .arg("--nodes")
        .arg("12")
        .arg("--archive-dir")
        .arg(archive_dir.as_os_str())
        .arg("--archive-label")
        .arg("integration-test")
        .arg("--publish-dir")
        .arg(publish_dir.as_os_str())
        .status()
        .expect("run cargo xtask chaos");
    assert!(status.success(), "cargo xtask chaos failed: {status:?}");

    let diff_path = chaos_dir.join("status.diff.json");
    let diff_contents = fs::read_to_string(&diff_path).expect("read diff");
    assert_eq!(
        diff_contents.trim(),
        "[]",
        "expected empty diff when no baseline provided"
    );

    let provider_path = chaos_dir.join("provider.failover.json");
    let provider_json = read_json(&provider_path);
    let provider_has_diff = provider_json
        .as_array()
        .map(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("total_diff_entries")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default()
                    > 0
            })
        })
        .unwrap_or(false);
    assert!(provider_has_diff, "expected provider failover diff entries");

    let latest_manifest = archive_dir.join("latest.json");
    assert!(latest_manifest.exists(), "archive manifest missing");
    let latest_json = read_json(&latest_manifest);
    let manifest_rel = latest_json
        .get("manifest")
        .and_then(|value| value.as_str())
        .expect("manifest path present");
    let manifest_path = archive_dir.join(manifest_rel);
    assert!(manifest_path.exists(), "run manifest missing");
    let manifest_json = read_json(&manifest_path);
    let run_id = manifest_json
        .get("run_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let artifacts = manifest_json
        .get("artifacts")
        .and_then(|value| value.as_object())
        .expect("artifacts object present");
    assert!(
        artifacts.contains_key("attestations"),
        "attestations missing from archive manifest"
    );
    for entry in artifacts.values() {
        let file_name = entry
            .get("file")
            .and_then(|value| value.as_str())
            .expect("artifact file present");
        let path = archive_dir.join(run_id).join(file_name);
        assert!(
            path.exists(),
            "archived artefact missing: {}",
            path.display()
        );
    }

    let bundle_meta = manifest_json
        .get("bundle")
        .and_then(|value| value.as_object())
        .expect("bundle metadata present");
    let bundle_file = bundle_meta
        .get("file")
        .and_then(|value| value.as_str())
        .expect("bundle file present");
    let bundle_path = archive_dir.join(bundle_file);
    assert!(bundle_path.exists(), "archive bundle missing");
    let bundle_size = bundle_meta
        .get("size")
        .and_then(|value| value.as_u64())
        .expect("bundle size recorded");
    let on_disk = fs::metadata(&bundle_path).expect("bundle metadata");
    assert_eq!(on_disk.len(), bundle_size, "bundle size mismatch");

    let mirrored_latest = publish_dir.join("latest.json");
    assert!(mirrored_latest.exists(), "mirrored latest manifest missing");
    let mirrored_manifest = publish_dir.join(run_id).join("manifest.json");
    assert!(
        mirrored_manifest.exists(),
        "mirrored manifest missing: {}",
        mirrored_manifest.display()
    );
    let mirrored_bundle = publish_dir.join(bundle_file);
    assert!(
        mirrored_bundle.exists(),
        "mirrored bundle missing: {}",
        mirrored_bundle.display()
    );
}

#[test]
fn chaos_xtask_require_diff_fails_on_identical_baseline() {
    let repo = repo_root();
    let temp = tempdir().expect("temp dir");
    let first_dir = temp.path().join("first");

    let status = Command::new("cargo")
        .current_dir(&repo)
        .env("TB_SIM_SEED", "4242")
        .args(["run", "-p", "xtask", "--bin", "xtask", "--", "chaos"])
        .arg("--out-dir")
        .arg(first_dir.as_os_str())
        .arg("--steps")
        .arg("3")
        .arg("--nodes")
        .arg("10")
        .status()
        .expect("run initial chaos");
    assert!(status.success(), "initial chaos run failed: {status:?}");

    let snapshot_path = first_dir.join("status.snapshot.json");
    let baseline_path = first_dir.join("status.baseline.json");
    fs::copy(&snapshot_path, &baseline_path).expect("seed baseline from snapshot");

    let second_dir = temp.path().join("second");
    let status = Command::new("cargo")
        .current_dir(&repo)
        .env("TB_SIM_SEED", "4242")
        .args(["run", "-p", "xtask", "--bin", "xtask", "--", "chaos"])
        .arg("--out-dir")
        .arg(second_dir.as_os_str())
        .arg("--steps")
        .arg("3")
        .arg("--nodes")
        .arg("10")
        .arg("--baseline")
        .arg(baseline_path.as_os_str())
        .arg("--require-diff")
        .status()
        .expect("run chaos with require-diff");
    assert!(
        !status.success(),
        "expected require-diff to fail when baseline matches snapshot"
    );
}

#[test]
fn chaos_xtask_produces_overlay_diff_with_baseline() {
    let repo = repo_root();
    let temp = tempdir().expect("temp dir");
    let first_dir = temp.path().join("first");
    let archive_dir = temp.path().join("archive2");

    let status = Command::new("cargo")
        .current_dir(&repo)
        .env("TB_SIM_SEED", "9001")
        .args(["run", "-p", "xtask", "--bin", "xtask", "--", "chaos"])
        .arg("--out-dir")
        .arg(first_dir.as_os_str())
        .arg("--steps")
        .arg("12")
        .arg("--nodes")
        .arg("14")
        .arg("--archive-dir")
        .arg(archive_dir.as_os_str())
        .status()
        .expect("initial chaos run");
    assert!(status.success(), "initial chaos run failed: {status:?}");

    let snapshot_path = first_dir.join("status.snapshot.json");
    let baseline_path = first_dir.join("status.baseline.json");
    fs::copy(&snapshot_path, &baseline_path).expect("baseline copy");

    let second_dir = temp.path().join("second");
    let status = Command::new("cargo")
        .current_dir(&repo)
        .env("TB_SIM_SEED", "1337")
        .args(["run", "-p", "xtask", "--bin", "xtask", "--", "chaos"])
        .arg("--out-dir")
        .arg(second_dir.as_os_str())
        .arg("--steps")
        .arg("12")
        .arg("--nodes")
        .arg("14")
        .arg("--baseline")
        .arg(baseline_path.as_os_str())
        .arg("--require-diff")
        .status()
        .expect("chaos run with baseline");
    assert!(
        status.success(),
        "chaos run with baseline failed: {status:?}"
    );

    let diff_path = second_dir.join("status.diff.json");
    let diff_json = read_json(&diff_path);
    let diff_entries = diff_json.as_array().expect("diff array present");
    assert!(
        !diff_entries.is_empty(),
        "expected non-empty diff when baseline differs"
    );
    let overlay_entries: Vec<_> = diff_entries
        .iter()
        .filter(|entry| entry.get("module").and_then(|m| m.as_str()) == Some("overlay"))
        .collect();
    assert!(
        !overlay_entries.is_empty(),
        "expected overlay entries in diff"
    );

    let provider_path = second_dir.join("provider.failover.json");
    let provider_json = read_json(&provider_path);
    let provider_has_diff = provider_json
        .as_array()
        .map(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("total_diff_entries")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default()
                    > 0
            })
        })
        .unwrap_or(false);
    assert!(provider_has_diff, "provider failover diff entries missing");
}
