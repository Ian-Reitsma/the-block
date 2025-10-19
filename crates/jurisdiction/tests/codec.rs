use jurisdiction::{
    decode_policy_diff, decode_policy_pack, decode_signed_pack, encode_policy_diff,
    encode_policy_pack, encode_signed_pack, load_signed_pack, persist_signed_pack, PolicyPack,
    SignedPack,
};
use sys::tempfile::tempdir;

#[test]
fn policy_pack_binary_roundtrip() {
    let pack = PolicyPack {
        region: "US".into(),
        consent_required: true,
        features: vec!["wallet".into(), "dex".into()],
        parent: Some("US".into()),
    };

    let encoded = encode_policy_pack(&pack);
    let decoded = decode_policy_pack(&encoded).expect("decode policy pack");
    assert_eq!(decoded, pack);
}

#[test]
fn signed_pack_binary_roundtrip() {
    let pack = PolicyPack {
        region: "EU".into(),
        consent_required: false,
        features: vec!["wallet".into()],
        parent: None,
    };
    let signed = SignedPack {
        pack: pack.clone(),
        signature: vec![1, 2, 3, 4],
    };

    let encoded = encode_signed_pack(&signed);
    let decoded = decode_signed_pack(&encoded).expect("decode signed pack");
    assert_eq!(decoded.pack, pack);
    assert_eq!(decoded.signature, vec![1, 2, 3, 4]);
}

#[test]
fn policy_diff_binary_roundtrip() {
    let base = PolicyPack {
        region: "US".into(),
        consent_required: true,
        features: vec!["wallet".into()],
        parent: None,
    };
    let updated = PolicyPack {
        region: "US".into(),
        consent_required: false,
        features: vec!["wallet".into(), "dex".into()],
        parent: None,
    };

    let diff = PolicyPack::diff(&base, &updated);
    let encoded = encode_policy_diff(&diff);
    let decoded = decode_policy_diff(&encoded).expect("decode diff");
    assert_eq!(decoded.consent_required, diff.consent_required);
    assert_eq!(decoded.features, diff.features);
}

#[test]
fn persist_and_load_signed_pack_roundtrip() {
    let dir = tempdir().expect("tempdir");
    let json_path = dir.path().join("policy.json");
    let pack = PolicyPack {
        region: "US".into(),
        consent_required: true,
        features: vec!["wallet".into()],
        parent: None,
    };
    let signed = SignedPack {
        pack: pack.clone(),
        signature: vec![42, 43, 44],
    };

    persist_signed_pack(&json_path, &signed).expect("persist pack");
    assert!(json_path.exists());
    assert!(json_path.with_extension("bin").exists());

    let loaded = load_signed_pack(&json_path).expect("load pack");
    assert_eq!(loaded.pack, pack);
    assert_eq!(loaded.signature, signed.signature);
}
