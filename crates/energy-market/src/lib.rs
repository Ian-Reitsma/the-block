#![forbid(unsafe_code)]

use crypto_suite::hashing::blake3::Hasher as Blake3;
use foundation_metrics::{gauge, histogram, increment_counter, recorder_installed};
use foundation_serialization::{binary, Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

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
        let delta = reading
            .total_kwh
            .saturating_sub(provider.last_meter_value.unwrap_or(reading.total_kwh));
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
