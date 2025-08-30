use the_block::net::message::{self, SUPPORTED_VERSION};

pub fn run(data: &[u8]) {
    let _ = SUPPORTED_VERSION;
    let _ = message::decode(data);
}
