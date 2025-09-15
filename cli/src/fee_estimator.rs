use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

/// Rolling median fee estimator over a fixed window.
pub struct RollingMedianEstimator {
    lower: BinaryHeap<u64>,
    upper: BinaryHeap<Reverse<u64>>,
    window: VecDeque<u64>,
    max: usize,
    garbage_lower: HashMap<u64, usize>,
    garbage_upper: HashMap<u64, usize>,
}

impl RollingMedianEstimator {
    pub fn new(max: usize) -> Self {
        Self {
            lower: BinaryHeap::new(),
            upper: BinaryHeap::new(),
            window: VecDeque::with_capacity(max),
            max,
            garbage_lower: HashMap::new(),
            garbage_upper: HashMap::new(),
        }
    }

    /// Record an observed tip fee.
    pub fn record(&mut self, fee: u64) {
        if self.window.len() == self.max {
            let old = self.window.pop_front().unwrap();
            if self.lower.peek().map_or(false, |&x| old <= x) {
                *self.garbage_lower.entry(old).or_insert(0) += 1;
                self.prune_lower();
            } else {
                *self.garbage_upper.entry(old).or_insert(0) += 1;
                self.prune_upper();
            }
        }
        if self.lower.peek().map_or(true, |&x| fee <= x) {
            self.lower.push(fee);
        } else {
            self.upper.push(Reverse(fee));
        }
        self.balance();
        self.window.push_back(fee);
    }

    fn prune_lower(&mut self) {
        while let Some(&top) = self.lower.peek() {
            if let Some(cnt) = self.garbage_lower.get_mut(&top) {
                if *cnt == 1 {
                    self.garbage_lower.remove(&top);
                } else {
                    *cnt -= 1;
                }
                self.lower.pop();
            } else {
                break;
            }
        }
    }

    fn prune_upper(&mut self) {
        while let Some(&Reverse(top)) = self.upper.peek() {
            if let Some(cnt) = self.garbage_upper.get_mut(&top) {
                if *cnt == 1 {
                    self.garbage_upper.remove(&top);
                } else {
                    *cnt -= 1;
                }
                self.upper.pop();
            } else {
                break;
            }
        }
    }

    fn balance(&mut self) {
        if self.lower.len() > self.upper.len() + 1 {
            if let Some(v) = self.lower.pop() {
                self.upper.push(Reverse(v));
            }
            self.prune_lower();
        } else if self.upper.len() > self.lower.len() {
            if let Some(Reverse(v)) = self.upper.pop() {
                self.lower.push(v);
            }
            self.prune_upper();
        }
    }

    /// Suggest a fee based on the median of the window.
    pub fn suggest(&mut self) -> u64 {
        if self.window.is_empty() {
            return 0;
        }
        self.prune_lower();
        self.prune_upper();
        if self.lower.len() >= self.upper.len() {
            *self.lower.peek().unwrap()
        } else {
            self.upper.peek().unwrap().0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RollingMedianEstimator;

    #[test]
    fn median_updates() {
        let mut est = RollingMedianEstimator::new(3);
        est.record(5);
        est.record(1);
        est.record(9);
        assert_eq!(est.suggest(), 5);
        est.record(10);
        // window now [1,9,10]; median 9
        assert_eq!(est.suggest(), 9);
    }
}
