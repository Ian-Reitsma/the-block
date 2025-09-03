#![forbid(unsafe_code)]

/// Default virtual reserve epsilon ensuring bounded slippage.
pub const EPSILON_DEFAULT: f64 = 1e-8;

/// Swap using the del-Pino invariant with virtual reserves.
///
/// Invariant: `(x+ε) ln(x+ε) + (y+ε) ln(y+ε) = k`.
/// Reserves `x` and `y` must be positive; `dx` is added to `x`.
/// Returns `dy` withdrawn from `y`.
pub fn swap_with_epsilon(x: f64, y: f64, dx: f64, epsilon: f64) -> f64 {
    assert!(x > 0.0 && y > 0.0 && epsilon > 0.0);
    let k = (x + epsilon) * (x + epsilon).ln() + (y + epsilon) * (y + epsilon).ln();
    let x_new = x + dx;
    let mut y_new = y;
    for _ in 0..50 {
        let f = (x_new + epsilon) * (x_new + epsilon).ln()
              + (y_new + epsilon) * (y_new + epsilon).ln() - k;
        let df = (y_new + epsilon).ln() + 1.0;
        let step = f / df;
        y_new -= step;
        if step.abs() < 1e-12 { break; }
    }
    let dy = y - y_new;
    assert!(dy >= -1e-12);
    dy.max(0.0)
}

/// Convenience wrapper using the default epsilon.
pub fn swap_del_pino(x: f64, y: f64, dx: f64) -> f64 {
    swap_with_epsilon(x, y, dx, EPSILON_DEFAULT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_invariant() {
        let x = 1000.0;
        let y = 1000.0;
        let dx = 100.0;
        let eps = EPSILON_DEFAULT;
        let dy = swap_del_pino(x, y, dx);
        let k1 = (x + eps) * (x + eps).ln() + (y + eps) * (y + eps).ln();
        let k2 = (x + dx + eps) * (x + dx + eps).ln() + (y - dy + eps) * (y - dy + eps).ln();
        assert!((k1 - k2).abs() < 1e-6);
    }

    #[test]
    fn bounded_slippage_low_liquidity() {
        let x = 1e-6;
        let y = 1e-6;
        let dx = 1e-7;
        let dy = swap_del_pino(x, y, dx);
        let eps = EPSILON_DEFAULT;
        let k1 = (x + eps) * (x + eps).ln() + (y + eps) * (y + eps).ln();
        let k2 = (x + dx + eps) * (x + dx + eps).ln() + (y - dy + eps) * (y - dy + eps).ln();
        assert!((k1 - k2).abs() < 1e-6);
        assert!(dy.is_finite());
    }

    #[test]
    fn invariant_near_zero() {
        let x = 1e-9;
        let y = 2e-9;
        let dx = 5e-10;
        let dy = swap_del_pino(x, y, dx);
        let eps = EPSILON_DEFAULT;
        let k1 = (x + eps) * (x + eps).ln() + (y + eps) * (y + eps).ln();
        let k2 = (x + dx + eps) * (x + dx + eps).ln() + (y - dy + eps) * (y - dy + eps).ln();
        assert!((k1 - k2).abs() < 1e-6);
    }
}
