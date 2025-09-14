#![cfg(feature = "quantum")]
use criterion::{criterion_group, criterion_main, Criterion};

fn verify_bench(c: &mut Criterion) {
    let (pk, sk) = crypto::dilithium::keypair();
    let msg = b"bench";
    let sig = crypto::dilithium::sign(&sk, msg);
    c.bench_function("dilithium_verify", |b| {
        b.iter(|| crypto::dilithium::verify(&pk, msg, &sig))
    });
}

criterion_group!(benches, verify_bench);
criterion_main!(benches);
