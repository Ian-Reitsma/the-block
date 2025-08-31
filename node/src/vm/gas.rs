/// Basic gas meter for tracking consumption.
#[derive(Debug, Clone)]
pub struct GasMeter {
    limit: u64,
    used: u64,
}

impl GasMeter {
    pub fn new(limit: u64) -> Self {
        Self { limit, used: 0 }
    }

    /// Charge some amount of gas.
    pub fn charge(&mut self, amount: u64) -> Result<(), &'static str> {
        self.used += amount;
        if self.used > self.limit {
            Err("out of gas")
        } else {
            Ok(())
        }
    }

    #[must_use]
    pub fn used(&self) -> u64 {
        self.used
    }
}
