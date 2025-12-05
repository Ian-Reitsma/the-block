//! Economic control laws for The Block.
//!
//! This module implements the four-layer adaptive control system:
//! 1. **Inflation Controller**: Maintains target inflation rate via adaptive issuance
//! 2. **Subsidy Allocator**: Reallocates subsidies based on market distress
//! 3. **Market Multipliers**: Dual control (utilization + cost-coverage)
//! 4. **Ad & Tariff Controllers**: Drift splits and tariffs toward governance targets

pub mod inflation_controller;
pub mod subsidy_allocator;
pub mod market_multiplier;
pub mod ad_market_controller;
pub mod event;

pub use inflation_controller::InflationController;
pub use subsidy_allocator::SubsidyAllocator;
pub use market_multiplier::MarketMultiplierController;
pub use ad_market_controller::{AdMarketDriftController, TariffController};
pub use event::*;

use foundation_serialization::{Deserialize, Serialize};

/// Complete economic state snapshot for an epoch
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EconomicSnapshot {
    pub epoch: u64,
    pub inflation: InflationSnapshot,
    pub subsidies: SubsidySnapshot,
    pub multipliers: MultiplierSnapshot,
    pub ad_market: AdMarketSnapshot,
    pub tariff: TariffSnapshot,
}

/// Inflation state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct InflationSnapshot {
    pub circulating_ct: u64,
    pub annual_issuance_ct: u64,
    pub realized_inflation_bps: u16,
    pub target_inflation_bps: u16,
}

/// Subsidy allocation state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SubsidySnapshot {
    pub storage_share_bps: u16,
    pub compute_share_bps: u16,
    pub energy_share_bps: u16,
    pub ad_share_bps: u16,
}

/// Market multiplier state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MultiplierSnapshot {
    pub storage_multiplier: f64,
    pub compute_multiplier: f64,
    pub energy_multiplier: f64,
    pub ad_multiplier: f64,
}

/// Ad market split state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdMarketSnapshot {
    pub platform_take_bps: u16,
    pub user_share_bps: u16,
    pub publisher_share_bps: u16,
}

/// Tariff state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TariffSnapshot {
    pub tariff_bps: u16,
    pub non_kyc_volume_ct: u64,
    pub treasury_contribution_bps: u16,
}

/// Market metrics for control loop input
#[derive(Debug, Clone, Default)]
pub struct MarketMetrics {
    pub storage: MarketMetric,
    pub compute: MarketMetric,
    pub energy: MarketMetric,
    pub ad: MarketMetric,
}

/// Individual market metric
#[derive(Debug, Clone, Default)]
pub struct MarketMetric {
    /// Utilization ratio [0.0, 1.0]
    pub utilization: f64,
    /// Average unit cost in CT
    pub average_cost_ct: f64,
    /// Effective unit payout in CT
    pub effective_payout_ct: f64,
    /// Realized provider margin ratio (negative if unprofitable)
    pub provider_margin: f64,
}

/// Execute full economic control loop for an epoch
pub fn execute_epoch_economics(
    epoch: u64,
    metrics: &MarketMetrics,
    circulating_ct: u64,
    non_kyc_volume_ct: u64,
    total_ad_spend_ct: u64,
    treasury_inflow_ct: u64,
    gov_params: &GovernanceEconomicParams,
) -> EconomicSnapshot {
    // Layer 1: Inflation
    let inflation_controller = InflationController::new(gov_params.inflation.clone());
    let inflation = inflation_controller.compute_epoch_issuance(circulating_ct);

    // Layer 2: Subsidy allocation
    let subsidy_allocator = SubsidyAllocator::new(gov_params.subsidy.clone());
    let subsidies = subsidy_allocator.compute_next_allocation(metrics, &gov_params.subsidy_prev);

    // Layer 3: Market multipliers
    let multiplier_controller = MarketMultiplierController::new(gov_params.multiplier.clone());
    let multipliers = multiplier_controller.compute_multipliers(metrics);

    // Layer 4: Ad & tariff
    let ad_drift_controller = AdMarketDriftController::new(gov_params.ad_market.clone());
    let ad_market = ad_drift_controller.compute_next_splits(total_ad_spend_ct);

    let tariff_controller = TariffController::new(gov_params.tariff.clone());
    let tariff = tariff_controller.compute_next_tariff(
        non_kyc_volume_ct,
        treasury_inflow_ct,
        gov_params.tariff_prev.tariff_bps,
    );

    EconomicSnapshot {
        epoch,
        inflation,
        subsidies,
        multipliers,
        ad_market,
        tariff,
    }
}

/// All governance parameters needed for economic control laws
#[derive(Debug, Clone)]
pub struct GovernanceEconomicParams {
    pub inflation: inflation_controller::InflationParams,
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
        previous_annual_issuance_ct: u64,
        subsidy_prev: SubsidySnapshot,
        tariff_prev: TariffSnapshot,
    ) -> Self {
        Self {
            inflation: inflation_controller::InflationParams {
                target_inflation_bps: gov.inflation_target_bps as u16,
                controller_gain: (gov.inflation_controller_gain as f64) / 1000.0,
                min_annual_issuance_ct: gov.min_annual_issuance_ct as u64,
                max_annual_issuance_ct: gov.max_annual_issuance_ct as u64,
                previous_annual_issuance_ct,
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
