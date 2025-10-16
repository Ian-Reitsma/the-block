#![forbid(unsafe_code)]

use the_block::range_boost::parse_discovery_packet;

pub fn run(data: &[u8]) {
    let _ = parse_discovery_packet(data);
}

fn main() {}
