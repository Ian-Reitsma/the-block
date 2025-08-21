use std::collections::HashSet;

#[test]
fn telemetry_fields_documented() {
    let docs = std::fs::read_to_string("docs/telemetry.md").expect("read docs");
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
    assert_eq!(expected, seen, "telemetry.md fields mismatch");
}
