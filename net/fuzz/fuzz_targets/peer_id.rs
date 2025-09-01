#![no_main]
use libfuzzer_sys::fuzz_target;
use libp2p::PeerId;
use std::str::from_utf8;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = from_utf8(data) {
        if let Ok(id) = s.parse::<PeerId>() {
            let round = id.to_string().parse::<PeerId>().expect("roundtrip");
            assert_eq!(id, round);
        }
    }
});
