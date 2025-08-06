use std::process::{Command, Stdio};

#[test]
fn demo_runs_clean() {
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let path = tmp.path().to_owned();
    let out = tmp.reopen().expect("stdout handle");
    let err = tmp.reopen().expect("stderr handle");
    let status = Command::new(".venv/bin/python")
        .arg("demo.py")
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .status()
        .expect("spawn demo");
    if !status.success() {
        tmp.keep().expect("persist log");
        panic!("demo failed; see {}", path.display());
    }
}
