#![forbid(unsafe_code)]

/// Trait for cross-chain exchange adapters.
pub trait ExchangeAdapter {
    /// Swap `input` amount for the target asset, ensuring at least `min_output` is received.
    fn swap(&self, input: u64, min_output: u64) -> Result<u64, &'static str>;
}

/// Simplified Uniswap adapter using a constant product formula with 0.3% fee.
pub struct UniswapAdapter {
    pub reserve_in: u64,
    pub reserve_out: u64,
}

impl ExchangeAdapter for UniswapAdapter {
    fn swap(&self, input: u64, min_output: u64) -> Result<u64, &'static str> {
        let input_with_fee = input * 997 / 1000; // 0.3% fee
        let numerator = input_with_fee * self.reserve_out;
        let denominator = self.reserve_in + input_with_fee;
        let output = numerator / denominator;
        if output < min_output {
            return Err("slippage");
        }
        Ok(output)
    }
}

/// Simplified Osmosis adapter using a constant product formula with 0.2% fee.
pub struct OsmosisAdapter {
    pub reserve_in: u64,
    pub reserve_out: u64,
}

impl ExchangeAdapter for OsmosisAdapter {
    fn swap(&self, input: u64, min_output: u64) -> Result<u64, &'static str> {
        let input_with_fee = input * 998 / 1000; // 0.2% fee
        let numerator = input_with_fee * self.reserve_out;
        let denominator = self.reserve_in + input_with_fee;
        let output = numerator / denominator;
        if output < min_output {
            return Err("slippage");
        }
        Ok(output)
    }
}
