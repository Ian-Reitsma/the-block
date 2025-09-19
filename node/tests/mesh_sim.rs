#![cfg(feature = "integration-tests")]
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;
use the_block::range_boost;

#[test]
fn unix_mesh_prefers_low_latency() {
    let dir = tempdir().unwrap();
    let fast_path = dir.path().join("fast.sock");
    let slow_path = dir.path().join("slow.sock");
    let fast_listener = UnixListener::bind(&fast_path).unwrap();
    let slow_listener = UnixListener::bind(&slow_path).unwrap();

    thread::spawn(move || {
        if let Ok((mut s, _)) = fast_listener.accept() {
            let mut buf = [0u8; 1];
            let _ = s.read(&mut buf);
            let _ = s.write_all(&buf);
        }
    });
    thread::spawn(move || {
        if let Ok((mut s, _)) = slow_listener.accept() {
            thread::sleep(Duration::from_millis(50));
            let mut buf = [0u8; 1];
            let _ = s.read(&mut buf);
            let _ = s.write_all(&buf);
        }
    });

    std::env::set_var(
        "TB_MESH_STATIC_PEERS",
        format!("unix:{},unix:{}", fast_path.display(), slow_path.display()),
    );
    let peers = range_boost::discover_peers();
    assert!(peers.len() >= 2);
    assert_eq!(peers[0].addr, format!("unix:{}", fast_path.display()));
}
