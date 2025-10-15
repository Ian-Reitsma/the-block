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

    let provenance_path = release_dir.join("provenance.json");
    let provenance = fs::read_to_string(&provenance_path).expect("read provenance");
    assert!(provenance.contains("\"dependency_check\""));
    assert!(provenance.contains("\"summary\""));
    assert!(provenance.contains("\"dependency_metrics\""));

    fs::remove_dir_all(release_dir).expect("cleanup release dir");
}
