use serde::Serialize;

#[derive(Debug, Serialize)]
pub enum MarketError {
    NoPriceData,
    InvalidWorkload,
    JobNotFound,
    Internal,
}

impl MarketError {
    pub fn code(&self) -> i32 {
        match self {
            MarketError::NoPriceData => -33000,
            MarketError::InvalidWorkload => -33001,
            MarketError::JobNotFound => -33002,
            MarketError::Internal => -33099,
        }
    }
    pub fn message(&self) -> &'static str {
        match self {
            MarketError::NoPriceData => "no price data",
            MarketError::InvalidWorkload => "invalid workload",
            MarketError::JobNotFound => "job not found",
            MarketError::Internal => "internal error",
        }
    }
}
