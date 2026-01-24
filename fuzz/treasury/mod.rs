#![forbid(unsafe_code)]

use foundation_serialization::binary;
use the_block::governance::treasury::{DisbursementPayload, TreasuryDisbursement};

pub fn run(data: &[u8]) {
    if let Ok(payload) = binary::decode::<DisbursementPayload>(data) {
        let _ = binary::encode(&payload);
    }
    if let Ok(disbursement) = binary::decode::<TreasuryDisbursement>(data) {
        let _ = binary::encode(&disbursement);
    }
}
