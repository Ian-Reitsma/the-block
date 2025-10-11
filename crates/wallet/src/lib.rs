use crypto_suite::key_derivation::inhouse as kdf_inhouse;
use crypto_suite::signatures::ed25519::{Signature, SigningKey, VerifyingKey};
use crypto_suite::ConstantTimeEq;
use rand::rngs::OsRng;
use thiserror::Error;

/// Common signer interface for software and hardware wallets.
pub trait WalletSigner {
    /// Return the primary public key for this signer.
    fn public_key(&self) -> VerifyingKey;
    /// Produce a single signature over `msg`.
    fn sign(&self, msg: &[u8]) -> Result<Signature, WalletError>;

    /// Return all participating public keys when operating in multisig mode.
    fn public_keys(&self) -> Vec<VerifyingKey> {
        vec![self.public_key()]
    }

    /// Produce signatures from all required parties. Default implementation
    /// falls back to a single signer and returns the caller's public key
    /// alongside its signature so downstream consumers can forward the
    /// approving set.
    fn sign_multisig(&self, msg: &[u8]) -> Result<Vec<(VerifyingKey, Signature)>, WalletError> {
        self.sign(msg).map(|s| vec![(self.public_key(), s)])
    }
}

#[derive(Debug, Error)]
pub enum WalletError {
    #[error("device not connected")]
    NotConnected,
    #[error("remote signer timed out")]
    Timeout,
    #[error("signing failed: {0}")]
    Failure(String),
}

/// Derive a 32-byte key using the first-party HKDF backend.
pub fn derive_key(master: &[u8], info: &[u8]) -> [u8; 32] {
    kdf_inhouse::derive_key_with_info(info, master)
}

/// Perform a constant-time equality check to avoid timing leaks.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// A software wallet holding an Ed25519 keypair.
pub struct Wallet {
    signing_key: SigningKey,
}

pub mod hd;
pub mod psbt;
pub mod remote_signer;
pub mod stake;

#[cfg(feature = "dilithium")]
pub mod pq {
    use pqcrypto_dilithium::dilithium2::{self, DetachedSignature, PublicKey, SecretKey};

    /// Generate a Dilithium keypair.
    pub fn generate() -> (PublicKey, SecretKey) {
        dilithium2::keypair()
    }

    /// Sign a message using Dilithium2.
    pub fn sign(sk: &SecretKey, msg: &[u8]) -> DetachedSignature {
        dilithium2::detached_sign(msg, sk)
    }

    /// Verify a Dilithium2 signature.
    pub fn verify(pk: &PublicKey, msg: &[u8], sig: &DetachedSignature) -> bool {
        dilithium2::verify_detached_signature(sig, msg, pk).is_ok()
    }
}

impl Wallet {
    /// Create a wallet from a 32-byte seed.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(seed);
        Self { signing_key }
    }

    /// Generate a new wallet with OS randomness.
    pub fn generate() -> Self {
        let mut rng = OsRng::default();
        let signing_key = SigningKey::generate(&mut rng);
        Self { signing_key }
    }

    /// Sign a staking message for a given role and amount.
    /// The message format is `{action}:{role}:{amount}` where `action` is
    /// `bond` or `unbond`. Returns the signature on success.
    pub fn sign_stake(
        &self,
        role: &str,
        amount: u64,
        withdraw: bool,
    ) -> Result<Signature, WalletError> {
        let action = if withdraw { "unbond" } else { "bond" };
        let msg = format!("{action}:{role}:{amount}");
        self.sign(msg.as_bytes())
    }

    /// Return the public key encoded as lowercase hex.
    pub fn public_key_hex(&self) -> String {
        crypto_suite::hex::encode(self.public_key().to_bytes())
    }
}

impl WalletSigner for Wallet {
    fn public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    fn sign(&self, msg: &[u8]) -> Result<Signature, WalletError> {
        Ok(self.signing_key.sign(msg))
    }
}

pub mod hardware {
    use super::{Signature, SigningKey, VerifyingKey, WalletError, WalletSigner};
    use rand::rngs::OsRng;

    /// Mock hardware wallet implementing the signer interface.
    pub struct MockHardwareWallet {
        signing_key: SigningKey,
        connected: bool,
    }

    impl Default for MockHardwareWallet {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockHardwareWallet {
        pub fn new() -> Self {
            let mut rng = OsRng::default();
            let signing_key = SigningKey::generate(&mut rng);
            Self {
                signing_key,
                connected: false,
            }
        }
        pub fn connect(&mut self) {
            self.connected = true;
        }
        pub fn disconnect(&mut self) {
            self.connected = false;
        }
    }

    impl WalletSigner for MockHardwareWallet {
        fn public_key(&self) -> VerifyingKey {
            self.signing_key.verifying_key()
        }

        fn sign(&self, msg: &[u8]) -> Result<Signature, WalletError> {
            if !self.connected {
                return Err(WalletError::NotConnected);
            }
            Ok(self.signing_key.sign(msg))
        }
    }

    #[cfg(feature = "hid")]
    pub struct LedgerHid;
    #[cfg(feature = "hid")]
    impl LedgerHid {
        pub fn connect() -> Result<Self, WalletError> {
            Err(WalletError::Failure("ledger hid not implemented".into()))
        }
    }

    #[cfg(feature = "webusb")]
    pub struct TrezorWebUsb;
    #[cfg(feature = "webusb")]
    impl TrezorWebUsb {
        pub fn connect() -> Result<Self, WalletError> {
            Err(WalletError::Failure("trezor webusb not implemented".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_derivation() {
        let seed = [7u8; 32];
        let wallet1 = Wallet::from_seed(&seed);
        let wallet2 = Wallet::from_seed(&seed);
        assert_eq!(wallet1.public_key(), wallet2.public_key());
    }

    #[test]
    fn deterministic_signing() {
        let seed = [42u8; 32];
        let wallet = Wallet::from_seed(&seed);
        let msg = b"test message";
        let sig1 = wallet.sign(msg).unwrap();
        let sig2 = wallet.sign(msg).unwrap();
        assert_eq!(sig1.to_bytes(), sig2.to_bytes());
    }

    #[test]
    fn mock_hardware_signing() {
        use crate::hardware::MockHardwareWallet;
        let mut hw = MockHardwareWallet::new();
        let msg = b"hello";
        assert!(hw.sign(msg).is_err());
        hw.connect();
        let sig1 = hw.sign(msg).unwrap();
        let sig2 = hw.sign(msg).unwrap();
        assert_eq!(sig1.to_bytes(), sig2.to_bytes());
        hw.disconnect();
        assert!(hw.sign(msg).is_err());
    }

    #[test]
    fn hkdf_derivation() {
        let master = b"master";
        let info = b"ctx";
        let k1 = derive_key(master, info);
        let k2 = derive_key(master, info);
        assert_eq!(k1, k2);
        assert!(constant_time_eq(&k1, &k2));
        assert_eq!(
            crypto_suite::hex::encode(k1),
            "2c853709dfc2ed183862bea523a45bfb03d62ab1e63708e32218a4b69997f2c8"
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

    #[cfg(feature = "dilithium")]
    #[test]
    fn dilithium_round_trip() {
        use crate::pq;
        let (pk, sk) = pq::generate();
        let msg = b"pq";
        let sig = pq::sign(&sk, msg);
        assert!(pq::verify(&pk, msg, &sig));
    }
}
