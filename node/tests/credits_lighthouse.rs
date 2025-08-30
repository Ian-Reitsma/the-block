use credits::Source;
use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::credits::issuance::{issue, set_params, set_region_density, IssuanceParams};

#[test]
fn low_density_increases_rewards() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 0, 0.0, 0);
    set_params(IssuanceParams {
        lighthouse_low_density_multiplier_max: 2_000_000,
        ..Default::default()
    });
    set_region_density("r1", 500_000);
    issue("prov", "r1", Source::Civic, "e1", 100);
    assert_eq!(Settlement::balance("prov"), 150);
}
