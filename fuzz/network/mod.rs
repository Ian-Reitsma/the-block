use the_block::net::message;

pub fn run(data: &[u8]) {
    let _ = message::decode(data);
}
