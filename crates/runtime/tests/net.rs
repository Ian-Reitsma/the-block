#![cfg(feature = "inhouse-backend")]

use runtime::io::{read_length_prefixed, write_length_prefixed};
use runtime::net::{TcpListener, TcpStream, UdpSocket};
use runtime::{self, sleep, spawn};
use std::net::SocketAddr;
use std::sync::Once;
use std::time::Duration;

fn ensure_inhouse_backend() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::env::set_var("TB_RUNTIME_BACKEND", "inhouse");
        assert_eq!(runtime::handle().backend_name(), "inhouse");
    });
}

#[test]
fn tcp_length_prefixed_echo() {
    ensure_inhouse_backend();

    runtime::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
            .await
            .expect("bind tcp listener");
        let addr = listener.local_addr().expect("listener address");

        let server = spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept connection");
            if let Some(frame) = read_length_prefixed(&mut stream, 1024)
                .await
                .expect("read frame")
            {
                write_length_prefixed(&mut stream, &frame)
                    .await
                    .expect("write frame");
                stream.flush().await.expect("flush frame");
            }
        });

        sleep(Duration::from_millis(10)).await;

        let mut client = TcpStream::connect(addr)
            .await
            .expect("connect to echo server");
        write_length_prefixed(&mut client, b"ping")
            .await
            .expect("send frame");
        let echoed = read_length_prefixed(&mut client, 1024)
            .await
            .expect("read echoed frame")
            .expect("frame present");
        assert_eq!(echoed, b"ping");

        server.await.expect("server task");
    });
}

#[test]
fn udp_round_trip() {
    ensure_inhouse_backend();

    runtime::block_on(async {
        let server_socket = UdpSocket::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
            .await
            .expect("bind udp server");
        let server_addr = server_socket.local_addr().expect("server addr");

        let server = spawn(async move {
            let mut socket = server_socket;
            let mut buf = [0u8; 16];
            let (len, peer) = socket.recv_from(&mut buf).await.expect("receive datagram");
            socket
                .send_to(&buf[..len], peer)
                .await
                .expect("echo datagram");
        });

        let mut client = UdpSocket::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap())
            .await
            .expect("bind udp client");
        client
            .send_to(b"ping", server_addr)
            .await
            .expect("send datagram");

        let mut buf = [0u8; 16];
        let (len, _) = client
            .recv_from(&mut buf)
            .await
            .expect("receive echoed datagram");
        assert_eq!(&buf[..len], b"ping");

        server.await.expect("udp server task");
    });
}
