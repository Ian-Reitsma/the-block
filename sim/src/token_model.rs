#![forbid(unsafe_code)]

use ledger::{Emission, TokenRegistry};

/// Simple macroeconomic simulation of multiple tokens.
pub fn simulate(height: u64) -> TokenRegistry {
    let mut reg = TokenRegistry::new();
    reg.register(
        "CT",
        Emission::Linear {
            initial: 0,
            rate: 5,
        },
    );
    reg.register(
        "IT",
        Emission::Linear {
            initial: 0,
            rate: 3,
        },
    );
    // advance height to compute supplies
    for _ in 0..height {
        // placeholder; real model would update state
    }
    reg
}
