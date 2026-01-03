#![cfg(feature = "integration-tests")]
use std::collections::HashSet;

#[test]
fn telemetry_fields_documented() {
    let docs_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/telemetry.md");
    let docs = std::fs::read_to_string(&docs_path).expect("read docs");
    let expected: HashSet<&str> = [
        "subsystem",
        "op",
        "sender",
        "nonce",
        "reason",
        "code",
        "fpb",
    ]
    .into_iter()
    .collect();
    let seen: HashSet<&str> = docs
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- `"))
        .filter_map(|l| l.split('`').next())
        .collect();
    assert!(expected.is_subset(&seen), "telemetry.md fields mismatch");
}
