#![no_main]
use libfuzzer_sys::fuzz_target;
use the_block::range_boost::parse_discovery_packet;

fuzz_target!(|data: &[u8]| {
    let _ = parse_discovery_packet(data);
});
