use nalgebra::{DMatrix, DVector};
use statrs::distribution::{ChiSquared, ContinuousCDF};

/// Augmented risk-sensitive Kalman filter retaining previous utilisation.
pub struct KalmanLqg {
    pub x: DVector<f64>, // [theta(4), u_prev(4)]
    pub p: DMatrix<f64>, // 8x8 covariance
}

impl KalmanLqg {
    #[allow(dead_code)]
    pub fn new(theta: &[f64; 4]) -> Self {
        let mut x = DVector::from_element(8, 0.0);
        for i in 0..4 {
            x[i] = theta[i];
        }
        KalmanLqg {
            x,
            p: DMatrix::identity(8, 8) * 1e-3,
        }
    }

    /// Perform one prediction/update step.
    pub fn step(&mut self, meas: &[f64; 4], tau_epoch: f64, risk_lambda: f64) {
        let d = 4;
        // A* = [[I,0],[I,0]]
        let mut a = DMatrix::<f64>::zeros(2 * d, 2 * d);
        for i in 0..d {
            a[(i, i)] = 1.0;
            a[(d + i, i)] = 1.0;
        }
        // C* = diag(1/tau)
        let mut c = DMatrix::<f64>::zeros(d, 2 * d);
        for i in 0..d {
            c[(i, i)] = 1.0 / tau_epoch;
        }
        let q = DMatrix::<f64>::identity(2 * d, 2 * d) * 1e-6;
        let r = DMatrix::<f64>::identity(d, d) * 1e-6;
        // Predict
        self.x = &a * &self.x;
        self.p = &a * &self.p * a.transpose() + &q;
        // Risk-sensitive lambda
        let chi = ChiSquared::new(d as f64).unwrap().inverse_cdf(0.99);
        let v_inv = r.clone().try_inverse().unwrap_or(DMatrix::identity(d, d));
        let lambda = risk_lambda.min((self.p.trace() * v_inv.trace() / chi) as f64);
        // Update
        let s = &c * &self.p * c.transpose() + &r + DMatrix::identity(d, d) * lambda;
        let k = &self.p * c.transpose() * s.try_inverse().unwrap_or(DMatrix::zeros(d, d));
        let z = DVector::from_row_slice(meas);
        let y = &z - &c * &self.x;
        self.x += &k * y;
        self.p = (&DMatrix::<f64>::identity(2 * d, 2 * d) - &k * &c) * &self.p;
    }

    pub fn theta(&self) -> [f64; 4] {
        [self.x[0], self.x[1], self.x[2], self.x[3]]
    }
}
