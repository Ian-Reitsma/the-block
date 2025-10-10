use crypto_suite::hashing::{blake3, sha3::Sha3_256};
use crypto_suite::key_derivation::inhouse as kdf_inhouse;
use crypto_suite::signatures::ed25519::{Signature, SignatureError, SigningKey, VerifyingKey};
use crypto_suite::signatures::internal::Sha512;

const L_BYTES: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
];

const IDENTITY_COMPRESSED: [u8; 32] = {
    let mut bytes = [0u8; 32];
    bytes[0] = 1;
    bytes
};

fn vector_seed(hex: &str) -> [u8; 32] {
    let bytes = crypto_suite::hex::decode(hex).expect("hex");
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    arr
}

fn vector_signature(hex: &str) -> Signature {
    let bytes = crypto_suite::hex::decode(hex).expect("hex");
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&bytes);
    Signature::from_bytes(&arr)
}

#[test]
fn rfc8032_vector1() {
    let seed = vector_seed("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
    let expected_public = crypto_suite::hex::decode(
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
    )
    .unwrap();
    let expected_signature = vector_signature("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b");

    let signing_key = SigningKey::from_bytes(&seed);
    assert_eq!(
        signing_key.verifying_key().to_bytes().to_vec(),
        expected_public
    );

    let signature = signing_key.sign(b"");
    assert_eq!(signature.to_bytes(), expected_signature.to_bytes());
    signing_key
        .verifying_key()
        .verify(b"", &signature)
        .expect("valid signature");
}

#[test]
fn rfc8032_vector2() {
    let seed = vector_seed("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb");
    let expected_public = crypto_suite::hex::decode(
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
    )
    .unwrap();
    let message = crypto_suite::hex::decode("72").unwrap();
    let expected_signature = vector_signature("92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00");

    let signing_key = SigningKey::from_bytes(&seed);
    assert_eq!(
        signing_key.verifying_key().to_bytes().to_vec(),
        expected_public
    );

    let signature = signing_key.sign(&message);
    assert_eq!(signature.to_bytes(), expected_signature.to_bytes());
    signing_key
        .verifying_key()
        .verify(&message, &signature)
        .expect("valid signature");
}

#[test]
fn signature_rejects_modified_message() {
    let seed = vector_seed("c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7");
    let message = b"Ed25519 takes on all corners";

    let signing_key = SigningKey::from_bytes(&seed);
    let signature = signing_key.sign(message);

    signing_key
        .verifying_key()
        .verify(message, &signature)
        .expect("valid signature");
    signing_key
        .verifying_key()
        .verify(b"Ed25519 takes on some corners", &signature)
        .expect_err("modified message must fail");
}

#[test]
fn signature_rejects_noncanonical_scalar() {
    let seed = vector_seed("f5e5767cf153319517630f226876b86c8160cc583bc013744c6bf255f5cc0ee5");
    let message = b"noncanonical";
    let signing_key = SigningKey::from_bytes(&seed);
    let mut tampered = signing_key.sign(message).to_bytes();
    tampered[32..].copy_from_slice(&L_BYTES);
    let signature = Signature::from_bytes(&tampered);
    let verifying_key = signing_key.verifying_key();
    let err = verifying_key
        .verify_strict(message, &signature)
        .expect_err("non-canonical scalar must fail");
    assert!(matches!(err, SignatureError::InvalidSignature));
}

#[test]
fn signature_rejects_small_order_r() {
    let seed = vector_seed("833fe62409237b9d62ec77587520911e9a759cec1d19755b7da901b96dca3d42");
    let message = b"small-order R";
    let signing_key = SigningKey::from_bytes(&seed);
    let mut tampered = signing_key.sign(message).to_bytes();
    tampered[..32].copy_from_slice(&IDENTITY_COMPRESSED);
    let signature = Signature::from_bytes(&tampered);
    let verifying_key = signing_key.verifying_key();
    let err = verifying_key
        .verify(message, &signature)
        .expect_err("small-order R must be rejected");
    assert!(matches!(err, SignatureError::InvalidSignature));
}

#[test]
fn verifying_key_rejects_small_order_point() {
    assert!(matches!(
        VerifyingKey::from_bytes(&IDENTITY_COMPRESSED),
        Err(SignatureError::InvalidKey)
    ));
}

#[test]
fn verifying_key_rejects_invalid_encoding() {
    let invalid = [0xFFu8; 32];
    assert!(matches!(
        VerifyingKey::from_bytes(&invalid),
        Err(SignatureError::InvalidKey)
    ));
}

#[test]
fn pkcs8_roundtrip_matches() {
    let seed = vector_seed("0305334e381af78f141cb666f6199f574f8f930293f1efc7d9ae6d1e32d8a3b7");
    let signing_key = SigningKey::from_bytes(&seed);
    let der = signing_key.to_pkcs8_der().expect("pkcs8");
    let restored = SigningKey::from_pkcs8_der(der.as_bytes()).expect("decode");
    assert_eq!(
        restored.verifying_key().to_bytes(),
        signing_key.verifying_key().to_bytes()
    );
}

#[test]
fn sha512_rfc6234_empty() {
    let digest = Sha512::digest(b"");
    assert_eq!(
        crypto_suite::hex::encode(digest),
        "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
    );
}

#[test]
fn sha512_rfc6234_abc() {
    let digest = Sha512::digest(b"abc");
    assert_eq!(
        crypto_suite::hex::encode(digest),
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );
}

#[test]
fn hkdf_matches_rfc5869_case1() {
    let ikm = [0x0bu8; 22];
    let salt = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
    ];
    let info = [0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9];
    let mut okm = [0u8; 42];
    kdf_inhouse::derive_key_material(Some(&salt), &info, &ikm, &mut okm);
    assert_eq!(
        crypto_suite::hex::encode(okm),
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
    );
}

#[test]
fn blake3_vectors_match_spec() {
    let mut input = vec![0u8; 0];
    let cases = [
        (
            0usize,
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
        ),
        (
            1,
            "2d3adedff11b61f14c886e35afa036736dcd87a74d27b5c1510225d0f592e213",
        ),
        (
            64,
            "4eed7141ea4a5cd4b788606bd23f46e212af9cacebacdc7d1f4c6dc7f2511b98",
        ),
        (
            1024,
            "42214739f095a406f3fc83deb889744ac00df831c10daa55189b5d121c855af7",
        ),
    ];
    for (len, expected) in cases {
        input.resize(len, 0);
        for i in 0..len {
            input[i] = (i % 251) as u8;
        }
        let digest = blake3::hash(&input);
        assert_eq!(crypto_suite::hex::encode(digest.as_bytes()), expected);
    }

    let key_bytes: [u8; blake3::KEY_LEN] = *b"whats the Elvish word for friend";
    let keyed = blake3::keyed_hash(&key_bytes, b"");
    assert_eq!(
        crypto_suite::hex::encode(keyed.as_bytes()),
        "92b2b75604ed3c761f9d6f62392c8a9227ad0ea3f09573e783f1498a4ed60d26"
    );

    let derived = blake3::derive_key("BLAKE3 2019-12-27 16:29:52 test vectors context", b"");
    assert_eq!(
        crypto_suite::hex::encode(derived),
        "2cc39783c223154fea8dfb7c1b1660f2ac2dcbd1c1de8277b0b0dd39b7e50d7d"
    );
}

#[test]
fn sha3_vectors_match_spec() {
    let cases: &[(&[u8], &str)] = &[
        (
            &b""[..],
            "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a",
        ),
        (
            &b"abc"[..],
            "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532",
        ),
        (
            &b"The quick brown fox jumps over the lazy dog"[..],
            "69070dda01975c8c120c3aada1b282394e7f032fa9cf32f4cb2259a0897dfc04",
        ),
    ];
    for &(message, expected) in cases {
        let mut hasher = Sha3_256::new();
        hasher.update(message);
        let digest = hasher.finalize();
        assert_eq!(crypto_suite::hex::encode(digest.as_slice()), expected);
    }
}
