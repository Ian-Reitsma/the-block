//! Economic control law events for telemetry and auditing
//!
//! Every epoch, the control law execution emits events capturing:
//! - Inflation adjustments
//! - Subsidy reallocations
//! - Multiplier changes
//! - Ad split drifts
//! - Tariff adjustments

use super::*;
use foundation_serialization::{Deserialize, Serialize};

/// Complete control law update event (emitted each epoch)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ControlLawUpdateEvent {
    pub epoch: u64,
    pub timestamp: u64,
    pub inflation: InflationUpdateEvent,
    pub subsidies: SubsidyUpdateEvent,
    pub multipliers: MultiplierUpdateEvent,
    pub ad_market: AdMarketUpdateEvent,
    pub tariff: TariffUpdateEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct InflationUpdateEvent {
    pub previous_annual_issuance_ct: u64,
    pub new_annual_issuance_ct: u64,
    pub circulating_ct: u64,
    pub realized_inflation_bps: u16,
    pub target_inflation_bps: u16,
    pub error_bps: i32, // π* - π_t (can be negative)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SubsidyUpdateEvent {
    pub storage_share_bps: u16,
    pub compute_share_bps: u16,
    pub energy_share_bps: u16,
    pub ad_share_bps: u16,
    pub storage_distress: f64,
    pub compute_distress: f64,
    pub energy_distress: f64,
    pub ad_distress: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MultiplierUpdateEvent {
    pub storage_multiplier: f64,
    pub compute_multiplier: f64,
    pub energy_multiplier: f64,
    pub ad_multiplier: f64,
    pub storage_util: f64,
    pub compute_util: f64,
    pub energy_util: f64,
    pub ad_util: f64,
    pub storage_margin: f64,
    pub compute_margin: f64,
    pub energy_margin: f64,
    pub ad_margin: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdMarketUpdateEvent {
    pub platform_take_bps: u16,
    pub user_share_bps: u16,
    pub publisher_share_bps: u16,
    pub total_ad_spend_ct: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TariffUpdateEvent {
    pub tariff_bps: u16,
    pub non_kyc_volume_ct: u64,
    pub treasury_contribution_bps: u16,
    pub target_contribution_bps: u16,
}

impl ControlLawUpdateEvent {
    /// Create event from economic snapshot and metrics
    pub fn from_snapshot(
        snapshot: &EconomicSnapshot,
        metrics: &MarketMetrics,
        previous_issuance: u64,
        total_ad_spend: u64,
        target_contribution_bps: u16,
    ) -> Self {
        Self {
            epoch: snapshot.epoch,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            inflation: InflationUpdateEvent {
                previous_annual_issuance_ct: previous_issuance,
                new_annual_issuance_ct: snapshot.inflation.annual_issuance_ct,
                circulating_ct: snapshot.inflation.circulating_ct,
                realized_inflation_bps: snapshot.inflation.realized_inflation_bps,
                target_inflation_bps: snapshot.inflation.target_inflation_bps,
                error_bps: (snapshot.inflation.target_inflation_bps as i32)
                    - (snapshot.inflation.realized_inflation_bps as i32),
            },
            subsidies: SubsidyUpdateEvent {
                storage_share_bps: snapshot.subsidies.storage_share_bps,
                compute_share_bps: snapshot.subsidies.compute_share_bps,
                energy_share_bps: snapshot.subsidies.energy_share_bps,
                ad_share_bps: snapshot.subsidies.ad_share_bps,
                // Distress scores would be computed by allocator
                storage_distress: 0.0,
                compute_distress: 0.0,
                energy_distress: 0.0,
                ad_distress: 0.0,
            },
            multipliers: MultiplierUpdateEvent {
                storage_multiplier: snapshot.multipliers.storage_multiplier,
                compute_multiplier: snapshot.multipliers.compute_multiplier,
                energy_multiplier: snapshot.multipliers.energy_multiplier,
                ad_multiplier: snapshot.multipliers.ad_multiplier,
                storage_util: metrics.storage.utilization,
                compute_util: metrics.compute.utilization,
                energy_util: metrics.energy.utilization,
                ad_util: metrics.ad.utilization,
                storage_margin: metrics.storage.provider_margin,
                compute_margin: metrics.compute.provider_margin,
                energy_margin: metrics.energy.provider_margin,
                ad_margin: metrics.ad.provider_margin,
            },
            ad_market: AdMarketUpdateEvent {
                platform_take_bps: snapshot.ad_market.platform_take_bps,
                user_share_bps: snapshot.ad_market.user_share_bps,
                publisher_share_bps: snapshot.ad_market.publisher_share_bps,
                total_ad_spend_ct: total_ad_spend,
            },
            tariff: TariffUpdateEvent {
                tariff_bps: snapshot.tariff.tariff_bps,
                non_kyc_volume_ct: snapshot.tariff.non_kyc_volume_ct,
                treasury_contribution_bps: snapshot.tariff.treasury_contribution_bps,
                target_contribution_bps,
            },
        }
    }
}
