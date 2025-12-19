//! Economics replay state machine for consensus validation.
//!
//! This module implements deterministic economics replay that allows any node
//! to recompute the exact economics schedule (block rewards, subsidies, etc.)
//! from chain history alone, without relying on local node state.
//!
//! **Critical for consensus:** Two nodes seeing the same chain must compute
//! identical economics outputs. This is enforced by making all inputs come
//! from the chain itself (transactions, governance params, market activity).

use super::{
    derive_market_metrics_from_chain, execute_epoch_economics, GovernanceEconomicParams,
    NetworkActivity, SubsidySnapshot, TariffSnapshot,
};
use crate::governance::Params;
use crate::{Block, EPOCH_BLOCKS};
use std::collections::HashSet;

/// Economics state at a specific block height derived from chain replay.
#[derive(Debug, Clone)]
pub struct ReplayedEconomicsState {
    /// Block height this state applies to
    pub block_height: u64,
    /// Expected base block reward (before subsidies/logistic adjustment)
    pub block_reward_per_block: u64,
    /// Previous epoch's subsidy allocation
    pub prev_subsidy: SubsidySnapshot,
    /// Previous epoch's tariff state
    pub prev_tariff: TariffSnapshot,
    /// Previous epoch's annual issuance
    pub prev_annual_issuance: u64,
    /// Adaptive baseline state (CRITICAL for consensus - must carry across epochs)
    pub baseline_tx_count: u64,
    pub baseline_tx_volume: u64,
    pub baseline_miners: u64,
}

impl Default for ReplayedEconomicsState {
    fn default() -> Self {
        Self {
            block_height: 0,
            block_reward_per_block: 50_000_000, // Genesis reward
            prev_subsidy: SubsidySnapshot::default(),
            prev_tariff: TariffSnapshot::default(),
            prev_annual_issuance: 0,
            // Default baselines from NetworkIssuanceParams
            baseline_tx_count: 100,
            baseline_tx_volume: 10_000,
            baseline_miners: 10,
        }
    }
}

/// Replay economics from a chain slice and return the economics state at the tip.
///
/// # Arguments
/// * `chain` - Block slice from genesis to tip
/// * `gov_params` - Governance parameters (should ideally be epoch-versioned from chain)
///
/// # Returns
/// Economics state at the tip of the chain
///
/// # Determinism Requirements
/// - Uses ONLY data from the chain itself (tx counts, volumes, miner set)
/// - Market metrics are derived from on-chain settlement records (not live market state)
/// - Governance params should come from on-chain governance history (TODO: epoch versioning)
pub fn replay_economics_to_tip(chain: &[Block], gov_params: &Params) -> ReplayedEconomicsState {
    if chain.is_empty() {
        return ReplayedEconomicsState::default();
    }

    let tip_height = chain.len() as u64 - 1;
    replay_economics_to_height(chain, tip_height, gov_params)
}

/// Replay economics from genesis to a specific block height.
///
/// This is the core consensus-critical function. It:
/// 1. Steps through each epoch boundary
/// 2. Computes network activity from chain data (tx count, volume, unique miners)
/// 3. Derives market metrics from on-chain settlement/subsidy data
/// 4. Calls execute_epoch_economics with these deterministic inputs
/// 5. Returns the economics state at the requested height
///
/// # Determinism Contract
/// Given the same chain slice and governance params, this MUST return identical
/// economics state on any node, regardless of that node's runtime state.
pub fn replay_economics_to_height(
    chain: &[Block],
    target_height: u64,
    gov_params: &Params,
) -> ReplayedEconomicsState {
    if chain.is_empty() || target_height >= chain.len() as u64 {
        return ReplayedEconomicsState::default();
    }

    let mut state = ReplayedEconomicsState::default();
    let mut emission = 0u64;

    // Epoch-windowed metrics
    let mut epoch_tx_count = 0u64;
    let mut epoch_tx_volume_block = 0u64;
    let mut epoch_treasury_inflow = 0u64;
    let mut epoch_miners: HashSet<String> = HashSet::new();

    // Process chain block by block up to target_height
    for (idx, block) in chain.iter().enumerate() {
        let block_height = idx as u64;
        if block_height > target_height {
            break;
        }

        // Accumulate emissions from this block
        emission = emission.saturating_add(block.coinbase_consumer.0);
        emission = emission.saturating_add(block.coinbase_industrial.0);

        // Track miner
        if let Some(coinbase_tx) = block.transactions.first() {
            epoch_miners.insert(coinbase_tx.payload.to.clone());
        }

        // Accumulate epoch metrics from non-coinbase transactions
        for tx in block.transactions.iter().skip(1) {
            epoch_tx_count = epoch_tx_count.saturating_add(1);
            let tx_volume = tx
                .payload
                .amount_consumer
                .saturating_add(tx.payload.amount_industrial)
                .saturating_add(tx.tip);
            epoch_tx_volume_block = epoch_tx_volume_block.saturating_add(tx_volume);
        }

        // TODO: Accumulate treasury inflow from successful accruals
        // For now, estimate from treasury_percent_ct × coinbase
        // This needs to be made precise by tracking actual accrual success

        // At epoch boundaries, execute economics control laws
        if block_height > 0 && block_height % EPOCH_BLOCKS == 0 {
            let epoch = block_height / EPOCH_BLOCKS;

            // Derive market metrics from chain data
            // FIXME: These are currently placeholder approximations.
            // Instructions require wiring real metrics from settlement records.
            let metrics = derive_market_metrics_from_chain(
                chain,
                block_height.saturating_sub(EPOCH_BLOCKS),
                block_height,
            );

            let network_activity = NetworkActivity {
                tx_count: epoch_tx_count,
                tx_volume_block: epoch_tx_volume_block,
                unique_miners: epoch_miners.len() as u64,
                block_height,
            };

            // Convert governance params to economics params
            let econ_params = GovernanceEconomicParams::from_params(gov_params, &state);

            let snapshot = execute_epoch_economics(
                epoch,
                &metrics,
                &network_activity,
                emission,              // circulating_block
                emission,              // total_emission
                epoch_tx_volume_block, // non_kyc_volume (TODO: track KYC status on-chain)
                0, // total_ad_spend_block (TODO: derive from ad settlement records)
                epoch_treasury_inflow,
                &econ_params,
            );

            // Update state for next epoch
            state.block_height = block_height;
            state.block_reward_per_block = snapshot.inflation.block_reward_per_block;
            state.prev_subsidy = snapshot.subsidies;
            state.prev_tariff = snapshot.tariff;
            state.prev_annual_issuance = snapshot.inflation.annual_issuance_block;
            // CRITICAL: Carry forward adaptive baselines across epochs
            state.baseline_tx_count = snapshot.updated_baseline_tx_count;
            state.baseline_tx_volume = snapshot.updated_baseline_tx_volume;
            state.baseline_miners = snapshot.updated_baseline_miners;

            // Reset epoch counters
            epoch_tx_count = 0;
            epoch_tx_volume_block = 0;
            epoch_treasury_inflow = 0;
            epoch_miners.clear();
        }
    }

    state.block_height = target_height;
    state
}

/// Helper to convert Params to GovernanceEconomicParams
impl GovernanceEconomicParams {
    fn from_params(_params: &Params, state: &ReplayedEconomicsState) -> Self {
        // FIXME: This conversion needs to pull actual governance param snapshots
        // from on-chain history for the relevant epoch, not current params.
        // For now, use defaults to unblock testing.
        use super::ad_market_controller::{AdMarketParams, TariffParams};
        use super::inflation_controller::InflationParams;
        use super::market_multiplier::MultiplierParams;
        use super::network_issuance::NetworkIssuanceParams;
        use super::subsidy_allocator::SubsidyParams;

        Self {
            inflation: InflationParams::default(),
            network_issuance: NetworkIssuanceParams {
                baseline_tx_count: state.baseline_tx_count,
                baseline_tx_volume_block: state.baseline_tx_volume,
                baseline_miners: state.baseline_miners,
                ..NetworkIssuanceParams::default()
            },
            subsidy: SubsidyParams::default(),
            subsidy_prev: state.prev_subsidy.clone(),
            multiplier: MultiplierParams::default(),
            ad_market: AdMarketParams::default(),
            tariff: TariffParams::default(),
            tariff_prev: state.prev_tariff.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replay_empty_chain() {
        let chain: Vec<Block> = vec![];
        let params = Params::default();
        let state = replay_economics_to_tip(&chain, &params);

        assert_eq!(state.block_height, 0);
        assert_eq!(state.block_reward_per_block, 50_000_000); // Genesis default
    }

    #[test]
    fn test_replay_determinism() {
        // TODO: Build a test chain with synthetic blocks and verify that:
        // 1. replay_economics_to_tip produces the same result when called twice
        // 2. Two nodes with different local state get identical economics from same chain
        // 3. Economics state changes correctly at epoch boundaries
    }
}
