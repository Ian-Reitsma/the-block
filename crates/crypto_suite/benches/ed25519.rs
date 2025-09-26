use criterion::{black_box, criterion_group, criterion_main, Criterion};
use crypto_suite::signatures::ed25519::SigningKey;
use crypto_suite::transactions::TransactionSigner;
use ed25519_dalek::Signer as DalekSigner;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};

fn bench_signing(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(1337);
    let signing_key = SigningKey::generate(&mut rng);
    let dalek_key = ed25519_dalek::SigningKey::from_bytes(&signing_key.secret_bytes());
    let signer = TransactionSigner::from_chain_id(1);
    let mut payload = vec![0u8; 128];
    rng.fill_bytes(&mut payload);

    c.bench_function("suite::sign", |b| {
        b.iter(|| {
            let sig = signer.sign(&signing_key, black_box(&payload));
            black_box(sig);
        });
    });

    c.bench_function("dalek::sign", |b| {
        b.iter(|| {
            let message = signer.message(&payload);
            let sig = dalek_key.sign(black_box(&message));
            black_box(sig);
        });
    });
}

criterion_group!(benches, bench_signing);
criterion_main!(benches);
