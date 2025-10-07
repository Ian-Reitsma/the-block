#![cfg(feature = "allow-third-party")]

use coding::{
    decrypt_xchacha20_poly1305, default_compressor, default_encryptor, encrypt_xchacha20_poly1305,
    ErasureCoder, FountainCoder, CHACHA20_POLY1305_KEY_LEN, XCHACHA20_POLY1305_NONCE_LEN,
};
use rand::seq::SliceRandom;
use rand::{rngs::StdRng, Rng, SeedableRng};

#[test]
fn encrypt_round_trip_random_inputs() {
    let mut rng = StdRng::seed_from_u64(1337);
    for size in [0usize, 1, 7, 64, 255, 1024, 4096] {
        let mut key = [0u8; CHACHA20_POLY1305_KEY_LEN];
        rng.fill(&mut key);
        let encryptor = default_encryptor(&key).expect("encryptor");
        let mut payload = vec![0u8; size];
        rng.fill(payload.as_mut_slice());
        let ciphertext = encryptor.encrypt(&payload).expect("encrypt");
        let plaintext = encryptor.decrypt(&ciphertext).expect("decrypt");
        assert_eq!(plaintext, payload);
    }
}

#[test]
fn encrypt_detects_single_bit_flips() {
    let mut rng = StdRng::seed_from_u64(9001);
    let mut key = [0u8; CHACHA20_POLY1305_KEY_LEN];
    rng.fill(&mut key);
    let encryptor = default_encryptor(&key).expect("encryptor");
    let mut payload = vec![0u8; 512];
    rng.fill(payload.as_mut_slice());
    let ciphertext = encryptor.encrypt(&payload).expect("encrypt");
    for idx in CHACHA20_POLY1305_KEY_LEN..ciphertext.len() {
        let mut tampered = ciphertext.clone();
        tampered[idx] ^= 0x80;
        assert!(encryptor.decrypt(&tampered).is_err());
    }
}

#[test]
fn xchacha_round_trip_random_inputs() {
    let mut rng = StdRng::seed_from_u64(424242);
    for size in [0usize, 7, 128, 4096] {
        let mut key = [0u8; CHACHA20_POLY1305_KEY_LEN];
        rng.fill(&mut key);
        let mut payload = vec![0u8; size];
        rng.fill(payload.as_mut_slice());
        let ciphertext = encrypt_xchacha20_poly1305(&key, &payload).expect("encrypt");
        let plaintext = decrypt_xchacha20_poly1305(&key, &ciphertext).expect("decrypt");
        assert_eq!(plaintext, payload);
    }
}

#[test]
fn xchacha_detects_tampering() {
    let mut rng = StdRng::seed_from_u64(0xfeed5eed);
    let mut key = [0u8; CHACHA20_POLY1305_KEY_LEN];
    rng.fill(&mut key);
    let mut payload = vec![0u8; 256];
    rng.fill(payload.as_mut_slice());
    let ciphertext = encrypt_xchacha20_poly1305(&key, &payload).expect("encrypt");
    for idx in XCHACHA20_POLY1305_NONCE_LEN..ciphertext.len() {
        let mut tampered = ciphertext.clone();
        tampered[idx] ^= 0x01;
        assert!(
            decrypt_xchacha20_poly1305(&key, &tampered).is_err(),
            "tampering index {idx} passed"
        );
    }
}

#[test]
fn hybrid_compressor_handles_entropy_spectrum() {
    let compressor = default_compressor();
    let mut rng = StdRng::seed_from_u64(4242);
    for len in [0usize, 32, 128, 1024, 8192] {
        let mut data = vec![0u8; len];
        rng.fill(data.as_mut_slice());
        let encoded = compressor.compress(&data).expect("compress");
        let decoded = compressor.decompress(&encoded).expect("decompress");
        assert_eq!(decoded, data);
    }
}

#[test]
fn hybrid_preserves_low_entropy_data() {
    let compressor = default_compressor();
    let mut data = Vec::new();
    data.extend(std::iter::repeat(0u8).take(8192));
    data.extend((0u8..=255).cycle().take(2048));
    let encoded = compressor.compress(&data).expect("compress");
    let decoded = compressor.decompress(&encoded).expect("decompress");
    assert_eq!(decoded, data);
}

#[test]
fn reed_solomon_recovers_varied_losses() {
    use coding::InhouseReedSolomon;
    let coder = InhouseReedSolomon::new(8, 4).expect("coder");
    let mut rng = StdRng::seed_from_u64(0x5eed5eed);
    let data: Vec<u8> = (0..16_384).map(|idx| (idx % 251) as u8).collect();
    let batch = coder.encode(&data).expect("encode");
    let total = batch.shards.len();
    for missing in [1usize, 2, 4] {
        let mut slots: Vec<Option<coding::ErasureShard>> =
            batch.shards.clone().into_iter().map(Some).collect();
        let mut indices: Vec<usize> = (0..total).collect();
        indices.shuffle(&mut rng);
        for idx in indices.iter().take(missing) {
            slots[*idx] = None;
        }
        let recovered = coder
            .reconstruct(&batch.metadata, &slots)
            .expect("reconstruct");
        assert_eq!(recovered, data, "loss {missing}");
    }
}

#[test]
fn fountain_recovers_after_packet_loss() {
    use coding::InhouseLtFountain;
    let coder = InhouseLtFountain::new(512, 1.5).expect("fountain");
    let mut rng = StdRng::seed_from_u64(0x1234_5678);
    let mut data = vec![0u8; 32 * 1024];
    rng.fill(data.as_mut_slice());
    let batch = coder.encode(&data).expect("encode");
    let (metadata, mut packets) = batch.into_parts();
    packets.shuffle(&mut rng);
    let losses = packets.len() / 5;
    packets.truncate(packets.len() - losses);
    let recovered = coder.decode(&metadata, &packets).expect("decode");
    assert_eq!(recovered, data);
}
