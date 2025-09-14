use crypto::{DILITHIUM_DOMAIN_TAG, ED25519_DOMAIN_TAG};

#[cfg(feature = "quantum")]
#[test]
fn domain_tags_unique() {
    assert_ne!(ED25519_DOMAIN_TAG, DILITHIUM_DOMAIN_TAG);
}
