use criterion::{criterion_group, criterion_main, Criterion};
use the_block::net::{clear_peer_metrics, record_request, set_peer_metrics_sample_rate};

fn bench_peer_metrics_memory(c: &mut Criterion) {
    c.bench_function("peer_metrics_memory", |b| {
        b.iter(|| {
            clear_peer_metrics();
            set_peer_metrics_sample_rate(1);
            for i in 0..1000u8 {
                let mut pk = [0u8; 32];
                pk[0] = i;
                record_request(&pk);
            }
            clear_peer_metrics();
        })
    });
}

criterion_group!(benches, bench_peer_metrics_memory);
criterion_main!(benches);
