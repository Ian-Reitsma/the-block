use core::f64::consts::PI;

/// Chi-squared distribution with floating-point degrees of freedom.
#[derive(Debug, Clone, Copy)]
pub struct ChiSquared {
    dof: f64,
}

impl ChiSquared {
    /// Create a new chi-squared distribution.
    pub fn new(dof: f64) -> Option<Self> {
        if dof > 0.0 && dof.is_finite() {
            Some(Self { dof })
        } else {
            None
        }
    }

    /// Inverse cumulative distribution function.
    pub fn inverse_cdf(&self, p: f64) -> f64 {
        assert!((0.0..1.0).contains(&p), "probability must be in (0,1)");
        chi_squared_inv_cdf(self.dof, p)
    }
}

fn chi_squared_inv_cdf(dof: f64, p: f64) -> f64 {
    let mut hi = (dof + 10.0 * dof.sqrt() + 50.0).max(1.0);
    while chi_squared_cdf(dof, hi) < p {
        hi *= 2.0;
        if hi.is_infinite() || hi > 1e12 {
            break;
        }
    }
    let mut lo = 0.0;
    for _ in 0..96 {
        let mid = 0.5 * (lo + hi);
        let cdf = chi_squared_cdf(dof, mid);
        if cdf > p {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    0.5 * (lo + hi)
}

fn chi_squared_cdf(dof: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    regularized_lower_gamma(0.5 * dof, 0.5 * x)
}

fn regularized_lower_gamma(s: f64, x: f64) -> f64 {
    debug_assert!(s > 0.0);
    const MAX_ITER: usize = 2000;
    const EPS: f64 = 1e-12;
    const FPMIN: f64 = f64::MIN_POSITIVE / EPS;

    let ln_gamma_s = ln_gamma(s);
    if x < s + 1.0 {
        let mut sum = 1.0 / s;
        let mut term = sum;
        let mut ap = s;
        for _ in 0..MAX_ITER {
            ap += 1.0;
            term *= x / ap;
            sum += term;
            if term.abs() < EPS * sum.abs() {
                break;
            }
        }
        (sum * (-x + s * x.ln() - ln_gamma_s).exp()).clamp(0.0, 1.0)
    } else {
        let mut b = x + 1.0 - s;
        let mut c = 1.0 / FPMIN;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..=MAX_ITER {
            let an = -(i as f64) * ((i as f64) - s);
            b += 2.0;
            d = an * d + b;
            if d.abs() < FPMIN {
                d = FPMIN;
            }
            c = b + an / c;
            if c.abs() < FPMIN {
                c = FPMIN;
            }
            d = 1.0 / d;
            let delta = d * c;
            h *= delta;
            if (delta - 1.0).abs() < EPS {
                break;
            }
        }
        (1.0 - (h * (-x + s * x.ln() - ln_gamma_s).exp())).clamp(0.0, 1.0)
    }
}

fn ln_gamma(z: f64) -> f64 {
    const COEFFS: [f64; 9] = [
        0.999_999_999_999_809_93,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_13,
        -176.615_029_162_140_59,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if z < 0.5 {
        return PI.ln() - (PI * z).sin().ln() - ln_gamma(1.0 - z);
    }
    let mut x = COEFFS[0];
    let z = z - 1.0;
    for (i, coeff) in COEFFS.iter().enumerate().skip(1) {
        x += coeff / (z + i as f64);
    }
    let t = z + 7.5;
    (0.5 * (2.0 * PI).ln()) + (z + 0.5) * t.ln() - t + x.ln()
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::ChiSquared;

    #[test]
    fn chi_squared_quantile_matches_reference() {
        let dist = ChiSquared::new(4.0).unwrap();
        let quantile = dist.inverse_cdf(0.99);
        assert_relative_eq!(quantile, 13.276_704, epsilon = 1e-3);
    }
}
