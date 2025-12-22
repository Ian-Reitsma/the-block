#![cfg(feature = "inhouse-backend")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "support/ws_shared.rs"]
mod ws_shared;
use ws_shared::*;

use foundation_async::sync::oneshot;
use runtime::net::TcpStream;
use runtime::ws::{self, Message, ServerStream};
use runtime::{self, spawn};
use std::net::SocketAddr;
use std::thread;

#[test]
fn server_sees_abnormal_close_when_client_disconnects() {
    ensure_inhouse_backend();
    let _guard = websocket_test_guard();
    runtime::block_on(async {
        let std_listener =
            std::net::TcpListener::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
                .expect("bind listener");
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
            ws::write_server_handshake(&mut stream, &key, &[])
                .await
                .expect("handshake resp");
            let mut ws = ServerStream::new(stream);
            let close = ws.recv().await.expect("close");
            match close {
                Some(Message::Close(Some(frame))) => {
                    assert_eq!(frame.code, ws::ABNORMAL_CLOSE_CODE);
                    assert_eq!(frame.reason, ws::ABNORMAL_CLOSE_REASON);
                }
                other => panic!("unexpected close result: {other:?}"),
            }
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
        drop(stream);
        runtime::yield_now().await;

        server.await.expect("server task");
    });
}

#[test]
fn server_handles_fragmented_close_payload_from_client() {
    ensure_inhouse_backend();
    let _guard = websocket_test_guard();
    runtime::block_on(async {
        let std_listener =
            std::net::TcpListener::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
                .expect("bind listener");
        let addr = std_listener.local_addr().unwrap();
        let reason = "client fragmented reason";
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
            ws::write_server_handshake(&mut stream, &key, &[])
                .await
                .expect("handshake resp");
            let mut ws = ServerStream::new(stream);

            let msg = ws.recv().await.expect("receive message");
            match msg {
                Some(Message::Close(Some(frame))) => {
                    assert_eq!(frame.code, 4002);
                    assert_eq!(frame.reason, reason);
                }
                other => panic!("expected close frame, got: {other:?}"),
            }
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
        write_fragmented_close_payload(&mut stream, 4002, reason).await;
        drop(stream);
        runtime::yield_now().await;

        server.await.expect("server task");
    });
}
