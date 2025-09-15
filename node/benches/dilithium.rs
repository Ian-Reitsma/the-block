use criterion::{criterion_group, criterion_main, Criterion};
use the_block::transaction::{verify_signed_tx, RawTxPayload, SignedTransaction, TxSignature, TxVersion, FeeLane};

#[cfg(feature = "quantum")]
fn verify_bench(c: &mut Criterion) {
    use crypto::dilithium;

    let (pk, sk) = dilithium::keypair();
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
    let mut msg = the_block::constants::domain_tag().to_vec();
    msg.extend(the_block::transaction::canonical_payload_bytes(&payload));
    let sig = dilithium::sign(&sk, &msg);
    let tx = SignedTransaction {
        payload,
        public_key: Vec::new(),
        dilithium_public_key: pk.clone(),
        signature: TxSignature { ed25519: Vec::new(), dilithium: sig },
        tip: 0,
        signer_pubkeys: Vec::new(),
        aggregate_signature: Vec::new(),
        threshold: 0,
        lane: FeeLane::Consumer,
        version: TxVersion::DilithiumOnly,
    };
    c.bench_function("verify_tx_dilithium", |b| {
        b.iter(|| assert!(verify_signed_tx(&tx)))
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
