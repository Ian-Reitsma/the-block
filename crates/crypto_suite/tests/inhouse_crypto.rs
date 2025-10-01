use crypto_suite::signatures::ed25519::{Signature, SignatureError, SigningKey, VerifyingKey};

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
    let bytes = hex::decode(hex).expect("hex");
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    arr
}

fn vector_signature(hex: &str) -> Signature {
    let bytes = hex::decode(hex).expect("hex");
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&bytes);
    Signature::from_bytes(&arr)
}

#[test]
fn rfc8032_vector1() {
    let seed = vector_seed("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
    let expected_public =
        hex::decode("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a").unwrap();
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
    let expected_public =
        hex::decode("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c").unwrap();
    let message = hex::decode("72").unwrap();
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
