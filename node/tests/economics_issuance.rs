#![cfg(feature = "integration-tests")]

use sys::tempfile::tempdir;
use the_block::{
    economics::{
        network_issuance::NetworkMetrics, GovernanceEconomicParams, MarketMetric,
        NetworkIssuanceController,
    },
    Blockchain,
};

#[test]
fn network_issuance_controller_drives_block_reward() {
    let dir = tempdir().unwrap();
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.genesis_block().expect("genesis");

    // Seed reasonable network activity so issuance controller has signal.
    bc.economics_epoch_tx_count = 25;
    bc.economics_epoch_tx_volume_block = 5_000;
    bc.economics_prev_market_metrics = the_block::economics::MarketMetrics {
        storage: MarketMetric {
            utilization: 0.25,
            ..MarketMetric::default()
        },
        compute: MarketMetric {
            utilization: 0.35,
            ..MarketMetric::default()
        },
        energy: MarketMetric {
            utilization: 0.15,
            ..MarketMetric::default()
        },
        ad: MarketMetric {
            utilization: 0.10,
            ..MarketMetric::default()
        },
    };

    // Build controller inputs to mirror the node's issuance path.
    let econ_params = GovernanceEconomicParams::from_governance_params(
        &bc.params,
        bc.economics_prev_annual_issuance_block,
        bc.economics_prev_subsidy.clone(),
        bc.economics_prev_tariff.clone(),
        bc.economics_baseline_tx_count,
        bc.economics_baseline_tx_volume,
        bc.economics_baseline_miners,
    );
    let mut controller = NetworkIssuanceController::with_baselines(
        econ_params.network_issuance.clone(),
        bc.economics_baseline_tx_count,
        bc.economics_baseline_tx_volume,
        bc.economics_baseline_miners,
    );
    let metrics = NetworkMetrics {
        tx_count: bc.economics_epoch_tx_count,
        tx_volume_block: bc.economics_epoch_tx_volume_block,
        unique_miners: 1,
        avg_market_utilization: 0.2125, // matches the utilization mix above
        block_height: bc.block_height,
        total_emission: bc.emission,
    };
    let expected_reward = controller.compute_block_reward(&metrics);

    let block = bc
        .mine_block_at("miner", 1)
        .expect("block mined with controller reward");

    // Base reward captured in economics state and block_reward for explorer/telemetry parity.
    assert_eq!(bc.economics_block_reward_per_block, expected_reward);
    assert_eq!(bc.block_reward.0, expected_reward);

    // Coinbases must at least include the controller reward (logistic factor defaults to 1.0).
    assert!(
        block.coinbase_block.0 >= expected_reward,
        "coinbase {} should include controller reward {}",
        block.coinbase_block.0,
        expected_reward
    );
}
