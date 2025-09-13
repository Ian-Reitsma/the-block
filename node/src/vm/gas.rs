use super::opcodes::OpCode;

/// Gas costs for various VM operations.
pub const GAS_IMMEDIATE: u64 = 1;
pub const GAS_STORAGE_READ: u64 = 10;
pub const GAS_STORAGE_WRITE: u64 = 20;
pub const GAS_CODE_READ: u64 = 5;
pub const GAS_HASH: u64 = 50;

/// Return the gas cost for a single opcode (excluding immediates).
#[must_use]
pub fn cost(op: OpCode) -> u64 {
    match op {
        OpCode::Halt => 0,
        OpCode::Push => 1,
        OpCode::Add | OpCode::Sub | OpCode::And | OpCode::Or | OpCode::Xor => 1,
        OpCode::Mul => 2,
        OpCode::Div | OpCode::Mod => 3,
        OpCode::Load => GAS_STORAGE_READ,
        OpCode::Store => GAS_STORAGE_WRITE,
        OpCode::Hash => GAS_HASH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_matches() {
        assert_eq!(cost(OpCode::Add), 1);
        assert_eq!(cost(OpCode::Mod), 3);
        assert_eq!(cost(OpCode::Load), GAS_STORAGE_READ);
        assert_eq!(cost(OpCode::Hash), GAS_HASH);
    }
}

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
        match self.used.checked_add(amount) {
            Some(new) if new <= self.limit => {
                self.used = new;
                #[cfg(feature = "telemetry")]
                {
                    use crate::telemetry::{VM_GAS_USED_TOTAL, VM_OUT_OF_GAS_TOTAL};
                    VM_GAS_USED_TOTAL.inc_by(amount);
                    let _ = VM_OUT_OF_GAS_TOTAL; // silence unused when feature off
                }
                Ok(())
            }
            _ => {
                #[cfg(feature = "telemetry")]
                {
                    use crate::telemetry::VM_OUT_OF_GAS_TOTAL;
                    VM_OUT_OF_GAS_TOTAL.inc();
                }
                Err("out of gas")
            }
        }
    }

    #[must_use]
    pub fn used(&self) -> u64 {
        self.used
    }
}
