use criterion::{criterion_group, criterion_main, Criterion};

#[cfg(feature = "quantum")]
fn verify_bench(c: &mut Criterion) {
    let (pk, sk) = crypto::dilithium::keypair();
    let msg = b"bench";
    let sig = crypto::dilithium::sign(&sk, msg);
    c.bench_function("dilithium_verify", |b| {
        b.iter(|| assert!(crypto::dilithium::verify(&pk, msg, &sig)))
    });
}

#[cfg(feature = "quantum")]
criterion_group!(benches, verify_bench);
#[cfg(feature = "quantum")]
criterion_main!(benches);

#[cfg(not(feature = "quantum"))]
criterion_group!(benches,);
#[cfg(not(feature = "quantum"))]
criterion_main!(benches);
