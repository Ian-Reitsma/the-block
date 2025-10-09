//! Spectral transform helpers implemented with first-party routines.
//!
//! These helpers intentionally prioritise clarity and auditability over peak
//! performance so runtime call sites can rely on deterministic, dependency-free
//! behaviour. Optimised kernels can replace these implementations once
//! benchmarks capture the workload characteristics we need to target.

/// Compute an in-place, type-II discrete cosine transform over the provided
/// buffer.
///
/// The implementation follows the reference definition used by `rustdct` and
/// other FFT libraries:
///
/// ```text
/// X_k = \sum_{n=0}^{N-1} x_n * cos(\pi/N * (n + 0.5) * k)
/// ```
///
/// The scaling factor is intentionally left as-is because the burst veto logic
/// only compares band energy ratios, making the constant multipliers irrelevant
/// to the result. The routine accepts any buffer length greater than zero and
/// leaves empty inputs untouched.
pub fn dct2_inplace(data: &mut [f64]) {
    let n = data.len();
    if n <= 1 {
        return;
    }

    let factor = std::f64::consts::PI / n as f64;
    let mut output = vec![0.0f64; n];

    for k in 0..n {
        let mut sum = 0.0;
        for (i, &x) in data.iter().enumerate() {
            let angle = factor * (i as f64 + 0.5) * k as f64;
            sum += x * angle.cos();
        }
        output[k] = sum;
    }

    data.copy_from_slice(&output);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dct2_matches_reference_definition() {
        let mut data = [
            0.0f64,
            0.19509032201612825,
            0.3826834323650898,
            0.5555702330196022,
            0.7071067811865475,
            0.8314696123025452,
            0.9238795325112867,
            0.9807852804032304,
        ];
        let expected = [
            4.57658519380443,
            -1.846801405099262,
            -0.29685903315095585,
            -0.14896136038438823,
            -0.06339962150781264,
            -0.04366125191989845,
            -0.019937495037371855,
            -0.010975379839653021,
        ];
        dct2_inplace(&mut data);
        for (value, target) in data.iter().zip(expected.iter()) {
            assert!((value - target).abs() < 1e-9);
        }
    }

    #[test]
    fn empty_and_singleton_inputs_are_stable() {
        let mut empty: [f64; 0] = [];
        dct2_inplace(&mut empty);
        assert!(empty.is_empty());

        let mut singleton = [42.0f64];
        dct2_inplace(&mut singleton);
        assert_eq!(singleton, [42.0]);
    }
}
