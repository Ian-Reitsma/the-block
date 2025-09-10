use criterion::{criterion_group, criterion_main, Criterion};
use the_block::telemetry;

fn bench_telemetry_memory(c: &mut Criterion) {
    #[cfg(feature = "telemetry")]
    c.bench_function("telemetry_memory", |b| {
        b.iter(|| {
            telemetry::set_sample_rate(1.0);
            for _ in 0..1000 {
                telemetry::sampled_inc(&telemetry::TTL_DROP_TOTAL);
            }
            telemetry::force_compact();
            telemetry::current_alloc_bytes()
        })
    });
}

criterion_group!(benches, bench_telemetry_memory);
criterion_main!(benches);
