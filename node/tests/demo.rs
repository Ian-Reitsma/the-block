#![cfg(feature = "integration-tests")]
use std::{env, process::Command, str};

#[test]
fn demo_exits_when_bridge_disabled() {
    let python = env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
    if Command::new(&python)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        eprintln!("skipping demo_exits_when_bridge_disabled: unable to invoke {python}");
        return;
    }

    let output = Command::new(&python)
        .arg("demo.py")
        .arg("--max-runtime")
        .arg("5")
        .env("PYTHONUNBUFFERED", "1")
        .output();

    let output = match output {
        Ok(out) => out,
        Err(err) => {
            eprintln!("skipping demo_exits_when_bridge_disabled: failed to run demo.py ({err})");
            return;
        }
    };

    assert!(
        !output.status.success(),
        "demo.py unexpectedly succeeded without python bindings"
    );

    let stdout = str::from_utf8(&output.stdout).expect("stdout is valid utf-8");
    assert!(
        stdout.contains("python bridge is not yet available"),
        "demo.py stdout should mention disabled python bridge, got: {stdout}"
    );
}
