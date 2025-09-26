#![cfg(unix)]

use std::{
    env, fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use tempfile::tempdir;

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

#[test]
fn release_provenance_requires_snapshot() {
    let repo = repo_root();
    let stubs = tempdir().expect("stub dir");
    let bin_dir = stubs.path();

    write_stub(
        bin_dir,
        "cargo",
        "#!/usr/bin/env bash\nset -euo pipefail\ncmd=${1:-}\nshift || true\ncase \"$cmd\" in\n  run)\n    if [[ ${1:-} == '-p' && ${2:-} == 'dependency_registry' ]]; then\n      shift 2\n      if [[ ${1:-} == '--' ]]; then shift; fi\n      out_dir=''\n      while [[ $# -gt 0 ]]; do\n        case \"$1\" in\n          --out-dir) out_dir=$2; shift 2 ;;\n          --snapshot) shift 2 ;;\n          --check) shift ;;\n          *) shift ;;\n        esac\n      done\n      mkdir -p \"$out_dir\"\n      echo '{}' > \"$out_dir/dependency-registry.json\"\n      exit 0\n    fi\n    ;;\n  vendor)\n    dest=''\n    while [[ $# -gt 0 ]]; do\n      case \"$1\" in\n        --locked|--versioned-dirs) shift ;;\n        *) dest=$1; shift ;;\n      esac\n    done\n    mkdir -p \"$dest\"\n    echo 'stub' > \"$dest/Cargo.toml\"\n    exit 0\n    ;;\n  build)\n    mkdir -p target/release\n    : > target/release/the_block\n    exit 0\n    ;;\n  *)\n    echo \"unexpected cargo invocation: $cmd $*\" >&2\n    exit 1\n    ;;\nesac\n",
    );

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

    let path = format!("{}:{}", bin_dir.display(), env::var("PATH").unwrap());

    let tag = "test-snapshot-missing";
    let status = Command::new(repo.join("scripts/release_provenance.sh"))
        .arg(tag)
        .current_dir(&repo)
        .env("PATH", path)
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
