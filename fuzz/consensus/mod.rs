#![forbid(unsafe_code)]

use foundation_serialization::binary;
use the_block::Block;

pub fn run(data: &[u8]) {
    if let Ok(block) = binary::decode::<Block>(data) {
        let _ = binary::encode(&block);
    }
}
