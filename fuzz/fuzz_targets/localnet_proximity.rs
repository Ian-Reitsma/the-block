#![no_main]
use foundation_serialization::binary;
use libfuzzer_sys::fuzz_target;
use the_block::localnet::{validate_proximity, AssistReceipt};

fuzz_target!(|data: &[u8]| {
    if let Ok(receipt) = binary::decode::<AssistReceipt>(data) {
        let _ = validate_proximity(receipt.device, receipt.rssi, receipt.rtt_ms);
    }
});
