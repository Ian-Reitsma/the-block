/// Integration test for economic control laws across 100+ epochs
///
/// This test verifies that the 4-layer economic control system converges
/// to stable equilibrium over time and responds appropriately to market changes.
use the_block::{
    economics::{
        execute_epoch_economics, GovernanceEconomicParams, MarketMetric, MarketMetrics,
        SubsidySnapshot, TariffSnapshot,
    },
    governance::Params,
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
        let subsidy_variance = vec![
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

    // Create controller with 3-streak requirement
    let window_secs = 3600;
    let mut ctrl = the_block::launch_governor::EconomicsController::new(window_secs);

    println!("\n=== Phase 1: Bootstrap (gate inactive) ===");

    // Initially gate should be inactive
    assert!(!ctrl.active(), "Gate should start inactive");
    assert_eq!(ctrl.enter(), 0, "Enter streak should be 0");

    // === Phase 2: Feed healthy economics for 3 epochs to trigger Enter ===
    println!("\n=== Phase 2: Healthy economics (should trigger Enter after 3 streaks) ===");

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

    // Epoch 1: healthy, enter_streak = 1
    let eval1 = ctrl.evaluate(1, &healthy_sample);
    assert!(eval1.is_none(), "Should not produce intent on first healthy sample");
    assert_eq!(ctrl.enter(), 1, "Enter streak should be 1");
    println!("Epoch 1: enter_streak={}, active={}", ctrl.enter(), ctrl.active());

    // Epoch 2: healthy, enter_streak = 2
    let eval2 = ctrl.evaluate(2, &healthy_sample);
    assert!(eval2.is_none(), "Should not produce intent on second healthy sample");
    assert_eq!(ctrl.enter(), 2, "Enter streak should be 2");
    println!("Epoch 2: enter_streak={}, active={}", ctrl.enter(), ctrl.active());

    // Epoch 3: healthy, enter_streak = 3, should trigger Enter
    let eval3 = ctrl.evaluate(3, &healthy_sample);
    assert!(eval3.is_some(), "Should produce Enter intent on third healthy sample");
    let eval3 = eval3.unwrap();
    assert_eq!(eval3.action, GateAction::Enter, "Action should be Enter");
    assert!(ctrl.active(), "Gate should be active after Enter");
    assert_eq!(ctrl.enter(), 3, "Enter streak should be 3");
    println!("Epoch 3: enter_streak={}, active={}, action={:?}", ctrl.enter(), ctrl.active(), eval3.action);
    println!("✓ Gate entered after 3 healthy epochs");

    // === Phase 3: Feed unhealthy economics for 3 epochs to trigger Exit ===
    println!("\n=== Phase 3: Unhealthy economics (should trigger Exit after 3 streaks) ===");

    // Create unhealthy sample (dead chain: insufficient activity)
    let unhealthy_sample = EconomicsSample {
        epoch_tx_count: 5,  // Below MIN_TX_COUNT=10
        epoch_tx_volume_block: 500,  // Below MIN_TX_VOLUME=1000
        epoch_treasury_inflow_block: 0,  // Zero treasury
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

    // Epoch 4: unhealthy, exit_streak = 1
    let eval4 = ctrl.evaluate(4, &unhealthy_sample);
    assert!(eval4.is_none(), "Should not produce intent on first unhealthy sample");
    assert_eq!(ctrl.exit(), 1, "Exit streak should be 1");
    assert_eq!(ctrl.enter(), 0, "Enter streak should reset to 0");
    assert!(ctrl.active(), "Gate should still be active");
    println!("Epoch 4: exit_streak={}, active={}", ctrl.exit(), ctrl.active());

    // Epoch 5: unhealthy, exit_streak = 2
    let eval5 = ctrl.evaluate(5, &unhealthy_sample);
    assert!(eval5.is_none(), "Should not produce intent on second unhealthy sample");
    assert_eq!(ctrl.exit(), 2, "Exit streak should be 2");
    println!("Epoch 5: exit_streak={}, active={}", ctrl.exit(), ctrl.active());

    // Epoch 6: unhealthy, exit_streak = 3, should trigger Exit
    let eval6 = ctrl.evaluate(6, &unhealthy_sample);
    assert!(eval6.is_some(), "Should produce Exit intent on third unhealthy sample");
    let eval6 = eval6.unwrap();
    assert_eq!(eval6.action, GateAction::Exit, "Action should be Exit");
    assert!(!ctrl.active(), "Gate should be inactive after Exit");
    assert_eq!(ctrl.exit(), 3, "Exit streak should be 3");
    println!("Epoch 6: exit_streak={}, active={}, action={:?}", ctrl.exit(), ctrl.active(), eval6.action);
    println!("✓ Gate exited after 3 unhealthy epochs");

    // === Phase 4: Feed healthy economics again to trigger re-Enter ===
    println!("\n=== Phase 4: Return to healthy economics (should trigger re-Enter) ===");

    // Epoch 7: healthy, enter_streak = 1
    let eval7 = ctrl.evaluate(7, &healthy_sample);
    assert!(eval7.is_none(), "Should not produce intent on first healthy sample");
    assert_eq!(ctrl.enter(), 1, "Enter streak should be 1");
    assert_eq!(ctrl.exit(), 0, "Exit streak should reset to 0");
    println!("Epoch 7: enter_streak={}, active={}", ctrl.enter(), ctrl.active());

    // Epoch 8: healthy, enter_streak = 2
    let eval8 = ctrl.evaluate(8, &healthy_sample);
    assert!(eval8.is_none(), "Should not produce intent on second healthy sample");
    assert_eq!(ctrl.enter(), 2, "Enter streak should be 2");
    println!("Epoch 8: enter_streak={}, active={}", ctrl.enter(), ctrl.active());

    // Epoch 9: healthy, enter_streak = 3, should trigger re-Enter
    let eval9 = ctrl.evaluate(9, &healthy_sample);
    assert!(eval9.is_some(), "Should produce Enter intent on third healthy sample");
    let eval9 = eval9.unwrap();
    assert_eq!(eval9.action, GateAction::Enter, "Action should be Enter");
    assert!(ctrl.active(), "Gate should be active after re-Enter");
    assert_eq!(ctrl.enter(), 3, "Enter streak should be 3");
    println!("Epoch 9: enter_streak={}, active={}, action={:?}", ctrl.enter(), ctrl.active(), eval9.action);
    println!("✓ Gate re-entered after returning to healthy economics");

    println!("\n✓ Launch Governor economics gate lifecycle test completed successfully");
    println!("  - Gate activated when economics became healthy");
    println!("  - Gate deactivated when economics became unhealthy");
    println!("  - Gate reactivated when economics recovered");
}
