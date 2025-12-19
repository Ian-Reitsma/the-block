#![cfg(feature = "inhouse-backend")]
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use concurrency::Lazy;
use rand::RngCore;
use runtime;
use runtime::net::TcpStream;
use std::sync::{Mutex, MutexGuard};

pub fn ensure_inhouse_backend() {
    assert_eq!(
        runtime::handle().backend_name(),
        "inhouse",
        "inhouse backend should be active"
    );
}

pub fn websocket_test_guard() -> MutexGuard<'static, ()> {
    static GUARD: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
    GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
