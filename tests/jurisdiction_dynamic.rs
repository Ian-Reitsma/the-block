use jurisdiction::PolicyPack;

#[test]
fn policy_rollout_and_revoke() {
    let base = PolicyPack {
        region: "US".into(),
        consent_required: true,
        features: vec!["wallet".into()],
        parent: None,
    };
    let rollout = PolicyPack {
        region: "US".into(),
        consent_required: false,
        features: vec!["wallet".into(), "dex".into()],
        parent: None,
    };
    let diff = PolicyPack::diff(&base, &rollout);
    assert!(diff.consent_required.is_some());
    assert!(diff.features.is_some());
    let diff_json = diff.to_json_value();
    assert!(diff_json.get("consent_required").is_some());
    assert!(diff_json.get("features").is_some());
    let revoke = base.clone();
    let diff_back = PolicyPack::diff(&rollout, &revoke);
    assert!(diff_back.consent_required.is_some());
}
