use foundation_serialization::binary;
use the_block::receipts::ComputeReceipt;

pub fn run(data: &[u8]) {
    if let Ok(receipt) = binary::decode::<ComputeReceipt>(data) {
        let _ = binary::encode(&receipt);
    }
}
