#![cfg(feature = "integration-tests")]
use std::{fs, path::Path, process::Command, time::Duration};

use wait_timeout::ChildExt;

#[test]
#[ignore = "slow"]
fn demo_runs_clean() {
    if !Path::new(".venv/bin/python").exists() {
        eprintln!("skipping demo_runs_clean: .venv/bin/python missing (run scripts/bootstrap.sh)");
        return;
    }

    if !Path::new(".venv/bin/maturin").exists()
        && Command::new(".venv/bin/python")
            .args(["-m", "pip", "install", "--upgrade", "maturin"])
            .status()
            .is_err()
    {
        eprintln!("skipping demo_runs_clean: failed to install maturin");
        return;
    }

    if Command::new(".venv/bin/maturin")
        .args([
            "develop",
            "--release",
            "-F",
            "pyo3/extension-module",
            "-F",
            "telemetry",
        ])
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        eprintln!("skipping demo_runs_clean: maturin build failed");
        return;
    }

    let mut child = Command::new(".venv/bin/python")
        .arg("demo.py")
        .arg("--max-runtime")
        .arg("15")
        .env("TB_PURGE_LOOP_SECS", "1")
        .env("TB_SAVE_LOGS", "1")
        .env_remove("TB_DEMO_MANUAL_PURGE")
        .env("PYTHONUNBUFFERED", "1")
        .spawn()
        .expect("spawn demo");
    match child
        // Allow extra time on first run so the demo binary can build.
        .wait_timeout(Duration::from_secs(60))
        .expect("wait demo")
    {
        Some(status) if status.success() => {}
        Some(_) => {
            let out_log = fs::read_to_string("demo_logs/stdout.log").unwrap_or_default();
            let err_log = fs::read_to_string("demo_logs/stderr.log").unwrap_or_default();
            eprintln!("stdout:\n{out_log}");
            eprintln!("stderr:\n{err_log}");
            panic!("demo failed; logs persisted in demo_logs/");
        }
        None => {
            let _ = child.kill();
            let out_log = fs::read_to_string("demo_logs/stdout.log").unwrap_or_default();
            let err_log = fs::read_to_string("demo_logs/stderr.log").unwrap_or_default();
            eprintln!("stdout:\n{out_log}");
            eprintln!("stderr:\n{err_log}");
            panic!("demo timed out; logs persisted in demo_logs/");
        }
    }
}
