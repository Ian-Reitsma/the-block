#![cfg(feature = "inhouse-backend")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "support/ws_shared.rs"]
mod ws_shared;
use ws_shared::*;

use foundation_async::sync::oneshot;
use rand::RngCore;
use runtime::net::TcpStream;
use runtime::ws::{self, Message, ServerStream};
use runtime::{self, spawn};
use std::net::SocketAddr;
use std::thread;

#[test]
fn server_unmasks_payloads() {
    ensure_inhouse_backend();
    let _guard = websocket_test_guard();
    runtime::block_on(async {
        let requested_addr = "127.0.0.1:0".parse::<SocketAddr>().unwrap();
        let Some(std_listener) = bind_listener_or_skip(requested_addr).await else {
            return;
        };
        let addr = std_listener.local_addr().unwrap();
        let (conn_tx, conn_rx) = oneshot::channel();
        thread::spawn(move || {
            let (stream, _) = std_listener.accept().expect("accept");
            let _ = conn_tx.send(stream);
        });

        let server = spawn(async move {
            let std_stream = conn_rx.await.expect("accepted stream");
            let mut stream =
                TcpStream::from_std(std_stream).expect("convert accepted stream to runtime");
            let request = read_handshake_request(&mut stream).await;
            let key = extract_key(&request).to_string();
            ws::write_server_handshake(&mut stream, &key, &[]) // respond
                .await
                .expect("handshake resp");
            runtime::yield_now().await;
            let mut ws = ServerStream::new(stream);
            if let Some(Message::Binary(data)) = ws.recv().await.expect("recv binary") {
                assert_eq!(data, vec![1, 2, 3, 4]);
            } else {
                panic!("expected binary frame");
            }
            ws.send(Message::Close(None)).await.expect("close");
        });

        runtime::yield_now().await;
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
    let _guard = websocket_test_guard();
    runtime::block_on(async {
        let requested_addr = "127.0.0.1:0".parse::<SocketAddr>().unwrap();
        let Some(std_listener) = bind_listener_or_skip(requested_addr).await else {
            return;
        };
        let addr = std_listener.local_addr().unwrap();
        let (conn_tx, conn_rx) = oneshot::channel();
        thread::spawn(move || {
            let (stream, _) = std_listener.accept().expect("accept");
            let _ = conn_tx.send(stream);
        });

        let server = spawn(async move {
            let std_stream = conn_rx.await.expect("accepted stream");
            let mut stream =
                TcpStream::from_std(std_stream).expect("convert accepted stream to runtime");
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

        runtime::yield_now().await;
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
    let _guard = websocket_test_guard();
    runtime::block_on(async {
        let requested_addr = "127.0.0.1:0".parse::<SocketAddr>().unwrap();
        let Some(std_listener) = bind_listener_or_skip(requested_addr).await else {
            return;
        };
        let addr = std_listener.local_addr().unwrap();
        let (conn_tx, conn_rx) = oneshot::channel();
        thread::spawn(move || {
            let (stream, _) = std_listener.accept().expect("accept");
            let _ = conn_tx.send(stream);
        });

        let server = spawn(async move {
            let std_stream = conn_rx.await.expect("accepted stream");
            let mut stream =
                TcpStream::from_std(std_stream).expect("convert accepted stream to runtime");
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

        runtime::yield_now().await;
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

#[test]
fn abrupt_server_disconnect_reports_abnormal_close() {
    ensure_inhouse_backend();
    let _guard = websocket_test_guard();
    runtime::block_on(async {
        let requested_addr = "127.0.0.1:0".parse::<SocketAddr>().unwrap();
        let Some(std_listener) = bind_listener_or_skip(requested_addr).await else {
            return;
        };
        let addr = std_listener.local_addr().unwrap();
        let (conn_tx, conn_rx) = oneshot::channel();
        thread::spawn(move || {
            let (stream, _) = std_listener.accept().expect("accept");
            let _ = conn_tx.send(stream);
        });

        let server = spawn(async move {
            let std_stream = conn_rx.await.expect("accepted stream");
            let mut stream =
                TcpStream::from_std(std_stream).expect("convert accepted stream to runtime");
            let request = read_handshake_request(&mut stream).await;
            let key = extract_key(&request).to_string();
            ws::write_server_handshake(&mut stream, &key, &[]) // respond
                .await
                .expect("handshake resp");
            runtime::yield_now().await;
            // Drop the stream without sending a close frame to simulate an abrupt disconnect.
            drop(stream);
        });

        runtime::yield_now().await;
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
        let close = client.recv().await.expect("close");
        match close {
            Some(Message::Close(Some(frame))) => {
                assert_eq!(frame.code, ws::ABNORMAL_CLOSE_CODE);
                assert_eq!(frame.reason, ws::ABNORMAL_CLOSE_REASON);
            }
            other => panic!("unexpected close result: {other:?}"),
        }

        server.await.expect("server task");
    });
}

#[test]
fn fragmented_close_frame_payload_delivered() {
    ensure_inhouse_backend();
    let _guard = websocket_test_guard();
    runtime::block_on(async {
        let requested_addr = "127.0.0.1:0".parse::<SocketAddr>().unwrap();
        let Some(std_listener) = bind_listener_or_skip(requested_addr).await else {
            return;
        };
        let addr = std_listener.local_addr().unwrap();
        let reason = "fragmented reason";
        let (conn_tx, conn_rx) = oneshot::channel();
        thread::spawn(move || {
            let (stream, _) = std_listener.accept().expect("accept");
            let _ = conn_tx.send(stream);
        });

        let server = spawn(async move {
            let std_stream = conn_rx.await.expect("accepted stream");
            let mut stream =
                TcpStream::from_std(std_stream).expect("convert accepted stream to runtime");
            let request = read_handshake_request(&mut stream).await;
            let key = extract_key(&request).to_string();
            ws::write_server_handshake(&mut stream, &key, &[]) // respond
                .await
                .expect("handshake resp");
            runtime::yield_now().await;
            let mut ready_signal = [0u8; 5];
            stream
                .read_exact(&mut ready_signal)
                .await
                .expect("read ready signal");
            assert_eq!(&ready_signal, b"ready");

            let code = 4001u16;
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
                .expect("write first chunk");
            runtime::yield_now().await;
            stream
                .write_all(&masked[split..])
                .await
                .expect("write second chunk");
            runtime::yield_now().await;
            let mut ack_buf = [0u8; 16];
            let _ = stream.read(&mut ack_buf).await;
        });

        runtime::yield_now().await;
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
        stream
            .write_all(b"ready")
            .await
            .expect("write ready signal");

        let mut client = ws::ClientStream::new(stream);
        match client.recv().await.expect("close payload") {
            Some(Message::Close(Some(frame))) => {
                assert_eq!(frame.code, 4001);
                assert_eq!(frame.reason, reason);
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        server.await.expect("server task");
    });
}
