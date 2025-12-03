#![forbid(unsafe_code)]

use energy_market::{OracleAddress, ProviderId, UnixTimestamp};
use foundation_serialization::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

pub type Signature = Vec<u8>;

pub trait SignatureVerifier: Send + Sync + 'static {
    fn verify(&self, provider_id: &ProviderId, payload: &[u8], signature: &[u8]) -> bool;
}

#[derive(Default)]
pub struct NoopSignatureVerifier;

impl SignatureVerifier for NoopSignatureVerifier {
    fn verify(&self, _provider_id: &ProviderId, _payload: &[u8], _signature: &[u8]) -> bool {
        true
    }
}

pub trait MeterReading {
    fn timestamp(&self) -> UnixTimestamp;
    fn provider_id(&self) -> &ProviderId;
    fn meter_address(&self) -> &OracleAddress;
    fn kwh_reading(&self) -> u64;
    fn signature(&self) -> &[u8];
    fn signing_bytes(&self) -> Vec<u8>;

    fn verify<V: SignatureVerifier>(&self, verifier: &V) -> bool {
        verifier.verify(self.provider_id(), &self.signing_bytes(), self.signature())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MeterReadingPayload {
    pub provider_id: ProviderId,
    pub meter_address: OracleAddress,
    pub kwh_reading: u64,
    pub timestamp: UnixTimestamp,
    pub signature: Signature,
}

impl MeterReadingPayload {
    pub fn new(
        provider_id: ProviderId,
        meter_address: OracleAddress,
        kwh_reading: u64,
        timestamp: UnixTimestamp,
        signature: Signature,
    ) -> Self {
        Self {
            provider_id,
            meter_address,
            kwh_reading,
            timestamp,
            signature,
        }
    }
}

impl MeterReading for MeterReadingPayload {
    fn timestamp(&self) -> UnixTimestamp {
        self.timestamp
    }

    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    fn meter_address(&self) -> &OracleAddress {
        &self.meter_address
    }

    fn kwh_reading(&self) -> u64 {
        self.kwh_reading
    }

    fn signature(&self) -> &[u8] {
        &self.signature
    }

    fn signing_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(self.provider_id.as_bytes());
        out.extend_from_slice(self.meter_address.as_bytes());
        out.extend_from_slice(&self.kwh_reading.to_le_bytes());
        out.extend_from_slice(&self.timestamp.to_le_bytes());
        out
    }
}

#[derive(Debug, Error)]
pub enum OracleError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("invalid signature for provider {0}")]
    InvalidSignature(ProviderId),
    #[error("submit error: {0}")]
    Submit(String),
}

pub struct OracleAdapter<F, S, V>
where
    F: Fn(&str) -> Result<MeterReadingPayload, OracleError> + Send + Sync + 'static,
    S: Fn(&MeterReadingPayload) -> Result<(), OracleError> + Send + Sync + 'static,
    V: SignatureVerifier,
{
    fetcher: Arc<F>,
    submitter: Arc<S>,
    verifier: Arc<V>,
}

impl<F, S, V> OracleAdapter<F, S, V>
where
    F: Fn(&str) -> Result<MeterReadingPayload, OracleError> + Send + Sync + 'static,
    S: Fn(&MeterReadingPayload) -> Result<(), OracleError> + Send + Sync + 'static,
    V: SignatureVerifier,
{
    pub fn new(fetcher: F, submitter: S, verifier: V) -> Self {
        Self {
            fetcher: Arc::new(fetcher),
            submitter: Arc::new(submitter),
            verifier: Arc::new(verifier),
        }
    }

    pub async fn fetch_meter_reading(
        &self,
        meter_address: &str,
    ) -> Result<MeterReadingPayload, OracleError> {
        let reading = (self.fetcher)(meter_address)?;
        if !reading.verify(self.verifier.as_ref()) {
            return Err(OracleError::InvalidSignature(reading.provider_id.clone()));
        }
        Ok(reading)
    }

    pub fn submit_reading_to_chain(&self, reading: MeterReadingPayload) -> Result<(), OracleError> {
        (self.submitter)(&reading)
    }
}
