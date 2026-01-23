#![forbid(unsafe_code)]

use foundation_serialization::binary;
use the_block::receipts::{EnergyReceipt, EnergySlashReceipt};

pub fn run(data: &[u8]) {
    if let Ok(receipt) = binary::decode::<EnergyReceipt>(data) {
        let _ = binary::encode(&receipt);
    }
    if let Ok(slash) = binary::decode::<EnergySlashReceipt>(data) {
        let _ = binary::encode(&slash);
    }
}
