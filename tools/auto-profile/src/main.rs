use criterion::{black_box, Criterion};
use crypto_suite::hashing::blake3;
use pprof::ProfilerGuard;
use std::fs::File;

fn main() {
    let guard = ProfilerGuard::new(100).ok();
    let mut c = Criterion::default();
    c.bench_function("hash_blake3", |b| {
        b.iter(|| {
            let h = blake3::hash(black_box(b"the-block"));
            black_box(h);
        });
    });
    if let Some(g) = guard {
        if let Ok(report) = g.report().build() {
            if let Ok(file) = File::create("auto_profile.svg") {
                let _ = report.flamegraph(file);
            }
        }
    }
}
