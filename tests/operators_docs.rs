use std::process::Command;

#[test]
fn verify_release_help() {
    assert!(Command::new("scripts/verify_release.sh")
        .arg("-h")
        .status()
        .expect("run verify script")
        .success());
}

#[test]
fn node_help() {
    let node = env!("CARGO_BIN_EXE_node");
    assert!(Command::new(node)
        .arg("--help")
        .status()
        .expect("node --help")
        .success());
}
