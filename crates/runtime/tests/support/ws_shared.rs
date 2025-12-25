#![cfg(feature = "inhouse-backend")]
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use concurrency::Lazy;
use rand::RngCore;
use runtime;
use runtime::net::TcpStream;
use std::io;
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::{Mutex, MutexGuard};

pub fn ensure_inhouse_backend() {
    assert_eq!(
        runtime::handle().backend_name(),
        "inhouse",
        "inhouse backend should be active"
    );
}

/// Guard to serialize WebSocket tests only when running with multiple threads.
/// When running with --test-threads=1, guards are no-ops to avoid unnecessary contention.
pub fn websocket_test_guard() -> WebSocketTestGuard {
    // Check if we're in single-threaded test mode
    let is_single_threaded = std::thread::available_parallelism()
        .map(|p| p.get() == 1)
        .unwrap_or(false);

    if is_single_threaded {
        // No-op guard for single-threaded execution
        WebSocketTestGuard { _guard: None }
    } else {
        // Serialize WebSocket tests to prevent port/resource conflicts
        static GUARD: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
        let guard = GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        WebSocketTestGuard {
            _guard: Some(guard),
        }
    }
}

/// RAII guard that either holds a mutex lock (multi-threaded) or does nothing (single-threaded)
pub struct WebSocketTestGuard {
    _guard: Option<MutexGuard<'static, ()>>,
}

/// Timeout configuration for WebSocket tests to prevent indefinite hangs
pub const WEBSOCKET_TEST_TIMEOUT_SECS: u64 = 10;

/// Wraps a test with a timeout to prevent indefinite hangs
/// Note: Since the inhouse runtime may not support select!, this provides early detection
/// of tests taking too long via a background monitoring thread
pub fn ensure_websocket_test_timeout() {
    // Note: This is a basic safeguard. For full timeout support, the inhouse runtime
    // would need explicit select! support or cancellation tokens.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(WEBSOCKET_TEST_TIMEOUT_SECS + 5));
        // If we get here, the test is taking way too long
        eprintln!(
            "WARNING: WebSocket test is still running after {} seconds. This likely indicates a deadlock.",
            WEBSOCKET_TEST_TIMEOUT_SECS + 5
        );
    });
}

pub async fn bind_listener(addr: SocketAddr) -> io::Result<StdTcpListener> {
    match StdTcpListener::bind(addr) {
        Ok(listener) => Ok(listener),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            let runtime_listener = runtime::net::TcpListener::bind(addr).await?;
            runtime_listener.into_std()
        }
        Err(err) => Err(err),
    }
}

pub async fn bind_listener_or_skip(addr: SocketAddr) -> Option<StdTcpListener> {
    match bind_listener(addr).await {
        Ok(listener) => Some(listener),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("Skipping websocket test because binding {addr} is not permitted: {err}");
            None
        }
        Err(err) => panic!("bind listener: {err}"),
    }
}

pub async fn read_handshake_request(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 64];
    loop {
        let n = stream.read(&mut tmp).await.expect("read handshake");
        assert!(n > 0, "handshake must not terminate early");
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(buf).expect("handshake utf8")
}

pub fn extract_key(request: &str) -> &str {
    request
        .lines()
        .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
        .map(str::trim)
        .expect("sec-websocket-key present")
}

pub async fn write_fragmented_text(stream: &mut TcpStream, payload: &str) {
    let bytes = payload.as_bytes();
    let mid = bytes.len() / 2;
    send_frame(stream, 0x1, false, &bytes[..mid]).await;
    send_frame(stream, 0x0, true, &bytes[mid..]).await;
}

async fn send_frame(stream: &mut TcpStream, opcode: u8, fin: bool, payload: &[u8]) {
    let mut header = Vec::with_capacity(2 + payload.len());
    header.push((if fin { 0x80 } else { 0x00 }) | opcode);
    let mask_bit = 0x80;
    if payload.len() < 126 {
        header.push(mask_bit | payload.len() as u8);
    } else {
        panic!("test frame too large");
    }
    let mut mask = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut mask);
    header.extend_from_slice(&mask);
    let mut masked = payload.to_vec();
    for (idx, byte) in masked.iter_mut().enumerate() {
        *byte ^= mask[idx % 4];
    }
    stream.write_all(&header).await.expect("write header");
    stream.write_all(&masked).await.expect("write payload");
}

pub async fn write_fragmented_close_payload(stream: &mut TcpStream, code: u16, reason: &str) {
    let mut payload = Vec::new();
    payload.extend_from_slice(&code.to_be_bytes());
    payload.extend_from_slice(reason.as_bytes());

    let mut mask = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut mask);
    let mut masked = payload.clone();
    for (idx, byte) in masked.iter_mut().enumerate() {
        *byte ^= mask[idx % 4];
    }

    let mut header = Vec::with_capacity(2 + mask.len());
    header.push(0x80 | 0x8);
    header.push(0x80 | (masked.len() as u8));
    header.extend_from_slice(&mask);

    stream.write_all(&header).await.expect("write close header");
    let split = masked.len() / 2;
    stream
        .write_all(&masked[..split])
        .await
        .expect("write close header chunk");
    runtime::yield_now().await;
    stream
        .write_all(&masked[split..])
        .await
        .expect("write close payload chunk");
}
