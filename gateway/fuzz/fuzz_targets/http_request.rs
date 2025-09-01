#![no_main]
use libfuzzer_sys::fuzz_target;
use the_block::gateway::http::parse_request;

fuzz_target!(|data: &[u8]| {
    parse_request(data);
});
