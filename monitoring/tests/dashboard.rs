use std::fs;

#[test]
fn dashboard_snapshot() {
    let generated = fs::read_to_string("grafana/dashboard.json").unwrap();
    let expected = fs::read_to_string("tests/snapshots/dashboard.json").unwrap();
    assert_eq!(generated, expected);
}
