use ed25519_dalek::{Keypair, PublicKey, Signature, Signer};
use hkdf::Hkdf;
use rand::rngs::OsRng;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;

/// Common signer interface for software and hardware wallets.
pub trait WalletSigner {
    /// Return the primary public key for this signer.
    fn public_key(&self) -> PublicKey;
    /// Produce a single signature over `msg`.
    fn sign(&self, msg: &[u8]) -> Result<Signature, WalletError>;

    /// Return all participating public keys when operating in multisig mode.
    fn public_keys(&self) -> Vec<PublicKey> {
        vec![self.public_key()]
    }

    /// Produce signatures from all required parties. Default implementation
    /// falls back to a single signer.
    fn sign_multisig(&self, msg: &[u8]) -> Result<Vec<Signature>, WalletError> {
        self.sign(msg).map(|s| vec![s])
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

/// Derive a 32-byte key using HKDF-SHA256.
pub fn derive_key(master: &[u8], info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(None, master);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm).expect("hkdf expand");
    okm
}

/// Perform a constant-time equality check to avoid timing leaks.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// A software wallet holding an Ed25519 keypair.
pub struct Wallet {
    keypair: Keypair,
}

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
        let secret = ed25519_dalek::SecretKey::from_bytes(seed).expect("seed length");
        let public = PublicKey::from(&secret);
        Self {
            keypair: Keypair { secret, public },
        }
    }

    /// Generate a new wallet with OS randomness.
    pub fn generate() -> Self {
        let mut rng = OsRng;
        let keypair = Keypair::generate(&mut rng);
        Self { keypair }
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
        hex::encode(self.public_key().to_bytes())
    }
}

impl WalletSigner for Wallet {
    fn public_key(&self) -> PublicKey {
        self.keypair.public
    }

    fn sign(&self, msg: &[u8]) -> Result<Signature, WalletError> {
        Ok(self.keypair.sign(msg))
    }
}

pub mod hardware {
    use super::{Keypair, PublicKey, Signature, Signer, WalletError, WalletSigner};
    use rand::rngs::OsRng;

    /// Mock hardware wallet implementing the signer interface.
    pub struct MockHardwareWallet {
        keypair: Keypair,
        connected: bool,
    }

    impl Default for MockHardwareWallet {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockHardwareWallet {
        pub fn new() -> Self {
            let mut rng = OsRng;
            let keypair = Keypair::generate(&mut rng);
            Self {
                keypair,
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
        fn public_key(&self) -> PublicKey {
            self.keypair.public
        }

        fn sign(&self, msg: &[u8]) -> Result<Signature, WalletError> {
            if !self.connected {
                return Err(WalletError::NotConnected);
            }
            Ok(self.keypair.sign(msg))
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
