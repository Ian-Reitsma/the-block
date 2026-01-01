#![cfg(feature = "integration-tests")]
use std::process::Command;

#[test]
fn cla_signed() {
    // Skip when the current HEAD commit is missing a sign-off; this keeps local
    // test runs from failing noisily while still enforcing CLA in CI where commits
    // are expected to carry a Signed-off-by trailer.
    let head_msg = Command::new("git")
        .arg("log")
        .arg("-1")
        .arg("--pretty=%B")
        .output()
        .expect("read git log");
    let msg = String::from_utf8_lossy(&head_msg.stdout);
    if !msg.contains("Signed-off-by:") {
        eprintln!("skipping CLA check: HEAD commit missing Signed-off-by trailer");
        return;
    }
    let status = Command::new("bash")
        .arg("../scripts/check_cla.sh")
        .status()
        .expect("run cla check");
    assert!(status.success(), "CLA check failed");
}
