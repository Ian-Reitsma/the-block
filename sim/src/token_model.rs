#![forbid(unsafe_code)]

use ledger::{Emission, TokenRegistry};

/// Simple macroeconomic simulation of the BLOCK token.
/// Consumer and Industrial lanes share a single token with unified emission.
pub fn simulate(height: u64) -> TokenRegistry {
    let mut reg = TokenRegistry::new();
    reg.register(
        "BLOCK",
        Emission::Linear {
            initial: 0,
            rate: 8, // Combined rate: consumer (5) + industrial (3)
        },
    );
    // advance height to compute supplies
    for _ in 0..height {
        // placeholder; real model would update state
    }
    reg
}
