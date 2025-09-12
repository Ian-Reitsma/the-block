use jurisdiction::PolicyPack;

#[test]
fn templates_roundtrip() {
    let us = PolicyPack::template("US").expect("us template");
    let ser = serde_json::to_string(&us).unwrap();
    let de: PolicyPack = serde_json::from_str(&ser).unwrap();
    assert_eq!(us, de);
}
