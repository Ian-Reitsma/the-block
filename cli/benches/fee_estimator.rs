use contract_cli::fee_estimator::RollingMedianEstimator as NewEstimator;
use criterion::{criterion_group, criterion_main, Criterion};
use std::collections::VecDeque;

struct OldEstimator {
    window: VecDeque<u64>,
    max: usize,
}

impl OldEstimator {
    fn new(max: usize) -> Self {
        Self {
            window: VecDeque::with_capacity(max),
            max,
        }
    }
    fn record(&mut self, fee: u64) {
        if self.window.len() == self.max {
            self.window.pop_front();
        }
        self.window.push_back(fee);
    }
    fn suggest(&self) -> u64 {
        let mut v: Vec<u64> = self.window.iter().copied().collect();
        if v.is_empty() {
            return 0;
        }
        v.sort_unstable();
        v[v.len() / 2]
    }
}

fn bench_estimators(c: &mut Criterion) {
    let samples: Vec<u64> = (0..1000).map(|i| i as u64).collect();
    c.bench_function("fee_estimator_old", |b| {
        b.iter(|| {
            let mut est = OldEstimator::new(64);
            for &s in &samples {
                est.record(s);
                let _ = est.suggest();
            }
        })
    });
    c.bench_function("fee_estimator_new", |b| {
        b.iter(|| {
            let mut est = NewEstimator::new(64);
            for &s in &samples {
                est.record(s);
                let _ = est.suggest();
            }
        })
    });
}

criterion_group!(benches, bench_estimators);
criterion_main!(benches);
