use bincode;
use blake3;
use crypto_suite::signatures::{
    ed25519::{Signature, VerifyingKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH},
    Verifier,
};
use hex;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;

pub mod proximity;
pub use proximity::{validate_proximity, DeviceClass};

#[derive(Serialize, Deserialize, Debug)]
pub struct AssistReceipt {
    pub provider: String,
    pub region: String,
    pub pubkey: Vec<u8>,
    pub sig: Vec<u8>,
    pub device: DeviceClass,
    pub rssi: i8,
    pub rtt_ms: u32,
}

impl AssistReceipt {
    pub fn verify(&self) -> bool {
        let pk: [u8; PUBLIC_KEY_LENGTH] = match self.pubkey.as_slice().try_into() {
            Ok(v) => v,
            Err(_) => return false,
        };
        let sig_bytes: [u8; SIGNATURE_LENGTH] = match self.sig.as_slice().try_into() {
            Ok(v) => v,
            Err(_) => return false,
        };
        if let Ok(vk) = VerifyingKey::from_bytes(&pk) {
            let sig = Signature::from_bytes(&sig_bytes);
            let mut msg = Vec::new();
            msg.extend(self.provider.as_bytes());
            msg.extend(self.region.as_bytes());
            msg.push(self.device as u8);
            msg.push(self.rssi as u8);
            msg.extend_from_slice(&self.rtt_ms.to_le_bytes());
            return vk.verify(&msg, &sig).is_ok();
        }
        false
    }

    pub fn hash(&self) -> String {
        let bytes = bincode::serialize(self).unwrap_or_default();
        hex::encode(blake3::hash(&bytes).as_bytes())
    }
}
