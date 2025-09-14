//! Minimal mobile-style relay demonstrating RangeBoost usage.
//!
//! Run with:
//! `cargo run --example mobile_relay -- -s /tmp/range.sock`

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::time::Duration;

use the_block::range_boost;

fn main() {
    let socket = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/mobile_relay.sock".into());

    let listener = UnixListener::bind(&socket).expect("bind socket");
    std::env::set_var("TB_MESH_STATIC_PEERS", format!("unix:{}", socket));
    range_boost::set_enabled(true);
    std::thread::spawn(|| loop {
        range_boost::discover_peers();
        std::thread::sleep(Duration::from_secs(10));
    });

    if let Ok((mut stream, _)) = listener.accept() {
        let mut buf = [0u8; 1];
        let _ = stream.read(&mut buf);
        let _ = stream.write_all(&buf);
    }
}
