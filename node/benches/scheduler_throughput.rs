use criterion::{criterion_group, criterion_main, Criterion};
use the_block::compute_market::scheduler::{self, Capability, Priority};

fn bench_enqueue_start(c: &mut Criterion) {
    c.bench_function("enqueue_start", |b| {
        b.iter(|| {
            scheduler::reset_for_test();
            scheduler::start_job_with_priority(
                "bench",
                "prov",
                Capability::default(),
                Priority::High,
            );
            scheduler::end_job("bench");
        })
    });
}

criterion_group!(benches, bench_enqueue_start);
criterion_main!(benches);
