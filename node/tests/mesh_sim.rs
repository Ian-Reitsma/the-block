#![cfg(feature = "integration-tests")]
#![cfg(unix)]

use foundation_serialization::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;
use sys::tempfile::tempdir;
use the_block::range_boost;
use the_block::relay::RelayJob;

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

#[test]
fn tcp_forwarder_delivers_bundle() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(&buf);
        }
        if let Ok((mut stream, _)) = listener.accept() {
            let mut len_bytes = [0u8; 4];
            stream.read_exact(&mut len_bytes).unwrap();
            let len = u32::from_le_bytes(len_bytes) as usize;
            let mut buf = vec![0u8; len];
            stream.read_exact(&mut buf).unwrap();
            tx.send(buf).unwrap();
        }
    });

    std::env::set_var("TB_MESH_STATIC_PEERS", addr.to_string());
    range_boost::set_enabled(false);
    range_boost::discover_peers();

    let queue = Arc::new(Mutex::new(range_boost::RangeBoost::new()));
    range_boost::spawn_forwarder(&queue);
    {
        let mut guard = queue.lock().unwrap();
        guard.enqueue(b"mesh-test".to_vec(), stub_job());
    }

    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());

    range_boost::set_enabled(true);

    let payload = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let bundle: range_boost::Bundle = json::from_slice(&payload).unwrap();
    assert_eq!(bundle.payload, b"mesh-test");
    range_boost::set_enabled(false);
}

fn stub_job() -> RelayJob {
    RelayJob {
        job_id: "mesh".into(),
        provider: "provider".into(),
        campaign_id: None,
        creative_id: None,
        mesh_peer: None,
        mesh_transport: None,
        mesh_latency_ms: None,
        clearing_price_usd_micros: 0,
        resource_floor_usd_micros: 0,
        price_per_mib_usd_micros: 0,
        total_usd_micros: 0,
        bytes: 0,
        offered_at_micros: 0,
    }
}
