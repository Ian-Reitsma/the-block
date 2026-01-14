//! Economic control laws for The Block.
//!
//! This module implements the four-layer adaptive control system:
//! 1. **Network-Driven Issuance**: BLOCK rewards based on transactions, nodes, and activity
//! 2. **Subsidy Allocator**: Reallocates subsidies based on market distress
//! 3. **Market Multipliers**: Dual control (utilization + cost-coverage)
//! 4. **Ad & Tariff Controllers**: Drift splits and tariffs toward governance targets

pub mod ad_market_controller;
pub mod deterministic_metrics;
pub mod event;
pub mod inflation_controller; // Legacy - kept for backward compatibility
pub mod market_multiplier;
pub mod network_issuance; // NEW: Formula-driven issuance
pub mod replay; // Consensus-critical economics replay
pub mod subsidy_allocator;

pub use ad_market_controller::{AdMarketDriftController, TariffController};
pub use deterministic_metrics::derive_market_metrics_from_chain;
pub use event::*;
pub use inflation_controller::InflationController; // Legacy
pub use market_multiplier::MarketMultiplierController;
pub use network_issuance::NetworkIssuanceController; // NEW
pub use replay::{replay_economics_to_height, replay_economics_to_tip, ReplayedEconomicsState};
pub use subsidy_allocator::SubsidyAllocator;

use foundation_serialization::{Deserialize, Serialize};

/// Complete economic state snapshot for an epoch
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EconomicSnapshot {
    pub epoch: u64,
    pub inflation: InflationSnapshot,
    pub subsidies: SubsidySnapshot,
    pub multipliers: MultiplierSnapshot,
    pub ad_market: AdMarketSnapshot,
    pub tariff: TariffSnapshot,
    /// Updated adaptive baselines (to be persisted for next epoch)
    pub updated_baseline_tx_count: u64,
    pub updated_baseline_tx_volume: u64,
    pub updated_baseline_miners: u64,
}

/// Inflation state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct InflationSnapshot {
    pub circulating_block: u64,
    pub annual_issuance_block: u64,
    pub realized_inflation_bps: u16,
    pub target_inflation_bps: u16,
    pub block_reward_per_block: u64,
}

/// Subsidy allocation state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SubsidySnapshot {
    pub storage_share_bps: u16,
    pub compute_share_bps: u16,
    pub energy_share_bps: u16,
    pub ad_share_bps: u16,
}

/// Market multiplier state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MultiplierSnapshot {
    pub storage_multiplier: f64,
    pub compute_multiplier: f64,
    pub energy_multiplier: f64,
    pub ad_multiplier: f64,
}

/// Ad market split state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdMarketSnapshot {
    pub platform_take_bps: u16,
    pub user_share_bps: u16,
    pub publisher_share_bps: u16,
}

/// Tariff state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TariffSnapshot {
    pub tariff_bps: u16,
    pub non_kyc_volume_block: u64,
    pub treasury_contribution_bps: u16,
}

impl Default for SubsidySnapshot {
    fn default() -> Self {
        Self {
            storage_share_bps: 0,
            compute_share_bps: 0,
            energy_share_bps: 0,
            ad_share_bps: 0,
        }
    }
}

impl Default for TariffSnapshot {
    fn default() -> Self {
        Self {
            tariff_bps: 0,
            non_kyc_volume_block: 0,
            treasury_contribution_bps: 0,
        }
    }
}

/// Market metrics for control loop input
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MarketMetrics {
    pub storage: MarketMetric,
    pub compute: MarketMetric,
    pub energy: MarketMetric,
    pub ad: MarketMetric,
}

/// Individual market metric
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MarketMetric {
    /// Utilization ratio [0.0, 1.0]
    pub utilization: f64,
    /// Average unit cost in BLOCK
    pub average_cost_block: f64,
    /// Effective unit payout in BLOCK
    pub effective_payout_block: f64,
    /// Realized provider margin ratio (negative if unprofitable)
    pub provider_margin: f64,
}

/// Network activity metrics for formula-driven issuance
#[derive(Debug, Clone)]
pub struct NetworkActivity {
    pub tx_count: u64,
    pub tx_volume_block: u64,
    pub unique_miners: u64,
    pub block_height: u64,
}

/// Execute full economic control loop for an epoch
///
/// # Arguments
/// * `epoch` - Current epoch number
/// * `metrics` - Market utilization and provider margins
/// * `network_activity` - Transaction count, volume, active miners
/// * `circulating_block` - Total BLOCK in circulation
/// * `total_emission` - Total BLOCK minted to date
/// * `non_kyc_volume_block` - Transaction volume from non-KYC accounts
/// * `total_ad_spend_block` - Ad marketplace spending this epoch
/// * `treasury_inflow_block` - Treasury revenue this epoch
/// * `gov_params` - Governance-controlled economic parameters
pub fn execute_epoch_economics(
    epoch: u64,
    metrics: &MarketMetrics,
    network_activity: &NetworkActivity,
    circulating_block: u64,
    total_emission: u64,
    non_kyc_volume_block: u64,
    total_ad_spend_block: u64,
    treasury_inflow_block: u64,
    gov_params: &GovernanceEconomicParams,
) -> EconomicSnapshot {
    // Layer 1: Network-Driven Issuance (Formula-Based)
    // CRITICAL: Use with_baselines() to preserve adaptive baseline state across epochs.
    // Using new() here would reset baselines every epoch, defeating the "adaptive" feature.
    let mut network_issuance = NetworkIssuanceController::with_baselines(
        gov_params.network_issuance.clone(),
        gov_params.network_issuance.baseline_tx_count,
        gov_params.network_issuance.baseline_tx_volume_block,
        gov_params.network_issuance.baseline_miners,
    );

    // Compute average market utilization for activity multiplier
    let avg_market_utilization = (metrics.storage.utilization
        + metrics.compute.utilization
        + metrics.energy.utilization
        + metrics.ad.utilization)
        / 4.0;

    let network_metrics = network_issuance::NetworkMetrics {
        tx_count: network_activity.tx_count,
        tx_volume_block: network_activity.tx_volume_block,
        unique_miners: network_activity.unique_miners,
        avg_market_utilization,
        block_height: network_activity.block_height,
        total_emission,
    };

    let block_reward = network_issuance.compute_block_reward(&network_metrics);
    let annual_issuance = network_issuance.estimate_annual_issuance(block_reward);

    // Compute realized inflation for monitoring
    let realized_inflation_bps = if circulating_block > 0 {
        ((annual_issuance as f64 / circulating_block as f64) * 10_000.0).round() as u16
    } else {
        0
    };

    let inflation = InflationSnapshot {
        circulating_block,
        annual_issuance_block: annual_issuance,
        realized_inflation_bps,
        target_inflation_bps: 0, // No target - formula-driven
        block_reward_per_block: block_reward,
    };

    // Layer 2: Subsidy allocation
    let subsidy_allocator = SubsidyAllocator::new(gov_params.subsidy.clone());
    let subsidies = subsidy_allocator.compute_next_allocation(metrics, &gov_params.subsidy_prev);

    // Layer 3: Market multipliers
    let multiplier_controller = MarketMultiplierController::new(gov_params.multiplier.clone());
    let multipliers = multiplier_controller.compute_multipliers(metrics);

    // Layer 4: Ad & tariff
    let ad_drift_controller = AdMarketDriftController::new(gov_params.ad_market.clone());
    let ad_market = ad_drift_controller.compute_next_splits(total_ad_spend_block);

    let tariff_controller = TariffController::new(gov_params.tariff.clone());
    let tariff = tariff_controller.compute_next_tariff(
        non_kyc_volume_block,
        treasury_inflow_block,
        gov_params.tariff_prev.tariff_bps,
    );

    // Get updated baselines from the controller (after compute_block_reward updated them)
    let (updated_baseline_tx_count, updated_baseline_tx_volume, updated_baseline_miners) =
        network_issuance.get_adaptive_baselines();

    EconomicSnapshot {
        epoch,
        inflation,
        subsidies,
        multipliers,
        ad_market,
        tariff,
        updated_baseline_tx_count,
        updated_baseline_tx_volume,
        updated_baseline_miners,
    }
}

/// All governance parameters needed for economic control laws
#[derive(Debug, Clone)]
pub struct GovernanceEconomicParams {
    pub inflation: inflation_controller::InflationParams, // Legacy - kept for backward compat
    pub network_issuance: network_issuance::NetworkIssuanceParams, // NEW: Formula-driven
    pub subsidy: subsidy_allocator::SubsidyParams,
    pub subsidy_prev: SubsidySnapshot,
    pub multiplier: market_multiplier::MultiplierParams,
    pub ad_market: ad_market_controller::AdMarketParams,
    pub tariff: ad_market_controller::TariffParams,
    pub tariff_prev: TariffSnapshot,
}

impl GovernanceEconomicParams {
    /// Convert from blockchain governance Params to economics params
    ///
    /// Governance stores fractional values as integers (millis = value * 1000)
    /// This function converts them back to f64 for use in controllers
    pub fn from_governance_params(
        gov: &crate::governance::Params,
        previous_annual_issuance_block: u64,
        subsidy_prev: SubsidySnapshot,
        tariff_prev: TariffSnapshot,
        baseline_tx_count: u64,
        baseline_tx_volume: u64,
        baseline_miners: u64,
    ) -> Self {
        Self {
            inflation: inflation_controller::InflationParams {
                target_inflation_bps: gov.inflation_target_bps as u16,
                controller_gain: (gov.inflation_controller_gain as f64) / 1000.0,
                min_annual_issuance_block: gov.min_annual_issuance_block as u64,
                max_annual_issuance_block: gov.max_annual_issuance_block as u64,
                previous_annual_issuance_block,
            },
            network_issuance: network_issuance::NetworkIssuanceParams {
                max_supply_block: 40_000_000,      // Hard-coded: 40M BLOCK cap
                expected_total_blocks: 20_000_000, // ~231 days at 1 block/sec
                baseline_tx_count,                 // From persisted state
                baseline_tx_volume_block: baseline_tx_volume, // From persisted state
                baseline_miners,                   // From persisted state
                activity_multiplier_min: 0.5,
                activity_multiplier_max: 2.0,
                decentralization_multiplier_min: 0.5,
                decentralization_multiplier_max: 1.5,
                // Adaptive baselines enabled by default
                adaptive_baselines_enabled: true,
                baseline_ema_alpha: 0.05, // 20-epoch smoothing
                baseline_min_tx_count: 50,
                baseline_max_tx_count: 10_000,
                baseline_min_tx_volume: 5_000,
                baseline_max_tx_volume: 1_000_000,
                baseline_min_miners: 5,
                baseline_max_miners: 100,
            },
            subsidy: subsidy_allocator::SubsidyParams {
                storage_util_target_bps: gov.storage_util_target_bps as u16,
                compute_util_target_bps: gov.compute_util_target_bps as u16,
                energy_util_target_bps: gov.energy_util_target_bps as u16,
                ad_util_target_bps: gov.ad_util_target_bps as u16,
                storage_margin_target_bps: gov.storage_margin_target_bps as u16,
                compute_margin_target_bps: gov.compute_margin_target_bps as u16,
                energy_margin_target_bps: gov.energy_margin_target_bps as u16,
                ad_margin_target_bps: gov.ad_margin_target_bps as u16,
                alpha: (gov.subsidy_allocator_alpha as f64) / 1000.0,
                beta: (gov.subsidy_allocator_beta as f64) / 1000.0,
                temperature: (gov.subsidy_allocator_temperature as f64) / 1000.0,
                drift_rate: (gov.subsidy_allocator_drift_rate as f64) / 1000.0,
            },
            subsidy_prev,
            multiplier: market_multiplier::MultiplierParams {
                storage: market_multiplier::MarketMultiplierParams {
                    util_target_bps: gov.storage_util_target_bps as u16,
                    margin_target_bps: gov.storage_margin_target_bps as u16,
                    util_responsiveness: (gov.storage_util_responsiveness as f64) / 1000.0,
                    cost_responsiveness: (gov.storage_cost_responsiveness as f64) / 1000.0,
                    multiplier_floor: (gov.storage_multiplier_floor as f64) / 1000.0,
                    multiplier_ceiling: (gov.storage_multiplier_ceiling as f64) / 1000.0,
                },
                compute: market_multiplier::MarketMultiplierParams {
                    util_target_bps: gov.compute_util_target_bps as u16,
                    margin_target_bps: gov.compute_margin_target_bps as u16,
                    util_responsiveness: (gov.compute_util_responsiveness as f64) / 1000.0,
                    cost_responsiveness: (gov.compute_cost_responsiveness as f64) / 1000.0,
                    multiplier_floor: (gov.compute_multiplier_floor as f64) / 1000.0,
                    multiplier_ceiling: (gov.compute_multiplier_ceiling as f64) / 1000.0,
                },
                energy: market_multiplier::MarketMultiplierParams {
                    util_target_bps: gov.energy_util_target_bps as u16,
                    margin_target_bps: gov.energy_margin_target_bps as u16,
                    util_responsiveness: (gov.energy_util_responsiveness as f64) / 1000.0,
                    cost_responsiveness: (gov.energy_cost_responsiveness as f64) / 1000.0,
                    multiplier_floor: (gov.energy_multiplier_floor as f64) / 1000.0,
                    multiplier_ceiling: (gov.energy_multiplier_ceiling as f64) / 1000.0,
                },
                ad: market_multiplier::MarketMultiplierParams {
                    util_target_bps: gov.ad_util_target_bps as u16,
                    margin_target_bps: gov.ad_margin_target_bps as u16,
                    util_responsiveness: (gov.ad_util_responsiveness as f64) / 1000.0,
                    cost_responsiveness: (gov.ad_cost_responsiveness as f64) / 1000.0,
                    multiplier_floor: (gov.ad_multiplier_floor as f64) / 1000.0,
                    multiplier_ceiling: (gov.ad_multiplier_ceiling as f64) / 1000.0,
                },
            },
            ad_market: ad_market_controller::AdMarketParams {
                platform_take_target_bps: gov.ad_platform_take_target_bps as u16,
                user_share_target_bps: gov.ad_user_share_target_bps as u16,
                drift_rate: (gov.ad_drift_rate as f64) / 1000.0,
            },
            tariff: ad_market_controller::TariffParams {
                public_revenue_target_bps: gov.tariff_public_revenue_target_bps as u16,
                drift_rate: (gov.tariff_drift_rate as f64) / 1000.0,
                tariff_min_bps: gov.tariff_min_bps as u16,
                tariff_max_bps: gov.tariff_max_bps as u16,
            },
            tariff_prev,
        }
    }
}
