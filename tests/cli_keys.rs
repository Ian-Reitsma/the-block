use std::process::Command;

mod util;

#[test]
fn import_missing_key() {
    let tmp = util::temp::temp_dir("missing_key_home");
    let output = Command::new("cargo")
        .args([
            "run",
            "--bin",
            "node",
            "--",
            "import-key",
            "nonexistent.pem",
        ])
        .env("HOME", tmp.path())
        .output()
        .expect("run node");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("key file not found: nonexistent.pem"),
        "stderr: {stderr}"
    );
}
