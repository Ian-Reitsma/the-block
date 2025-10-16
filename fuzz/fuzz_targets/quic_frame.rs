#![forbid(unsafe_code)]

use foundation_serialization::binary;
use the_block::net::fuzz_decode_message;

pub fn run(data: &[u8]) {
    if let Ok(msg) = fuzz_decode_message(data) {
        let _ = binary::encode(&msg).ok();
    }
}

fn main() {}
