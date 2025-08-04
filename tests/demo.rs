#[test]
fn demo_runs_clean() {
    let status = std::process::Command::new(".venv/bin/python")
        .arg("demo.py")
        .status()
        .expect("spawn demo");
    assert!(status.success());
}

