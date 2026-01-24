#![forbid(unsafe_code)]

use foundation_serialization::binary;
use the_block::{
    energy::{EnergyDispute, EnergySlash},
    receipts::{EnergyReceipt, EnergySlashReceipt},
};

pub fn run(data: &[u8]) {
    if let Ok(receipt) = binary::decode::<EnergyReceipt>(data) {
        let _ = binary::encode(&receipt);
    }
    if let Ok(slash) = binary::decode::<EnergySlashReceipt>(data) {
        let _ = binary::encode(&slash);
    }
    if let Ok(dispute) = binary::decode::<EnergyDispute>(data) {
        let _ = binary::encode(&dispute);
    }
    if let Ok(slash) = binary::decode::<EnergySlash>(data) {
        let _ = binary::encode(&slash);
    }
}
