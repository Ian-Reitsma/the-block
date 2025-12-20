/// Integration test for economic control laws across 100+ epochs
///
/// This test verifies that the 4-layer economic control system converges
/// to stable equilibrium over time and responds appropriately to market changes.
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    sync::{Arc, Mutex},
};

use sys::tempfile::tempdir;

use the_block::{
    economics::{
        execute_epoch_economics, GovernanceEconomicParams, MarketMetric, MarketMetrics,
        SubsidySnapshot, TariffSnapshot,
    },
    governance::Params,
    launch_governor::{LiveSignalProvider, SignalProvider},
    ledger_binary, Blockchain, ChainDisk, TokenAmount,
};

#[test]
fn test_economic_convergence_over_100_epochs() {
    // Initialize with default governance parameters
    let gov_params = Params::default();

    // Bootstrap values
    let mut prev_annual_issuance_block = 40_000_000u64; // Bootstrap: 40M BLOCK/year
    let mut prev_subsidy = SubsidySnapshot {
        storage_share_bps: 1500, // 15%
        compute_share_bps: 3000, // 30%
        energy_share_bps: 2000,  // 20%
        ad_share_bps: 3500,      // 35%
    };
    let mut prev_tariff = TariffSnapshot {
        tariff_bps: 0,
        non_kyc_volume_block: 0,
        treasury_contribution_bps: 0,
    };

    // Starting conditions
    // Circulating supply adjusted for 40M annual issuance
    // Target: 5% inflation → need ~1B circulating to allow controller flexibility
    // Starting: 40M / 1B = 4% → controller increases to ~50M / 1B = 5%
    let circulating_block = 1_000_000_000u64;

    // Track convergence metrics
    let mut inflation_history = Vec::new();
    let mut subsidy_variance_history = Vec::new();

    // Simulate 150 epochs to ensure convergence
    for epoch in 1..=150 {
        // Convert governance params
        let econ_params = GovernanceEconomicParams::from_governance_params(
            &gov_params,
            prev_annual_issuance_block,
            prev_subsidy.clone(),
            prev_tariff.clone(),
            100,    // baseline_tx_count
            10_000, // baseline_tx_volume
            10,     // baseline_miners
        );

        // Simulate market conditions that vary over time
        let storage_util = 0.40 + (epoch as f64 * 0.01).sin() * 0.05;
        let compute_util = 0.60 + (epoch as f64 * 0.02).sin() * 0.10;
        let energy_util = 0.50;
        let ad_util = 0.50;

        let metrics = MarketMetrics {
            storage: MarketMetric {
                utilization: storage_util,
                provider_margin: 0.50,
                ..Default::default()
            },
            compute: MarketMetric {
                utilization: compute_util,
                provider_margin: 0.50,
                ..Default::default()
            },
            energy: MarketMetric {
                utilization: energy_util,
                provider_margin: 0.25,
                ..Default::default()
            },
            ad: MarketMetric {
                utilization: ad_util,
                provider_margin: 0.30,
                ..Default::default()
            },
        };

        // Network activity metrics for formula-based issuance
        let network_activity = the_block::economics::NetworkActivity {
            tx_count: 100, // Baseline transaction activity
            tx_volume_block: 10_000,
            unique_miners: 10,
            block_height: epoch * 120, // Assuming 120 blocks per epoch
        };

        // Execute economic control laws
        let snapshot = execute_epoch_economics(
            epoch,
            &metrics,
            &network_activity,
            circulating_block,
            epoch * 1000, // Rough estimate: total emission grows each epoch
            100_000_000,  // non-KYC volume
            50_000_000,   // ad spend
            10_000_000,   // treasury inflow
            &econ_params,
        );

        assert!(
            snapshot.inflation.block_reward_per_block > 0,
            "block reward must be positive"
        );

        // Record metrics
        inflation_history.push(snapshot.inflation.realized_inflation_bps);

        // Compute subsidy variance (should decrease over time as system stabilizes)
        let subsidy_variance = [
            snapshot.subsidies.storage_share_bps,
            snapshot.subsidies.compute_share_bps,
            snapshot.subsidies.energy_share_bps,
            snapshot.subsidies.ad_share_bps,
        ]
        .iter()
        .map(|&x| {
            let mean = 2500.0; // 25% if evenly distributed
            let diff = (x as f64) - mean;
            diff * diff
        })
        .sum::<f64>()
            / 4.0;
        subsidy_variance_history.push(subsidy_variance);

        // Update for next epoch
        prev_annual_issuance_block = snapshot.inflation.annual_issuance_block;
        prev_subsidy = snapshot.subsidies.clone();
        prev_tariff = snapshot.tariff.clone();

        // Log progress every 25 epochs
        if epoch % 25 == 0 {
            println!(
                "Epoch {}: Inflation={} bps, Issuance={} BLOCK, Subsidies=[{}%, {}%, {}%, {}%], Tariff={} bps",
                epoch,
                snapshot.inflation.realized_inflation_bps,
                snapshot.inflation.annual_issuance_block,
                snapshot.subsidies.storage_share_bps / 100,
                snapshot.subsidies.compute_share_bps / 100,
                snapshot.subsidies.energy_share_bps / 100,
                snapshot.subsidies.ad_share_bps / 100,
                snapshot.tariff.tariff_bps,
            );
        }
    }

    // === Verification: System should remain stable with formula-driven issuance ===

    // 1. Formula-driven inflation should be stable (not oscillating wildly)
    let final_inflation = inflation_history.last().unwrap();
    println!(
        "Final inflation: {} bps (formula-driven, no target)",
        final_inflation
    );

    // Check inflation is reasonable (not zero, not runaway)
    assert!(
        *final_inflation > 0 && *final_inflation < 2000,
        "Inflation should be reasonable: got {} bps",
        final_inflation
    );

    // Check inflation is relatively stable (last 20 epochs should have low variance)
    let late_inflation: Vec<u16> = inflation_history.iter().rev().take(20).copied().collect();
    let late_avg =
        late_inflation.iter().map(|&x| x as f64).sum::<f64>() / late_inflation.len() as f64;
    let late_variance = late_inflation
        .iter()
        .map(|&x| {
            let diff = (x as f64) - late_avg;
            diff * diff
        })
        .sum::<f64>()
        / late_inflation.len() as f64;
    let late_stddev = late_variance.sqrt();

    println!(
        "Late inflation stability: avg={:.1} bps, stddev={:.1} bps",
        late_avg, late_stddev
    );

    // Inflation should be stable (low variance in final epochs)
    assert!(
        late_stddev < 50.0,
        "Inflation should be stable in late epochs: stddev={:.1} bps",
        late_stddev
    );

    // 2. Subsidy variance should decrease (system stabilizing)
    let early_variance = subsidy_variance_history[10..30].iter().sum::<f64>() / 20.0;
    let late_variance = subsidy_variance_history[130..150].iter().sum::<f64>() / 20.0;
    println!(
        "Early subsidy variance: {:.2}, Late subsidy variance: {:.2}",
        early_variance, late_variance
    );

    // Variance should reduce by at least 20% as system stabilizes
    assert!(
        late_variance < early_variance * 0.80,
        "Subsidy allocation failed to stabilize"
    );

    // 3. All subsidy shares should sum to 10000 bps
    assert_eq!(
        prev_subsidy.storage_share_bps as u32
            + prev_subsidy.compute_share_bps as u32
            + prev_subsidy.energy_share_bps as u32
            + prev_subsidy.ad_share_bps as u32,
        10_000,
        "Subsidy shares must sum to 100%"
    );

    println!("✓ Economic control laws converged successfully over 150 epochs");
}

#[test]
fn test_economic_response_to_market_shock() {
    // Test that the system responds appropriately to sudden market changes
    let gov_params = Params::default();

    let mut prev_annual_issuance_block = 200_000_000u64;
    let mut prev_subsidy = SubsidySnapshot {
        storage_share_bps: 2500,
        compute_share_bps: 2500,
        energy_share_bps: 2500,
        ad_share_bps: 2500,
    };
    let mut prev_tariff = TariffSnapshot {
        tariff_bps: 50,
        non_kyc_volume_block: 100_000_000,
        treasury_contribution_bps: 500,
    };

    // Baseline metrics (all markets healthy)
    let baseline_metrics = MarketMetrics {
        storage: MarketMetric {
            utilization: 0.40,
            provider_margin: 0.50,
            ..Default::default()
        },
        compute: MarketMetric {
            utilization: 0.60,
            provider_margin: 0.50,
            ..Default::default()
        },
        energy: MarketMetric {
            utilization: 0.50,
            provider_margin: 0.25,
            ..Default::default()
        },
        ad: MarketMetric {
            utilization: 0.50,
            provider_margin: 0.30,
            ..Default::default()
        },
    };

    // Run baseline epoch
    let econ_params = GovernanceEconomicParams::from_governance_params(
        &gov_params,
        prev_annual_issuance_block,
        prev_subsidy.clone(),
        prev_tariff.clone(),
        100,    // baseline_tx_count
        10_000, // baseline_tx_volume
        10,     // baseline_miners
    );

    let network_activity = the_block::economics::NetworkActivity {
        tx_count: 100,
        tx_volume_block: 10_000,
        unique_miners: 10,
        block_height: 100 * 120,
    };

    let baseline_snapshot = execute_epoch_economics(
        100,
        &baseline_metrics,
        &network_activity,
        4_000_000_000,
        10_000_000, // total emission
        100_000_000,
        50_000_000,
        10_000_000,
        &econ_params,
    );

    prev_annual_issuance_block = baseline_snapshot.inflation.annual_issuance_block;
    prev_subsidy = baseline_snapshot.subsidies.clone();
    prev_tariff = baseline_snapshot.tariff.clone();

    // === Apply market shock: Energy market becomes distressed ===
    let shocked_metrics = MarketMetrics {
        storage: MarketMetric {
            utilization: 0.40,
            provider_margin: 0.50,
            ..Default::default()
        },
        compute: MarketMetric {
            utilization: 0.60,
            provider_margin: 0.50,
            ..Default::default()
        },
        energy: MarketMetric {
            utilization: 0.20,      // Low utilization
            provider_margin: -0.15, // Unprofitable!
            ..Default::default()
        },
        ad: MarketMetric {
            utilization: 0.50,
            provider_margin: 0.30,
            ..Default::default()
        },
    };

    let econ_params = GovernanceEconomicParams::from_governance_params(
        &gov_params,
        prev_annual_issuance_block,
        prev_subsidy.clone(),
        prev_tariff.clone(),
        100,    // baseline_tx_count
        10_000, // baseline_tx_volume
        10,     // baseline_miners
    );

    let shocked_activity = the_block::economics::NetworkActivity {
        tx_count: 100,
        tx_volume_block: 10_000,
        unique_miners: 10,
        block_height: 101 * 120,
    };

    let shocked_snapshot = execute_epoch_economics(
        101,
        &shocked_metrics,
        &shocked_activity,
        4_000_000_000,
        10_100_000, // total emission (slightly higher)
        100_000_000,
        50_000_000,
        10_000_000,
        &econ_params,
    );

    // === Verification: System should respond to distress ===

    // Energy subsidy should increase to help distressed market
    assert!(
        shocked_snapshot.subsidies.energy_share_bps > baseline_snapshot.subsidies.energy_share_bps,
        "Energy subsidy should increase in response to market distress. Before: {}, After: {}",
        baseline_snapshot.subsidies.energy_share_bps,
        shocked_snapshot.subsidies.energy_share_bps
    );

    println!(
        "✓ Energy subsidy increased from {}% to {}% in response to market shock",
        baseline_snapshot.subsidies.energy_share_bps / 100,
        shocked_snapshot.subsidies.energy_share_bps / 100
    );
}

#[test]
fn test_tariff_controller_convergence() {
    // Test that the tariff controller adjusts to maintain target treasury contribution
    let gov_params = Params::default();

    let mut prev_tariff = TariffSnapshot {
        tariff_bps: 10, // Start at 0.1%
        non_kyc_volume_block: 200_000_000,
        treasury_contribution_bps: 100, // Only 1% of treasury (target is 10%)
    };

    // Fixed metrics for this test
    let metrics = MarketMetrics::default();

    for epoch in 1..=50 {
        let econ_params = GovernanceEconomicParams::from_governance_params(
            &gov_params,
            200_000_000,
            SubsidySnapshot {
                storage_share_bps: 2500,
                compute_share_bps: 2500,
                energy_share_bps: 2500,
                ad_share_bps: 2500,
            },
            prev_tariff.clone(),
            100,    // baseline_tx_count
            10_000, // baseline_tx_volume
            10,     // baseline_miners
        );

        let network_activity = the_block::economics::NetworkActivity {
            tx_count: 100,
            tx_volume_block: 200_000_000, // Match non-KYC volume
            unique_miners: 10,
            block_height: epoch * 120,
        };

        let snapshot = execute_epoch_economics(
            epoch,
            &metrics,
            &network_activity,
            4_000_000_000,
            epoch * 100_000, // Gradual emission growth
            200_000_000,     // Consistent non-KYC volume
            50_000_000,
            20_000_000, // Treasury inflow
            &econ_params,
        );

        prev_tariff = snapshot.tariff.clone();

        if epoch % 10 == 0 {
            println!(
                "Epoch {}: Tariff={} bps, Treasury contribution={} bps (target: 1000 bps)",
                epoch, snapshot.tariff.tariff_bps, snapshot.tariff.treasury_contribution_bps
            );
        }
    }

    // Tariff controller should converge toward target contribution (1000 bps = 10%)
    let final_contribution = prev_tariff.treasury_contribution_bps;
    let contribution_error = (final_contribution as i32 - 1000).abs();

    println!(
        "Final treasury contribution: {} bps (target: 1000 bps)",
        final_contribution
    );

    // Should be within 200 bps of target after 50 epochs
    assert!(
        contribution_error < 200,
        "Tariff controller failed to converge: {} bps from target",
        contribution_error
    );

    println!("✓ Tariff controller converged successfully");
}

#[test]
fn test_launch_governor_economics_gate_lifecycle() {
    // Integration test for Launch Governor economics gate showing full lifecycle:
    // healthy → unhealthy → healthy with gate Enter/Exit actions

    use the_block::launch_governor::{EconomicsSample, GateAction};

    // Create controller whose streak length ties back to runtime window sizing.
    let window_secs = 3600;
    let mut ctrl = the_block::launch_governor::EconomicsController::new(window_secs);
    let required = ctrl.required();

    println!("\n=== Phase 1: Bootstrap (gate inactive) ===");

    // Initially gate should be inactive
    assert!(!ctrl.active(), "Gate should start inactive");
    assert_eq!(ctrl.enter(), 0, "Enter streak should be 0");

    // === Phase 2: Feed healthy economics for {required} epochs to trigger Enter ===
    println!(
        "\n=== Phase 2: Healthy economics (should trigger Enter after {required} streaks) ==="
    );

    let healthy_sample = EconomicsSample {
        epoch_tx_count: 100,
        epoch_tx_volume_block: 10_000,
        epoch_treasury_inflow_block: 5_000,
        block_reward_per_block: 100,
        market_metrics: MarketMetrics {
            storage: MarketMetric {
                utilization: 0.40,
                provider_margin: 0.50,
                ..Default::default()
            },
            compute: MarketMetric {
                utilization: 0.60,
                provider_margin: 0.50,
                ..Default::default()
            },
            energy: MarketMetric {
                utilization: 0.50,
                provider_margin: 0.25,
                ..Default::default()
            },
            ad: MarketMetric {
                utilization: 0.50,
                provider_margin: 0.30,
                ..Default::default()
            },
        },
    };

    let warmup = ctrl.evaluate(1, &healthy_sample);
    assert!(warmup.is_none(), "Warm-up sample should not trigger intent");

    for epoch in 2..=required + 1 {
        let eval = ctrl.evaluate(epoch, &healthy_sample);
        if epoch < required + 1 {
            assert!(
                eval.is_none(),
                "Should not produce intent before entering streak"
            );
            assert_eq!(
                ctrl.enter(),
                epoch - 1,
                "Enter streak should increment each healthy epoch"
            );
        } else {
            let enter_eval =
                eval.expect("Should produce Enter intent once healthy streak requirement is met");
            assert_eq!(
                enter_eval.action,
                GateAction::Enter,
                "Action should be Enter"
            );
            assert!(ctrl.active(), "Gate should be active after Enter");
            assert_eq!(
                ctrl.enter(),
                required,
                "Enter streak should match required streak after Enter intent"
            );
            println!(
                "Epoch {epoch}: enter_streak={}, active={}, action={:?}",
                ctrl.enter(),
                ctrl.active(),
                enter_eval.action
            );
        }
    }
    println!("✓ Gate entered after {required} healthy epochs");

    // === Phase 3: Feed unhealthy economics for 3 epochs to trigger Exit ===
    println!(
        "\n=== Phase 3: Unhealthy economics (should trigger Exit after {required} streaks) ==="
    );

    // Create unhealthy sample (dead chain: insufficient activity)
    let unhealthy_sample = EconomicsSample {
        epoch_tx_count: 5,              // Below MIN_TX_COUNT=10
        epoch_tx_volume_block: 500,     // Below MIN_TX_VOLUME=1000
        epoch_treasury_inflow_block: 0, // Zero treasury
        block_reward_per_block: 100,
        market_metrics: MarketMetrics {
            storage: MarketMetric {
                utilization: 0.40,
                provider_margin: 0.50,
                ..Default::default()
            },
            compute: MarketMetric {
                utilization: 0.60,
                provider_margin: 0.50,
                ..Default::default()
            },
            energy: MarketMetric {
                utilization: 0.50,
                provider_margin: 0.25,
                ..Default::default()
            },
            ad: MarketMetric {
                utilization: 0.50,
                provider_margin: 0.30,
                ..Default::default()
            },
        },
    };

    let unhealthy_start = required + 2;
    for offset in 0..required {
        let epoch = unhealthy_start + offset;
        let eval = ctrl.evaluate(epoch, &unhealthy_sample);
        if offset < required - 1 {
            assert!(
                eval.is_none(),
                "Should not produce exit intent before required streak"
            );
            assert_eq!(
                ctrl.exit(),
                offset + 1,
                "Exit streak should increment for each unhealthy epoch"
            );
            assert!(
                ctrl.active(),
                "Gate should remain active until exit intent fires"
            );
        } else {
            let exit_eval =
                eval.expect("Should produce Exit intent once exit streak requirement is met");
            assert_eq!(exit_eval.action, GateAction::Exit, "Action should be Exit");
            assert!(!ctrl.active(), "Gate should be inactive after Exit");
            assert_eq!(
                ctrl.exit(),
                required,
                "Exit streak should match required streak after exit intent"
            );
            println!(
                "Epoch {epoch}: exit_streak={}, active={}, action={:?}",
                ctrl.exit(),
                ctrl.active(),
                exit_eval.action
            );
        }
    }
    println!("✓ Gate exited after {required} unhealthy epochs");

    // === Phase 4: Feed healthy economics again to trigger re-Enter ===
    println!(
        "\n=== Phase 4: Return to healthy economics (should trigger re-Enter after {required} healthy epochs) ==="
    );

    let recovery_start = unhealthy_start + required;
    for offset in 0..required {
        let epoch = recovery_start + offset;
        let eval = ctrl.evaluate(epoch, &healthy_sample);
        if offset < required - 1 {
            assert!(
                eval.is_none(),
                "Should not produce re-enter intent before required streak"
            );
            assert_eq!(
                ctrl.enter(),
                offset + 1,
                "Enter streak should increment while recovering"
            );
            assert_eq!(
                ctrl.exit(),
                0,
                "Exit streak should reset once healthy sampling begins"
            );
        } else {
            let reenter_eval =
                eval.expect("Should produce Enter intent once healthy streak requirement is met");
            assert_eq!(
                reenter_eval.action,
                GateAction::Enter,
                "Action should be Enter"
            );
            assert!(ctrl.active(), "Gate should be active after re-Enter");
            assert_eq!(
                ctrl.enter(),
                required,
                "Enter streak should match required streak after re-Enter"
            );
            println!(
                "Epoch {epoch}: enter_streak={}, active={}, action={:?}",
                ctrl.enter(),
                ctrl.active(),
                reenter_eval.action
            );
        }
    }
    println!("✓ Gate re-entered after returning to healthy economics");

    println!("\n✓ Launch Governor economics gate lifecycle test completed successfully");
    println!("  - Gate activated when economics became healthy");
    println!("  - Gate deactivated when economics became unhealthy");
    println!("  - Gate reactivated when economics recovered");
}

#[test]
fn test_chain_disk_roundtrip_preserves_market_metrics() {
    let metrics = sample_market_metrics();
    let disk = ChainDisk {
        schema_version: 42,
        chain: Vec::new(),
        accounts: HashMap::new(),
        emission: 0,
        emission_year_ago: 0,
        inflation_epoch_marker: 0,
        block_reward: TokenAmount::new(0),
        block_height: 0,
        mempool: Vec::new(),
        base_fee: 1,
        params: Params::default(),
        epoch_storage_bytes: 0,
        epoch_read_bytes: 0,
        epoch_cpu_ms: 0,
        epoch_bytes_out: 0,
        recent_timestamps: Vec::new(),
        economics_block_reward_per_block: 0,
        economics_prev_annual_issuance_block: 0,
        economics_prev_subsidy: SubsidySnapshot::default(),
        economics_prev_tariff: TariffSnapshot::default(),
        economics_prev_market_metrics: metrics.clone(),
        economics_epoch_tx_volume_block: 0,
        economics_epoch_tx_count: 0,
        economics_epoch_treasury_inflow_block: 0,
        economics_epoch_storage_payout_block: 0,
        economics_epoch_compute_payout_block: 0,
        economics_epoch_ad_payout_block: 0,
        economics_baseline_tx_count: 100,
        economics_baseline_tx_volume: 10_000,
        economics_baseline_miners: 10,
    };

    let encoded =
        ledger_binary::encode_chain_disk(&disk).expect("encoding chain disk should succeed");
    let decoded =
        ledger_binary::decode_chain_disk(&encoded).expect("decoding chain disk should succeed");

    assert_market_metrics_close(&metrics, &decoded.economics_prev_market_metrics);
}

#[test]
fn test_launch_governor_economics_sample_retains_metrics_after_restart() {
    let _preserve = EnvVarGuard::set("TB_PRESERVE", "1");
    let tmp_dir = tempdir().expect("create temporary directory");
    let db_path = tmp_dir.path().to_str().expect("valid path").to_owned();
    let expected_metrics = sample_market_metrics();

    {
        let mut blockchain =
            Blockchain::open(&db_path).expect("open blockchain before persisting metrics");
        blockchain.economics_prev_market_metrics = expected_metrics.clone();
        blockchain.persist_chain().expect("persist chain disk");
    }

    let reopened = Blockchain::open(&db_path).expect("re-open blockchain after restart");
    {
        let chain = Arc::new(Mutex::new(reopened));
        let provider = LiveSignalProvider::new(chain.clone());
        let sample = provider.economics_sample(0);
        assert_market_metrics_close(&expected_metrics, &sample.market_metrics);
    }
}

const METRIC_EPSILON: f64 = 1.0e-6;

fn sample_market_metrics() -> MarketMetrics {
    MarketMetrics {
        storage: market_metric(0.31, 120.5, 130.0, 0.12),
        compute: market_metric(0.72, 210.0, 190.0, -0.05),
        energy: market_metric(0.58, 310.25, 315.0, 0.02),
        ad: market_metric(0.12, 42.999_999, 43.0, 0.01),
    }
}

fn market_metric(utilization: f64, average_cost: f64, payout: f64, margin: f64) -> MarketMetric {
    MarketMetric {
        utilization,
        average_cost_block: average_cost,
        effective_payout_block: payout,
        provider_margin: margin,
    }
}

fn assert_market_metrics_close(expected: &MarketMetrics, actual: &MarketMetrics) {
    assert_market_metric_close(&expected.storage, &actual.storage);
    assert_market_metric_close(&expected.compute, &actual.compute);
    assert_market_metric_close(&expected.energy, &actual.energy);
    assert_market_metric_close(&expected.ad, &actual.ad);
}

fn assert_market_metric_close(expected: &MarketMetric, actual: &MarketMetric) {
    assert!(
        (expected.utilization - actual.utilization).abs() <= METRIC_EPSILON,
        "utilization mismatch: expected {expected:?}, actual {actual:?}"
    );
    assert!(
        (expected.average_cost_block - actual.average_cost_block).abs() <= METRIC_EPSILON,
        "average cost mismatch: expected {expected:?}, actual {actual:?}"
    );
    assert!(
        (expected.effective_payout_block - actual.effective_payout_block).abs() <= METRIC_EPSILON,
        "effective payout mismatch: expected {expected:?}, actual {actual:?}"
    );
    assert!(
        (expected.provider_margin - actual.provider_margin).abs() <= METRIC_EPSILON,
        "provider margin mismatch: expected {expected:?}, actual {actual:?}"
    );
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = env::var_os(key);
        env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            env::set_var(self.key, previous);
        } else {
            env::remove_var(self.key);
        }
    }
}
