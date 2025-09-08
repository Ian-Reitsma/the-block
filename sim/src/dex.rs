#![forbid(unsafe_code)]

/// Compute output amount `dy` for input `dx` using the del‑Pino logarithmic
/// invariant `x ln x + y ln y = k`.
///
/// Reserves `x` and `y` must be positive. The function solves for the new `y`
/// such that the invariant holds after adding `dx` to `x` and returns the
/// amount of `y` that leaves the pool.
pub fn swap_del_pino(x: f64, y: f64, dx: f64) -> f64 {
    assert!(x > 0.0 && y > 0.0);
    let k = x * x.ln() + y * y.ln();
    let x_new = x + dx;
    let mut y_new = y; // initial guess
                       // Newton–Raphson iterations
    for _ in 0..20 {
        let f = x_new * x_new.ln() + y_new * y_new.ln() - k;
        let df = y_new.ln() + 1.0;
        let step = f / df;
        y_new -= step;
        if step.abs() < 1e-12 {
            break;
        }
    }
    let dy = y - y_new;
    assert!(dy >= 0.0);
    dy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_invariant() {
        let x = 1000.0;
        let y = 1000.0;
        let dx = 100.0;
        let dy = swap_del_pino(x, y, dx);
        let k1 = x * x.ln() + y * y.ln();
        let k2 = (x + dx) * (x + dx).ln() + (y - dy) * (y - dy).ln();
        assert!((k1 - k2).abs() < 1e-6);
    }
}
