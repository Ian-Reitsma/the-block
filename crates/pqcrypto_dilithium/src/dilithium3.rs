use crate::common::{
    self, DetachedSignature as DetachedSignatureImpl, Error, PublicKey as PublicKeyImpl,
    SecretKey as SecretKeyImpl,
};

pub const PUBLIC_KEY_BYTES: usize = 1952;
pub const SECRET_KEY_BYTES: usize = 4000;
pub const SIGNATURE_BYTES: usize = 3293;

pub type PublicKey = PublicKeyImpl<PUBLIC_KEY_BYTES>;
pub type SecretKey = SecretKeyImpl<SECRET_KEY_BYTES, PUBLIC_KEY_BYTES>;
pub type DetachedSignature = DetachedSignatureImpl<SIGNATURE_BYTES>;

pub fn keypair() -> (PublicKey, SecretKey) {
    common::keypair::<PUBLIC_KEY_BYTES, SECRET_KEY_BYTES>()
}

pub fn detached_sign(message: &[u8], secret: &SecretKey) -> DetachedSignature {
    common::detached_sign::<SIGNATURE_BYTES, SECRET_KEY_BYTES, PUBLIC_KEY_BYTES>(message, secret)
}

pub fn verify_detached_signature(
    signature: &DetachedSignature,
    message: &[u8],
    public: &PublicKey,
) -> Result<(), Error> {
    common::verify_detached_signature::<SIGNATURE_BYTES, PUBLIC_KEY_BYTES>(
        signature, message, public,
    )
}
