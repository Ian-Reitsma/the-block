use foundation_serialization::Serialize;
use std::fmt;

#[derive(Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum MarketError {
    NoPriceData,
    InvalidWorkload,
    JobNotFound,
    Internal,
    Capacity,
    FairShare,
    BurstExhausted,
    Capability,
    Rehearsal,
}

impl MarketError {
    pub fn code(&self) -> i32 {
        match self {
            MarketError::NoPriceData => -33000,
            MarketError::InvalidWorkload => -33001,
            MarketError::JobNotFound => -33002,
            MarketError::Internal => -33099,
            MarketError::Capacity => -33100,
            MarketError::FairShare => -33101,
            MarketError::BurstExhausted => -33102,
            MarketError::Capability => -33103,
            MarketError::Rehearsal => -33104,
        }
    }
    pub fn message(&self) -> &'static str {
        match self {
            MarketError::NoPriceData => "no price data",
            MarketError::InvalidWorkload => "invalid workload",
            MarketError::JobNotFound => "job not found",
            MarketError::Internal => "internal error",
            MarketError::Capacity => "insufficient capacity",
            MarketError::FairShare => "fair share cap exceeded",
            MarketError::BurstExhausted => "burst quota exhausted",
            MarketError::Capability => "capability mismatch",
            MarketError::Rehearsal => "compute market in rehearsal mode",
        }
    }
}

impl fmt::Display for MarketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}
