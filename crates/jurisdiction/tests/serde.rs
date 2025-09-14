use jurisdiction::PolicyPack;

#[test]
fn templates_roundtrip() {
    let us = PolicyPack::template("US").expect("us template");
    let ser = serde_json::to_string(&us).unwrap();
    let de: PolicyPack = serde_json::from_str(&ser).unwrap();
    assert_eq!(us, de);
}

#[test]
fn resolves_inheritance() {
    // Create a child policy that inherits from US and adds a feature.
    let mut child = PolicyPack {
        region: "US-CA".into(),
        consent_required: true,
        features: vec!["extra".into()],
        parent: Some("US".into()),
    };
    child = child.resolve();
    let mut expected = PolicyPack::template("US").unwrap();
    expected.features.push("extra".into());
    expected.region = "US-CA".into();
    assert_eq!(child.features, expected.features);
}
