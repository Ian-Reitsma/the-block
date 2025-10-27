#![cfg(unix)]

use std::{
    env, fs,
    io::Write,
    os::unix::fs::PermissionsExt,
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

fn write_stub(path: &Path, name: &str, script: &str) {
    let file_path = path.join(name);
    let mut file = fs::File::create(&file_path).expect("create stub");
    file.write_all(script.as_bytes()).expect("write stub");
    drop(file);
    fs::set_permissions(&file_path, fs::Permissions::from_mode(0o755)).expect("chmod stub");
}

fn install_release_stubs(bin_dir: &Path) {
    let cargo_script = r#"#!/usr/bin/env bash
set -euo pipefail
cmd=${1:-}
shift || true
case "$cmd" in
  run)
    if [[ ${1:-} == '-p' && ${2:-} == 'dependency_registry' ]]; then
      shift 2
      if [[ ${1:-} == '--' ]]; then shift; fi
      out_dir=''
      snapshot=''
      while [[ $# -gt 0 ]]; do
        case "$1" in
          --out-dir) out_dir=$2; shift 2 ;;
          --snapshot) snapshot=$2; shift 2 ;;
          --check) shift ;;
          *) shift ;;
        esac
      done
      if [[ -n $snapshot && -z ${TB_DEPENDENCY_REGISTRY_SKIP_SNAPSHOT:-} ]]; then
        mkdir -p "$(dirname "$snapshot")"
        echo '{}' > "$snapshot"
      fi
      mkdir -p "$out_dir"
      echo '{}' > "$out_dir/dependency-registry.json"
      echo '[]' > "$out_dir/dependency-violations.json"
      cat <<'TELEMETRY' > "$out_dir/dependency-check.telemetry"
# HELP dependency_registry_check_status stub
dependency_registry_check_status{status="pass",detail="ok"} 1
TELEMETRY
      cat <<'SUMMARY' > "$out_dir/dependency-check.summary.json"
{"status":"pass","detail":"ok","counts":{}}
SUMMARY
      cat <<'METRICS' > "$out_dir/dependency-metrics.telemetry"
# HELP dependency_policy_violation_total stub
dependency_policy_violation_total 0
METRICS
      exit 0
    fi
    ;;
  xtask)
    subcmd=${1:-}
    shift || true
    if [[ $subcmd == 'chaos' ]]; then
      out_dir="target/chaos"
      while [[ $# -gt 0 ]]; do
        case "$1" in
          --out-dir)
            out_dir=$2
            shift 2
            ;;
          --status-endpoint|--baseline|--steps|--nodes)
            shift 2
            ;;
          --require-diff)
            shift
            ;;
          *)
            shift
            ;;
        esac
      done
      mkdir -p "$out_dir"
      echo '[]' > "$out_dir/status.snapshot.json"
      echo '[]' > "$out_dir/status.diff.json"
      echo '[]' > "$out_dir/overlay.readiness.json"
      cat <<'JSON' > "$out_dir/provider.failover.json"
[]
JSON
      exit 0
    fi
    ;;
  vendor)
    dest=''
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --locked|--versioned-dirs) shift ;;
        *) dest=$1; shift ;;
      esac
    done
    mkdir -p "$dest"
    echo 'stub' > "$dest/Cargo.toml"
    exit 0
    ;;
  bom)
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --format)
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done
    echo '{}'
    exit 0
    ;;
  build)
    mkdir -p target/release
    : > target/release/the_block
    exit 0
    ;;
  *)
    echo "unexpected cargo invocation: $cmd $*" >&2
    exit 1
    ;;
esac
"#;

    write_stub(bin_dir, "cargo", cargo_script);
    write_stub(
        bin_dir,
        "cargo-bom",
        "#!/usr/bin/env bash\ncat <<'JSON'\n{}\nJSON\n",
    );
    write_stub(
        bin_dir,
        "rustc",
        "#!/usr/bin/env bash\nif [[ ${1:-} == '-V' ]]; then\n  echo 'rustc 1.82.0 (stub)'\nelif [[ ${1:-} == '-Vv' ]]; then\n  cat <<INFO\nrustc 1.82.0 (stub)\nhost: x86_64-unknown-linux-gnu\ncommit-hash: deadbeef\nINFO\nelse\n  echo 'stub rustc' >&2\n  exit 1\nfi\n",
    );
    write_stub(
        bin_dir,
        "ld",
        "#!/usr/bin/env bash\necho 'ld (GNU Binutils) stub'\n",
    );
    write_stub(bin_dir, "cosign", "#!/usr/bin/env bash\nexit 0\n");
}

#[test]
fn release_provenance_requires_snapshot() {
    let repo = repo_root();
    let stubs = tempdir().expect("stub dir");
    let bin_dir = stubs.path();

    install_release_stubs(bin_dir);

    let path = format!("{}:{}", bin_dir.display(), env::var("PATH").unwrap());

    let tag = "test-snapshot-missing";
    let status = Command::new(repo.join("scripts/release_provenance.sh"))
        .arg(tag)
        .current_dir(&repo)
        .env("PATH", path)
        .env("TB_DEPENDENCY_REGISTRY_SKIP_SNAPSHOT", "1")
        .env("SOURCE_DATE_EPOCH", "0")
        .status()
        .expect("run release_provenance");
    assert!(
        !status.success(),
        "release_provenance should fail when dependency snapshots are absent"
    );

    let release_dir = repo.join("releases").join(tag);
    if release_dir.exists() {
        fs::remove_dir_all(release_dir).expect("cleanup release dir");
    }
}

#[test]
fn release_provenance_archives_dependency_telemetry() {
    let repo = repo_root();
    let stubs = tempdir().expect("stub dir");
    let bin_dir = stubs.path();

    install_release_stubs(bin_dir);

    let path = format!("{}:{}", bin_dir.display(), env::var("PATH").unwrap());

    let tag = "test-telemetry-artifacts";
    let status = Command::new(repo.join("scripts/release_provenance.sh"))
        .arg(tag)
        .current_dir(&repo)
        .env("PATH", path)
        .env("SOURCE_DATE_EPOCH", "0")
        .status()
        .expect("run release_provenance");
    assert!(
        status.success(),
        "release_provenance should succeed when dependency telemetry is emitted"
    );

    let release_dir = repo.join("releases").join(tag);
    let telemetry_path = release_dir.join("dependency-check.telemetry");
    let summary_path = release_dir.join("dependency-check.summary.json");
    let metrics_path = release_dir.join("dependency-metrics.telemetry");
    assert!(telemetry_path.exists(), "telemetry artifact missing");
    assert!(summary_path.exists(), "summary artifact missing");
    assert!(metrics_path.exists(), "metrics artifact missing");

    let chaos_dir = release_dir.join("chaos");
    assert!(chaos_dir.exists(), "chaos artefact directory missing");
    for name in [
        "status.snapshot.json",
        "status.diff.json",
        "overlay.readiness.json",
        "provider.failover.json",
    ] {
        let path = chaos_dir.join(name);
        assert!(path.exists(), "chaos artefact missing: {}", name);
        let metadata = fs::metadata(&path).expect("chaos artefact metadata");
        assert!(metadata.len() > 0, "chaos artefact empty: {}", name);
    }

    let provenance_path = release_dir.join("provenance.json");
    let provenance = fs::read_to_string(&provenance_path).expect("read provenance");
    assert!(provenance.contains("\"dependency_check\""));
    assert!(provenance.contains("\"summary\""));
    assert!(provenance.contains("\"dependency_metrics\""));

    fs::remove_dir_all(release_dir).expect("cleanup release dir");
}

#[test]
fn verify_release_requires_chaos_artifacts() {
    let repo = repo_root();
    let stubs = tempdir().expect("stub dir");
    let bin_dir = stubs.path();
    install_release_stubs(bin_dir);

    let path = format!("{}:{}", bin_dir.display(), env::var("PATH").unwrap());

    let release_dir = tempdir().expect("release dir");
    let release_path = release_dir.path();
    let archive_path = release_path.join("the_block.tar.gz");
    fs::write(&archive_path, b"stub archive").expect("write archive");
    let sha_output = Command::new("sha256sum")
        .arg(&archive_path)
        .output()
        .expect("sha256sum archive");
    assert!(sha_output.status.success(), "sha256sum failed");
    let sha = String::from_utf8(sha_output.stdout).expect("sha utf8");
    let sha = sha.split_whitespace().next().expect("sha value");

    let checks_path = release_path.join("checksums.txt");
    let mut checks = fs::File::create(&checks_path).expect("create checksums");
    writeln!(
        checks,
        "{sha}  {}",
        archive_path.file_name().unwrap().to_string_lossy()
    )
    .expect("write archive checksum");
    writeln!(
        checks,
        "vendor-tree  deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
    )
    .expect("write vendor hash");
    drop(checks);

    let sig_path = release_path.join("checksums.txt.sig");
    fs::write(&sig_path, b"stub signature").expect("write signature");

    let snapshot_src = repo.join("docs/dependency_inventory.json");
    let snapshot_dst = release_path.join("dependency-snapshot.json");
    fs::copy(&snapshot_src, &snapshot_dst).expect("copy dependency snapshot");
    fs::write(release_path.join("SBOM-x86_64.json"), b"{}").expect("write sbom");

    let verify_script = repo.join("scripts/verify_release.sh");
    let run_verify = |expect_success: bool| {
        let status = Command::new(&verify_script)
            .current_dir(&repo)
            .env("PATH", &path)
            .arg(&archive_path)
            .arg(&checks_path)
            .arg(&sig_path)
            .status()
            .expect("run verify_release");
        if expect_success {
            assert!(status.success(), "verify_release should succeed");
        } else {
            assert!(!status.success(), "verify_release should fail");
        }
    };

    // Missing chaos artefacts should fail verification.
    run_verify(false);

    // Populate chaos artefacts and retry.
    let chaos_dir = release_path.join("chaos");
    fs::create_dir_all(&chaos_dir).expect("create chaos dir");
    for name in [
        "status.snapshot.json",
        "status.diff.json",
        "overlay.readiness.json",
        "provider.failover.json",
    ] {
        fs::write(chaos_dir.join(name), b"[]").expect("write chaos artefact");
    }

    run_verify(true);
}
