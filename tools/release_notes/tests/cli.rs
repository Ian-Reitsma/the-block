use std::{
    env,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn fixture_dir() -> PathBuf {
    let base = env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let dir = base.join(format!("release_notes_cli_test_{nanos}"));
    fs::create_dir_all(&dir).expect("create fixture dir");
    dir
}

fn release_notes_binary() -> PathBuf {
    let mut path = env::current_exe().expect("current exe path");
    path.pop();
    path.pop();
    let primary = if cfg!(target_os = "windows") {
        "release-notes.exe"
    } else {
        "release-notes"
    };
    path.push(primary);
    if path.exists() {
        return path;
    }

    let mut alt = path.clone();
    let fallback = if cfg!(target_os = "windows") {
        "release_notes.exe"
    } else {
        "release_notes"
    };
    alt.set_file_name(fallback);
    if alt.exists() {
        return alt;
    }

    panic!(
        "release-notes binary not found; checked {:?} and {:?}",
        path, alt
    );
}

fn write_history(dir: &Path) {
    let history_dir = dir.join("governance").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    let path = history_dir.join("dependency_policy.json");
    let mut file = File::create(path).expect("history file");
    let payload = r#"[
        {"epoch": 1, "proposal_id": 1, "kind": "runtime_backend", "allowed": ["inhouse"]},
        {"epoch": 2, "proposal_id": 2, "kind": "runtime_backend", "allowed": ["inhouse", "stub"]},
        {"epoch": 3, "proposal_id": 3, "kind": "transport_provider", "allowed": ["inhouse"]}
    ]"#;
    file.write_all(payload.as_bytes()).expect("write history");
}

#[test]
fn json_output_produces_valid_payload() {
    let dir = fixture_dir();
    write_history(dir.as_path());
    let binary = release_notes_binary();
    let output = Command::new(&binary)
        .arg("--state-dir")
        .arg(dir.to_str().unwrap())
        .arg("--json")
        .output()
        .expect("run release-notes");
    assert!(output.status.success(), "release-notes exited with failure");
    let value = json::value_from_slice(&output.stdout).expect("valid JSON emitted");
    let object = value.as_object().expect("summary root object");
    assert!(object.contains_key("updates"));
    assert!(object.contains_key("latest"));
}

#[test]
fn text_output_retains_human_readable_summary() {
    let dir = fixture_dir();
    write_history(dir.as_path());
    let binary = release_notes_binary();
    let output = Command::new(&binary)
        .arg("--state-dir")
        .arg(dir.to_str().unwrap())
        .output()
        .expect("run release-notes");
    assert!(output.status.success(), "release-notes exited with failure");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(stdout.contains("## Governance Dependency Policy Updates"));
    assert!(stdout.contains("Runtime backend"));
}
