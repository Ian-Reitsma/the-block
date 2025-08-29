#![no_main]
use libfuzzer_sys::fuzz_target;
mod network;

fuzz_target!(|data: &[u8]| {
    network::run(data);
});
