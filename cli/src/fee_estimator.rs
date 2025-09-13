use std::collections::VecDeque;

/// Rolling median fee estimator over a fixed window.
pub struct RollingMedianEstimator {
    window: VecDeque<u64>,
    max: usize,
}

impl RollingMedianEstimator {
    pub fn new(max: usize) -> Self {
        Self {
            window: VecDeque::with_capacity(max),
            max,
        }
    }

    /// Record an observed tip fee.
    pub fn record(&mut self, fee: u64) {
        if self.window.len() == self.max {
            self.window.pop_front();
        }
        self.window.push_back(fee);
    }

    /// Suggest a fee based on the median of the window.
    pub fn suggest(&self) -> u64 {
        let mut v: Vec<u64> = self.window.iter().copied().collect();
        if v.is_empty() {
            return 0;
        }
        v.sort_unstable();
        v[v.len() / 2]
    }
}
