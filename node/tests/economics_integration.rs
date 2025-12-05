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
    let mut prev_annual_issuance_ct = 200_000_000u64;
    let mut prev_subsidy = SubsidySnapshot {
        storage_share_bps: 1500, // 15%
        compute_share_bps: 3000, // 30%
        energy_share_bps: 2000,  // 20%
        ad_share_bps: 3500,      // 35%
    };
    let mut prev_tariff = TariffSnapshot {
        tariff_bps: 0,
        non_kyc_volume_ct: 0,
        treasury_contribution_bps: 0,
    };

    // Starting conditions
    let circulating_ct = 4_000_000_000u64;

    // Track convergence metrics
    let mut inflation_history = Vec::new();
    let mut subsidy_variance_history = Vec::new();

    // Simulate 150 epochs to ensure convergence
    for epoch in 1..=150 {
        // Convert governance params
        let econ_params = GovernanceEconomicParams::from_governance_params(
            &gov_params,
            prev_annual_issuance_ct,
            prev_subsidy.clone(),
            prev_tariff.clone(),
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

        // Execute economic control laws
        let snapshot = execute_epoch_economics(
            epoch,
            &metrics,
            circulating_ct,
            100_000_000, // non-KYC volume
            50_000_000,  // ad spend
            10_000_000,  // treasury inflow
            &econ_params,
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
        prev_annual_issuance_ct = snapshot.inflation.annual_issuance_ct;
        prev_subsidy = snapshot.subsidies.clone();
        prev_tariff = snapshot.tariff.clone();

        // Log progress every 25 epochs
        if epoch % 25 == 0 {
            println!(
                "Epoch {}: Inflation={} bps, Issuance={} CT, Subsidies=[{}%, {}%, {}%, {}%], Tariff={} bps",
                epoch,
                snapshot.inflation.realized_inflation_bps,
                snapshot.inflation.annual_issuance_ct,
                snapshot.subsidies.storage_share_bps / 100,
                snapshot.subsidies.compute_share_bps / 100,
                snapshot.subsidies.energy_share_bps / 100,
                snapshot.subsidies.ad_share_bps / 100,
                snapshot.tariff.tariff_bps,
            );
        }
    }

    // === Verification: System should converge to stable equilibrium ===

    // 1. Inflation should stabilize near target (500 bps = 5%)
    let final_inflation = inflation_history.last().unwrap();
    let inflation_error = (*final_inflation as i32 - 500).abs();
    println!("Final inflation: {} bps (target: 500 bps, error: {} bps)", final_inflation, inflation_error);

    // After 150 epochs, inflation should be within 50 bps of target
    assert!(
        inflation_error < 50,
        "Inflation failed to converge: {} bps from target",
        inflation_error
    );

    // 2. Subsidy variance should decrease (system stabilizing)
    let early_variance = subsidy_variance_history[10..30].iter().sum::<f64>() / 20.0;
    let late_variance = subsidy_variance_history[130..150].iter().sum::<f64>() / 20.0;
    println!("Early subsidy variance: {:.2}, Late subsidy variance: {:.2}", early_variance, late_variance);

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

    let mut prev_annual_issuance_ct = 200_000_000u64;
    let mut prev_subsidy = SubsidySnapshot {
        storage_share_bps: 2500,
        compute_share_bps: 2500,
        energy_share_bps: 2500,
        ad_share_bps: 2500,
    };
    let mut prev_tariff = TariffSnapshot {
        tariff_bps: 50,
        non_kyc_volume_ct: 100_000_000,
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
        prev_annual_issuance_ct,
        prev_subsidy.clone(),
        prev_tariff.clone(),
    );

    let baseline_snapshot = execute_epoch_economics(
        100,
        &baseline_metrics,
        4_000_000_000,
        100_000_000,
        50_000_000,
        10_000_000,
        &econ_params,
    );

    prev_annual_issuance_ct = baseline_snapshot.inflation.annual_issuance_ct;
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
            utilization: 0.20, // Low utilization
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
        prev_annual_issuance_ct,
        prev_subsidy.clone(),
        prev_tariff.clone(),
    );

    let shocked_snapshot = execute_epoch_economics(
        101,
        &shocked_metrics,
        4_000_000_000,
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
        non_kyc_volume_ct: 200_000_000,
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
        );

        let snapshot = execute_epoch_economics(
            epoch,
            &metrics,
            4_000_000_000,
            200_000_000, // Consistent non-KYC volume
            50_000_000,
            20_000_000,  // Treasury inflow
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

    println!("Final treasury contribution: {} bps (target: 1000 bps)", final_contribution);

    // Should be within 200 bps of target after 50 epochs
    assert!(
        contribution_error < 200,
        "Tariff controller failed to converge: {} bps from target",
        contribution_error
    );

    println!("✓ Tariff controller converged successfully");
}
