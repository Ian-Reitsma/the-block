use foundation_serialization::json;
use foundation_serialization::json::Value;
use jurisdiction::{PolicyPack, SignedPack};

#[test]
fn templates_roundtrip() {
    let us = PolicyPack::template("US").expect("us template");
    let ser = json::to_string_value(&us.to_json_value());
    let parsed = json::value_from_str(&ser).unwrap();
    let de = PolicyPack::from_json_value(&parsed).unwrap();
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

#[test]
fn rejects_invalid_features_entry() {
    let value = foundation_serialization::json!({
        "region": "US",
        "consent_required": true,
        "features": ["wallet", 1],
    });

    let err = PolicyPack::from_json_value(&value).expect_err("invalid feature type");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("features"));
}

#[test]
fn signed_pack_roundtrip_from_array_signature() {
    let pack = PolicyPack {
        region: "US".into(),
        consent_required: true,
        features: vec!["wallet".into()],
        parent: None,
    };

    let signed = SignedPack {
        pack: pack.clone(),
        signature: vec![1, 2, 3],
    };

    let value = signed.to_json_value();
    let restored = SignedPack::from_json_value(&value).expect("parse array signature");
    assert_eq!(restored.pack, pack);
    assert_eq!(restored.signature, vec![1, 2, 3]);
}

#[test]
fn signed_pack_rejects_invalid_signature_variant() {
    let value = foundation_serialization::json!({
        "pack": {
            "region": "US",
            "consent_required": false,
            "features": [],
        },
        "signature": {"unexpected": true},
    });

    let err = SignedPack::from_json_value(&value).expect_err("invalid signature variant");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("signature"));
}

#[test]
fn signed_pack_accepts_base64_signature() {
    let value = foundation_serialization::json!({
        "pack": {
            "region": "EU",
            "consent_required": false,
            "features": ["wallet"],
        },
        "signature": "AQID",
    });

    let parsed = SignedPack::from_json_value(&value).expect("base64 signature");
    assert_eq!(parsed.signature, vec![1, 2, 3]);
    assert_eq!(parsed.pack.region, "EU");
}

#[test]
fn diff_reports_changed_fields() {
    let base = PolicyPack {
        region: "US".into(),
        consent_required: false,
        features: vec!["wallet".into()],
        parent: None,
    };
    let updated = PolicyPack {
        consent_required: true,
        features: vec!["wallet".into(), "dex".into()],
        ..base.clone()
    };

    let diff = PolicyPack::diff(&base, &updated);
    let object = match diff {
        Value::Object(map) => map,
        other => panic!("expected object diff, got {other:?}"),
    };

    let consent = object
        .get("consent_required")
        .expect("consent diff present");
    assert_eq!(
        consent,
        &foundation_serialization::json!({"old": false, "new": true})
    );

    let features = object.get("features").expect("features diff present");
    assert_eq!(
        features,
        &foundation_serialization::json!({
            "old": ["wallet"],
            "new": ["wallet", "dex"],
        })
    );
}
