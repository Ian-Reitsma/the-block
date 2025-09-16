#![no_main]

use libfuzzer_sys::fuzz_target;
use the_block::net::message;

fuzz_target!(|data: &[u8]| {
    if let Ok(msg) = message::decode(data) {
        let _ = bincode::serialize(&msg).ok();
    }
});
