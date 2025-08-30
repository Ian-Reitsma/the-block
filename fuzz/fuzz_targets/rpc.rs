#![no_main]
use libfuzzer_sys::fuzz_target;
mod rpc;

fuzz_target!(|data: &[u8]| {
    rpc::run(data);
});
