#![cfg(target_os = "windows")]

use std::net::SocketAddr;
use std::os::windows::io::AsRawSocket;
use std::time::Duration;

use sys::net::{bind_tcp_listener, connect};
use sys::reactor::{Events, Interest, Poll, Token};

#[test]
fn reactor_reports_connection_readiness_and_deregisters() {
    let poll = Poll::new().expect("create poll");
    let mut events = Events::with_capacity(8);

    let listener =
        bind_tcp_listener("127.0.0.1:0".parse::<SocketAddr>().unwrap()).expect("bind listener");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    let (mut stream, connected) = connect(addr).expect("connect");

    poll.register(listener.as_raw_socket(), Token(0), Interest::READABLE)
        .expect("register listener");
    poll.register(
        stream.as_raw_socket(),
        Token(1),
        Interest::WRITABLE | Interest::READABLE,
    )
    .expect("register stream");

    let mut seen_listener = false;
    let mut seen_stream = connected;

    for _ in 0..10 {
        poll.poll(&mut events, Some(Duration::from_millis(50)))
            .expect("poll");
        for event in events.iter() {
            match event.token().0 {
                0 => seen_listener = true,
                1 => seen_stream = true,
                _ => {}
            }
        }
        if seen_listener && seen_stream {
            break;
        }
    }

    assert!(seen_listener, "listener readiness not observed");
    assert!(seen_stream, "stream readiness not observed");

    // Drain the pending connection to keep the listener stable.
    let _ = listener.accept().expect("accept pending");

    poll.deregister(stream.as_raw_socket(), Token(1))
        .expect("deregister stream");
    poll.deregister(listener.as_raw_socket(), Token(0))
        .expect("deregister listener");
}
