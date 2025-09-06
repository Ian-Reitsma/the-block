use the_block::net::{message, SUPPORTED_VERSION};

pub fn run(data: &[u8]) {
    let _ = SUPPORTED_VERSION;
    let _ = message::decode(data);
}
