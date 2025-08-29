#![no_main]
use libfuzzer_sys::fuzz_target;
mod compute_market;

fuzz_target!(|data: &[u8]| {
    compute_market::run(data);
});
