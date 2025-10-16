use the_block::net::{fuzz_decode_message, SUPPORTED_VERSION};

pub fn run(data: &[u8]) {
    let _ = SUPPORTED_VERSION;
    let _ = fuzz_decode_message(data);
}
