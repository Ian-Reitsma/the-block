use foundation_math::distribution::ChiSquared;
use foundation_math::linalg::{Matrix, Vector};

/// Augmented risk-sensitive Kalman filter retaining previous utilisation.
pub struct KalmanLqg {
    pub x: Vector<8>,    // [theta(4), u_prev(4)]
    pub p: Matrix<8, 8>, // 8x8 covariance
}

impl KalmanLqg {
    /// Perform one prediction/update step.
    pub fn step(&mut self, meas: &[f64; 4], tau_epoch: f64, risk_lambda: f64) {
        let d = 4;
        // A* = [[I,0],[I,0]]
        let mut a = Matrix::<8, 8>::zeros();
        for i in 0..d {
            a[(i, i)] = 1.0;
            a[(d + i, i)] = 1.0;
        }
        // C* = diag(1/tau)
        let mut c = Matrix::<4, 8>::zeros();
        for i in 0..d {
            c[(i, i)] = 1.0 / tau_epoch;
        }
        let q = Matrix::<8, 8>::identity().scale(1e-6);
        let r = Matrix::<4, 4>::identity().scale(1e-6);
        // Predict
        self.x = a.mul_vector(&self.x);
        let ap = a.mul_matrix(&self.p);
        self.p = ap.mul_matrix(&a.transpose()).add(&q);
        // Risk-sensitive lambda
        let chi = ChiSquared::new(d as f64)
            .expect("positive degrees of freedom")
            .inverse_cdf(0.99);
        let v_inv = r
            .try_inverse()
            .unwrap_or_else(|| Matrix::<4, 4>::identity());
        let lambda = risk_lambda.min(self.p.trace() * v_inv.trace() / chi);
        // Update
        let s = c
            .mul_matrix(&self.p)
            .mul_matrix(&c.transpose())
            .add(&r)
            .add(&Matrix::<4, 4>::identity().scale(lambda));
        let s_inv = s.try_inverse().unwrap_or_else(|| Matrix::<4, 4>::zeros());
        let k = self.p.clone().mul_matrix(&c.transpose()).mul_matrix(&s_inv);
        let z = Vector::<4>::from_array(*meas);
        let y = z.sub(&c.mul_vector(&self.x));
        self.x.add_assign(&k.mul_vector(&y));
        let identity = Matrix::<8, 8>::identity();
        self.p = identity.sub(&k.mul_matrix(&c)).mul_matrix(&self.p);
    }

    pub fn theta(&self) -> [f64; 4] {
        [self.x[0], self.x[1], self.x[2], self.x[3]]
    }
}
