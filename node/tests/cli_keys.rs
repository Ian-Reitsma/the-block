#![cfg(feature = "integration-tests")]
use governance_spec::{codec::encode_binary, ApprovedRelease};
use sled::Config;
use std::path::Path;
use std::process::Command;

#[path = "util/temp.rs"]
mod temp;

fn seed_release_approval(path: &Path) {
    let db = Config::new()
        .path(path)
        .temporary(false)
        .open()
        .expect("open gov db");
    let tree = db
        .open_tree("approved_releases")
        .expect("open approved_releases tree");
    let approved = ApprovedRelease {
        build_hash: env!("BUILD_BIN_HASH").to_string(),
        activated_epoch: 0,
        proposer: "integration-test".into(),
        signatures: Vec::new(),
        signature_threshold: 0,
        signer_set: Vec::new(),
        install_times: Vec::new(),
    };
    tree.insert(
        approved.build_hash.as_bytes(),
        encode_binary(&approved).expect("serialize approved release"),
    )
    .expect("insert approved release");
    tree.flush().expect("flush approved release");
    drop(tree);
    drop(db);
}

#[test]
fn import_missing_key() {
    let tmp = temp::temp_dir("missing_key_home");
    let gov_dir = temp::temp_dir("missing_key_gov");
    seed_release_approval(gov_dir.path());
    let gov_path = gov_dir.path().to_str().expect("gov path");
    let output = Command::new(env!("CARGO_BIN_EXE_node"))
        .args(["import-key", "nonexistent.pem"])
        .env("HOME", tmp.path())
        .env("TB_GOV_DB_PATH", gov_path)
        .output()
        .expect("run node");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("key file not found: nonexistent.pem"),
        "stderr: {stderr}"
    );
}
