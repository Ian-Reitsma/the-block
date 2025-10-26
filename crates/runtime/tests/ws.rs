#![cfg(feature = "inhouse-backend")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use runtime::net::{TcpListener, TcpStream};
use runtime::ws::{self, Message, ServerStream};
use runtime::{self, spawn};
use std::net::SocketAddr;
use std::sync::Once;

fn ensure_inhouse_backend() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::env::set_var("TB_RUNTIME_BACKEND", "inhouse");
        assert_eq!(runtime::handle().backend_name(), "inhouse");
    });
}

async fn read_handshake_request(stream: &mut TcpStream) -> String {
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

fn extract_key(request: &str) -> &str {
    request
        .lines()
        .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
        .map(str::trim)
        .expect("sec-websocket-key present")
}

async fn write_fragmented_text(stream: &mut TcpStream, payload: &str) {
    let bytes = payload.as_bytes();
    let mid = bytes.len() / 2;
    send_frame(stream, 0x1, false, &bytes[..mid]).await;
    send_frame(stream, 0x0, true, &bytes[mid..]).await;
}

async fn send_frame(stream: &mut TcpStream, opcode: u8, fin: bool, payload: &[u8]) {
    use rand::RngCore;
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

#[test]
fn server_unmasks_payloads() {
    ensure_inhouse_backend();
    runtime::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
            .await
            .expect("bind listener");
        let addr = listener.local_addr().unwrap();

        let server = spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let request = read_handshake_request(&mut stream).await;
            let key = extract_key(&request).to_string();
            ws::write_server_handshake(&mut stream, &key, &[]) // respond
                .await
                .expect("handshake resp");
            let mut ws = ServerStream::new(stream);
            if let Some(Message::Binary(data)) = ws.recv().await.expect("recv binary") {
                assert_eq!(data, vec![1, 2, 3, 4]);
            } else {
                panic!("expected binary frame");
            }
            ws.send(Message::Close(None)).await.expect("close");
        });

        let mut stream = TcpStream::connect(addr).await.expect("connect");
        let key = ws::handshake_key();
        let request = format!(
            "GET /logs HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("write request");
        let expected_accept = ws::handshake_accept(&key).expect("handshake accept");
        ws::read_client_handshake(&mut stream, &expected_accept)
            .await
            .expect("validate handshake");
        let mut client = ws::ClientStream::new(stream);
        client
            .send(Message::Binary(vec![1, 2, 3, 4]))
            .await
            .expect("send binary");
        client.recv().await.expect("close ack");
        server.await.expect("server task");
    });
}

#[test]
fn fragmented_frames_are_joined() {
    ensure_inhouse_backend();
    runtime::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
            .await
            .expect("bind listener");
        let addr = listener.local_addr().unwrap();

        let server = spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let request = read_handshake_request(&mut stream).await;
            let key = extract_key(&request).to_string();
            ws::write_server_handshake(&mut stream, &key, &[]) // respond
                .await
                .expect("handshake resp");
            let mut ws = ServerStream::new(stream);
            match ws.recv().await.expect("recv text") {
                Some(Message::Text(text)) => assert_eq!(text, "hello world"),
                other => panic!("unexpected payload: {other:?}"),
            }
            ws.send(Message::Close(None)).await.expect("close");
        });

        let mut stream = TcpStream::connect(addr).await.expect("connect");
        let key = ws::handshake_key();
        let request = format!(
            "GET /trace HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("write request");
        let expected_accept = ws::handshake_accept(&key).expect("handshake accept");
        ws::read_client_handshake(&mut stream, &expected_accept)
            .await
            .expect("handshake");
        write_fragmented_text(&mut stream, "hello world").await;
        let mut client = ws::ClientStream::new(stream);
        client.recv().await.expect("close");
        server.await.expect("server task");
    });
}

#[test]
fn ping_pong_cycle() {
    ensure_inhouse_backend();
    runtime::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
            .await
            .expect("bind listener");
        let addr = listener.local_addr().unwrap();

        let server = spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let request = read_handshake_request(&mut stream).await;
            let key = extract_key(&request).to_string();
            ws::write_server_handshake(&mut stream, &key, &[]) // respond
                .await
                .expect("handshake resp");
            let mut ws = ServerStream::new(stream);
            match ws.recv().await.expect("recv ping") {
                Some(Message::Ping(payload)) => assert_eq!(payload, b"hi"),
                other => panic!("unexpected payload: {other:?}"),
            }
            ws.close().await.expect("close");
        });

        let mut stream = TcpStream::connect(addr).await.expect("connect");
        let key = ws::handshake_key();
        let request = format!(
            "GET /metrics HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("write request");
        let expected_accept = ws::handshake_accept(&key).expect("handshake accept");
        ws::read_client_handshake(&mut stream, &expected_accept)
            .await
            .expect("handshake");
        let mut client = ws::ClientStream::new(stream);
        client
            .send(Message::Ping(b"hi".to_vec()))
            .await
            .expect("send ping");
        match client.recv().await.expect("recv pong") {
            Some(Message::Pong(payload)) => assert_eq!(payload, b"hi"),
            other => panic!("unexpected response: {other:?}"),
        }
        client.recv().await.expect("close");
        server.await.expect("server task");
    });
}
