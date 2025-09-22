use std::path::PathBuf;

use dependency_registry::{build_registry, BuildOptions, PolicyConfig, RiskTier, ViolationKind};

fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/simple_workspace")
        .join(relative)
}

#[test]
fn assigns_policy_tiers_and_detects_license_violations() {
    let manifest = fixture_path("Cargo.toml");
    let policy_path = fixture_path("policy.toml");
    let policy = PolicyConfig::load(&policy_path).expect("policy should load");

    let build = build_registry(BuildOptions {
        manifest_path: Some(manifest.as_path()),
        policy: &policy,
        config_path: &policy_path,
        override_depth: None,
    })
    .expect("registry build should succeed");

    let dep_b = build
        .registry
        .entries
        .iter()
        .find(|entry| entry.name == "dep_b")
        .expect("dep_b present");
    assert_eq!(dep_b.tier, RiskTier::Forbidden);
    assert!(dep_b.license.as_deref() == Some("AGPL-3.0"));

    let dep_a = build
        .registry
        .entries
        .iter()
        .find(|entry| entry.name == "dep_a")
        .expect("dep_a present");
    assert_eq!(dep_a.tier, RiskTier::Replaceable);

    assert!(build
        .violations
        .entries
        .iter()
        .any(|v| v.name == "dep_b" && matches!(v.kind, ViolationKind::License)));
}

#[test]
fn enforces_maximum_depth() {
    let manifest = fixture_path("Cargo.toml");
    let policy_path = fixture_path("policy.toml");
    let policy = PolicyConfig::load(&policy_path).expect("policy should load");

    let build = build_registry(BuildOptions {
        manifest_path: Some(manifest.as_path()),
        policy: &policy,
        config_path: &policy_path,
        override_depth: Some(1),
    })
    .expect("registry build should succeed");

    assert!(build
        .violations
        .entries
        .iter()
        .any(|v| v.name == "deep_dep" && matches!(v.kind, ViolationKind::Depth)));
}
