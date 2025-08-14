use std::{
    io::Read,
    process::{Command, Stdio},
    time::Duration,
};

use wait_timeout::ChildExt;

#[test]
fn demo_runs_clean() {
    let out = tempfile::NamedTempFile::new().expect("stdout tmp");
    let err = tempfile::NamedTempFile::new().expect("stderr tmp");
    let out_path = out.path().to_owned();
    let err_path = err.path().to_owned();
    let mut child = Command::new(".venv/bin/python")
        .arg("demo.py")
        .env("TB_PURGE_LOOP_SECS", "1")
        .env_remove("TB_DEMO_MANUAL_PURGE")
        .env("PYTHONUNBUFFERED", "1")
        .stdout(Stdio::from(out.reopen().expect("stdout handle")))
        .stderr(Stdio::from(err.reopen().expect("stderr handle")))
        .spawn()
        .expect("spawn demo");
    match child
        .wait_timeout(Duration::from_secs(20))
        .expect("wait demo")
    {
        Some(status) if status.success() => {}
        Some(_) => {
            let mut out_log = String::new();
            let mut err_log = String::new();
            std::fs::File::open(&out_path)
                .and_then(|mut f| f.read_to_string(&mut out_log))
                .ok();
            std::fs::File::open(&err_path)
                .and_then(|mut f| f.read_to_string(&mut err_log))
                .ok();
            eprintln!("stdout:\n{out_log}");
            eprintln!("stderr:\n{err_log}");
            out.keep().expect("persist stdout");
            err.keep().expect("persist stderr");
            panic!(
                "demo failed; stdout: {}, stderr: {}",
                out_path.display(),
                err_path.display()
            );
        }
        None => {
            let _ = child.kill();
            let mut out_log = String::new();
            let mut err_log = String::new();
            std::fs::File::open(&out_path)
                .and_then(|mut f| f.read_to_string(&mut out_log))
                .ok();
            std::fs::File::open(&err_path)
                .and_then(|mut f| f.read_to_string(&mut err_log))
                .ok();
            eprintln!("stdout:\n{out_log}");
            eprintln!("stderr:\n{err_log}");
            out.keep().expect("persist stdout");
            err.keep().expect("persist stderr");
            panic!(
                "demo timed out; stdout: {}, stderr: {}",
                out_path.display(),
                err_path.display()
            );
        }
    }
}
