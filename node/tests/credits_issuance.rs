use credits::Source;
use serial_test::serial;
use std::collections::HashMap;
use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    credits::issuance::{issue, set_params, IssuanceParams},
};

#[test]
#[serial]
fn credits_issuance_caps() {
    let dir = tempfile::tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0);

    let mut weights = HashMap::new();
    weights.insert(Source::LocalNetAssist, 2_000_000); // 2x
    weights.insert(Source::Civic, 1_000_000);
    weights.insert(Source::Uptime, 1_000_000);
    weights.insert(Source::ProvenStorage, 1_000_000);
    let mut expiry = HashMap::new();
    expiry.insert(Source::LocalNetAssist, 30);
    expiry.insert(Source::Civic, u64::MAX);
    expiry.insert(Source::Uptime, u64::MAX);
    expiry.insert(Source::ProvenStorage, u64::MAX);
    set_params(IssuanceParams {
        weights_ppm: weights,
        cap_per_identity: 3,
        cap_per_region: 10,
        expiry_days: expiry,
    });

    issue("alice", "r1", Source::LocalNetAssist, "e1", 1);
    assert_eq!(Settlement::balance("alice"), 2); // weighted

    issue("alice", "r1", Source::Civic, "e2", 2); // would exceed cap_per_identity
    assert_eq!(Settlement::balance("alice"), 2); // unchanged

    set_params(IssuanceParams::default());
}
