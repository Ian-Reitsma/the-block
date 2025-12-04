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
    pub reputation_score: f64,
    pub meter_address: OracleAddress,
    pub total_delivered_kwh: u64,
    pub staked_balance: Balance,
    pub last_fulfillment_latency_ms: Option<u64>,
    pub last_meter_value: Option<u64>,
    pub last_meter_timestamp: Option<UnixTimestamp>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergyMarketConfig {
    pub min_stake: Balance,
    pub treasury_fee_bps: u16,
    pub ewma_alpha: f64,
    pub jurisdiction_fee_bps: u16,
    pub oracle_timeout_blocks: u64,
    pub slashing_rate_bps: u16,
}

impl Default for EnergyMarketConfig {
    fn default() -> Self {
        Self {
            min_stake: 1_000,
            treasury_fee_bps: 500, // 5%
            ewma_alpha: 0.3,
            jurisdiction_fee_bps: 0,
            oracle_timeout_blocks: 0,
            slashing_rate_bps: 0,
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
            self.verifier_registry.verify(&reading)?;
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
        let delta = reading
            .total_kwh
            .saturating_sub(previous_value);
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

    pub fn settle_energy_delivery(
        &mut self,
        buyer: AccountId,
        provider_id: &ProviderId,
        kwh_consumed: u64,
        block: BlockNumber,
        meter_hash: H256,
    ) -> Result<EnergyReceipt, EnergyMarketError> {
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
            total_cost.saturating_mul(self.config.treasury_fee_bps as u64) / BASIS_POINTS_DIVISOR;
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
        use crypto_suite::signatures::Signer;

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

        // Serialize and deserialize to simulate restart
        let serialized = market.to_bytes().expect("serialization succeeds");
        let mut restored = EnergyMarket::from_bytes(&serialized).expect("deserialization succeeds");

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
        market.register_provider_key(provider_id.clone(), pk_bytes.to_vec(), SignatureScheme::Ed25519);

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
        market.register_provider_key(provider_id.clone(), pk_bytes.to_vec(), SignatureScheme::Ed25519);

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
}
