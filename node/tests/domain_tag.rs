use ed25519_dalek::{Signer, SigningKey, Verifier};
use the_block::{domain_tag, domain_tag_for};

#[test]
fn domain_tag_mismatch_fails() {
    let sk = SigningKey::from_bytes(&[1u8; 32]);
    let payload = b"msg";
    let mut msg = domain_tag_for(2).to_vec();
    msg.extend_from_slice(payload);
    let sig = sk.sign(&msg);
    let mut wrong = domain_tag().to_vec();
    wrong.extend_from_slice(payload);
    let vk = sk.verifying_key();
    assert!(vk.verify(&wrong, &sig).is_err());
}
