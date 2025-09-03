//! Staking advice utilities using Cornish–Fisher CVaR correction.
#![allow(clippy::many_single_char_names)]

use std::f64::consts::PI;

/// Return recommended Kelly fraction and CVaR_{0.999} for a validator stake.
///
/// * `p` – probability of winning the block reward.
/// * `reward` – reward in CT if the block is won.
/// * `loss` – loss in CT if slashed.
/// * `sigma` – standard deviation of returns.
/// * `skew`/`kurt` – skewness and kurtosis of the return distribution.
pub fn stake_advice(
    p: f64,
    reward: f64,
    loss: f64,
    sigma: f64,
    skew: f64,
    kurt: f64,
) -> (f64, f64) {
    // Cornish–Fisher expansion for z at alpha=0.999
    let z = 3.09023230616781; // Phi^{-1}(0.999)
    let z_cf = z
        + (1.0 / 6.0) * (z * z - 1.0) * skew
        + (1.0 / 24.0) * (z * z * z - 3.0 * z) * (kurt - 3.0)
        - (1.0 / 36.0) * (2.0 * z * z * z - 5.0 * z) * skew * skew;

    let sigma_star = sigma
        * (1.0
            + (skew * z_cf) / 6.0
            + (kurt - 3.0) * (z_cf * z_cf - 1.0) / 24.0
            - skew * skew * (2.0 * z_cf * z_cf - 1.0) / 36.0);

    // CVaR for normal approx: mu=loss, tail beyond VaR
    let phi = (-0.5 * z_cf * z_cf).exp() / (2.0 * PI).sqrt();
    let cvar = loss + sigma_star * phi / (1.0 - 0.999);

    // Kelly fraction with corrected tail risk
    let kelly = (p * reward - (1.0 - p) * cvar) / cvar;

    (kelly.max(0.0), cvar)
}

#[cfg(test)]
mod tests {
    use super::stake_advice;

    #[test]
    fn cvar_reasonable() {
        let (f, cvar) = stake_advice(0.1, 10.0, 5.0, 1.0, 0.0, 3.0);
        assert!(f >= 0.0);
        assert!(cvar > 5.0);
    }
}

