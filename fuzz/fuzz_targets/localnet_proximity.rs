#![forbid(unsafe_code)]

use foundation_serialization::binary;
use the_block::localnet::{validate_proximity, AssistReceipt};

pub fn run(data: &[u8]) {
    if let Ok(receipt) = binary::decode::<AssistReceipt>(data) {
        let _ = validate_proximity(receipt.device, receipt.rssi, receipt.rtt_ms);
    }
}

fn main() {}
