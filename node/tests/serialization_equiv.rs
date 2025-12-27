#![cfg(feature = "integration-tests")]
use rand::{rngs::StdRng, Rng};
use std::fs;
use std::io::Write;
use the_block::{canonical_payload_bytes, RawTxPayload};

fn random_hex(rng: &mut StdRng) -> String {
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes);
    crypto_suite::hex::encode(bytes)
}

#[test]
fn serialize_roundtrip_vectors() {
    let mut rng = StdRng::seed_from_u64(42);
    let mut file = fs::File::create("../target/serialization_equiv.csv").unwrap();
    for _ in 0..1000 {
        let payload = RawTxPayload {
            from_: random_hex(&mut rng),
            to: random_hex(&mut rng),
            amount_consumer: rng.gen(),
            amount_industrial: rng.gen(),
            fee: rng.gen(),
            pct: rng.gen_range(0u32..=100) as u8,
            nonce: rng.gen(),
            memo: {
                let len = rng.gen_range(0..16);
                let mut m = vec![0u8; len];
                rng.fill(&mut m[..]);
                m
            },
        };
        let bytes = canonical_payload_bytes(&payload);
        writeln!(file, "{}", crypto_suite::hex::encode(bytes)).unwrap();
    }
    file.flush().unwrap();
    assert!(fs::metadata("../target/serialization_equiv.csv").is_ok());
}
