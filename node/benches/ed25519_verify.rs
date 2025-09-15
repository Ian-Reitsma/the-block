use criterion::{criterion_group, criterion_main, Criterion};
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use the_block::transaction::{sign_tx, verify_signed_tx, RawTxPayload};

fn verify_bench(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(42);
    let mut sk_bytes = [0u8; 32];
    rng.fill_bytes(&mut sk_bytes);
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee: 0,
        pct_ct: 100,
        nonce: 0,
        memo: Vec::new(),
    };
    let tx = sign_tx(&sk_bytes, &payload).expect("sign");
    c.bench_function("verify_tx_ed25519", |b| {
        b.iter(|| assert!(verify_signed_tx(&tx)))
    });
}

criterion_group!(benches, verify_bench);
criterion_main!(benches);
