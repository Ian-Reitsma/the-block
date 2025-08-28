use std::process::Command;

#[test]
#[ignore]
fn cla_signed() {
    let status = Command::new("bash")
        .arg("../scripts/check_cla.sh")
        .status()
        .expect("run cla check");
    assert!(status.success(), "CLA check failed");
}
