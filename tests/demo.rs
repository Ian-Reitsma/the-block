use std::{
    io::Read,
    process::{Command, Stdio},
    time::Duration,
};

use wait_timeout::ChildExt;

#[test]
fn demo_runs_clean() {
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let path = tmp.path().to_owned();
    let out = tmp.reopen().expect("stdout handle");
    let err = tmp.reopen().expect("stderr handle");
    let mut child = Command::new(".venv/bin/python")
        .arg("demo.py")
        .env("TB_PURGE_LOOP_SECS", "1")
        .env("TB_DEMO_MANUAL_PURGE", "")
        .env("PYTHONUNBUFFERED", "1")
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .expect("spawn demo");
    match child
        .wait_timeout(Duration::from_secs(10))
        .expect("wait demo")
    {
        Some(status) if status.success() => {}
        Some(_) => {
            let mut log = String::new();
            std::fs::File::open(&path)
                .and_then(|mut f| f.read_to_string(&mut log))
                .ok();
            eprintln!("{log}");
            tmp.keep().expect("persist log");
            panic!("demo failed; see {}", path.display());
        }
        None => {
            let _ = child.kill();
            let mut log = String::new();
            std::fs::File::open(&path)
                .and_then(|mut f| f.read_to_string(&mut log))
                .ok();
            eprintln!("{log}");
            tmp.keep().expect("persist log");
            panic!("demo timed out; see {}", path.display());
        }
    }
}
