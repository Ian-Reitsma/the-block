use criterion::{criterion_group, criterion_main, Criterion};
use the_block::parallel::{ParallelExecutor, Task};

fn workload() {
    let mut x = 0u64;
    for _ in 0..10_000 {
        x += 1;
    }
    std::hint::black_box(x);
}

fn bench_parallel(c: &mut Criterion) {
    c.bench_function("parallel", |b| {
        b.iter(|| {
            let tasks: Vec<Task<()>> = (0..8)
                .map(|i| Task::new(vec![format!("r{i}")], vec![format!("w{i}")], workload))
                .collect();
            ParallelExecutor::execute(tasks);
        });
    });

    c.bench_function("sequential", |b| {
        b.iter(|| {
            // All tasks share the same write key forcing serialization.
            let tasks: Vec<Task<()>> = (0..8)
                .map(|_| Task::new(vec!["r".into()], vec!["w".into()], workload))
                .collect();
            ParallelExecutor::execute(tasks);
        });
    });
}

criterion_group!(benches, bench_parallel);
criterion_main!(benches);
