#![forbid(unsafe_code)]

pub mod verifier;

use crypto_suite::hashing::blake3::Hasher as Blake3;
use foundation_metrics::{gauge, histogram, increment_counter, recorder_installed};
use foundation_serialization::{binary, Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub use verifier::{
    Ed25519Verifier, ProviderKey, SignatureScheme, SignatureVerifier, VerificationError,
    VerifierRegistry,
};

#[cfg(feature = "pq-crypto")]
pub use verifier::DilithiumVerifier;

pub type ProviderId = String;
pub type AccountId = String;
pub type JurisdictionId = String;
pub type OracleAddress = String;
pub type Balance = u64;
pub type BlockNumber = u64;
pub type UnixTimestamp = u64;
pub type H256 = [u8; 32];

const BASIS_POINTS_DIVISOR: u64 = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergyProvider {
    pub provider_id: ProviderId,
    pub owner: AccountId,
    pub location: JurisdictionId,
    pub capacity_kwh: u64,
    pub available_kwh: u64,
    pub price_per_kwh: Balance,
    pub reputation_score: f64, // Legacy simple score (for backward compat)
    pub meter_address: OracleAddress,
    pub total_delivered_kwh: u64,
    pub staked_balance: Balance,
    pub last_fulfillment_latency_ms: Option<u64>,
    pub last_meter_value: Option<u64>,
    pub last_meter_timestamp: Option<UnixTimestamp>,
    #[serde(default)]
    pub bayesian_reputation: BayesianReputation, // Advanced multi-factor reputation
}

impl EnergyProvider {
    fn ensure_capacity(&self, kwh: u64) -> Result<(), EnergyMarketError> {
        if self.available_kwh < kwh {
            return Err(EnergyMarketError::InsufficientCapacity {
                provider_id: self.provider_id.clone(),
                requested_kwh: kwh,
                available_kwh: self.available_kwh,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergyCredit {
    pub amount_kwh: u64,
    pub provider: ProviderId,
    pub timestamp: BlockNumber,
    pub meter_reading_hash: H256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergyReceipt {
    pub buyer: AccountId,
    pub seller: ProviderId,
    pub kwh_delivered: u64,
    pub price_paid: Balance,
    pub block_settled: BlockNumber,
    pub treasury_fee: Balance,
    pub meter_reading_hash: H256,
    pub slash_applied: Balance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MeterReading {
    pub provider_id: ProviderId,
    pub meter_address: OracleAddress,
    pub total_kwh: u64,
    pub timestamp: UnixTimestamp,
    pub signature: Vec<u8>,
}

impl MeterReading {
    pub fn hash(&self) -> H256 {
        let mut hasher = Blake3::new();
        hasher.update(self.provider_id.as_bytes());
        hasher.update(self.meter_address.as_bytes());
        hasher.update(&self.total_kwh.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&(self.signature.len() as u32).to_le_bytes());
        hasher.update(&self.signature);
        hasher.finalize().into()
    }
}

/// Bayesian reputation using Beta distributions for multi-factor trust scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct BayesianReputation {
    /// Delivery reliability: Beta(alpha_delivery, beta_delivery)
    /// Tracks: on-time delivery vs late/failed delivery
    #[serde(default = "default_beta_alpha")]
    pub delivery_alpha: f64,
    #[serde(default = "default_beta_beta")]
    pub delivery_beta: f64,

    /// Meter accuracy: Beta(alpha_meter, beta_meter)
    /// Tracks: consistent meter readings vs suspicious deviations
    #[serde(default = "default_beta_alpha")]
    pub meter_alpha: f64,
    #[serde(default = "default_beta_beta")]
    pub meter_beta: f64,

    /// Response speed: Beta(alpha_latency, beta_latency)
    /// Tracks: fast responses vs slow fulfillment
    #[serde(default = "default_beta_alpha")]
    pub latency_alpha: f64,
    #[serde(default = "default_beta_beta")]
    pub latency_beta: f64,

    /// Capacity stability: Beta(alpha_capacity, beta_capacity)
    /// Tracks: stable availability vs volatile/unreliable capacity
    #[serde(default = "default_beta_alpha")]
    pub capacity_alpha: f64,
    #[serde(default = "default_beta_beta")]
    pub capacity_beta: f64,

    /// Total observations (for confidence weighting)
    #[serde(default)]
    pub total_observations: u64,

    /// Last computed composite score (cached)
    #[serde(default = "default_score")]
    pub composite_score: f64,
}

#[allow(dead_code)] // Used by serde default attribute
fn default_beta_alpha() -> f64 {
    1.0
}
#[allow(dead_code)] // Used by serde default attribute
fn default_beta_beta() -> f64 {
    1.0
}
#[allow(dead_code)] // Used by serde default attribute
fn default_score() -> f64 {
    0.5
}

impl Default for BayesianReputation {
    fn default() -> Self {
        Self {
            delivery_alpha: 1.0,
            delivery_beta: 1.0,
            meter_alpha: 1.0,
            meter_beta: 1.0,
            latency_alpha: 1.0,
            latency_beta: 1.0,
            capacity_alpha: 1.0,
            capacity_beta: 1.0,
            total_observations: 0,
            composite_score: 0.5, // Neutral prior
        }
    }
}

impl BayesianReputation {
    /// Update delivery reliability based on success/failure
    pub fn update_delivery(&mut self, success: bool) {
        if success {
            self.delivery_alpha += 1.0;
        } else {
            self.delivery_beta += 1.0;
        }
        self.total_observations += 1;
        self.recompute_composite();
    }

    /// Update meter accuracy based on consistency check
    pub fn update_meter_accuracy(&mut self, consistent: bool) {
        if consistent {
            self.meter_alpha += 1.0;
        } else {
            self.meter_beta += 1.0;
        }
        self.total_observations += 1;
        self.recompute_composite();
    }

    /// Update response latency based on fulfillment speed
    pub fn update_latency(&mut self, latency_ms: u64, threshold_ms: u64) {
        if latency_ms <= threshold_ms {
            self.latency_alpha += 1.0;
        } else {
            // Partial credit for slow-but-acceptable responses
            let ratio = (threshold_ms as f64) / (latency_ms as f64);
            self.latency_alpha += ratio;
            self.latency_beta += 1.0 - ratio;
        }
        self.total_observations += 1;
        self.recompute_composite();
    }

    /// Update capacity stability (called periodically)
    pub fn update_capacity_stability(&mut self, current_available: u64, expected_available: u64) {
        if expected_available == 0 {
            return; // Avoid division by zero
        }
        let availability_ratio = (current_available as f64) / (expected_available as f64);

        // Stable if within 20% of expected
        if availability_ratio >= 0.8 && availability_ratio <= 1.2 {
            self.capacity_alpha += 1.0;
        } else {
            self.capacity_beta += 1.0;
        }
        self.total_observations += 1;
        self.recompute_composite();
    }

    /// Apply penalty (e.g., for slashing events)
    pub fn penalize(&mut self, severity: f64) {
        // Apply penalty to all beta parameters (increase failure counts)
        let penalty = severity.clamp(0.0, 10.0);
        self.delivery_beta += penalty;
        self.meter_beta += penalty;
        self.latency_beta += penalty;
        self.capacity_beta += penalty;
        self.recompute_composite();
    }

    /// Recompute composite score from Beta distributions
    fn recompute_composite(&mut self) {
        // Beta distribution mean: alpha / (alpha + beta)
        let delivery_score = self.delivery_alpha / (self.delivery_alpha + self.delivery_beta);
        let meter_score = self.meter_alpha / (self.meter_alpha + self.meter_beta);
        let latency_score = self.latency_alpha / (self.latency_alpha + self.latency_beta);
        let capacity_score = self.capacity_alpha / (self.capacity_alpha + self.capacity_beta);

        // Weighted geometric mean (ensures all factors matter)
        const W_DELIVERY: f64 = 0.35; // Most important
        const W_METER: f64 = 0.25;
        const W_LATENCY: f64 = 0.25;
        const W_CAPACITY: f64 = 0.15;

        self.composite_score = (delivery_score.powf(W_DELIVERY)
            * meter_score.powf(W_METER)
            * latency_score.powf(W_LATENCY)
            * capacity_score.powf(W_CAPACITY))
        .clamp(0.0, 1.0);
    }

    /// Get current composite score
    pub fn score(&self) -> f64 {
        self.composite_score
    }

    /// Get confidence level based on number of observations
    pub fn confidence(&self) -> f64 {
        // Sigmoid confidence: approaches 1.0 as observations increase
        let observations = self.total_observations as f64;
        (observations / (observations + 20.0)).clamp(0.0, 1.0)
    }

    /// Check if provider should be deactivated due to poor reputation
    pub fn should_deactivate(&self, min_score: f64, min_confidence: f64) -> bool {
        self.composite_score < min_score && self.confidence() >= min_confidence
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergyMarketConfig {
    pub min_stake: Balance,
    pub treasury_fee_bps: u16, // Now a base/minimum fee (dynamic fees added above this)
    pub ewma_alpha: f64,
    pub jurisdiction_fee_bps: u16,
    pub oracle_timeout_blocks: u64,
    pub slashing_rate_bps: u16,
    // Dynamic fee parameters
    pub dynamic_fees_enabled: bool,
    pub congestion_sensitivity: f64,
    pub target_utilization: f64,
    pub peak_hour_utc: u8,
    // Bayesian reputation parameters
    pub bayesian_reputation_enabled: bool,
    pub latency_threshold_ms: u64, // Acceptable fulfillment latency
    pub min_reputation_score: f64, // Minimum score before deactivation
    pub min_reputation_confidence: f64, // Minimum confidence before enforcing deactivation
}

impl Default for EnergyMarketConfig {
    fn default() -> Self {
        Self {
            min_stake: 1_000,
            treasury_fee_bps: 250, // 2.5% base fee (was 5% fixed)
            ewma_alpha: 0.3,
            jurisdiction_fee_bps: 0,
            oracle_timeout_blocks: 0,
            slashing_rate_bps: 0,
            // Dynamic fee defaults
            dynamic_fees_enabled: true,
            congestion_sensitivity: 0.1, // Controls how aggressively fees respond to congestion
            target_utilization: 0.7,     // 70% target grid utilization
            peak_hour_utc: 19,           // 19:00 UTC (7pm)
            // Bayesian reputation defaults
            bayesian_reputation_enabled: true,
            latency_threshold_ms: 5000,     // 5 seconds acceptable latency
            min_reputation_score: 0.3,      // Deactivate providers below 30% score
            min_reputation_confidence: 0.7, // Require 70% confidence before deactivation
        }
    }
}

#[derive(Debug, Error)]
pub enum EnergyMarketError {
    #[error("provider already registered: {provider_id}")]
    ProviderExists { provider_id: ProviderId },
    #[error("meter address already claimed: {meter_address}")]
    MeterAddressInUse { meter_address: OracleAddress },
    #[error("stake {stake} below required minimum {min}")]
    InsufficientStake { stake: Balance, min: Balance },
    #[error("insufficient capacity for provider {provider_id}: requested {requested_kwh} kWh but only {available_kwh} kWh remain")]
    InsufficientCapacity {
        provider_id: ProviderId,
        requested_kwh: u64,
        available_kwh: u64,
    },
    #[error("unknown provider {0}")]
    UnknownProvider(ProviderId),
    #[error("reading timestamp regression for provider {provider_id}")]
    StaleReading { provider_id: ProviderId },
    #[error("reading totalized kWh decreased for provider {provider_id}")]
    InvalidMeterValue { provider_id: ProviderId },
    #[error("meter reading hash {0:?} not tracked")]
    UnknownReading(H256),
    #[error("requested {requested_kwh} kWh exceeds credit {available_kwh}")]
    InsufficientCredit {
        requested_kwh: u64,
        available_kwh: u64,
    },
    #[error("meter reading {0:?} expired")]
    CreditExpired(H256),
    #[error("signature verification failed: {0}")]
    SignatureVerificationFailed(#[from] VerificationError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergyMarket {
    config: EnergyMarketConfig,
    providers: BTreeMap<ProviderId, EnergyProvider>,
    meter_index: BTreeMap<OracleAddress, ProviderId>,
    credits: BTreeMap<H256, EnergyCredit>,
    receipts: Vec<EnergyReceipt>,
    next_provider_id: u64,
    total_price_paid: u128,
    total_kwh_settled: u64,
    #[serde(default)]
    verifier_registry: VerifierRegistry,
}

impl Default for EnergyMarket {
    fn default() -> Self {
        Self {
            config: EnergyMarketConfig::default(),
            providers: BTreeMap::new(),
            meter_index: BTreeMap::new(),
            credits: BTreeMap::new(),
            receipts: Vec::new(),
            next_provider_id: 0,
            total_price_paid: 0,
            total_kwh_settled: 0,
            verifier_registry: VerifierRegistry::new(),
        }
    }
}

impl EnergyMarket {
    pub fn new(config: EnergyMarketConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    pub fn config(&self) -> &EnergyMarketConfig {
        &self.config
    }

    pub fn set_config(&mut self, cfg: EnergyMarketConfig) {
        self.config = cfg;
    }

    pub fn provider(&self, id: &str) -> Option<&EnergyProvider> {
        self.providers.get(id)
    }

    pub fn providers(&self) -> impl Iterator<Item = &EnergyProvider> {
        self.providers.values()
    }

    pub fn credits(&self) -> impl Iterator<Item = (&H256, &EnergyCredit)> {
        self.credits.iter()
    }

    pub fn receipts(&self) -> &[EnergyReceipt] {
        &self.receipts
    }

    /// Drain all pending receipts and return them for inclusion in a block.
    pub fn drain_receipts(&mut self) -> Vec<EnergyReceipt> {
        std::mem::take(&mut self.receipts)
    }

    pub fn verifier_registry(&self) -> &VerifierRegistry {
        &self.verifier_registry
    }

    pub fn verifier_registry_mut(&mut self) -> &mut VerifierRegistry {
        &mut self.verifier_registry
    }

    pub fn register_provider_key(
        &mut self,
        provider_id: ProviderId,
        public_key: Vec<u8>,
        scheme: SignatureScheme,
    ) {
        self.verifier_registry
            .register(provider_id, public_key, scheme);
    }

    pub fn register_energy_provider(
        &mut self,
        owner: AccountId,
        capacity_kwh: u64,
        initial_price: Balance,
        meter_address: OracleAddress,
        jurisdiction: JurisdictionId,
        stake: Balance,
    ) -> Result<ProviderId, EnergyMarketError> {
        if stake < self.config.min_stake {
            return Err(EnergyMarketError::InsufficientStake {
                stake,
                min: self.config.min_stake,
            });
        }
        if self.meter_index.contains_key(&meter_address) {
            return Err(EnergyMarketError::MeterAddressInUse { meter_address });
        }
        let provider_id = format!("energy-{:#010x}", self.next_provider_id);
        if self.providers.contains_key(&provider_id) {
            return Err(EnergyMarketError::ProviderExists { provider_id });
        }
        self.next_provider_id = self.next_provider_id.saturating_add(1);
        let provider = EnergyProvider {
            provider_id: provider_id.clone(),
            owner,
            location: jurisdiction,
            capacity_kwh,
            available_kwh: capacity_kwh,
            price_per_kwh: initial_price,
            reputation_score: 1.0,
            meter_address: meter_address.clone(),
            total_delivered_kwh: 0,
            staked_balance: stake,
            last_fulfillment_latency_ms: None,
            last_meter_value: None,
            last_meter_timestamp: None,
            bayesian_reputation: BayesianReputation::default(),
        };
        self.providers.insert(provider_id.clone(), provider);
        self.meter_index.insert(meter_address, provider_id.clone());
        self.emit_provider_gauge();
        Ok(provider_id)
    }

    pub fn record_meter_reading(
        &mut self,
        reading: MeterReading,
        block: BlockNumber,
    ) -> Result<EnergyCredit, EnergyMarketError> {
        // Verify signature if provider has registered a key
        // During shadow mode, this is optional; once enforced, will reject unregistered providers
        if self.verifier_registry.get(&reading.provider_id).is_some() {
            if let Err(err) = self.verifier_registry.verify(&reading) {
                #[cfg(feature = "telemetry")]
                increment_counter!(
                    "energy_signature_failure_total",
                    1.0,
                    "provider" => reading.provider_id.as_str(),
                    "reason" => err.label()
                );
                return Err(err.into());
            }
        }

        let provider = self
            .providers
            .get_mut(&reading.provider_id)
            .ok_or_else(|| EnergyMarketError::UnknownProvider(reading.provider_id.clone()))?;
        if provider.meter_address != reading.meter_address {
            return Err(EnergyMarketError::MeterAddressInUse {
                meter_address: reading.meter_address,
            });
        }
        if let Some(last_ts) = provider.last_meter_timestamp {
            if reading.timestamp <= last_ts {
                return Err(EnergyMarketError::StaleReading {
                    provider_id: provider.provider_id.clone(),
                });
            }
        }
        if let Some(last_value) = provider.last_meter_value {
            if reading.total_kwh < last_value {
                return Err(EnergyMarketError::InvalidMeterValue {
                    provider_id: provider.provider_id.clone(),
                });
            }
        }
        let previous_value = provider.last_meter_value.unwrap_or(0);
        let delta = reading.total_kwh.saturating_sub(previous_value);
        provider.last_meter_timestamp = Some(reading.timestamp);
        provider.last_meter_value = Some(reading.total_kwh);
        let hash = reading.hash();
        let credit = EnergyCredit {
            amount_kwh: delta,
            provider: provider.provider_id.clone(),
            timestamp: block,
            meter_reading_hash: hash,
        };
        self.credits.insert(hash, credit.clone());
        self.record_oracle_latency(reading.timestamp);
        Ok(credit)
    }

    /// Compute dynamic treasury fee based on network conditions
    ///
    /// Formula: fee_bps = base_fee × congestion_multiplier × liquidity_discount × time_factor
    ///
    /// This implements congestion pricing to optimize revenue and encourage off-peak usage.
    fn compute_dynamic_treasury_fee(&self, timestamp_secs: u64) -> u16 {
        if !self.config.dynamic_fees_enabled {
            return self.config.treasury_fee_bps;
        }

        let base_fee_bps = self.config.treasury_fee_bps as f64;

        // 1. Congestion multiplier (higher utilization → higher fees)
        let total_capacity: u64 = self.providers.values().map(|p| p.capacity_kwh).sum();
        let total_consumed: u64 = self
            .providers
            .values()
            .map(|p| p.capacity_kwh.saturating_sub(p.available_kwh))
            .sum();

        let current_utilization = if total_capacity > 0 {
            total_consumed as f64 / total_capacity as f64
        } else {
            0.0
        };

        let utilization_error = current_utilization - self.config.target_utilization;
        let congestion_multiplier =
            1.0 + utilization_error.tanh() * (1.0 / self.config.congestion_sensitivity);
        let congestion_multiplier = congestion_multiplier.clamp(0.5, 2.0);

        // 2. Liquidity discount (more providers → lower fees to encourage competition)
        let provider_count = self.providers.len() as f64;
        let liquidity_discount = 1.0 - (provider_count / 100.0).sqrt().min(0.3);

        // 3. Time-of-day factor (peak hours → higher fees, off-peak → lower fees)
        let hour_of_day = (timestamp_secs / 3600) % 24;
        let peak_hour = self.config.peak_hour_utc as u64;
        let hour_diff = if hour_of_day >= peak_hour {
            hour_of_day - peak_hour
        } else {
            peak_hour - hour_of_day
        };

        // Sinusoidal curve: peak at peak_hour, trough 12 hours away
        let time_factor = 1.0 + 0.3 * (std::f64::consts::PI * hour_diff as f64 / 12.0).cos();

        // Combine all factors
        let effective_fee = base_fee_bps * congestion_multiplier * liquidity_discount * time_factor;

        // Clamp to reasonable bounds (1% minimum, 10% maximum)
        effective_fee.clamp(100.0, 1000.0).round() as u16
    }

    pub fn settle_energy_delivery(
        &mut self,
        buyer: AccountId,
        provider_id: &ProviderId,
        kwh_consumed: u64,
        block: BlockNumber,
        meter_hash: H256,
    ) -> Result<EnergyReceipt, EnergyMarketError> {
        // Compute dynamic treasury fee BEFORE getting mutable references (avoid borrow conflicts)
        let timestamp_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let dynamic_treasury_fee_bps = self.compute_dynamic_treasury_fee(timestamp_secs);

        let provider = self
            .providers
            .get_mut(provider_id)
            .ok_or_else(|| EnergyMarketError::UnknownProvider(provider_id.clone()))?;
        provider.ensure_capacity(kwh_consumed)?;
        let credit = self
            .credits
            .get_mut(&meter_hash)
            .ok_or(EnergyMarketError::UnknownReading(meter_hash))?;
        if self.config.oracle_timeout_blocks > 0
            && block
                .saturating_sub(credit.timestamp)
                .gt(&self.config.oracle_timeout_blocks)
        {
            return Err(EnergyMarketError::CreditExpired(meter_hash));
        }
        if credit.amount_kwh < kwh_consumed {
            return Err(EnergyMarketError::InsufficientCredit {
                requested_kwh: kwh_consumed,
                available_kwh: credit.amount_kwh,
            });
        }
        credit.amount_kwh -= kwh_consumed;
        if credit.amount_kwh == 0 {
            self.credits.remove(&meter_hash);
        }
        provider.available_kwh = provider.available_kwh.saturating_sub(kwh_consumed);
        provider.total_delivered_kwh = provider.total_delivered_kwh.saturating_add(kwh_consumed);
        let total_cost = kwh_consumed.saturating_mul(provider.price_per_kwh);

        let treasury_fee =
            total_cost.saturating_mul(dynamic_treasury_fee_bps as u64) / BASIS_POINTS_DIVISOR;
        let _jurisdiction_fee = total_cost.saturating_mul(self.config.jurisdiction_fee_bps as u64)
            / BASIS_POINTS_DIVISOR;
        let slash_amount =
            total_cost.saturating_mul(self.config.slashing_rate_bps as u64) / BASIS_POINTS_DIVISOR;
        provider.staked_balance = provider.staked_balance.saturating_sub(slash_amount);
        let receipt = EnergyReceipt {
            buyer,
            seller: provider.provider_id.clone(),
            kwh_delivered: kwh_consumed,
            price_paid: total_cost,
            block_settled: block,
            treasury_fee,
            meter_reading_hash: meter_hash,
            slash_applied: slash_amount,
        };
        self.total_price_paid = self.total_price_paid.saturating_add(u128::from(total_cost));
        self.total_kwh_settled = self.total_kwh_settled.saturating_add(kwh_consumed);
        self.receipts.push(receipt.clone());
        increment_counter!("energy_kwh_traded_total", kwh_consumed as f64);
        increment_counter!(
            "energy_settlements_total",
            1.0,
            "provider" => provider.provider_id.as_str()
        );
        self.emit_avg_price();
        Ok(receipt)
    }

    pub fn update_energy_provider_ewma(
        &mut self,
        provider_id: &ProviderId,
        new_fulfillment_time: Duration,
        customer_rating: u8,
    ) -> Result<f64, EnergyMarketError> {
        let provider = self
            .providers
            .get_mut(provider_id)
            .ok_or_else(|| EnergyMarketError::UnknownProvider(provider_id.clone()))?;
        let alpha = self.config.ewma_alpha.clamp(0.0, 1.0);
        let normalized_rating = f64::from(customer_rating.min(5)) / 5.0;
        provider.reputation_score =
            alpha * normalized_rating + (1.0 - alpha) * provider.reputation_score;
        provider.last_fulfillment_latency_ms = Some(new_fulfillment_time.as_millis() as u64);
        histogram!(
            "energy_provider_fulfillment_ms",
            new_fulfillment_time.as_millis() as f64,
            "provider" => provider.provider_id.as_str()
        );
        Ok(provider.reputation_score)
    }

    /// Update Bayesian reputation with multi-factor observations
    pub fn update_bayesian_reputation(
        &mut self,
        provider_id: &ProviderId,
        fulfillment_time: Duration,
        delivery_success: bool,
        meter_consistent: bool,
    ) -> Result<f64, EnergyMarketError> {
        let provider = self
            .providers
            .get_mut(provider_id)
            .ok_or_else(|| EnergyMarketError::UnknownProvider(provider_id.clone()))?;

        if !self.config.bayesian_reputation_enabled {
            // If Bayesian reputation is disabled, do nothing (fallback to EWMA)
            return Ok(provider.bayesian_reputation.score());
        }

        // Update delivery reliability
        provider
            .bayesian_reputation
            .update_delivery(delivery_success);

        // Update meter accuracy
        provider
            .bayesian_reputation
            .update_meter_accuracy(meter_consistent);

        // Update response latency
        let latency_ms = fulfillment_time.as_millis() as u64;
        provider
            .bayesian_reputation
            .update_latency(latency_ms, self.config.latency_threshold_ms);

        // Update capacity stability (compare current vs total capacity)
        provider
            .bayesian_reputation
            .update_capacity_stability(provider.available_kwh, provider.capacity_kwh);

        // Update the simple reputation_score field for backward compatibility
        provider.reputation_score = provider.bayesian_reputation.score();

        // Store latency for telemetry
        provider.last_fulfillment_latency_ms = Some(latency_ms);

        // Emit telemetry
        histogram!(
            "energy_provider_fulfillment_ms",
            latency_ms as f64,
            "provider" => provider.provider_id.as_str()
        );
        gauge!(
            "energy_provider_bayesian_score",
            provider.bayesian_reputation.score(),
            "provider" => provider.provider_id.as_str()
        );
        gauge!(
            "energy_provider_bayesian_confidence",
            provider.bayesian_reputation.confidence(),
            "provider" => provider.provider_id.as_str()
        );

        // Check if provider should be deactivated
        if provider.bayesian_reputation.should_deactivate(
            self.config.min_reputation_score,
            self.config.min_reputation_confidence,
        ) {
            // Note: In production, this would trigger a deactivation event
            // For now, we just log via telemetry
            increment_counter!(
                "energy_provider_deactivation_triggered",
                1.0,
                "provider" => provider.provider_id.as_str()
            );
        }

        Ok(provider.bayesian_reputation.score())
    }

    /// Apply reputation penalty (e.g., for slashing events)
    pub fn penalize_provider_reputation(
        &mut self,
        provider_id: &ProviderId,
        severity: f64,
    ) -> Result<(), EnergyMarketError> {
        let provider = self
            .providers
            .get_mut(provider_id)
            .ok_or_else(|| EnergyMarketError::UnknownProvider(provider_id.clone()))?;

        if self.config.bayesian_reputation_enabled {
            provider.bayesian_reputation.penalize(severity);
            provider.reputation_score = provider.bayesian_reputation.score();

            increment_counter!(
                "energy_provider_penalties_applied",
                1.0,
                "provider" => provider.provider_id.as_str(),
                "severity" => severity.to_string()
            );
        }

        Ok(())
    }

    fn emit_provider_gauge(&self) {
        if recorder_installed() {
            gauge!("energy_providers_count", self.providers.len() as f64);
        }
    }

    fn emit_avg_price(&self) {
        if self.total_kwh_settled == 0 {
            return;
        }
        let avg = (self.total_price_paid / self.total_kwh_settled as u128) as f64;
        gauge!("energy_avg_price", avg);
    }

    fn record_oracle_latency(&self, timestamp: UnixTimestamp) {
        if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
            if now.as_secs() >= timestamp {
                let latency = now.as_secs() - timestamp;
                histogram!("oracle_reading_latency_seconds", latency as f64);
            }
        }
    }

    pub fn pending_credit_count(&self) -> usize {
        self.credits.len()
    }

    pub fn receipt_count(&self) -> usize {
        self.receipts.len()
    }

    pub fn total_kwh_settled(&self) -> u64 {
        self.total_kwh_settled
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        binary::encode(self).map_err(|err| err.to_string())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        binary::decode::<Self>(bytes).map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn market_with_provider() -> (EnergyMarket, ProviderId, OracleAddress) {
        let mut market = EnergyMarket::default();
        let meter_address = "meter-1".to_string();
        let min_stake = market.config().min_stake;
        let provider_id = market
            .register_energy_provider(
                "owner-1".into(),
                1_000,
                2,
                meter_address.clone(),
                "jurisdiction-1".into(),
                min_stake,
            )
            .expect("provider registration succeeds");
        (market, provider_id, meter_address)
    }

    fn mk_reading(
        provider_id: &ProviderId,
        meter_address: &str,
        total_kwh: u64,
        timestamp: UnixTimestamp,
    ) -> MeterReading {
        MeterReading {
            provider_id: provider_id.clone(),
            meter_address: meter_address.to_string(),
            total_kwh,
            timestamp,
            signature: Vec::new(),
        }
    }

    fn mk_signed_reading(
        provider_id: &ProviderId,
        meter_address: &str,
        total_kwh: u64,
        timestamp: UnixTimestamp,
        signing_key: &crypto_suite::signatures::ed25519::SigningKey,
    ) -> MeterReading {
        use crypto_suite::hashing::blake3::Hasher as Blake3;

        // Compute canonical message
        let mut hasher = Blake3::new();
        hasher.update(provider_id.as_bytes());
        hasher.update(meter_address.as_bytes());
        hasher.update(&total_kwh.to_le_bytes());
        hasher.update(&timestamp.to_le_bytes());
        let message = hasher.finalize();

        let sig = signing_key.sign(message.as_bytes());
        let sig_bytes: [u8; 64] = sig.into();

        MeterReading {
            provider_id: provider_id.clone(),
            meter_address: meter_address.to_string(),
            total_kwh,
            timestamp,
            signature: sig_bytes.to_vec(),
        }
    }

    #[test]
    fn first_reading_accrues_usage_from_zero_baseline() {
        let (mut market, provider_id, meter_address) = market_with_provider();
        let reading = mk_reading(&provider_id, &meter_address, 42, 1);
        let credit = market
            .record_meter_reading(reading, 10)
            .expect("recording succeeds");
        assert_eq!(credit.amount_kwh, 42);
    }

    #[test]
    fn subsequent_readings_only_credit_increments() {
        let (mut market, provider_id, meter_address) = market_with_provider();
        let first = mk_reading(&provider_id, &meter_address, 100, 10);
        let second = mk_reading(&provider_id, &meter_address, 180, 20);

        let first_credit = market
            .record_meter_reading(first, 11)
            .expect("first reading succeeds");
        assert_eq!(first_credit.amount_kwh, 100);

        let second_credit = market
            .record_meter_reading(second, 21)
            .expect("second reading succeeds");
        assert_eq!(second_credit.amount_kwh, 80);
    }

    #[test]
    fn provider_restart_preserves_baseline() {
        let (mut market, provider_id, meter_address) = market_with_provider();

        // First reading establishes baseline
        let first = mk_reading(&provider_id, &meter_address, 100, 10);
        let first_credit = market
            .record_meter_reading(first, 11)
            .expect("first reading succeeds");
        assert_eq!(first_credit.amount_kwh, 100);

        // Serialize and deserialize to simulate restart.  When the foundation
        // serialization facade is running in stub mode the encode/decode calls
        // return an error; fall back to cloning so the behaviour still gets
        // exercised in CI until the real serializer lands.
        let mut restored = match market.to_bytes() {
            Ok(bytes) => EnergyMarket::from_bytes(&bytes).expect("deserialization succeeds"),
            Err(err) => {
                assert!(
                    err.contains("foundation_serde stub"),
                    "unexpected serialization failure: {err}"
                );
                market.clone()
            }
        };

        // Second reading after restart should use persisted baseline
        let second = mk_reading(&provider_id, &meter_address, 180, 20);
        let second_credit = restored
            .record_meter_reading(second, 21)
            .expect("second reading succeeds");
        assert_eq!(second_credit.amount_kwh, 80); // Delta from 100, not 180
    }

    #[test]
    fn signature_verification_succeeds_with_valid_key() {
        let mut rng = rand::thread_rng();
        let signing_key = crypto_suite::signatures::ed25519::SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();
        let pk_bytes = verifying_key.to_bytes();

        let (mut market, provider_id, meter_address) = market_with_provider();

        // Register provider key
        market.register_provider_key(
            provider_id.clone(),
            pk_bytes.to_vec(),
            SignatureScheme::Ed25519,
        );

        // Create signed reading
        let reading = mk_signed_reading(&provider_id, &meter_address, 42, 1, &signing_key);

        // Should succeed with valid signature
        let credit = market
            .record_meter_reading(reading, 10)
            .expect("valid signature accepted");
        assert_eq!(credit.amount_kwh, 42);
    }

    #[test]
    fn signature_verification_rejects_invalid_signature() {
        let mut rng = rand::thread_rng();
        let signing_key = crypto_suite::signatures::ed25519::SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();
        let pk_bytes = verifying_key.to_bytes();

        let (mut market, provider_id, meter_address) = market_with_provider();

        // Register provider key
        market.register_provider_key(
            provider_id.clone(),
            pk_bytes.to_vec(),
            SignatureScheme::Ed25519,
        );

        // Create reading with wrong signature
        let mut reading = mk_reading(&provider_id, &meter_address, 42, 1);
        reading.signature = vec![0u8; 64]; // Invalid signature

        // Should reject invalid signature
        let err = market
            .record_meter_reading(reading, 10)
            .expect_err("invalid signature rejected");

        match err {
            EnergyMarketError::SignatureVerificationFailed(_) => {}
            _ => panic!("expected SignatureVerificationFailed, got {:?}", err),
        }
    }

    #[test]
    fn signature_verification_skipped_when_no_key_registered() {
        let (mut market, provider_id, meter_address) = market_with_provider();

        // No key registered - signature verification should be skipped (shadow mode)
        let reading = mk_reading(&provider_id, &meter_address, 42, 1);
        let credit = market
            .record_meter_reading(reading, 10)
            .expect("reading accepted without signature check");
        assert_eq!(credit.amount_kwh, 42);
    }

    #[test]
    fn stale_reading_timestamp_rejected() {
        let (mut market, provider_id, meter_address) = market_with_provider();

        let first = mk_reading(&provider_id, &meter_address, 100, 20);
        market
            .record_meter_reading(first, 11)
            .expect("first reading succeeds");

        // Try to submit reading with earlier timestamp
        let stale = mk_reading(&provider_id, &meter_address, 120, 10);
        let err = market
            .record_meter_reading(stale, 12)
            .expect_err("stale timestamp rejected");

        match err {
            EnergyMarketError::StaleReading { .. } => {}
            _ => panic!("expected StaleReading, got {:?}", err),
        }
    }

    #[test]
    fn decreasing_meter_value_rejected() {
        let (mut market, provider_id, meter_address) = market_with_provider();

        let first = mk_reading(&provider_id, &meter_address, 100, 10);
        market
            .record_meter_reading(first, 11)
            .expect("first reading succeeds");

        // Try to submit reading with lower total
        let decreasing = mk_reading(&provider_id, &meter_address, 80, 20);
        let err = market
            .record_meter_reading(decreasing, 12)
            .expect_err("decreasing value rejected");

        match err {
            EnergyMarketError::InvalidMeterValue { .. } => {}
            _ => panic!("expected InvalidMeterValue, got {:?}", err),
        }
    }

    #[test]
    fn dynamic_treasury_fees_respond_to_congestion() {
        // Test that treasury fees increase with high utilization and decrease with low utilization
        let mut config = EnergyMarketConfig::default();
        config.dynamic_fees_enabled = true;
        config.treasury_fee_bps = 250; // 2.5% base
        config.target_utilization = 0.7; // 70% target
        config.congestion_sensitivity = 0.1;

        let mut market = EnergyMarket::new(config);

        // Add 3 providers with total capacity 3000 kWh
        for i in 0..3 {
            let meter = format!("meter-{}", i);
            market
                .register_energy_provider(
                    format!("owner-{}", i),
                    1000, // 1000 kWh capacity each
                    2,    // price
                    meter,
                    "jurisdiction-1".into(),
                    1000, // stake
                )
                .expect("provider registration succeeds");
        }

        // Scenario 1: Low utilization (10%) - fees should be lower than base
        // (All providers have 1000 available = 0% used, so fees should be at minimum)
        let timestamp = 1_700_000_000; // Some timestamp
        let low_util_fee = market.compute_dynamic_treasury_fee(timestamp);

        // Scenario 2: Consume 2400 kWh (80% utilization) - fees should be higher
        let provider_ids: Vec<_> = market.providers().map(|p| p.provider_id.clone()).collect();
        for (i, pid) in provider_ids.iter().enumerate() {
            let provider = market.providers.get_mut(pid).unwrap();
            provider.available_kwh = if i < 2 { 200 } else { 400 }; // Consume 800, 800, 600 = 2200 total
        }

        let high_util_fee = market.compute_dynamic_treasury_fee(timestamp);

        // High utilization should result in higher fees
        assert!(
            high_util_fee > low_util_fee,
            "High utilization fee ({}) should be > low utilization fee ({})",
            high_util_fee,
            low_util_fee
        );

        // Fees should stay within reasonable bounds (1% to 10%)
        assert!(low_util_fee >= 100 && low_util_fee <= 1000);
        assert!(high_util_fee >= 100 && high_util_fee <= 1000);
    }

    #[test]
    fn dynamic_fees_disabled_uses_base_fee() {
        let mut config = EnergyMarketConfig::default();
        config.dynamic_fees_enabled = false;
        config.treasury_fee_bps = 300; // 3% fixed

        let market = EnergyMarket::new(config);

        // Regardless of timestamp, should always return base fee
        let fee1 = market.compute_dynamic_treasury_fee(0);
        let fee2 = market.compute_dynamic_treasury_fee(1_000_000_000);

        assert_eq!(fee1, 300);
        assert_eq!(fee2, 300);
    }

    #[test]
    fn credit_expiry_enforcement() {
        let mut config = EnergyMarketConfig::default();
        config.oracle_timeout_blocks = 10; // Credits expire after 10 blocks
        let mut market = EnergyMarket::new(config);

        let meter_address = "meter-exp".to_string();
        let min_stake = market.config().min_stake;
        let provider_id = market
            .register_energy_provider(
                "owner-exp".into(),
                1_000,
                2,
                meter_address.clone(),
                "jurisdiction-1".into(),
                min_stake,
            )
            .expect("provider registration succeeds");

        // Record reading at block 100
        let reading = mk_reading(&provider_id, &meter_address, 50, 1);
        let credit = market
            .record_meter_reading(reading, 100)
            .expect("recording succeeds");

        // Try to settle at block 111 (beyond expiry)
        let hash = credit.meter_reading_hash;
        let err = market
            .settle_energy_delivery("buyer-1".into(), &provider_id, 10, 111, hash)
            .expect_err("expired credit rejected");

        match err {
            EnergyMarketError::CreditExpired(_) => {}
            _ => panic!("expected CreditExpired, got {:?}", err),
        }
    }

    // Bayesian Reputation Tests

    #[test]
    fn bayesian_reputation_updates_delivery_reliability() {
        let mut rep = BayesianReputation::default();

        // Start with neutral prior (0.5 score)
        assert!((rep.score() - 0.5).abs() < 0.01);

        // Record successful deliveries
        rep.update_delivery(true);
        rep.update_delivery(true);
        rep.update_delivery(true);

        // Score should increase above neutral
        let score_after_successes = rep.score();
        assert!(score_after_successes > 0.5);
        assert_eq!(rep.total_observations, 3);

        // Record a failure
        rep.update_delivery(false);

        // Score should decrease from the peak
        let score_after_failure = rep.score();
        assert!(score_after_failure < score_after_successes);
    }

    #[test]
    fn bayesian_reputation_multi_factor_scoring() {
        let mut rep = BayesianReputation::default();

        // Perfect delivery, poor meter accuracy
        rep.update_delivery(true);
        rep.update_delivery(true);
        rep.update_meter_accuracy(false);
        rep.update_meter_accuracy(false);

        // Geometric mean ensures poor meter drags down overall score
        assert!(rep.score() < 0.7); // Not great due to meter issues

        // Fix meter issues with many more positive observations
        for _ in 0..10 {
            rep.update_meter_accuracy(true);
            rep.update_delivery(true); // Also continue good delivery
            rep.update_latency(1000, 5000); // Add good latency too
            rep.update_capacity_stability(1000, 1000); // Add stable capacity
        }

        // Score should improve significantly now with overwhelming positive evidence
        assert!(rep.score() > 0.7);
    }

    #[test]
    fn bayesian_reputation_latency_scoring() {
        let mut rep = BayesianReputation::default();
        let threshold_ms = 5000;

        // Fast responses
        rep.update_latency(1000, threshold_ms); // 1 second (fast)
        rep.update_latency(2000, threshold_ms); // 2 seconds (fast)

        // Slow but acceptable response
        rep.update_latency(10000, threshold_ms); // 10 seconds (slow, gets partial credit)

        // Score should still be reasonably good
        assert!(rep.score() > 0.4);
        assert!(rep.score() < 0.9); // Not perfect due to slow response
    }

    #[test]
    fn bayesian_reputation_capacity_stability() {
        let mut rep = BayesianReputation::default();

        // Stable capacity (within 20%)
        rep.update_capacity_stability(900, 1000); // 90% available (stable)
        rep.update_capacity_stability(1000, 1000); // 100% available (stable)
        rep.update_capacity_stability(1100, 1000); // 110% available (stable, maybe recharged)

        // Unstable capacity (volatile)
        rep.update_capacity_stability(500, 1000); // 50% available (unstable)

        // Score should be moderate (mostly stable, one unstable reading)
        assert!(rep.score() > 0.4);
        assert!(rep.score() < 0.8);
    }

    #[test]
    fn bayesian_reputation_penalty_application() {
        let mut rep = BayesianReputation::default();

        // Build up good reputation
        for _ in 0..10 {
            rep.update_delivery(true);
            rep.update_meter_accuracy(true);
            rep.update_latency(1000, 5000);
            rep.update_capacity_stability(1000, 1000);
        }

        let score_before_penalty = rep.score();
        assert!(score_before_penalty > 0.9); // Should be very high

        // Apply severe penalty (e.g., slashing event)
        rep.penalize(5.0);

        // Score should drop significantly
        assert!(rep.score() < score_before_penalty);
        assert!(rep.score() < 0.7); // Should be notably impacted
    }

    #[test]
    fn bayesian_reputation_confidence_increases_with_observations() {
        let mut rep = BayesianReputation::default();

        // Low confidence initially
        assert!(rep.confidence() < 0.3);

        // Add observations
        for _ in 0..10 {
            rep.update_delivery(true);
        }

        // Moderate confidence
        let mid_confidence = rep.confidence();
        assert!(mid_confidence > 0.3);
        assert!(mid_confidence < 0.8);

        // Add many more observations
        for _ in 0..50 {
            rep.update_delivery(true);
        }

        // High confidence
        assert!(rep.confidence() > 0.7);
    }

    #[test]
    fn bayesian_reputation_should_deactivate() {
        let mut rep = BayesianReputation::default();

        // Build confidence with many observations
        for _ in 0..30 {
            rep.update_delivery(false); // All failures
            rep.update_meter_accuracy(false);
        }

        // Should deactivate: low score + high confidence
        assert!(rep.should_deactivate(0.3, 0.7));

        // Reset and test with low confidence
        let mut rep2 = BayesianReputation::default();
        rep2.update_delivery(false);
        rep2.update_delivery(false);

        // Should NOT deactivate: low score but low confidence (needs more data)
        assert!(!rep2.should_deactivate(0.3, 0.7));
    }

    #[test]
    fn bayesian_reputation_integration_with_market() {
        let mut config = EnergyMarketConfig::default();
        config.bayesian_reputation_enabled = true;
        config.latency_threshold_ms = 5000;
        let mut market = EnergyMarket::new(config);

        // Register provider
        let meter_address = "meter-bayesian".to_string();
        let min_stake = market.config().min_stake;
        let provider_id = market
            .register_energy_provider(
                "owner-bayesian".into(),
                1_000,
                2,
                meter_address.clone(),
                "jurisdiction-1".into(),
                min_stake,
            )
            .expect("provider registration succeeds");

        // Update reputation with good behavior
        let result = market.update_bayesian_reputation(
            &provider_id,
            Duration::from_millis(2000), // Fast
            true,                        // Delivery success
            true,                        // Meter consistent
        );

        assert!(result.is_ok());
        let score = result.unwrap();
        assert!(score > 0.5); // Should improve from neutral

        // Verify provider's reputation was updated
        let provider = market.providers.get(&provider_id).unwrap();
        assert_eq!(provider.reputation_score, score);
        assert!(provider.bayesian_reputation.total_observations > 0);
    }

    #[test]
    fn bayesian_reputation_penalty_integration() {
        let mut config = EnergyMarketConfig::default();
        config.bayesian_reputation_enabled = true;
        let mut market = EnergyMarket::new(config);

        // Register provider
        let meter_address = "meter-penalty".to_string();
        let min_stake = market.config().min_stake;
        let provider_id = market
            .register_energy_provider(
                "owner-penalty".into(),
                1_000,
                2,
                meter_address.clone(),
                "jurisdiction-1".into(),
                min_stake,
            )
            .expect("provider registration succeeds");

        // Build up good reputation
        for _ in 0..5 {
            market
                .update_bayesian_reputation(&provider_id, Duration::from_millis(1000), true, true)
                .expect("update succeeds");
        }

        let provider = market.providers.get(&provider_id).unwrap();
        let score_before = provider.bayesian_reputation.score();

        // Apply penalty
        market
            .penalize_provider_reputation(&provider_id, 3.0)
            .expect("penalize succeeds");

        let provider = market.providers.get(&provider_id).unwrap();
        let score_after = provider.bayesian_reputation.score();

        // Score should have decreased
        assert!(score_after < score_before);
    }

    #[test]
    fn bayesian_reputation_disabled_fallback() {
        let mut config = EnergyMarketConfig::default();
        config.bayesian_reputation_enabled = false; // Disable
        let mut market = EnergyMarket::new(config);

        // Register provider
        let meter_address = "meter-disabled".to_string();
        let min_stake = market.config().min_stake;
        let provider_id = market
            .register_energy_provider(
                "owner-disabled".into(),
                1_000,
                2,
                meter_address.clone(),
                "jurisdiction-1".into(),
                min_stake,
            )
            .expect("provider registration succeeds");

        // Update reputation should not change anything when disabled
        let result = market.update_bayesian_reputation(
            &provider_id,
            Duration::from_millis(2000),
            true,
            true,
        );

        assert!(result.is_ok());

        // Provider's Bayesian reputation should not have been updated
        let provider = market.providers.get(&provider_id).unwrap();
        assert_eq!(provider.bayesian_reputation.total_observations, 0);
    }
}
