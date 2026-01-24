#![forbid(unsafe_code)]

use foundation_serialization::binary;
use the_block::receipts::{BlockTorchReceiptMetadata, ComputeReceipt, Receipt};

pub fn run(data: &[u8]) {
    if let Ok(receipt) = binary::decode::<Receipt>(data) {
        let _ = binary::encode(&receipt);
        if let Receipt::Compute(compute) = &receipt {
            if let Some(metadata) = &compute.blocktorch {
                let _ = binary::encode(metadata);
            }
        }
        if let Receipt::ComputeSlash(slash) = &receipt {
            let _ = binary::encode(slash);
        }
        if let Receipt::EnergySlash(slash) = &receipt {
            let _ = binary::encode(slash);
        }
    }
    if let Ok(metadata) = binary::decode::<BlockTorchReceiptMetadata>(data) {
        let _ = binary::encode(&metadata);
    }
    if let Ok(compute) = binary::decode::<ComputeReceipt>(data) {
        let _ = binary::encode(&compute);
        if let Some(metadata) = &compute.blocktorch {
            let _ = binary::encode(metadata);
        }
    }
}
