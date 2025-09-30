#![no_main]
use libfuzzer_sys::fuzz_target;
use p2p_overlay::InhousePeerId;
use std::str::from_utf8;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = from_utf8(data) {
        if let Ok(id) = InhousePeerId::from_base58(s) {
            let round = InhousePeerId::from_base58(&id.to_base58()).expect("roundtrip");
            assert_eq!(id, round);
        }
    }
});
